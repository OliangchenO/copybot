> # Installation

# 安装

> This guide installs a separate instance. It does not update or connect to another operator's deployment. The supported service templates target Linux with systemd. Python 3.11+ is required for `tomllib`; the shipped runtime observers use the standard library.

本指南用于安装一个独立实例，不会更新或连接到其他操作员的部署。受支持的服务模板面向使用 systemd 的 Linux 系统。`tomllib` 要求 Python 3.11 或更高版本；随附的运行时观察器使用标准库。

> ## 1. Build

## 1. 构建

> Install Git, Python 3.11+, a C compiler/linker, and stable Rust using your operating system's package manager and the [official Rust installer](https://www.rust-lang.org/tools/install).

使用操作系统的软件包管理器和 [Rust 官方安装程序](https://www.rust-lang.org/tools/install)，安装 Git、Python 3.11 或更高版本、C 编译器/链接器以及 stable Rust。

```sh
git clone https://github.com/Abomination81/copybot.git
cd copybot
cargo build --locked --release --manifest-path hot/Cargo.toml --bin copybot-hot
cargo test --locked --manifest-path hot/Cargo.toml --lib --bin copybot-hot --tests
```

> This builds for the machine running the command. Build on the destination architecture or use your own cross-compilation setup. No prebuilt executable is supplied in this preview.

这些命令会针对当前执行命令的机器进行构建。请在目标架构上构建，或使用你自己的交叉编译环境。本预览版本不提供预构建的可执行文件。

> ## 2. Prepare a dedicated install

## 2. 准备独立安装

> The service examples use a dedicated `copybot` user and `/opt/copybot`. Have the host administrator create that account and directory, copy this source tree there, and grant that user ownership. Do not run as root. Copy the built `hot/target/release/copybot-hot` to `/opt/copybot/bin/copybot-hot`.

服务示例使用专用的 `copybot` 用户和 `/opt/copybot` 目录。请让主机管理员创建该账户和目录，将这份源代码树复制到那里，并把所有权授予该用户。不要以 root 身份运行。将构建好的 `hot/target/release/copybot-hot` 复制到 `/opt/copybot/bin/copybot-hot`。

> From `/opt/copybot`, as the `copybot` user:

以 `copybot` 用户身份，在 `/opt/copybot` 中执行：

```sh
mkdir -p bin data run
cp deploy/copybot2.example.toml deploy/copybot2.toml
cp deploy/copybot.env.example deploy/copybot.env
chmod 600 deploy/copybot2.toml deploy/copybot.env
```

> The source tree includes every module used by the shipped Python observers. Keep them together in `deploy/`; do not copy just `guardian.py`.

源代码树包含随附 Python 观察器使用的全部模块。请将这些模块完整保留在 `deploy/` 中；不要只复制 `guardian.py`。

> ## 3. Configure your instance

## 3. 配置你的实例

> Edit the local files with a private editor or approved secret manager. They are ignored by Git.

请使用私有编辑器或获批准的密钥管理器编辑本地文件。这些文件会被 Git 忽略。

> - `funder`: your custody/funding address. It is not always the signer address.

- `funder`：你的托管/资金地址，不一定是 signer 地址。

> - `signer`: the address corresponding to your signing key.

- `signer`：与你的签名密钥对应的地址。

> - `signature_type`: the signature/custody mode actually supported by your account. Do not assume every wallet is a Safe merely because the example uses type 2.

- `signature_type`：你的账户实际支持的签名/托管模式。不要因为示例使用 type 2，就认定所有钱包都是 Safe。

> - For a Deposit Wallet using `signature_type = 3` (`POLY_1271`), set `funder` to the Deposit Wallet contract and keep the configured `signer` as the owner EOA corresponding to `PRIVATE_KEY`. The engine uses the funder for both maker and signer inside orders, including exits, and uses the EOA for API authentication and to produce the wrapped signature. Types 0/1/2 retain the EOA as order signer. This mapping follows Polymarket's V2 builder; offline tests do not establish live CLOB acceptance. The startup `/data/orders` self-check tests API authentication only, not order acceptance.

- 对于使用 `signature_type = 3`（`POLY_1271`）的 Deposit Wallet，请将 `funder` 设置为 Deposit Wallet 合约，并保留已配置的 `signer`，使其作为与 `PRIVATE_KEY` 对应的所有者 EOA。引擎在订单（包括退出订单）中同时使用 `funder` 作为 maker 和 signer，并使用该 EOA 进行 API 身份验证以及生成封装签名。类型 0/1/2 仍将 EOA 作为订单 signer。此映射遵循 Polymarket 的 V2 builder；离线测试不能证明线上 CLOB 会接受订单。启动时的 `/data/orders` 自检只测试 API 身份验证，不测试订单是否被接受。

> - `PRIVATE_KEY`: your signer key, set locally. The engine derives its CLOB API credentials using the existing authentication path.

- `PRIVATE_KEY`：你的 signer 密钥，只在本地设置。引擎会通过现有的身份验证路径派生 CLOB API 凭据。

> - `feed.url`: your compatible pending-transaction WebSocket feed, including its private authentication if required.

- `feed.url`：与你兼容的待处理交易 WebSocket feed；如有要求，还要包含其私有身份验证信息。

> - `wallet`: the leader you intend to follow, with measured leader statistics and your own budget.

- `wallet`：你打算跟随的 leader，以及已测得的 leader 统计数据和你自己的预算。

> - Observer values: match `BOT_DIR`, `BOT_CONFIG`, `BOT_PORT`, `COPYBOT_API`, `OUR_WALLET`, `FILLWATCH_FUNDER`, `WATCH_WALLET`, and your RPC URL to this instance.

- 观察器配置值：让 `BOT_DIR`、`BOT_CONFIG`、`BOT_PORT`、`COPYBOT_API`、`OUR_WALLET`、`FILLWATCH_FUNDER`、`WATCH_WALLET` 以及 RPC URL 与此实例保持一致。

> Read [configuration](CONFIGURATION.md). The example is deliberately incomplete: all lanes are disabled, wallet fields are symbolic, and the statistics are zero. The existing engine refuses this configuration until you supply valid values. The example is not an investment recommendation.

请阅读[配置说明](CONFIGURATION.md)。示例配置是有意保持不完整的：所有 lane 都已禁用，钱包字段只是占位符，统计数据为零。在你提供有效值之前，现有引擎会拒绝此配置。该示例不是投资建议。

> ## 4. Establish private dashboard access

## 4. 建立私有仪表盘访问

> The backend binds to loopback. Use a dedicated Tailscale hostname and an access policy restricted to your operators. Tailscale Serve can terminate HTTPS and proxy to the loopback port:

后端绑定到回环地址。请使用专用的 Tailscale 主机名，并设置仅限你的操作员访问的策略。Tailscale Serve 可以负责 HTTPS 终止，并将请求代理到回环端口：

```sh
sudo tailscale serve --bg http://127.0.0.1:8807
```

> Confirm the resulting HTTPS hostname with `tailscale serve status`. Your dashboard is at `/pool`. Use Serve, not public Funnel. Do not open port 8807 in the cloud firewall or publish the backend through an unauthenticated proxy. Refer to the [Tailscale Serve documentation](https://tailscale.com/kb/1312/serve) for your installed version.

使用 `tailscale serve status` 确认生成的 HTTPS 主机名。仪表盘位于 `/pool`。请使用 Serve，不要使用公开的 Funnel。不要在云防火墙中开放 8807 端口，也不要通过未经身份验证的代理发布后端。请根据已安装的版本参阅 [Tailscale Serve 文档](https://tailscale.com/kb/1312/serve)。

> The application relies on this external access boundary; a private GitHub repository does not protect a deployed dashboard. The optional Cloudflare identity header in the engine is audit metadata, not standalone authentication.

应用依赖这道外部访问边界；私有 GitHub 仓库并不能保护已经部署的仪表盘。引擎中可选的 Cloudflare 身份标头只是审计元数据，不能独立承担身份验证功能。

> ## 5. Start in dry mode

## 5. 以干运行模式启动

> Keep `bot.mode = "dry"`. Fill in valid configuration and enable only the lane you intend to observe. Have the host administrator install the included service and timer files under `/etc/systemd/system/` and run `systemctl daemon-reload`.

保持 `bot.mode = "dry"`。填写有效配置，只启用你打算观察的 lane。让主机管理员把随附的 service 和 timer 文件安装到 `/etc/systemd/system/` 下，并运行 `systemctl daemon-reload`。

> Start only the engine initially:

开始时只启动引擎：

```sh
sudo systemctl start copybot-hot.service
sudo systemctl status copybot-hot.service
sudo journalctl -u copybot-hot.service -n 100 --no-pager
```

> Open the private HTTPS dashboard. Check feed activity, the selected leader, mode, lane readiness, balances, and the absence of boot faults. Do not share the unredacted journal or dashboard.

打开私有 HTTPS 仪表盘。检查 feed 活动、选定的 leader、运行模式、lane 就绪状态和余额，并确认没有启动故障。不要分享未经脱敏的日志或仪表盘。

> ## 6. Bring up the independent observers

## 6. 启动独立观察器

> Use the shared environment file with every service. `BOT_PORT` must match `DASHBOARD_PORT`; do not rely on their different code defaults. Start the watcher service and guardian, fillwatch, and buywatch timers from the supplied templates. Review their journals and confirm fresh observations. Enable `GUARDIAN_ENFORCE=1` when you intend its existing halt rules to operate; restart the observer processes to pick up changed environment values.

每个服务都要使用共享环境文件。`BOT_PORT` 必须与 `DASHBOARD_PORT` 一致；不要依赖它们各自不同的代码默认值。根据随附模板启动 watcher 服务，以及 guardian、fillwatch 和 buywatch 定时器。检查它们的日志，并确认已经产生最新观察结果。当你打算让 guardian 现有的停止规则生效时，启用 `GUARDIAN_ENFORCE=1`；重启观察器进程，使其读取变更后的环境变量。

> Service templates preserve the existing intervals. They are not automatically installed, enabled, or started by a build. Validate their paths and environment on your host before enabling startup at boot.

服务模板保留现有的执行间隔。构建过程不会自动安装、启用或启动这些服务。启用开机启动前，请在你的主机上验证它们的路径和环境配置。

> Settlement recovery runs inside the Rust engine every 60 seconds in live mode; it does not need a separate settlewatch service. If auto-redemption removes a position before the redeemable-position poll sees it, the engine uses redemption activity and a complete subsequent zero-balance snapshot to write a durable `settle` event. This closes tracked cost and books realized P&L together, with duplicate protection across polls and restarts. Pending orders, stale/incomplete snapshots, ambiguous token mappings, partial redemptions and shared token ownership defer automatic closure and emit `settle_deferred`. A missing token alone is never treated as proof of a winning payout. The history scan currently covers at most the newest 5,000 redemption rows per pass; older cases require separate investigation.

实盘模式下，结算恢复逻辑每 60 秒在 Rust 引擎内部运行一次，不需要单独的 settlewatch 服务。如果自动赎回在可赎回头寸轮询发现某个头寸之前就将其移除，引擎会结合赎回活动和随后一次完整的零余额快照，写入持久化的 `settle` 事件。这样会同时结清已跟踪成本并记录已实现的 P&L，而且轮询和重启之间具备重复保护。存在待处理订单、过期/不完整快照、无法明确对应的 token、部分赎回或 token 由多个对象共享时，系统会推迟自动结算并发出 `settle_deferred`。单独缺少某个 token，绝不能被视为获胜派彩的证据。当前每次历史扫描最多覆盖最新的 5,000 条赎回记录；更早的情况需要单独调查。

> `settlewatch.py` remains a legacy repair utility for previously released positions. Its default mode is read-only. Do not run it with `--apply` while the engine is running: it appends directly to the ledger, bypassing the engine's in-memory accounting and write serialization. Installing it as an automatic writer is not part of this deployment.

`settlewatch.py` 仍是用于修复此前已释放头寸的旧版工具。它的默认模式是只读的。引擎运行时不要使用 `--apply` 运行它：该参数会直接向账本追加记录，绕过引擎的内存记账和写入串行化。将它安装为自动写入器不属于本次部署范围。

> The auto-redemption snapshot includes archived holdings. Direct closure requires an explicit matching `asset`, not just a default outcome index: [Polymarket's August 10 API change](https://docs.polymarket.com/changelog/predictions) documents per-outcome redemption amounts and the archived-position filter. Legacy aggregate activity and mixed histories containing both an open residual and a neutral reconciliation release require review; closing just the residual would strand the previously released cost basis.

自动赎回快照包含已归档的持仓。直接结算要求提供明确匹配的 `asset`，不能只依赖默认的 outcome 索引：[Polymarket 8 月 10 日的 API 变更](https://docs.polymarket.com/changelog/predictions)记录了各 outcome 的赎回金额和已归档头寸筛选器。旧版聚合活动，以及同时包含未平仓剩余头寸和中性对账释放记录的混合历史，都需要人工检查；如果只结算剩余头寸，之前已释放的成本基础就会被遗留。

> ## 7. Enable live execution deliberately

## 7. 有意启用实盘执行

> Only after checking your configuration, credentials, access controls, and observer health, change `bot.mode` to `"live"` and restart your instance. A fresh lane must then be explicitly armed through the dashboard, using the confirmation phrase shown there. No page in `docs/` can arm a bot.

只有在检查完配置、凭据、访问控制和观察器健康状态之后，才能将 `bot.mode` 改为 `"live"` 并重启实例。新启用的 lane 随后还必须通过仪表盘，并使用页面显示的确认短语，明确完成 armed 操作。`docs/` 中的任何页面都不能替你 armed bot。

> Existing operator intent is persistent. An already-armed instance may resume after a restart. Use the stop controls described in [operations](OPERATIONS.md), not a restart, when you want trading stopped.

现有的操作员意图会持久保存。已经 armed 的实例可能会在重启后恢复运行。如果你想停止交易，请使用[操作说明](OPERATIONS.md)中描述的停止控件，不要通过重启来实现。
