# Configuration

The engine retains its existing TOML configuration format; runtime operator and registry state remain JSON. Packaging has not introduced a new configuration parser or strategy model.

## Configuration layers

| Layer | Location | Purpose |
| --- | --- | --- |
| Boot configuration | `deploy/copybot2.toml` | Custody, feeds, initial lanes, sizing, execution |
| Process environment | `deploy/copybot.env` | Signing key, observer configuration, feature gates |
| Operator state | Under configured `control_path` | Arm/off/halt intent and related state |
| Accounting and recovery | Configured ledger and `data/`, `run/` | Durable execution and reconciliation evidence |

### 本机文件名识别

| 文件名 | 当前用途 |
| --- | --- |
| `deploy/copybot2.toml` | **正在使用**：WSL 交易引擎的启动配置，包含实盘模式和目标钱包。 |
| `deploy/copybot.env` | **正在使用**：WSL 引擎及观察服务的环境变量和私钥。 |
| `run/control.json.wallets` | **正在使用**：持久化钱包列表；存在时优先于 TOML 中的初始钱包列表。 |
| `run/control.json.operator` | **正在使用**：各通道的 arm/off 操作状态。 |
| `deploy/copybot2.example.toml` | **示例，未加载**：`.example` 表示模板。 |
| `deploy/copybot.env.example` | **示例，未加载**：`.example` 表示模板。 |
| `leader-0x0c7c-lane.example.toml` | **示例，未加载**：单独的 lane 片段，不能代替完整启动配置。 |
| `run/control.json.wallets.inactive-20260926.json` | **旧钱包列表备份，未加载**：`.inactive` 表示停用。 |

WSL 当前安装的服务单元位于 `/etc/systemd/system/`；仓库 `deploy/` 下的 `.service` / `.timer` 文件是安装模板。`copybot-dry.service` 的 `dry` 是历史服务名，真实下单模式由 `deploy/copybot2.toml` 中的 `bot.mode` 决定。

Only examples ship. Do not commit any populated layer. The bot does not load `.env` files automatically; the service templates use systemd's `EnvironmentFile` mechanism.

`[bot].position_sweep_enabled` controls automatic position-cleanup SELLs after a leader position appears flat, proportional excess is found, or a post-sell dust remainder is detected. It defaults to `true` for older configurations. Set it to `false` to disable this cleanup path; direct mirrored leader SELLs and failed-order exit recovery remain enabled. The value is read at startup, so changing it requires restarting the engine.

For feed redundancy, configure two `[[feed]]` entries backed by different Polygon pending-transaction providers. `url_env` reads a `wss://` URL from the process environment; startup fails if a configured variable is missing. The watcher can independently persist observed leader fills to `run/watcher_recovery.jsonl`. Set `bot.recovery_rpc_env` to the name of an HTTPS Polygon JSON-RPC environment variable to enable recovery. Recovery fetches calldata by transaction hash and checks the live CLOB book; it accepts only a matching lane/token/side/size, a signal no more than 10 seconds old, and an executable quote within the lane's price limit. Its decisions use the normal order path. The watcher heartbeat reports exact `fire`, `skip`, inferred coverage, and misses over the retained 24-hour fill window.

## Sizing

`mode = "pct"` uses a fractional copy percentage: `0.005` means 0.5%, not 5%. `shares` and `usd` are also accepted by the existing parser. Percentage copying is subject to minimum order sizes, budgets, execution prices, and the configured exit policy; it is not a guarantee of an exact share ratio.

For a leader that builds positions through small BUY fills, set `aggregate_small_buys = true` and `aggregate_trigger_usd = 10.0` under `[lane.sizing]` with `mode = "pct"`. The switch is off by default. For each lane, market, and outcome token, the engine adds the leader's observed BUY cost (`fill shares × fill price`). It subtracts the follower's booked position cost plus the notional of open BUY orders, divided by the effective copy percentage. When that uncovered leader-equivalent amount exceeds `aggregate_trigger_usd`, it sizes one order at the copy percentage of the gap. Thus an actual $1.6 follower purchase at 10% covers $16 of leader purchases on the next check. YES and NO keep independent totals in this mode. Only fills observed since this mode began are accumulated; it does not backfill older leader trades. The order must also satisfy `min_order_usd`; `min_fill_floor` does not inflate it. Existing price bands and budgets still apply. Pending-chain observations can fail to settle, so this remains an execution experiment rather than a confirmed leader-position mirror. A dry run uses a separate `.aggregate-dry` signal-guard file and records hypothetical buys there; switching modes does not change an already running process.

Use either `bankroll_usd` with derived budget fractions or the parser's absolute-cap mode. Do not mix derived and absolute caps. Set measured `leader_max_order_usd` and `leader_peak_exposure_usd` for your selected leader. The engine validates these rather than borrowing another leader's statistics.

`compound` controls the existing realized-gain/loss sizing behavior. Set it intentionally. A lane budget is a strategy allocation inside a shared account, not security isolation between different owners.

## Execution

The parser accepts `taker` or `hybrid`. Split buy/sell slippage fields take precedence over the legacy combined field. `copy_makers` and `copy_maker_sells` are separate choices. Preserve them when transferring a lane between boot configuration and the runtime registry.

Do not infer units from the historical `_c` suffix alone: inspect `hot/src/config.rs` and `hot/src/lanes.rs` for the value's actual use. This export preserves those calculations.

`sell_all_frac = 0.0` requests the current any-sell flatten policy. A nonzero threshold changes when an exit becomes a full flatten. `sell_floor_frac` constrains the exit price relative to the leader; allowing broad slippage can fill at a substantially worse price. Setting it to zero does not guarantee a full fill in an empty market.

## Observers

All services need the same install directory and engine port. The legacy observer wallet environment variables must be supplied for your instance. Runtime lane discovery and shared-wallet checks still use the engine's API; these observers are not interchangeable with a multi-tenant permissions system.

Notification credentials and recipients are optional and empty in the template. `GUARDIAN_ENFORCE`, `MERGE_RECONCILE`, `COPYBOT_MERGE_ENABLE`, and `ORPHAN_SWEEP` retain their existing meanings. The example leaves optional live-action gates off; enabling an on-chain feature requires the corresponding custody/RPC setup. Do not set optional gates simply to clear a warning.
