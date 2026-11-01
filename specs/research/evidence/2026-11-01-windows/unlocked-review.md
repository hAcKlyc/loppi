# 解锁后源码与原生验收裁决

范围为f1b1db136485c0ff2104025c6fa7361f1bc2b520之后的轨道满高、Windows原生同色标题栏、DPI截图与真实入口修复。之前已审查的Windows集成不重复卷入。正式三包与发布核验仍是后续门槛。

三位全新上下文reviewer分别读取仓库requirements/adversarial/architecture技能与同一冻结契约；两路先审、root裁决修复、最后独立架构审查。源码和定向修正全部PASS。原始数据到达后，两路复核关闭原生证据缺口。

- 首轮未发现生产阻塞；托盘旧同步退出断言属于受影响测试缺口，改为验证不内联退出、泵送既有消息后再观察Quit。
- 实机发现拖动后inhibit未解除：复用kind7在未拖动的鼠标离开时重置；实机点击/拖动后重入均可展开。
- 实机发现右侧展开拖到左侧，保存坐标正确但可见中心暂偏96物理像素：释放锚点更新后，立即复用现有collapsed布局，再发送结束/保存；双向即时显示、重入、重启与单击复测通过。两路和最终架构均定向PASS。

静态/浏览器/Mac事实见邻目录timeline-fill。Windows原始对象为[unlocked-native-acceptance.json](unlocked-native-acceptance.json)，实际GDI裁剪图为[native-titlebar.webp](native-titlebar.webp)。并非WebView内部截图。具体尺寸/裁剪位置/缩放/合成内容范围保留在原报告；图片名早期包含onboarding，但交付图为主页，真正首次设置另有原始记录。

传输来源为证据专用Git提交8ca9966710a05eae8fd9ca71e0567cc0f55c3a6c，仅含三件证据，没有生产提交。JSON行尾转换精确对应两个原摘要；图片原字节匹配。参见[传输核对](unlocked-transport.json)与当前LF文件[校验清单](SHA256SUMS)。早先文本base64少1字符，未接受或猜补。

生产源六项哈希直接匹配。entry.rs的root单行filter改回Native三行闭包，即精确复现19608effab90172b2e1e2138b3b61913ffcfe3de3a801ff1b1549d78ce069faa，仅rustfmt换行差异。

保留以下原始事实并明确解释：

- DwmGetWindowAttribute对34/35/36返回E_INVALIDARG，不伪称读回成功。这三项为设置属性；实际caption/画布原始RGB均250/248/242，原生窗口操作已实测。
- 一轮键盘录制中Quit的pass:false来自“所有进程必须exit0”的额外夹具条件：GUI/Core/采集子进程0，全部子进程结束，单个worker1，未人工强杀、无Escape绕行。现有analysis.rs OwnedChild::drop取消时kill/wait，不要求模型子进程全部0；单个退出原因未捕获，不能把既有取消代码冒称本次原因。两位独立reviewer判为非阻塞观察，原始结果不改。
- 原报告root_added_assertions中“kind7 before hover-leave”是描述误写；真实源码先kind1离开、再kind7解除抑制，回归观察kind7最后，实机重入通过。解释纠正，原始对象不改。

本轮源码与原生验收已闭环。最终干净源码的三包、实际新包验收和远端校验仍须完成，不以dirty开发运行时冒充正式发行物。
