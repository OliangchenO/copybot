# BTC/ETH 5m 三钱包净仓共识改造设计

> 版本：V1 设计稿，2026-09-25。本文用于分阶段开发和验收；其中的收益、胜率和复制效果均不是已验证的策略结论。

## 1. 目标和成功标准

只由 OhioRiskManagement 产生入场候选；std0 负责确认；antsaslyku 负责增强或否决。三个钱包在**同一个 `condition_id`** 中的当前 Up/Down 持仓决定方向，单笔 BUY 不直接代表方向。

V1 完成的工程标准：

1. 能持续给出三钱包在 BTC/ETH 5m 市场的 `UP`、`DOWN`、`NEUTRAL`、`EMPTY`、`UNKNOWN` 状态及依据。
2. Ohio 符合入场条件后，只在 std0 同向、ants 未明确反向时生成 $5 模拟目标；三者同向时目标为**总计 $10**。
3. 重复推送、部分成交、进程重启和数据源暂时失败不造成重复买入，也不把未知状态当作空仓。
4. 能按事件当时可见的数据回放历史，并记录每次候选、确认、否决、跳过和模拟成交。

本设计先实现观察、回放和模拟决策。真实订单是否启用，取决于独立的回放结果与运行配置审查；设计完成不等于策略盈利或线上运行已验证。

## 2. 钱包身份和当前项目差距

| 角色 | Polymarket 个人页核对的代理钱包地址 | 权限 |
| --- | --- | --- |
| 主信号 OhioRiskManagement | `0x0c7c5204404e9d5402d258fedac59c7212bae4cb` | 产生候选；其净方向变化驱动本策略退出 |
| 第一确认 std0 | `0xdf7930e89a2c47560165331863c31deca0733dcd` | 同向确认或反向阻止入场 |
| 第二确认/否决 antsaslyku | `0x3c58ef422754ff22c7e806336feba0064d8b776b` | 同向提高目标，明确反向否决 |

