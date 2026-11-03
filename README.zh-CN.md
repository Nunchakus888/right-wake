# Waking

**Wake the right thread. 把事件送回正确的工作会话。**

[English](README.md)

Waking 在工作事件到来时，唤醒正确的**已有 Agent 会话**。把 Linear、GitHub、GitLab 中的新讨论送回正在负责这项工作的会话，让后续工作沿着原有上下文继续。

- **有事件才唤醒，减少无效 token 消耗。** 由 Webhook 触发工作；空闲且没有待处理任务时，Waking 不调用模型，不做定时模型轮询。
- **把事件归到对应会话。** 优先复用讨论级、Issue 级归属绑定；归属未知时，可由事件触发语义路由，比较权限范围内的上下文卡片，推荐一个已有会话供一次性认领。
- **续接原工作上下文。** 始终定位同一个 Host 和 Thread，通过支持的宿主继续其保留的原始历史。上下文卡片帮助找到目标，工作仍在原会话中继续，不自动另开替代会话。
- **支持本地与自己的云端服务器。** 可以在本机执行、在 VPS 上执行，或让云端 Relay 在线接收事件，本机恢复在线后继续原工作。

Waking 以一个原生 Rust 二进制提供 CLI 和 Codex 插件，**内嵌 SQLite，无需另装数据库服务**。归属绑定持久记录 **工作对象 → Host → Thread**，切换窗口不会改变目标。宿主忙碌、暂停或离线时保留队列，归属不明时等待认领；对外回复默认是草稿，经独立审批后才能发送。

