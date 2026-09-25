# 三钱包共识 V1 实施记录

依据 [设计稿](THREE_WALLET_CONSENSUS_DESIGN.md)实施。当前只提供 dry 模式的观察和模拟成交；真实下单入口被配置校验禁止，现有 `deploy/copybot2.toml` 未修改。

## 已实现

- `hot/src/consensus.rs`：BTC/ETH 5m 市场校验、Up/Down token 映射、微份额净仓纯度、Ohio 合格买入剩余成本、20 秒确认、ants 否决与 $5/$10 总目标。成交时间只有秒级且同秒反向交易无法排序时返回未知。
- `hot/src/data_api_v2.rs`、`hot/src/positions.rs`：v2 游标完整读取、旧持仓字段兼容；缺页、坏字段和游标异常都拒绝把持仓判成空。观察器还以链上 CTF 双 outcome 余额核对三钱包快照，使用零份额阈值避免 v2 默认过滤小仓位。
- `hot/src/consensus_observer.rs`：候选、确认、否决、未知、模拟成交和退出写入独立 JSONL 日志；重启时重放日志恢复模拟仓位及日用量。待确认的交易流只记为 tentative，不用于开仓。
- `hot/src/main.rs`：启用共识时监视三个角色钱包，Ohio lane 跳过旧单钱包复制决策；只在 `mode = "dry"` 时允许共识观察器启动。
- `hot/src/matchup.rs`、`deploy/guardian.py` 以及主进程的持仓、历史交易、种子活动和政治市场读取已迁到 v2。`deploy/consensus_report.py` 用事件当时记录的证据回放观察日志，输出 BTC/ETH 分项候选、确认、否决、未知、延迟、价格和模拟资金占用。

## 运行和验收

1. 从 `deploy/copybot-consensus.example.toml` 复制一份**独立** dry 配置，替换模拟钱包地址，提供 `POLYGON_WSS` 与 `POLYGON_CONSENSUS_RPC`，检查所有 `data/`、`run/` 路径只用于本次观察。不要用现有 live 配置运行。
2. 在 Linux/WSL 的项目根目录运行 `cargo run --manifest-path hot/Cargo.toml --release -- deploy/你的独立配置.toml`；观察器写入 `consensus.events_path`。不配置私钥仍可进行 dry 观察。
3. 运行 `python3 deploy/consensus_report.py data/consensus-observer.jsonl`。报告只统计实际记录的事件，不将市场结束后的持仓用于倒推历史信号。
4. 积累足够多的已结算 5m 市场及连续运行样本后，再独立核查确认率、未知率、模拟成交率、实际数据延迟和参数邻近值。当前日志没有完整历史订单簿、手续费及最终结算结果，因此不能计算净收益或与两种基线做同口径绩效比较。

## 上线门槛

- 主进程的赎回对账仍读取 v1 `/activity`：v2 `REDEEM` 活动缺少旧账本所需的 outcome index 与赎回份额，直接字段替换会造成错误结算。该路径需要另行用链上回执或等价可信数据补齐并验证，才能视作 v1 关键依赖迁移完成。
- 本机到公开 Data API 的直接网络连接不可用，尚未用在线样本确认 Gamma、v2、RPC、CLOB 四源在真实运行时的时效与一致性。Linux 编译、单元测试和 Python 语法检查不能代替观察期验收。
- P4 观察样本和 P5 部署审查未完成；不得将该版本切到 live 或据此声称已验证策略收益。
