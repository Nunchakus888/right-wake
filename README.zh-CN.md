# RightWake

**Wake the right Agent thread.**

事件与任务唤醒正确的已有 Agent 会话，  
让工作沿原有上下文继续。

[English](README.md)

- **有事件才唤醒，减少无效 token 消耗。** 由 Webhook 触发工作；空闲且没有待处理任务时，RightWake 不调用模型，不做定时模型轮询。
- **把事件归到对应会话。** 优先复用讨论级、Issue 级归属绑定；归属未知时，可由事件触发语义路由，比较权限范围内的上下文卡片，推荐一个已有会话供一次性认领。
- **续接原工作上下文。** 始终定位同一个 Host 和 Thread，通过支持的宿主继续其保留的原始历史。上下文卡片帮助找到目标，工作仍在原会话中继续，不自动另开替代会话。
- **支持本地与自己的云端服务器。** 可以在本机执行、在 VPS 上执行，或让云端 Relay 在线接收事件，本机恢复在线后继续原工作。

RightWake 以一个原生 Rust 二进制提供 CLI 和 Codex 插件，**内嵌 SQLite，无需另装数据库服务**。归属绑定持久记录 **工作对象 → Host → Thread**，切换窗口不会改变目标。宿主忙碌、暂停或离线时保留队列，归属不明时等待认领；对外回复默认是草稿，经独立审批后才能发送。

