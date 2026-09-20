# Abomination81 Copybot

> **Their move. Your execution.**

**他们出手，你来执行。**

> **Built by [Abomination81](https://github.com/Abomination81) · [X @Abomination81](https://x.com/Abomination81)**

**由 [Abomination81](https://github.com/Abomination81) 开发 · [X @Abomination81](https://x.com/Abomination81)**

![Abomination81 Copybot — low-latency copy execution, built by Abomination81](docs/assets/copybot-hero.svg)

> Abomination81 Copybot is a self-hosted Polymarket copy-trading engine. It watches configured leaders, sizes entries against your limits, signs orders locally, and follows the configured exit rules. A Rust execution engine handles the trading path; an independent Python guardian and a browser dashboard handle oversight.

Abomination81 Copybot 是一款自行托管的 Polymarket 跟单交易引擎。它会监控配置好的带单账户，根据你设定的限额确定入场规模，在本地签署订单，并按配置的退出规则执行。交易链路由 Rust 执行引擎负责，另有独立的 Python 守护程序和浏览器仪表盘负责监控。

> **Private preview · Linux deployment · Rust + Python · Bring your own wallet and feeds**

**私有预览版 · 部署于 Linux · Rust + Python · 需自备钱包和数据源**

> [Visual project guide](docs/index.html) · [Installation](docs/INSTALL.md) · [Configuration](docs/CONFIGURATION.md) · [Operations](docs/OPERATIONS.md) · [Release scope](docs/RELEASE-SCOPE.md)

[项目图文指南](docs/index.html) · [安装说明](docs/INSTALL.md) · [配置说明](docs/CONFIGURATION.md) · [运维指南](docs/OPERATIONS.md) · [发布范围](docs/RELEASE-SCOPE.md)

> ## What it does

## 功能概览

> - **Fast discovery.** Multiple WebSocket feeds, duplicate-signal handling, and optional transaction-pool discovery.

- **快速发现。** 支持多个 WebSocket 数据源、重复信号处理，以及可选的交易池发现功能。

> - **Controlled sizing.** Per-lane budgets, price bands, copy percentages, and configurable compounding.

- **仓位可控。** 可按通道设置预算、价格区间、跟单比例和复投方式。

> - **Local execution.** Local order signing, connection racing, and configured taker or maker behavior.

- **本地执行。** 在本地签署订单，竞速使用多个连接，并按配置采用吃单或挂单方式。

> - **Exit management.** Sell handling, reconciliation, and restart recovery through the existing engine.

- **退出管理。** 通过现有引擎处理卖出、账务核对和重启恢复。

> - **Independent oversight.** Guardian, watcher, fillwatch, and buywatch run separately from the trading loop.

- **独立监控。** Guardian、watcher、fillwatch 和 buywatch 均独立于交易循环运行。

> - **One operator view.** Pool balances, lanes, positions, execution state, and incidents in the included dashboard.

- **统一操作视图。** 随附的仪表盘集中展示资金池余额、通道、持仓、执行状态和异常事件。

> This is execution software, not a source of profitable leaders. Copying a profitable account does not guarantee the same prices or results. In particular, the current runtime-lane default can flatten a position on a leader's first sell; this is not necessarily a proportional mirror of every trim. Read the execution settings before enabling a lane.

这是一套交易执行软件，并不负责提供能够盈利的带单账户。即使跟随的是盈利账户，也不能保证获得相同的成交价格或结果。尤其需要注意：按目前运行时通道的默认设置，带单账户第一次卖出时，系统可能直接平掉整个仓位，而不一定按比例复制对方的每次减仓。启用通道前，请先读懂执行配置。

> ## Start here

## 快速开始

> On a Linux host with Git, a current stable Rust toolchain, and Python 3.11 or newer:

准备一台 Linux 主机，并安装 Git、当前稳定版 Rust 工具链以及 Python 3.11 或更高版本，然后执行：

```sh
git clone https://github.com/Abomination81/copybot.git
cd copybot
cargo build --locked --release --manifest-path hot/Cargo.toml --bin copybot-hot
cp deploy/copybot2.example.toml deploy/copybot2.toml
cp deploy/copybot.env.example deploy/copybot.env
chmod 600 deploy/copybot2.toml deploy/copybot.env
```

> The repository is private, so cloning requires access. Edit the two local files using the [installation guide](docs/INSTALL.md). The example intentionally has no usable wallet, leader, or feed and cannot trade as supplied. Do not paste credentials into issues, screenshots, terminal arguments, or AI chats.

该仓库为私有仓库，必须具备访问权限才能克隆。请参照[安装说明](docs/INSTALL.md)编辑这两个本地文件。示例配置特意没有提供可用的钱包、带单账户或数据源，因此直接使用时无法交易。不要把凭据粘贴到议题、截图、终端参数或 AI 对话中。

> There is no automatic live-trading installer. Building does not start the bot, and starting a fresh configuration does not arm a lane. Existing operator state can survive restarts—do not treat a restart as a disarm.

项目不提供自动开启实盘交易的安装程序。完成构建并不会启动机器人，使用一份全新配置启动程序，也不会让通道进入可交易状态。已有的操作状态可能在重启后继续保留——不要把重启当作解除交易状态的手段。

> ## Repository map

## 仓库结构

| Path / 路径 | Purpose / 用途 |
| --- | --- |
| `hot/src/` | Rust engine, accounting, execution, controls, and embedded regression tests<br>Rust 引擎、记账、交易执行、控制逻辑和内嵌回归测试 |
| `hot/assets/` | Dashboard chart library, with upstream license retained<br>仪表盘图表库，保留了上游许可证 |
| `deploy/dashboard/pool.html` | Actual operator dashboard; served by the bot<br>实际使用的操作仪表盘，由机器人提供服务 |
| `deploy/*.py` | Independent guardian and observation processes<br>独立的守护与观察进程 |
| `deploy/*.service`, `deploy/*.timer` | Linux service templates<br>Linux 服务模板 |
| `deploy/*.example.*`, `deploy/*.env.example` | Credential-free configuration templates<br>不含凭据的配置模板 |
| `docs/` | Project page and operator instructions; no live connection<br>项目页面和操作说明，不连接实盘环境 |

> ## Verify the source

## 验证源代码

```sh
cargo test --locked --manifest-path hot/Cargo.toml --lib --bin copybot-hot
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements-dev.txt
.venv/bin/python -m pytest deploy/ -q
python3 scripts/check_release.py
```

> Only self-contained regression tests are distributed. Tests containing private chain recordings and their payloads stay out of this repository. The release preparation checks the exported Rust runtime against the local source, excluding comments and test-only code. See [release scope](docs/RELEASE-SCOPE.md) for exactly what changed in packaging and what did not.

仓库只分发能够独立运行的回归测试。包含私有链上记录及其载荷的测试不会收入本仓库。准备发布时，检查程序会将导出的 Rust 运行时代码与本地源代码进行比对，但会排除注释和仅供测试使用的代码。有关打包过程中具体改动了什么、没有改动什么，请参阅[发布范围](docs/RELEASE-SCOPE.md)。

> ## Privacy and access

## 隐私与访问控制

> The repository contains no operator configuration, funded-wallet identities, leader list, portfolio snapshots, trading tapes, SSH keys, or previous Git history. Public protocol addresses and provider base URLs remain where the software needs them. An independent credential scan complements the release-specific privacy checks; neither is a promise that future commits cannot leak data.

该仓库不包含操作方配置、入金钱包身份、带单账户名单、投资组合快照、交易记录、SSH 密钥或此前的 Git 历史。软件运行所需的公开协议地址和服务商基础 URL 仍予保留。除发布专用的隐私检查外，项目还会独立扫描凭据；但这两项措施都不能保证今后的提交绝不会泄露数据。

> The dashboard is an operator control surface. Keep the backend on loopback and put authenticated private access in front of it. Do not expose its port directly to the internet. The documentation page is separate and has no trading controls or live data.

仪表盘是供操作人员使用的控制界面。后端应只监听本机回环地址，并在前端设置经过身份验证的私有访问入口。不要把仪表盘端口直接暴露到互联网。文档页面与仪表盘相互独立，不包含交易控制功能或实盘数据。

> ## Status and attribution

## 项目状态与归属声明

> Private source preview. No public release, uptime guarantee, latency benchmark, or return promise is implied. A project-wide open-source license has not been selected; this is not an MIT-licensed release. Third-party terms and notices remain applicable; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

本项目目前是私有源代码预览版，不代表已经公开发布，也不提供在线率保证、延迟基准或收益承诺。项目尚未选定适用于全项目的开源许可证；此次发布并不采用 MIT 许可证。第三方条款与声明仍然有效，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

> Independent software. Not affiliated with or endorsed by Polymarket.

本软件独立开发，与 Polymarket 无关联，也未获得其认可或背书。
