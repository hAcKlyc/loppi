# 稳定性与性能审视：实验材料

对应[优化方案](../../2026-10-16-hear-reliability-performance-optimization.md)。2026-10-16 在 Apple M5 / 24 GiB / macOS 26.6.2 上采集；源码 HEAD 为 `2c81ebd49601c85d89aa3529555aeddaa98d63f5`。

这些实验只验证局部机制，没有启动真实采集、访问云服务或修改用户数据库。不能据此宣布全天稳定性、真实语音质量或端到端响应时间达标。

## 材料

| 文件 | 内容与边界 |
| --- | --- |
| `local_aggregate_observation.json` | 通过只读连接获得的 schema 和聚合数字；没有保存音频、文字、人名或密钥。UTC 日期 10-15 对应北京时间 10-16。 |
| `sqlite_scaling.py` | 从源码提取音频块和会话表 schema、原待办领取 SQL；临时生成规模库。 |
| `sqlite_scaling_results.json` | 原查询与添加索引/计数原型后的 9 次暖缓存读耗时，含执行计划。 |
| `media_seal_bench.rs` | 直接引用生产 `media.rs`；只改变调用者输入粒度，测试整块与逐帧输入。 |
| `media_seal_results.csv` | 每组 16 次，交替执行顺序，报告统计去掉各组第 0 次。包含 push、seal、decode 耗时和文件大小。 |
| `media_bench.Cargo.lock` | 本次独立编码实验的依赖锁文件，未改变产品依赖。 |

## SQL 实验复现

在仓库根运行：

```sh
python3 specs/research/evidence/2026-10-16-reliability/sqlite_scaling.py > /tmp/hear-sql-scaling.json
```

归档说明（2026-10-31）：SQL脚本固定读取上述已审计提交，不随当前重构改变历史实验；这里的结果不代表0.1.14性能。

要求 Python 3。使用执行环境自带 SQLite，本次为 3.53.4，**不是产品 bundled SQLite 的性能基准**。所有数据库位于临时目录，结束后删除。脚本不会打开用户 hear 数据目录。

数据分布：两路交错的 5 秒块，每路每天一个 closed session，全部音频块被保留，媒体路径是合成字符串，没有创建音频文件。分别测无 pending 和仅末尾一个 pending；不是大积压压力模型。源码中的三个已有时间索引被保留，原型增加媒体路径、会话样本、pending 部分索引。容量计数只测读取，不包含维护计数的写入成本。

没有跑 ANALYZE，没有测冷启动或完整孤儿文件扫描。结果证明被测访问路径的规模成本与索引效果；真实业务分布、读写并发、迁移和端到端速度须另测。

## 编码实验复现

以下在仓库根执行；只在系统临时目录构建，默认使用本机已缓存依赖，不联网下载：

```python
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

root = Path.cwd()
evidence = root / "specs/research/evidence/2026-10-16-reliability"
with tempfile.TemporaryDirectory(prefix="hear-media-audit-") as location:
    tmp = Path(location)
    (tmp / "src").mkdir()
    (tmp / "Cargo.toml").write_text('''[package]
name="hear-media-audit"
version="0.0.0"
edition="2024"
[dependencies]
ogg="=0.9.2"
opus2={version="=0.4.0",features=["bundled"]}
uuid={version="1",features=["v4"]}
tempfile="3"
''')
    shutil.copyfile(evidence / "media_bench.Cargo.lock", tmp / "Cargo.lock")
    shutil.copyfile(evidence / "media_seal_bench.rs", tmp / "src/main.rs")
    import subprocess
    media = subprocess.check_output(["git", "show", "2c81ebd49601c85d89aa3529555aeddaa98d63f5:crates/loppi-core/src/media.rs"], cwd=root)
    (tmp / "media.rs").write_bytes(media)
    env = dict(os.environ, HEAR_AUDIT_MEDIA_RS=str(tmp / "media.rs"))
    subprocess.run(["cargo", "run", "--offline", "--locked", "--release",
                    "--manifest-path", str(tmp / "Cargo.toml")],
                   env=env, check=True)
```

本次编译器为 Rust 1.98.1。输入是 48 kHz、幅度 0.1、440 Hz、5 秒单声道合成音调；并非真人语音。每轮使用新临时目录和新 writer。创建 writer 的时间不包含在 push 计时中；seal 包括最后包、文件同步、rename、目录同步和 metadata；decode 单独计时并检查 240,000 样本长度。

逐帧方式仍使用同一个未修改的生产 writer，没有消除所有逐帧分配。它只是隔离整块输入时的头部搬移成本；不能当作最终优化实现或识别质量等价证明。当前 ASR 拼接已经使用逐帧输入。