从 [Codex 插件](#在-codex-中使用)、[Linear 接入](docs/linear.md)或[部署说明](docs/deployment.md)开始。

## 当前状态

**[v0.1.1 实验性预览版已发布](https://github.com/Nunchakus888/waking/releases/tag/v0.1.1)。** 包含内嵌 SQLite 3.54.1、四个平台的原生安装包，以及 Linux amd64/arm64 Docker 镜像和 Compose 配置。每个原生平台均通过 **94 项测试，0 失败、0 忽略**；两个 Linux 平台的真实 Docker/Compose 检查也已通过。具体运行证据见[验证记录](docs/releases/v0.1.1-verification.md)，真实平台集成边界仍见[验收记录](docs/acceptance.md)。

已记录的 v0.1.0 验证中，官方 **Codex CLI 0.166.0** 通过了无模型宿主探测，以及隔离空配置中的本地仓库和**真实 GitHub marketplace URL** 插件安装；缓存 MCP 的启动、工具发现、setup 和 status 也已通过。真实模型续接、桌面并发和平台授权读写仍需分别验收。自动向 Codex 提交工作要求支持的宿主、已落实的独占写入条件与工具权限隔离，详见[宿主兼容性](docs/host-compatibility.md)。

Waking 是独立项目，不是 OpenAI、Linear、GitHub、GitLab 或 Anthropic 的官方产品。

## 安装

**续接本机原会话用原生安装；VPS Relay 用 [Docker + Compose](deploy/README.md)。** 原生安装只需 shell、`curl`、`tar` 和 SHA-256 工具，**无需 Node.js、Python、Docker、Rust 或 sudo**，支持 macOS / glibc Linux 的 arm64 与 x86_64。安装器校验压缩包和版本，安装到 `~/.local/bin`，不自动启动服务。

安装固定版本 `v0.1.1`：

```sh
curl --proto '=https' --tlsv1.2 --fail --location \
  --output waking-install.sh \
  https://github.com/Nunchakus888/waking/releases/download/v0.1.1/install.sh
# 查看下载的安装脚本后执行。
sh waking-install.sh --version v0.1.1
export PATH="$HOME/.local/bin:$PATH"
waking --version
waking setup
waking --json doctor
```

需要开发时，可在仓库目录从源码安装：

```sh
cargo install --locked --path .
waking setup
waking --json schema
waking --json doctor
```

只有源码编译需要 Rust。本机配置和 SQLite 数据默认保存在 `~/.local/share/waking`，可通过 `WAKING_DATA_DIR` / `--data-dir` 指定；Docker 将其保存在挂载至 `/data` 的命名卷，无需数据库容器。[VPS 指南](deploy/README.md)提供 amd64/arm64 镜像包的 `docker load` 与显式启动 Compose 步骤。[安装与生命周期](docs/installation.md)说明备份、升级、服务和卸载。

## 在 Codex 中使用

添加仓库 marketplace：

```sh
codex plugin marketplace add https://github.com/Nunchakus888/waking
codex plugin add waking@waking --json
```

也可以在支持该插件格式的本地 Codex 中通过 `/plugins` 安装 **Waking**。安装后说：

> 为这个工作对象设置 Waking，让事件到来后继续它已有的 Codex 工作会话。先检查 schema、doctor 和现有绑定，确认准确目标，默认只生成草稿。

插件包含设置、归属绑定、状态与草稿审阅技能，以及本地 MCP 工具。插件引导可能打开新的设置对话，这个对话不会被自动当作工作目标。后台服务必须明确启动。详细流程与干净安装验证见[插件说明](docs/plugin.md)。

## 跑通第一条事件

安装后可以立即运行无需凭证的演示：

```sh
waking --json demo
```

它会验签、落盘、沿已有绑定续接两条**模拟会话**并生成草稿；不调用真实模型或平台。真实 Codex 原会话续接单独验收。

最小闭环需要一个合法事件源、一个可访问的事件接收入口、一个可安全续接的执行 Host，以及一个指向该 Host 已有工作会话的显式绑定。

**已经连接 Linear MCP？** 这提供的是访问 Linear 的工具能力。Waking 仍需要单独配置签名 Webhook、可达的接收入口、事件源和 Thread 绑定；连接 MCP 本身不会建立这条事件投递链路。具体步骤见 [Linear 接入说明](docs/linear.md)。

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

| 模式 | 接收事件 | 执行工作 | 会话历史 |
| --- | --- | --- | --- |
| 本地执行 | 经可达 HTTPS 入口转发到本地接收端 | 本机上支持的宿主 | 续接本机已有 Thread |
| VPS / 自托管云端执行 | 自己服务器上的接收端 | 同一服务器上支持的宿主 | 续接该服务器已能访问的 Thread |
| 云端 Relay + 本地执行 | 自托管在线 Relay | 本机主动连接 Relay，在本地宿主执行 | 原会话历史始终保留在本机 |

这里的云端指**自己的 VPS 或服务器**，当前没有 OpenAI 托管 Codex Cloud 适配器。Docker Relay 接收并保存事件，本地原生执行器在线后续接原 Thread；原会话历史和登录不会迁入容器。如果希望工作也在 VPS 上执行，则需要该 VPS 上支持的宿主及已有会话。

直接 Webhook 需要可达的 HTTPS 入口，否则要明确配置自托管 Relay；产品不假定已有公共托管 Relay。尽可能把平台 API 凭证和 Agent 登录保留在执行 Host，Relay 能读取其接收的事件正文。服务与 Relay 的配置路径见[部署及运行说明](docs/deployment.md)。

## 开发与验证

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/waking
python3 scripts/check-plugin.py
```

Python 用于仓库与发布检查，以及独立审阅设备上的可选工具，不是已安装 Waking 的运行时依赖。Rust 工具链固定在 [rust-toolchain.toml](rust-toolchain.toml)。日常 CI 只跑 Linux；本仓库 main 的成功 CI 会为尚未发布的版本运行四平台原生验证并发布预览版，已发布版本跳过该矩阵。仍支持 tag 和手动 release，具体保护条件见[发布流程](docs/installation.md#release-process)。

## 文档与许可

- [Linear Webhook 接入与原会话续接](docs/linear.md)
- [本地、VPS 与云端 Relay 部署](docs/deployment.md)
- [产品定义与用户旅程](docs/product.md)
- [技术架构与上下文路由](docs/architecture.md)
- [状态与恢复](docs/states.md)
- [安装、升级与卸载](docs/installation.md)
- [Codex 插件与 Agent 流程](docs/plugin.md)
- [32 项验收及证据](docs/acceptance.md)
- [配置与执行器示例](examples/README.md)
- [独立审阅与发送](docs/approvals.md)
- [官方资料与能力边界](docs/references.md)
- [参与贡献](CONTRIBUTING.md) · [安全报告](SECURITY.md) · [Apache 2.0 许可证](LICENSE)

本项目独立用 Rust 实现，参考 [WakeWire](https://github.com/glenncalleja/wakewire) 的事件桥问题定义和适配经验，不冒充其官方版本，也不据此宣称实现兼容。