从 [Codex 插件](#在-codex-中使用)、[Linear 接入](docs/linear.md)或[部署说明](docs/deployment.md)开始。

## 当前状态

当前分支准备 **v0.3.1**：增加固定 relay 连接配置、断线恢复和原生 relay worker 服务管理，复用 0.3.0 的协议与 SQLite。发布前，新命令需使用源码构建；下方已发布的 0.3.0 安装包尚不包含这些改动。参见 [0.3.1 改动及边界](docs/releases/v0.3.1.md)和[验证记录](docs/releases/v0.3.1-verification.md)。

**[v0.3.0 实验性预览版已发布](https://github.com/Nunchakus888/right-wake/releases/tag/v0.3.0)**，新增按 Linear 指派人监听与可选的 Codex 队列投递。 自动接收自己名下 issue 的新评论，以及新指派给自己的任务，无需每条评论加 mention，也不把今天的任务列表固化为永久范围。归属变化更新本地持久记录；归属未知时暂存，不调用模型。四个原生平台均通过 **129 项测试，0 失败、0 忽略**；两个 Linux 平台的 Docker / Compose 检查也已通过。具体运行与安装证据见 [v0.3.0 验证记录](docs/releases/v0.3.0-verification.md)；之前的结果保留在 [v0.2.0 记录](docs/releases/v0.2.0-verification.md)。

自动向 Codex 投递可选两种方式：直接 tool-output 要求实际独占写入；实验性的 Queue 方式使用原会话所属 App Server 的原子入队。两者都需要落实工具与凭证权限边界，Queue 还需要同一配置目录的上下文 MCP 工具。真实桌面原会话续接与平台授权读写仍需分别验收，详见[宿主兼容性](docs/host-compatibility.md)和[验收记录](docs/acceptance.md)。

RightWake 是独立项目，不是 OpenAI、Linear、GitHub、GitLab 或 Anthropic 的官方产品。

## 安装

命令为 `right-wake`，Codex 插件为 `right-wake@right-wake`。从 0.2.0 升级时，先备份，再复用原配置目录、事件源、绑定及接入基线；数据库采用增量升级。不安装旧名称别名。

**续接本机原会话用原生安装；VPS Relay 用 [Docker + Compose](deploy/README.md)。** 原生安装只需 shell、`curl`、`tar` 和 SHA-256 工具，**无需 Node.js、Python、Docker、Rust 或 sudo**，支持 macOS / glibc Linux 的 arm64 与 x86_64。安装器校验压缩包和版本，安装到 `~/.local/bin`，不自动启动服务。

确认[发布状态](docs/releases/v0.3.0-verification.md)后，安装固定版本 **v0.3.0**：

```sh
curl --proto '=https' --tlsv1.2 --fail --location \
  --output right-wake-install.sh \
  https://github.com/Nunchakus888/right-wake/releases/download/v0.3.0/install.sh
# 查看下载的安装脚本后执行。
sh right-wake-install.sh --version v0.3.0
export PATH="$HOME/.local/bin:$PATH"
right-wake --version
right-wake setup
right-wake --json doctor
```

需要开发时，可在仓库目录从源码安装：

```sh
cargo install --locked --path .
right-wake setup
right-wake --json schema
right-wake --json doctor
```

只有源码编译需要 Rust。本机配置和 SQLite 数据默认保存在 `~/.local/share/right-wake`，可通过 `RIGHT_WAKE_DATA_DIR` / `--data-dir` 指定；Docker 将其保存在挂载至 `/data` 的命名卷，无需数据库容器。[VPS 指南](deploy/README.md)提供 amd64/arm64 镜像包的 `docker load` 与显式启动 Compose 步骤。[安装与生命周期](docs/installation.md)说明备份、升级、服务和卸载。

## 在 Codex 中使用

添加仓库 marketplace：

```sh
codex plugin marketplace add https://github.com/Nunchakus888/right-wake
codex plugin add right-wake@right-wake --json
```

也可以在支持该插件格式的本地 Codex 中通过 `/plugins` 安装 **RightWake**。接入 Linear 时，在**准备续接的本地已有工作会话**中调用，把示例 Issue 替换成真实对象：

> 使用 $right-wake-setup 安装并配置 RightWake，把 Linear 的 ENG-123 绑定到当前这个已有工作会话。复用已授权连接和现有配置，启动所需的本地接收服务，逐步验证。默认只生成草稿；遇到确实缺失的条件再告诉我。

如果要监听个人 Linear 收件范围，可以直接说：

> 使用 $right-wake-setup 升级并配置 RightWake，监听我名下 issue 的新评论以及新指派给我的任务。保留原配置、已有绑定和已保存的接入基线，将事件分配给对应的已有工作会话。完成能完成的配置，准备缺失的管理员配置交接，并分别报告事件接收与原会话执行结果。默认只生成草稿。

现有 setup 技能已包含 [Linear 配置流程](docs/linear.md#agent-led-setup)：发现可复用条件、安装、配置并检查结果。权限、接收入口与准确的 Host/Thread 齐备时，Agent 可在一次调用中推进。已保存的 issue 列表用于初始化过滤依据，不代表已经订阅，也不会把所有任务绑定到设置对话。缺少管理员权限或公网地址时，先完成本地可执行部分，再给出具体交接材料。

插件包含三个技能和十二个本地 MCP 工具，安装配置由 Skill 编排原生 CLI 命令。安装插件不通过隐藏钩子启动服务；引导新建的设置对话也不会被自动当作工作目标。详见[插件说明](docs/plugin.md)。

已安装 `right-wake` 后，技能更新时让本地 Agent [刷新插件包](docs/plugin.md#refresh-an-existing-installation)，并检查原生二进制满足该插件包的版本要求。

## 跑通第一条事件

安装后可以立即运行无需凭证的演示：

```sh
right-wake --json demo
```

它会验签、落盘、沿已有绑定续接两条**模拟会话**并生成草稿；不调用真实模型或平台。真实 Codex 原会话续接单独验收。

最小闭环需要一个合法事件源、一个可访问的事件接收入口、一个可安全续接的执行 Host，以及一个指向该 Host 已有工作会话的显式绑定。

**已经连接 Linear MCP？** 这提供的是访问 Linear 的工具能力。RightWake 仍需要单独配置签名 Webhook、可达的接收入口、事件源和 Thread 绑定；连接 MCP 本身不会建立这条事件投递链路。具体步骤见 [Linear 接入说明](docs/linear.md)。

```sh
right-wake --json schema
right-wake --json doctor
right-wake --json source list
right-wake --json host list
right-wake --json bindings
```

通过安装版本的 schema 与各命令 `--help` 配置来源和 Host。凭证使用环境变量名引用，不把 token 值发进聊天或放入命令参数。确认工作对象的完整身份与真实 Thread，再读取绑定检查。使用带有凭证环境的 `right-wake serve`，或明确安装用户级服务；将平台 Webhook 指向可访问的接收端，在获准的测试工作对象中触发一条无害事件。

分别查看各阶段：

```sh
right-wake --json status
right-wake --json events list
right-wake --json jobs list
right-wake --json drafts list
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

需要**固定入口及电脑离线期间收件**时，使用受控的 HTTPS/WSS 云端 relay、持久磁盘和本机原生 worker，保留原 profile 与已配对设备。0.3.1 的 `relay configure` 保存地址，`service install --mode relay-connect` 使用现有用户服务管理 worker。固定域名、服务器和部署授权仍需真实具备；匿名 Quick Tunnel 仅适合临时试验。联网和重连成功不证明 Desktop 原宿主连接或自动任务权限隔离已经完成。[长期接入步骤](docs/deployment.md#cloud-relay-plus-local-worker)。

## 开发与验证

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/right-wake
python3 scripts/check-plugin.py
```

Python 用于仓库与发布检查，以及独立审阅设备上的可选工具，不是已安装 RightWake 的运行时依赖。Rust 工具链固定在 [rust-toolchain.toml](rust-toolchain.toml)。日常 CI 只跑 Linux；本仓库 main 的成功 CI 会为尚未发布的版本运行四平台原生验证并发布预览版，已发布版本跳过该矩阵。仍支持 tag 和手动 release，具体保护条件见[发布流程](docs/installation.md#release-process)。

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
