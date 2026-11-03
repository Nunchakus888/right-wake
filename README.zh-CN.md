# Waking

**Wake the right thread. 把事件送回正确的工作会话。**

[English](README.md)

Waking 是一个原生 Rust 事件桥：接收工作事件，判断它归属哪项工作，把新增信息送回**已有的本地 Agent 会话**。它面向任务在 Linear、GitHub、GitLab，分析与代码工作分散在多个 Codex 会话中的场景。

归属绑定持久记录“工作对象 → Host → Thread”。切换窗口不会改绑；原会话忙碌或离线时等待；无法确定归属时保留待认领，不自动另开工作会话。对外回复默认是草稿。

## 当前状态

**v0.1.0 是正在验证的首版实现。** [验收记录](docs/acceptance.md)分别列出实现、测试、发布和真实平台验收。假执行器测试通过，不代表已经验证某个真实 Codex 桌面版本。

仓库提供四平台发布流水线，但流水线存在不等于 release 已经发布。运行下载命令前，请检查 [Releases](https://github.com/Nunchakus888/waking/releases)。官方 Codex CLI 0.166.0 已通过无模型宿主探测，以及隔离空配置中的本地 marketplace 插件安装。真实原工作会话的模型续接、桌面并发、从 GitHub 下载插件、真实平台授权读写仍需分别验收。

## 核心行为

- 通过 Webhook 接收并校验事件；没有事件时不调用模型轮询平台。
- 先查讨论级和 Issue 级显式绑定，再使用会话目录中的证据判断候选。
- 本地保存队列、绑定和上下文卡片。卡片用于找到会话，原始工作历史仍由目标会话保存。
- 同一个 Host/Thread 串行处理；等待、失败和结果不确定都有可查看的状态。
- 草稿同时保留正文、发送位置、身份与版本。对外发送需要配置的独立审批。
- CLI 与 Codex 仓库插件使用同一个原生二进制。

来源、宿主、语义路由和部署的能力边界见[技术设计](docs/architecture.md)和[验收记录](docs/acceptance.md)。Waking 是独立项目，不是 OpenAI、Linear、GitHub、GitLab 或 Anthropic 的官方产品。

## 安装

普通用户只需要系统已有的 shell、`curl`、`tar` 和 SHA-256 工具，**无需 Node.js、Python、Docker、Rust 或 sudo**。目标平台为 macOS / glibc Linux 的 arm64 与 x86_64。安装脚本下载固定版本，校验压缩包与可执行文件版本，安装到 `~/.local/bin`；不自动启动服务，也不修改 Codex 配置。

`v0.1.0` 正式发布后：

```sh
curl --proto '=https' --tlsv1.2 --fail --location \
  --output waking-install.sh \
  https://github.com/Nunchakus888/waking/releases/download/v0.1.0/install.sh
# 查看下载的安装脚本后执行。
sh waking-install.sh --version v0.1.0
export PATH="$HOME/.local/bin:$PATH"
waking --version
waking setup
waking --json doctor
```

在首个 release 发布前，或者需要开发时，可在仓库目录从源码安装：

```sh
cargo install --locked --path .
waking --json schema
waking --json doctor
```

只有源码编译需要 Rust。[安装说明](docs/installation.md)包含离线包、平台要求、升级、回退、服务与卸载。

## 在 Codex 中使用

添加仓库 marketplace：

```sh
codex plugin marketplace add https://github.com/Nunchakus888/waking
codex plugin add waking@waking --json
```

也可以在支持该插件格式的本地 Codex 中通过 `/plugins` 安装 **Waking**。安装后说：

> 帮我设置 Waking，把这个工作对象后续的讨论交给它已有的 Codex 工作会话处理。默认只生成草稿。

插件包含设置、归属绑定、状态与草稿审阅技能，以及本地 MCP 工具。插件引导可能打开新的设置对话，这个对话不会被自动当作工作目标。后台服务必须明确启动。详细流程与干净安装验证见[插件说明](docs/plugin.md)。

## 跑通第一条事件

最小闭环需要一个合法事件源、一个可安全续接的本地 Host，以及一个指向已有工作会话的显式绑定。

```sh
waking --json schema
waking --json doctor
waking --json source list
waking --json host list
waking --json bindings
```

通过安装版本的 schema 与各命令 `--help` 配置来源和 Host。凭证使用环境变量名引用，不把 token 值发进聊天或放入命令参数。确认工作对象的完整身份与真实 Thread，再读取绑定检查。使用带有凭证环境的 `waking serve`，或明确安装用户级服务；将平台 Webhook 指向可访问的接收端，在获准的测试工作对象中触发一条无害事件。

分别查看各阶段：

```sh
waking --json status
waking --json events list
waking --json jobs list
waking --json drafts list
```

“已接收”“原会话已完成”“平台已收到回复”是三种结果。[产品流程](docs/product.md)和[状态说明](docs/states.md)给出每一步阻塞时的下一项操作。五分钟激活是外部权限就绪后的设计目标，目前不作为已测结论。

## 部署模式

| 模式 | 接收事件 | 执行工作 | 原本地会话历史 |
| --- | --- | --- | --- |
| 本地 | 可被平台访问的本地 Webhook 入口 | 本机 | 通过支持的本地宿主续接 |
| 混合 | 自托管在线 Relay | 本机主动向 Relay 连接 | 保留在本机 |
| 自托管云端 | 自己的服务器 | 同一服务器 | 必须已在该服务器存在，不自动继承本地聊天 |

产品不假定已有公开托管 Relay。直接 Webhook 需要可访问的 HTTPS 入口，否则需要明确配置自托管 Relay。尽可能把平台 API 凭证和 Agent 登录保留在执行 Host。Relay 能读取其接收的 Webhook 明文，不宣传服务端不可见或端到端加密。

## 开发与验证

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/waking
python3 scripts/check-plugin.py
```

Python 只用于仓库与发布检查，不是产品运行时依赖。Rust 工具链固定在 [rust-toolchain.toml](rust-toolchain.toml)。日常 CI 只跑 Linux；本仓库 main 的成功 CI 会为尚未发布的版本运行四平台原生验证并发布预览版，已发布版本跳过该矩阵。仍支持 tag 和手动 release，具体保护条件见[发布流程](docs/installation.md#release-process)。

## 文档与许可

- [产品定义与用户旅程](docs/product.md)
- [技术架构与上下文路由](docs/architecture.md)
- [状态与恢复](docs/states.md)
- [安装、升级与卸载](docs/installation.md)
- [Codex 插件与 Agent 流程](docs/plugin.md)
- [32 项验收及证据](docs/acceptance.md)
- [配置与执行器示例](examples/README.md)
- [官方资料与能力边界](docs/references.md)
- [参与贡献](CONTRIBUTING.md) · [安全报告](SECURITY.md) · [Apache 2.0 许可证](LICENSE)

本项目独立用 Rust 实现，参考 [WakeWire](https://github.com/glenncalleja/wakewire) 的事件桥问题定义和适配经验，不冒充其官方版本，也不据此宣称实现兼容。
