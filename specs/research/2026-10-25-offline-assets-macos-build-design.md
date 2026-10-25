---
status: proposal
date: 2026-10-25
scope: macOS arm64 and x86_64
---

# 必需离线模型与Mac构建发行设计

这是架构建议稿，尚未修改生产实现或生成含模型的新release。用户已明确本次完成Mac设计，Windows客户端适配单独做。讨论来源与未确认决策见[讨论台账](../prd/_drafts/offline-models-cross-platform-packaging.md)。

推荐采用：**模型作为Core bundle内的只读资源，随App升级；Core自动校验并继续管理worker；Mac默认发行一个Universal2完整离线DMG。**

## 1. 当前事实与需要修正的边界

- 0.1.7目前是arm64瘦包，来源为干净提交`4d2c5eb`，DMG约17.7MB；没有随包模型。详见[发行记录](2026-10-25-release-0.1.7.md)。
- X-ASR＋VAD资源176349407字节，CampPlus 28281138字节，共204630545字节。两者单独压成UDZO模型映像实测163855288字节。此前180–190MB是“当前单架构客户端＋模型”的估算，不能直接当成Universal2最终包大小；Universal2须实际打包后重新量。
- [model_pack.rs](../../crates/hear-core/src/model_pack.rs)集中持有当前SHA和网络安装，模型安装在用户数据目录。Core负责ModelState/PrepareModels/资源调度；[analysis.rs](../../crates/hear-core/src/analysis.rs)只向worker传绝对路径和计算请求。
- Core本身在`HearCore.app`登录项内，GUI、CLI按需启动及登录启动最终运行同一个Core。不能只靠GUI的Tauri resource_dir才能找到必需模型。
- 当前脚本默认宿主架构、共用固定暂存目录；package_macos.sh用`uname -m`命名，不能准确表示交叉编译或Universal结果。CI目前只有一项macos-15检查，不能据此声称Intel已验收。
- 当前首启完成条件还要求云服务配置与声音权限。本设计保证本地模型无需下载并可离线推理；云端事件整理/精转和权限规则保持原产品契约，不宣称整个应用所有功能离线。

## 2. 模型放在负责计算的Core bundle内

建议布局：

```text
hear.app/Contents/
  MacOS/
    hear-desktop                         # Universal2 GUI
    hear                                 # Universal2 CLI
  Resources/
    build-info.json
  Library/LoginItems/HearCore.app/Contents/
    MacOS/
      hear-core                          # Universal2
      hear-worker                        # Universal2
    Resources/
      models/
        models.lock.json
        speech-xasr-v1/
          silero_vad.onnx
          xasr/{encoder,decoder,joiner,tokens,LICENSE}
        speaker-v1/model.onnx
      licenses/                          # 模型、转换及原生依赖许可/声明
```

花括号仅为布局缩写，实际文件名维持当前固定模型身份。Core从自身可执行文件所属的bundle定位`Contents/Resources/models`，解析一次后将明确路径传给worker；不依赖`/Applications`固定位置、当前工作目录或GUI传入的任意目录。移动或重命名App后，下次启动重新解析。

模型是不可变软件资源，运行时直接只读加载；用户目录继续保存录音、数据库、日志和用户配置。这样只有随App的一份新模型，不必首次复制额外约205MB，也不需要新增模型版本选择、独立更新或缓存清理系统。这是基于当前“只读权重、模型随App升级”的推荐；此前提出首次复制的方案并非必要。