核对来源：[Ohio](https://polymarket.com/profile/0x0c7c5204404e9d5402d258fedac59c7212bae4cb)、[std0](https://polymarket.com/profile/0xdf7930e89a2c47560165331863c31deca0733dcd)、[antsaslyku](https://polymarket.com/profile/0x3c58ef422754ff22c7e806336feba0064d8b776b)。地址按个人页截至本文日期的显示记录；启用时再核对一次。

**现有地址不一致。** `deploy/copybot2.toml` 与 `run/control.json.wallets` 当前都将唯一启用 lane `ohioism_dry` 指向 `0x361528e242bc6cc789ac8da6fd5cb98046178fdf`，并非上表的 Ohio 地址。TOML 中 `bot.mode = "live"`，`bankroll_usd = 300`，买入价格范围是 2–98¢。不能通过改 lane 名称完成身份切换；实现时须检查 TOML、持久化钱包列表和实际运行时 lane 的地址一致。旧 lane 的用途在迁移前需单独确认。

项目已有 `calldata::Decoded` 的 `condition_id`、`token_id`、买卖方向、价格、成交份数和订单序号。当前 `main.rs` 在逐个 lane 解码单笔成交后，直接进入 `Router::decide`；`positions.rs` 能读取持仓，但尚无三钱包按市场聚合的共识状态。`signal_guard` 是订单去重及市场冲突闸门，不能充当其他两个钱包的持仓簿。

## 3. 数据定义和计算口径

### 3.1 市场与 outcome 映射

以标准化的小写 `condition_id` 为市场主键。市场元数据必须给出：标的 BTC/ETH、5m 周期、市场开始/截止时间、开盘状态，以及**恰好两个** Up/Down outcome 对应的 token ID。`condition_id`、token ID 和显示标签必须一一对应；缺失、重复或不一致时标记 `UNKNOWN_MARKET`，不进入买入决策。不要根据 token 数值顺序猜 Up/Down，也不要仅从成交价格或标题判断周期。

官方市场文档说明，市场对象提供 condition ID、slug、outcome 标签和 CLOB token ID；Gamma 的 outcome 与 token 数组按相同索引对应。实现时可复用项目已有的 CLOB `/markets/{condition_id}` 读取入口，并补齐 BTC/ETH 5m 的周期核验；若 CLOB 响应不足，使用 Gamma 元数据交叉核对。[官方市场字段说明](https://docs.polymarket.com/market-data/market-details)

### 3.2 钱包持仓

聚合键为 `(wallet_address, condition_id, token_id)`；每个钱包与市场得到 `up_shares`、`down_shares`、快照时间、最近确认事件和完整性状态。持仓数量使用微份额整数或等价的定点数，避免 60% 边界受浮点误差影响。

```text
net_shares = up_shares - down_shares
purity     = abs(net_shares) / (up_shares + down_shares)

UP      : net_shares > 0 且 purity >= 0.60
DOWN    : net_shares < 0 且 purity >= 0.60
NEUTRAL : 两边有仓，但 purity < 0.60
EMPTY   : 两边均为 0，且完整快照确认这一事实
UNKNOWN : 请求失败、分页不全、数据过期、映射缺失或账本冲突
```

`up_shares + down_shares = 0` 时不做除法。比较阈值时用 `100 × abs(net_shares) >= 60 × (up_shares + down_shares)`；60% 是包含边界的条件。**全部两边持仓都参加纯度计算**，包括 70¢以上取得的对冲仓；20–70¢仅限定 Ohio 的有效触发买入。

| 持仓样例 | 净方向 | 纯度 | V1 分类 |
| --- | ---: | ---: | --- |
| Up 30 / Down 8 | Up 22 | 57.89% | `NEUTRAL`；有净 Up，但不足以确认 |
| Up 30 / Down 7.5 | Up 22.5 | 60.00% | `UP` |
| Up 30 / Down 27 | Up 3 | 5.26% | `NEUTRAL` |
| Up 35.3 / Down 135 | Down 99.7 | 58.54% | `NEUTRAL`；不构成 ants 反向否决 |
| Up 0 / Down 0 | 0 | 不计算 | `EMPTY`，前提是快照完整 |

这套纯度衡量的是持仓构成，并非盈利能力；split 产生的等额双边份额也会改变分母。回放中应单独统计这一影响，不在 V1 临时改写公式。

### 3.3 Ohio 的 `$10 有效买入` 口径

仅统计**本 5m 市场开始后、已经确认、Ohio 买入当前主导 outcome、成交价在 20–70¢内**的成交。为避免“先买满 $10 再卖光”仍触发，按该 outcome 的持仓变化维护 `eligible_open_shares` 和对应成本：合格 BUY 增加；SELL 按卖出前合格份额占同侧总份额的比例扣减合格份额及成本。行情窗口外的 BUY 计入实际持仓和纯度，但不增加合格成本。

```text
eligible_buy_usd    = 仍持有的合格买入成本
eligible_vwap       = eligible_buy_usd / eligible_open_shares
directional_usd     = min(eligible_buy_usd,
                          abs(net_shares) × eligible_vwap)
Ohio 主触发门槛      = directional_usd >= 10.00
```

无法从完整成交历史重建合格成本时，门槛状态为 `UNKNOWN`，不把普通持仓成本代入。启动时可补拉该市场开盘以来的 Ohio 成交；split、merge、转账或成交与持仓无法对齐时先等待快照校正。这个口径要求 Ohio 仍承担至少约 $10 的未对冲方向风险，比历史 BUY 总额更贴近主信号意图。

## 4. 数据取得、确认与校正

1. **启动基线**：取得三个钱包的完整当前持仓；对仍在交易的目标 5m 市场补齐开盘以来成交，建立合格买入成本。分页、行字段和 outcome 映射必须全部有效。只取得第一页不能证明另一个 outcome 为零。
2. **低延迟候选**：现有 WSS/txpool 发现 Ohio、std0、ants 的待确认 `matchOrders` 后，可预取市场信息、订单簿和钱包快照，但只记为 `TENTATIVE`；交易可能丢失或回滚。
3. **确认增量**：交易回执确认后才将 BUY/SELL 记入持仓。持久化唯一事件键必须覆盖交易哈希和交易内成交位置/订单身份；仅凭 `(tx, token, side, size)` 不足以表示所有相同成交。多源重投和重启回放使用同一键去重。
4. **快照校正**：周期性读取完整持仓快照，结合其数据时间与本地已确认事件判断是否追上；过旧快照不能覆盖较新的本地增量。遇到无法解释的差额，相关钱包/市场进入 `UNKNOWN`，待恢复后再形成新决策。split/merge 和其他余额变更以完整快照重新锚定。
5. **过期处理**：市场截止后停止新候选；保留必要事件供退出、结算与审计，过期状态不得流入下一轮 `condition_id`。

新读取代码按 Data API v2 契约实现：`/v2/positions` 使用 `data` 包装和 `pagination.next_cursor`，持仓用 `condition_id`、`token_id`/`asset_id`、`outcome`、`current_size`；`/v2/trades` 提供钱包、市场、方向、份数、价格、时间与交易哈希。官方说明 v1 路由将于 **2026-10-24** 退役。当前 `positions.rs`、`main.rs`、`matchup.rs` 和 `deploy/guardian.py` 仍有 v1 读取，因此上线前要盘点并迁移**相关关键调用**；不能只给新模块接 v2 后仍让退出/守护依赖即将退役的 v1 路由。[钱包持仓与活动](https://docs.polymarket.com/trading/wallet-activity)、[成交字段](https://docs.polymarket.com/market-data/public-analytics)、[v1→v2 迁移](https://docs.polymarket.com/migrate/data-api-v1-to-v2)

## 5. 入场白名单与时效

候选市场必须同时满足：BTC 或 ETH、官方 Up/Down 二元 outcome、周期恰为 5 分钟、市场开放且仍接受订单、距截止至少 60 秒。不能把 BTC 15m、ETH 1m 或其他同名市场纳入。

Ohio 在 20–70¢ 的合格 BUY 形成候选后，允许最多 20 秒等待 std0 的确认；若此时 std0 已持有同市场合格同向仓，可立即判断。20 秒与“截止前至少 60 秒”均是**回放用的初始参数**，需要用实际数据评估确认率、成交价变化和错过交易率。确认窗口和市场截止任一先到，即结束新入场。钱包状态需有可证明的新鲜数据；初版将本地已核实状态超过 10 秒或上游新鲜度无法确定的结果视为 `UNKNOWN`，并统计因此错过的候选。

价格闸门分两层：Ohio 的触发 BUY 成交价必须在 20–70¢；我方实际拟提交的限价也必须在 20–70¢，并通过原有订单簿、滑点和风险检查。这样可避免 Ohio 在 70¢买入，但延迟确认时我方在 80¢追入。上述价格范围不限制卖出。

## 6. 共识决策与目标仓位

一份决策快照包含同一个 `condition_id` 下三个钱包的状态、时间和证据；不能用三个不同时刻的零散单笔 BUY 拼出“共识”。确认钱包无需与 Ohio 买入数量相同，但必须有当前非零持仓及纯度 ≥60%。

| Ohio 主条件 | std0 | antsaslyku | 决策与模拟目标 |
| --- | --- | --- | ---: |
| 不成立、`NEUTRAL`、`EMPTY` 或 `UNKNOWN` | 任意 | 任意 | $0 |
| 成立 | 同向且合格 | 同向且合格 | 总计 $10 |
| 成立 | 同向且合格 | `NEUTRAL` 或完整快照确认 `EMPTY` | 总计 $5 |
| 成立 | `NEUTRAL` 或 `EMPTY` | 任意 | $0，等待确认窗口 |
| 成立 | 明确反向 | 任意 | $0，候选否决 |
| 成立 | 同向且合格 | 明确反向 | $0，候选否决 |
| 成立 | `UNKNOWN` | 任意 | $0，等待数据恢复或窗口结束 |
| 成立 | 同向且合格 | `UNKNOWN` | $0，等待数据恢复或窗口结束 |

同一市场只建立一个目标方向。若 ants 在首次决策时同向，直接以 $10 为目标；若 $5 已实际成交后才同向，只补**剩余 $5**，并再次检查时效、价格、现有持仓、未决订单和风险预算。若 ants 在 $5 成交之后变成反向，它只能阻止后续加仓；已成交订单无法被追溯性地“否决”。

目标金额是**每个 `condition_id + outcome` 的累计已成交名义金额**，不是每笔 Ohio 成交都买 $5/$10。计算新增意图时，用 `target_usd - our_filled_usd - our_pending_usd`，非正值不下单。部分成交与未知提交结果先按现有 pending/ledger 路径核实，不能重试出第二份完整目标。现有 `Router` 的现金、日预算、单市场预算、最小订单额、价格和 arm/halt 闸门继续生效；需要增加一个按目标金额生成 BUY 意图的明确入口，不要伪造 Ohio 的 `Decoded` 来套用逐笔百分比跟单逻辑。

## 7. 候选状态机与退出

```text
OBSERVING
  └─ Ohio 确认有效 BUY + 白名单 + 当前净仓达标 → CANDIDATE
       ├─ std0/ants 明确反向 → VETOED（本市场不再入场）
       ├─ 数据 UNKNOWN → WAITING（直到恢复或超时）
       ├─ std0 同向、ants 中性/空仓 → TARGET_5
       ├─ 三者同向 → TARGET_10
       └─ 超时/临近截止/Ohio 失格 → EXPIRED
TARGET_5 ── ants 后续同向且仍可入场 → TARGET_10（只补差额）
```

V1 的持仓退出以**Ohio 已确认的净方向**为准：Ohio 从合格同向变为 `NEUTRAL`、`EMPTY` 或明确反向时，停止新 BUY、按现有撤单确认流程处理未成交买单，并为本策略实际持有的同市场 token 生成退出意图。Ohio 仅减少一部分同向仓但仍合格时，不因每笔 SELL 立即全部退出。std0/ants 入场后改向只阻止加仓并记事件，不独立触发卖出。实际 SELL 仍必须满足现有持仓、预留份额、成交价和订单生命周期检查；市场关闭后交由既有结算路径处理。

这意味着共识 lane 不能继续直接采用 `sell_all_frac = 0` 的“任一 Ohio SELL 就全卖”行为；退出应由净方向状态变化发起。其他现有 lane 的逐笔跟随与退出规则保持原样。

## 8. 代码落点与配置契约

| 位置 | 计划改动 |
| --- | --- |
| `hot/src/consensus.rs`（新增） | 定点数净仓/纯度、三钱包决策、候选状态机、目标金额计算；保持纯逻辑便于回放 |
| `hot/src/positions.rs` 或独立 `data_api_v2.rs` | v2 持仓/成交读取、完整游标遍历、字段验证、新鲜度与快照完整性 |
| `hot/src/main.rs` | 三钱包观察入口、确认事件送入聚合器；仅 Ohio 的共识结果可进入买入执行；把退出接到现有安全路径 |
| `hot/src/feeds.rs` | 监听地址为可执行钱包与两个只读观察钱包的并集；动态刷新时不得把观察钱包清掉 |
| `hot/src/config.rs`、策略配置示例 | 增加可选共识配置及启动前地址、范围、预算验证；std0/ants 不创建可执行 lane |
| `hot/src/lanes.rs`、现有 pending/ledger | 以目标仓位差额生成意图，复用预算和订单生命周期；不得产生第二套我方资产账本 |
| `hot/src/signal_guard.rs` | 复核同市场反向 token 锁和目标升级的幂等行为；只修改实测需要的部分 |
| `hot/src/matchup.rs`、`deploy/guardian.py` 等 | 对仍依赖 Data API v1 的关键读取做迁移或另列限期任务 |

建议的最小配置形态如下。它是**新策略配置示例**，不是对现有 live 配置的修改：

```toml
[consensus]
enabled = false
primary_lane = "ohio_consensus"
confirm_wallet = "0xdf7930e89a2c47560165331863c31deca0733dcd"
veto_wallet = "0x3c58ef422754ff22c7e806336feba0064d8b776b"
symbols = ["BTC", "ETH"]
interval_minutes = 5
min_purity = 0.60
min_primary_directional_usd = 10.0
min_buy_price = 0.20
max_buy_price = 0.70
two_wallet_target_usd = 5.0
three_wallet_target_usd = 10.0
confirm_window_seconds = 20
min_remaining_seconds = 60
max_state_age_seconds = 10

# 另建一个可执行 lane；钱包必须与官方 Ohio 地址一致：
# name = "ohio_consensus"
# wallet = "0x0c7c5204404e9d5402d258fedac59c7212bae4cb"
# budget.bankroll_usd = 500.0  （模拟账户）
```

启动前验证：角色地址互不相同且均为 20 字节；`primary_lane` 存在且地址为核对过的 Ohio 地址；$10 目标不超过该 lane 单市场上限；`20–70¢` 范围合法；持久化钱包列表与预期一致。任何一项失败时，新共识策略不得 armed。`enabled = false` 是兼容已有配置的默认值。

## 9. 回放、验收与上线门槛

按事件发生时已经可见的市场、成交、快照和订单簿数据回放；不能用市场结束后的最终持仓反推当时“已知”的共识。回放报告至少分别列出 BTC/ETH：Ohio 候选数、std0 确认率、ants 增强/否决率、未知/超时率、拟成交价与延迟、模拟成交率、手续费与滑点后结果、最大同时占用资金。将“Ohio 单独”“任意两钱包”“本方案”放在同口径下比较，但不因历史胜率直接推断可复制收益。

最低工程验收样例：

| 场景 | 预期 |
| --- | --- |
| Up 30 / Down 8；Up 30 / Down 7.5 | 前者不足 60%，后者恰好达到 60% |
| ants Down 135 / Up 35.3 | 纯度约 58.54%，不否决 |
| Ohio 20–70¢ 买满 $10 后卖出大部分 | 当前有效方向金额不足 $10 时无候选 |
| Ohio 60¢ 买入，确认时我方可成交价 71¢ | 跳过，记录价格原因 |
| BTC 5m 与下一轮 BTC 5m 都是 Up | `condition_id` 不同，绝不混仓或复用确认 |
| BTC 15m、ETH 1m、未知 outcome token | 不进入白名单 |
| 两个数据源推送同笔成交；进程重启后重放 | 持仓和订单目标只计算一次 |
| 待确认交易回滚；持仓 API 只返回部分页面 | 分别保持候选未确认、钱包状态 `UNKNOWN`；不下单 |
| 已成交 $5，未决补单 $3，目标升级 $10 | 新增最多 $2；未知提交先核实 |
| std0/ants 快照失败 | `UNKNOWN` 不得被当成中性或空仓 |
| Ohio 已确认净方向转中性 | 停止加仓并进入已持仓退出流程 |

实施顺序和每步完成标准：

1. **P1 净仓位底座**：核对三钱包身份，完成市场 outcome 映射、v2 完整快照/成交、确认增量、去重、重启恢复；以上方向/未知用例通过，输出只读状态与事件日志。
2. **P2 市场白名单**：只允许 BTC/ETH 5m，实际样本中周期、token 和截止时间匹配；缺失元数据全部有明确跳过原因。
3. **P3 共识与执行衔接**：加入候选状态机、$5/$10 总目标、价位复核和净方向退出；保留已有风险及订单核实路径，完成上述决策、部分成交、撤单和重启用例。
4. **P4 回放与模拟观察**：至少覆盖足够多的已结算 5m 市场，并连续观察一段实际运行时延与数据缺失；报告样本量和所有跳过原因。Rust 代码在 Linux/WSL 环境编译验证，模拟观察使用独立于现有 `mode = "live"` 文件的配置。
5. **P5 部署审查**：检查 v1 关键依赖迁移、钱包地址、实际加载的配置和二进制、运行状态及账本一致性，再单独决定是否启用真实订单。编译成功、API 请求成功或模拟 `would_fire` 均不代表真实成交。

## 10. 需要在回放中校准的参数

`min_purity = 0.60`、20 秒确认窗口、截止前 60 秒停止入场及 10 秒状态新鲜度都是 V1 假设。回放报告应列出这些参数在邻近取值下对样本数、未知率、成交延迟和结果的影响，再决定是否调整。三钱包公开历史表现只用于选择观察对象，不应写成策略已获利的验收条件。