macOS标准bundle将数据资源放在Resources，签名覆盖资源；运行时不改App内容。[Apple bundle结构](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html)、[签名顺序与资源](https://developer.apple.com/library/archive/technotes/tn2206/)。

| 对象 | 权威来源与职责 |
| --- | --- |
| 要使用哪些模型/精确文件 | 仓库中一份`config/models.lock.json`，固定模型ID、来源revision、字节数、SHA256、相对路径及许可来源 |
| 构建缓存 | 根据该lock获取和校验的派生文件；只影响构建速度，不决定采用哪版模型 |
| App里的模型 | lock指定字节的发行副本；包内lock与编译绑定的lock必须一致 |
| 是否可用、失败原因 | 现有Core ModelState；GUI只读状态，不自行维护“已安装模型”标记 |
| 模型计算、内存驻留 | 既有worker和Governor；资源路径明确，worker没有下载/更新权限 |
| 已生成结果归属 | 继续使用每条结果的model_version；历史行不因升级改写 |

lock应同时供构建工具和Core消费，替换现在分散的资产常量；打包出的清单为其复制件或派生记录，不另建第二套可编辑清单。release-info记录lock SHA、原生库身份和组件身份。当前X-ASR仍固定[已选HF revision与文件SHA](2026-10-25-local-asr-selection/model-identity.json)，不换成同名但字节不同的tar。

## 3. 首次启动与升级

正常流程为：**Core定位资源 → 后台逐文件校验 → 按Governor许可做现有VAD/ASR真实加载检查 → 本地识别就绪**。声纹文件校验沿用Core的独立状态，发行烟测另覆盖实际embed；可用性与是否已有常驻worker分开。UI保持可响应，资源检查不在录音回调执行，不为本次初始化开麦或发云请求。

复用Checking/Loading/Ready/Failed与现有PrepareModels入口。正式客户端的该入口改为重新检查本地组件，UI移除正常首启中的“下载模型”步骤；模型缺失/损坏说明App组件不完整并引导重装。资源压力导致暂时不能加载须与损坏区分，不把校验成功直接当作推理就绪。

正式包只采用自己的固定资源，运行时不自动联网补齐、不选用户目录旧包、不回退SenseVoice。模型源站下载移到构建期。开发与测试在独立非发行入口明确提供资源根目录，默认从构建缓存取得同一lock的资产；不通过正式用户配置扩展成模型管理器。

旧0.1.7用户目录中的下载包可以保留但不再作为新包运行来源，不在启动热路径删除。用户历史、人工纠错、暂停/录音意图和原有本地结果保持原语义。

覆盖升级需要整个App和后台进程一起交接：手动安装先使用菜单“退出hear”，排空Core/worker后覆盖，再重新打开；继续复用现有build identity与core.lock交接。新Core重新验证新包资源。已有“新GUI启动时核对旧owner”不能单独保证用户任意运行中覆盖App仍安全；以后自动更新器也应在替换前完成排空。保存一份模型到数据目录并不能解决旧Core启动新worker的代码混用问题。

## 4. Mac发行形态：推荐Universal2

| 项目 | Apple Silicon | Intel |
| --- | --- | --- |
| Rust真实构建target | aarch64-apple-darwin | x86_64-apple-darwin |
| 最终Mach-O slice | arm64 | x86_64 |
| 模型文件 | 同一份固定X-ASR/VAD/CampPlus | 同一份固定X-ASR/VAD/CampPlus |
| 验收机器 | 原生arm64 Mac | 原生Intel Mac |
| 默认用户下载 | 同一个hear_<version>_macos_universal2.dmg | 同一个DMG |

模型不因CPU架构复制两份；只有可执行代码包含两个slice。两种架构共用ONNX文件仍须实际运行验证，不由格式通用推定效果/性能完全相同。

GUI、CLI、Core、worker四个Mach-O都必须有两个slice；只将GUI做Universal不够。静态依赖在各target链接时选择匹配架构，两个已链接的可执行文件再合并。Apple明确要求附属可执行组件也支持目标架构，Tauri支持`universal-apple-darwin`虚拟目标。[Apple Universal构建](https://developer.apple.com/documentation/apple-silicon/building-a-universal-macos-binary)、[Tauri Universal命令](https://v2.tauri.app/distribute/app-store/)。这里借用其构建能力，发行方式仍为现有Developer ID＋DMG，不改App Store分发。

已查询sherpa-onnx官方2.0.0发行API，存在arm64、x64和Universal2静态包。推荐锁定`osx-universal2-static-lib`包供两次链接使用，继续静态链接；具体包digest为`8ed41bf3b27ce55d6527bbc0f7754598c5afbb18e814ff0bddbc9ecd50fb09c5`，41041332字节。此处digest来自官方API，尚未在本轮下载并逐成员核验；后续构建前必须校验压缩包及每个所用.a的真实架构。[官方发行](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v2.0.0)、[Rust链接规则](https://k2-fsa.github.io/sherpa/onnx/rust-api/advanced-install.html)。

Universal原生包是相对当前arm64原生包的新构建输入，不能仅因版本号都是2.0.0就视作既有验收未受影响。必须记录各slice的库身份与实际引擎/ORT版本；本方案采用它时，ARM和Intel两边的完整识别与资源验收都要重做，原报告只作为冻结对照。模型SHA相同不等于链接实现或数值输出相同。

保留当前macOS 13.0产品门槛，核对所有Mach-O/native库的deployment target不高于它；不因打包修改系统支持范围。现有源码用较新SDK编译，系统声还包含13.x的ScreenCaptureKit与较新系统的Tap路径，最低系统与最新系统均需安装验收。禁止发布构建使用`target-cpu=native`使Intel包暗中依赖构建机特有指令。

## 5. 构建脚本：一个入口、明确步骤、原有平台工具

推荐一个Node编排入口`node scripts/build.mjs`；现有build_release.sh只转发参数。Node已经是前端构建依赖，不新增运行时服务。重构并复用现有macOS构建/签名/DMG脚本，使其接收明确target、stage、App路径和输出位置；版本、目标选择和凭据读取只在入口做一次。

建议开发接口（以下均为设计，尚未实现）：

```text
node scripts/build.mjs build --target aarch64-apple-darwin
node scripts/build.mjs build --target x86_64-apple-darwin
node scripts/build.mjs release --target universal-apple-darwin
./build_release.sh                       # 默认完整Universal2发行
```

`build`产生明确标记的内部候选，不承诺签名公证；`release`必须完整通过发行门槛，没有静默跳过签名/公证的成功路径。`universal-apple-darwin`是Tauri的虚拟目标，不能当成Rust target直接传给cargo。

### 必需步骤和输入输出

| 步骤 | 工作与退出条件 |
| --- | --- |
| preflight | 检查干净且明确的源码提交、七份版本/锁文件与helper版本、固定Node/Rust/Xcode/SDK、Rust两个target、磁盘及签名公证配置；失败在编译前退出 |
| assets | 按models.lock校验/获取公共资产和许可；按目标配置锁定原生库URL/hash/版本/架构；缺失或不匹配即失败，不接受同名不同字节文件 |
| compile | 单一Git SHA、版本和HEAR_BUILD_EPOCH；Core/worker/CLI分别cargo build两个真实target，Tauri负责GUI两个target及前端；Cargo --locked、npm ci保持锁文件 |
| assemble | lipo合并CLI/Core/worker，Tauri组合Universal GUI；生成目标专用bundle.files配置，把模型仅放进Core Resources一次；完整包才进入下一步 |
| verify candidate | 四组件身份、两slice、依赖架构/最低OS、模型清单与全部hash、禁止私人数据/凭据、Core/worker隔离真实加载；失败不能得到release完成标记 |
| sign | 内容全部定稿，先worker、Core helper、CLI，再最外层App；沿用现有entitlements与hardened runtime，不用codesign --deep替代明确签名顺序 |
| package | 从确切已验App制作DMG；文件名由目标配置生成，不用uname -m推导产物架构 |
| notarize | Developer ID签DMG，Apple Accepted，装订、stapler validate与Gatekeeper通过 |
| verify artifact | 只读挂载实际DMG，内容与签名App逐文件对应，模型/许可齐备；两个原生平台安装验收证据与当前实际包绑定 |
| publish locally | 装订后计算最终DMG SHA256，输出release-info/SHA256SUMS和日志；远端发布仍是另一个明确动作 |

仅保留两类声明文件：模型lock，和Mac target/native库及工具链配置。源站地址只参与构建获取；不要在脚本、Rust常量和CI YAML维护三套模型清单。原生库版本与Cargo所锁crate版本须交叉核对。

缓存按SHA及target/工具链分隔；每次构建有独立可写暂存目录。共享的只有可校验缓存，不共用另一个发行中的App目录或固定build/macos。正式发行从干净checkout构建，脏工作区明确失败，不自动stash、提交或暗中忽略改动。版本升级是单独动作，release检查版本而不改版本；锁文件操作必须按本地包结构处理，避免全局字符串替换第三方依赖。

所有组件的build-info和模型lock副本在App签名前定稿；签名后不能往App塞模型或改清单。最终二进制hash和DMG hash在相应签名/装订完成后记录。签名时间戳使bit-for-bit重现不宜作为保证，保证的是输入身份明确、过程与实际产物可验证。

凭据继续仅从环境/本机dotenv的签名公证白名单读取，不执行dotenv、不导出整份环境到报告；使用参数数组启动子进程。构建工具只处理当前构建目录，不替换/重启正在使用的/Applications/hear.app。

输出建议归在`build/releases/<version>/macos-universal2/`，包含DMG、release-info.json、SHA256SUMS和日志。report应包含源码/工具链、四组件及slice、模型lock和文件hash、native库及引擎身份、测试/公证请求和最终包checksum；不能只写一个App版本号。

## 6. 持续验证与发行完成定义

PR检查跑两个真实target的构建/静态与组件检查，资源使用同一份固定公共模型缓存。发行增加两个原生平台的实际包验收；Apple Silicon上的Rosetta可做补充，不代替Intel原生资源和行为验证。GitHub当前列有macos-15（arm64）及macos-15-intel（x64）；应固定标签并检查实际arch/SDK，不能用macos-latest暗推架构。[官方runner矩阵](https://github.com/actions/runner-images/blob/main/README.md)。

必须覆盖：

1. 禁网、空用户数据目录、已安装App，Core在后台/GUI/CLI三种入口都能找到同一随包资源，真实ASR/VAD/embed可运行；不靠开发缓存、旧下载包或GUI先设置路径。
2. 把App放到含空格或中文的路径、重命名后重启、只读DMG资源检查；不存在固定/Applications路径和当前目录依赖。
3. 两架构四组件slice/同版本SHA/epoch身份；引擎实际版本与native锁记录相符，原生库最低OS/CPU指令要求有证据。
4. 少文件、错hash、错误slice、helper版本不一致和不完整缓存均不能生成合格发行包；完整包真实模型加载要通过，不能只验文件存在或只测静音。
5. 从0.1.7带旧下载包的安装升级到新包，经过完整退出/替换/重启，历史结果归属及用户意图保留，新计算采用随包模型，旧包不干扰。
6. 最终含模型DMG的签名、公证、票据、Gatekeeper和挂载内容校验；模型和所有许可在签名前已纳入资源封印，最终checksum取装订后文件。
7. Apple Silicon验证资源位置改变前后原始识别输出；采用新Universal原生包时ARM/Intel均完整执行现有682段回归与对应资源观测，单独报告质量/时延/实际物理足迹，不把arm测量移作Intel结果。数据/参考和旧报告保持冻结，只重跑受影响的已选X-ASR配置；具体质量准入阈值在开发契约定稿时写明。

架构设计尚未执行以上跨架构验收。Intel可构建、Universal文件存在和Intel已完整可用是不同事实；Intel原生证据缺失时，不能把Universal正式发行标成两平台验收完成。

## 7. Windows与本轮待确认项

Windows是用户明确排除的独立适配任务。本设计只让模型lock与构建编排不绑定某个CPU；将来Windows复用相同模型身份和“App提供资源、Core负责就绪、worker只读计算”的职责，再增加其安装布局/平台工具。不提前新增Windows脚本、平台接口框架或空实现。

两项核心建议仍待用户确认：**直接读取Core随包只读资源；Mac默认单一Universal2发行**。它们已形成具体设计，未当作已批准生产变更。后续实施PRD还须将Intel实测、最低系统验收和模型回归准入条件写成明确门槛，不能据本建议稿直接宣告可发布。

模型及原生组件的许可来源应跟随资产lock固定并随包保存；已核实[X-ASR Apache-2.0](https://github.com/Gilgamesh-J/X-ASR/blob/838297cd47fed858e6cf72eaf5a52f948a3edd73/LICENSE)、[Silero MIT](https://github.com/snakers4/silero-vad/blob/master/LICENSE)、[CampPlus官方模型卡](https://www.modelscope.cn/models/damo/speech_campplus_sv_zh-cn_16k-common)与[3D-Speaker Apache-2.0](https://github.com/modelscope/3D-Speaker/blob/main/LICENSE)。固定许可文件、native第三方声明的完整落盘和校验仍属于构建实现义务。
