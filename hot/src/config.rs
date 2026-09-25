use crate::lanes::{Execution, Lane, LaneConfig, Sizing};
use serde::Deserialize;
#[derive(Debug, Deserialize)]
pub struct Root {
    pub bot: Bot,
    #[serde(default)]
    pub feed: Vec<Feed>,
    pub lane: Vec<LaneToml>,
    #[serde(default)]
    pub consensus: Option<ConsensusToml>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConsensusToml {
    pub enabled: bool,
    pub primary_lane: String,
    pub confirm_wallet: String,
    pub veto_wallet: String,
    pub symbols: Vec<String>,
    pub interval_minutes: u64,
    pub min_purity: f64,
    pub min_primary_directional_usd: f64,
    pub min_buy_price: f64,
    pub max_buy_price: f64,
    pub two_wallet_target_usd: f64,
    pub three_wallet_target_usd: f64,
    pub confirm_window_seconds: i64,
    pub min_remaining_seconds: i64,
    pub max_state_age_seconds: i64,
    pub events_path: String,
    pub rpc_env: Option<String>,
}

impl Default for ConsensusToml {
    fn default() -> Self {
        Self {
            enabled: false,
            primary_lane: String::new(),
            confirm_wallet: crate::consensus::STD0.into(),
            veto_wallet: crate::consensus::ANTS.into(),
            symbols: vec!["BTC".into(), "ETH".into()],
            interval_minutes: 5,
            min_purity: 0.60,
            min_primary_directional_usd: 10.0,
            min_buy_price: 0.20,
            max_buy_price: 0.70,
            two_wallet_target_usd: 5.0,
            three_wallet_target_usd: 10.0,
            confirm_window_seconds: 20,
            min_remaining_seconds: 60,
            max_state_age_seconds: 10,
            events_path: "data/consensus-events.jsonl".into(),
            rpc_env: None,
        }
    }
}

impl ConsensusToml {
    pub fn validate(&self, root: &Root) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if root.bot.mode != "dry" {
            return Err("consensus V1 is simulation-only: bot.mode must be dry".into());
        }
        let primary = root.lane.iter().find(|l| l.enabled && l.name == self.primary_lane)
            .ok_or("consensus primary_lane must name one enabled lane")?;
        let ohio = addr20(crate::consensus::OHIO)?;
        let std0 = addr20(&self.confirm_wallet)?;
        let ants = addr20(&self.veto_wallet)?;
        if addr20(&primary.wallet)? != ohio || std0 != addr20(crate::consensus::STD0)?
            || ants != addr20(crate::consensus::ANTS)? || ohio == std0 || ohio == ants || std0 == ants
        {
            return Err("consensus role wallet addresses do not match the verified identities".into());
        }
        if self.interval_minutes != 5 || self.symbols.is_empty()
            || self.symbols.iter().any(|s| s != "BTC" && s != "ETH")
            || !(self.min_purity.is_finite() && self.min_purity > 0.0 && self.min_purity <= 1.0)
            || !(self.min_primary_directional_usd.is_finite() && self.min_primary_directional_usd > 0.0)
            || !(self.min_buy_price.is_finite() && self.max_buy_price.is_finite()
                && 0.0 < self.min_buy_price && self.min_buy_price < self.max_buy_price
                && self.max_buy_price < 1.0)
            || !(self.two_wallet_target_usd.is_finite() && self.two_wallet_target_usd > 0.0
                && self.three_wallet_target_usd.is_finite()
                && self.three_wallet_target_usd >= self.two_wallet_target_usd)
            || self.confirm_window_seconds <= 0 || self.min_remaining_seconds <= 0
            || self.max_state_age_seconds <= 0 || self.events_path.is_empty()
            || self.rpc_env.as_ref().is_some_and(|s| s.is_empty())
        {
            return Err("consensus parameters are invalid".into());
        }
        let (caps, _) = resolve_caps(primary)?;
        if caps.per_market_usd + 1e-9 < self.three_wallet_target_usd {
            return Err("consensus target exceeds the primary lane per-market budget".into());
        }
        if primary.budget.min_buy_price > self.min_buy_price
            || primary.budget.max_buy_price < self.max_buy_price
        {
            return Err("consensus lane buy band or exit policy conflicts with consensus rules".into());
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize)]
pub struct Bot {
    #[serde(default = "dry")]
    pub mode: String,
    pub clob_host: String,
    pub funder: String,
    #[serde(default)]
    pub signer: Option<String>,
    #[serde(default = "sig2")]
    pub signature_type: u8,
    pub events_path: String,
    pub control_path: String,
    #[serde(default = "dflt_ledger")]
    pub ledger_path: String,
    #[serde(default = "dflt_signal_guard")]
    pub signal_guard_path: String,
    #[serde(default = "dflt_recycle_secs")]
    pub recycle_secs: u64,
    #[serde(default)]
    pub txpool_rpc: Option<String>,
    #[serde(default)]
    pub recovery_rpc_env: Option<String>,
    #[serde(default = "dflt_recovery_path")]
    pub recovery_path: String,
    #[serde(default = "yes")]
    pub race_h2: bool,
}
fn dry() -> String {
    "dry".into()
}
fn dflt_ledger() -> String {
    "data/ledger.jsonl".into()
}
fn dflt_signal_guard() -> String {
    "data/signal-guard.jsonl".into()
}
fn dflt_recovery_path() -> String {
    "run/watcher_recovery.jsonl".into()
}
fn yes() -> bool {
    true
}
fn sig2() -> u8 {
    2
}
#[derive(Debug, Deserialize)]
pub struct Feed {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub url_env: Option<String>,
    #[serde(default = "one")]
    pub sockets: usize,
}
impl Feed {
    pub fn resolved_url(&self) -> Result<String, String> {
        let url = match &self.url_env {
            Some(name) if self.url.is_empty() && !name.is_empty() => std::env::var(name)
                .map_err(|_| format!("feed {}: environment variable {name} is missing", self.name))?,
            Some(_) => return Err(format!("feed {}: set exactly one of url or url_env", self.name)),
            None => self.url.clone(),
        };
        if !url.starts_with("wss://") {
            return Err(format!("feed {}: expected a wss:// URL", self.name));
        }
        Ok(url)
    }
}
pub fn dflt_recycle_secs() -> u64 {
    0
}
fn one() -> usize {
    1
}
#[derive(Debug, Deserialize)]
pub struct LaneToml {
    pub name: String,
    pub wallet: String,
    #[serde(default)]
    pub enabled: bool,
    pub sizing: SizingToml,
    pub budget: BudgetToml,
    pub execution: ExecToml,
    #[serde(default)]
    pub leader_max_order_usd: Option<f64>,
    #[serde(default)]
    pub leader_peak_exposure_usd: Option<f64>,
    #[serde(default)]
    pub risk: Option<crate::risk::RiskConfig>,
}
#[derive(Debug, Deserialize)]
pub struct SizingToml {
    pub mode: String,
    #[serde(default)]
    pub fixed_shares: f64,
    #[serde(default)]
    pub flat_usd: f64,
    #[serde(default)]
    pub pct: f64,
    #[serde(default = "f005")]
    pub max_effective_pct: f64,
    #[serde(default = "f")]
    pub compound: bool,
    #[serde(default)]
    pub max_usd_per_fill: Option<f64>,
    #[serde(default)]
    pub min_copy_usd: f64,
    #[serde(default = "f1")]
    pub min_order_usd: f64,
    #[serde(default = "t")]
    pub min_fill_floor: bool,
}
fn f1() -> f64 {
    1.0
}
fn t() -> bool {
    true
}
fn f() -> bool {
    false
}
#[derive(Debug, Deserialize)]
pub struct BudgetToml {
    #[serde(default)]
    pub bankroll_usd: Option<f64>,
    #[serde(default)]
    pub daily_usd: Option<f64>,
    #[serde(default)]
    pub per_market_usd: Option<f64>,
    #[serde(default)]
    pub max_open_usd: Option<f64>,
    #[serde(default = "f085")]
    pub open_frac: f64,
    #[serde(default = "f060")]
    pub per_market_frac: f64,
    #[serde(default = "f04167")]
    pub per_fill_frac: f64,
    #[serde(default = "f070")]
    pub daily_frac: f64,
    #[serde(default = "f095")]
    pub max_buy_price: f64,
    #[serde(default = "f002")]
    pub min_buy_price: f64,
}
fn f095() -> f64 {
    0.95
}
fn f002() -> f64 {
    0.02
}
fn f005() -> f64 {
    0.05
}
fn f085() -> f64 {
    0.85
}
fn f060() -> f64 {
    0.60
}
fn f04167() -> f64 {
    0.4167
}
fn f070() -> f64 {
    0.70
}
#[derive(Debug, Deserialize)]
pub struct ExecToml {
    #[serde(default = "taker")]
    pub mode: String,
    #[serde(default)]
    pub slippage_c: Option<f64>,
    #[serde(default)]
    pub buy_slippage_c: Option<f64>,
    #[serde(default)]
    pub sell_slippage_c: Option<f64>,
    #[serde(default)]
    pub sell_floor_frac: f64,
    #[serde(default = "f095f")]
    pub sell_all_frac: f64,
    #[serde(default)]
    pub copy_makers: bool,
    #[serde(default)]
    pub copy_maker_sells: bool,
    #[serde(default)]
    pub exclude_political: bool,
}
fn f095f() -> f64 {
    SELL_ALL_ON_ANY_SELL
}
fn taker() -> String {
    "taker".into()
}
const DEFAULT_SLIPPAGE: f64 = 0.02;
pub fn resolve_caps(l: &LaneToml) -> Result<(crate::budget::Caps, bool), String> {
    let (b, s) = (&l.budget, &l.sizing);
    match b.bankroll_usd {
        Some(bankroll) => {
            let mut typed: Vec<&str> = Vec::new();
            if b.daily_usd.is_some() {
                typed.push("budget.daily_usd");
            }
            if b.per_market_usd.is_some() {
                typed.push("budget.per_market_usd");
            }
            if b.max_open_usd.is_some() {
                typed.push("budget.max_open_usd");
            }
            if s.max_usd_per_fill.is_some() {
                typed.push("sizing.max_usd_per_fill");
            }
            if !typed.is_empty() {
                return Err(
                    format!(
                        "lane {}: bankroll_usd is set, so caps are DERIVED — but {} \
                     {} also set by hand. Remove {} (tune the *_frac values instead), \
                     or remove bankroll_usd to go back to absolute caps.",
                        l.name, typed.join(" and "), if typed.len() == 1 { "is" } else {
                        "are" }, if typed.len() == 1 { "it" } else { "them" }
                    ),
                );
            }
            if !bankroll.is_finite() || bankroll <= 0.0 {
                return Err(
                    format!(
                        "lane {}: bankroll_usd is {bankroll} — a lane with no capital must be \
                     disabled, not given a zero budget that reads as unlimited.",
                        l.name
                    ),
                );
            }
            let f = crate::budget::Fracs {
                open: b.open_frac,
                per_market: b.per_market_frac,
                per_fill: b.per_fill_frac,
                daily: b.daily_frac,
            };
            crate::budget::validate_fracs(&f)
                .map_err(|e| format!("lane {}: {e}", l.name))?;
            Ok((crate::budget::derive(bankroll, &f), true))
        }
        None => {
            let need = |v: Option<f64>, what: &str| {
                v.ok_or_else(|| {
                    format!(
                        "lane {}: no bankroll_usd, so budget.{what} is required", l.name
                    )
                })
            };
            Ok((
                crate::budget::Caps {
                    max_open_usd: need(b.max_open_usd, "max_open_usd")?,
                    per_market_usd: need(b.per_market_usd, "per_market_usd")?,
                    daily_usd: need(b.daily_usd, "daily_usd")?,
                    max_usd_per_fill: s.max_usd_per_fill.unwrap_or(250.0),
                },
                false,
            ))
        }
    }
}
pub const RUNTIME_BUY_SLIPPAGE: f64 = 0.15;
pub const RUNTIME_SELL_SLIPPAGE: f64 = 1.0;
pub const RUNTIME_SELL_FLOOR_FRAC: f64 = 0.5;
pub const SELL_ALL_ON_ANY_SELL: f64 = 0.0;
pub const RUNTIME_SELL_ALL_FRAC: f64 = SELL_ALL_ON_ANY_SELL;
pub fn build_runtime_lane(
    spec: &crate::wallets::WalletSpec,
) -> Result<(Lane, crate::risk::RiskConfig), String> {
    let wallet20 = addr20(&spec.leader)?;
    if !(spec.seed_usd > 0.0) {
        return Err(format!("wallet {}: seed_usd must be positive", spec.name));
    }
    if !(spec.pct > 0.0 && spec.pct <= 1.0) {
        return Err(format!("wallet {}: pct must be in (0,1]", spec.name));
    }
    if !(spec.min_buy_price > 0.0 && spec.max_buy_price < 1.0
        && spec.min_buy_price < spec.max_buy_price)
    {
        return Err(
            format!(
                "wallet {}: buy band must be 0 < min ({}) < max ({}) < 1", spec.name,
                spec.min_buy_price, spec.max_buy_price
            ),
        );
    }
    let fracs = crate::budget::Fracs::default();
    crate::budget::validate_fracs(&fracs)
        .map_err(|e| format!("wallet {}: {e}", spec.name))?;
    let caps = crate::budget::derive(spec.seed_usd, &fracs);
    let leader = spec.leader_stats()?;
    let aggressive = spec.max_effective_pct > crate::budget::MAX_EFFECTIVE_PCT;
    match crate::budget::check_pct_fits(&spec.name, spec.pct, &caps, &leader) {
        Ok(()) => {}
        Err(e) if aggressive => {
            eprintln!(
                "[budget] ADVISORY (wallet opted into {:.0}% sizing): {e}", spec
                .max_effective_pct * 100.0
            )
        }
        Err(e) => return Err(e),
    }
    if let Some(adv) = crate::budget::open_cap_advisory(
        &spec.name,
        spec.pct,
        &caps,
        &leader,
    ) {
        eprintln!("[budget] ADVISORY: {adv}");
    }
    let cfg = LaneConfig {
        name: spec.name.clone(),
        wallet20,
        sizing: Sizing::Pct(spec.pct),
        execution: Execution::Taker,
        buy_slippage_c: spec.buy_slippage_c.unwrap_or(RUNTIME_BUY_SLIPPAGE),
        sell_slippage_c: spec.sell_slippage_c.unwrap_or(RUNTIME_SELL_SLIPPAGE),
        copy_maker_sells: spec.copy_maker_sells(),
        sell_floor_frac: spec.sell_floor_frac.unwrap_or(RUNTIME_SELL_FLOOR_FRAC),
        min_copy_usd: 0.0,
        min_order_usd: spec.min_order_usd.unwrap_or(1.0),
        max_usd_per_fill: caps.max_usd_per_fill,
        daily_budget_usd: caps.daily_usd,
        per_market_usd: caps.per_market_usd,
        max_open_usd: caps.max_open_usd,
        max_buy_price: spec.max_buy_price,
        min_buy_price: spec.min_buy_price,
        min_fill_floor: spec.min_fill_floor.unwrap_or(true),
        sell_all_frac: spec.sell_all_frac.unwrap_or(RUNTIME_SELL_ALL_FRAC),
        max_effective_pct: crate::budget::effective_ceiling(
            spec.pct,
            spec.max_effective_pct,
        ),
        compound: spec.compound,
        copy_makers: spec.copy_makers,
        exclude_political: spec.exclude_political,
    };
    cfg.validate()?;
    let risk = spec
        .risk
        .clone()
        .unwrap_or_else(|| crate::risk::RiskConfig {
            max_drawdown_usd: (spec.seed_usd * 0.15).max(1.0),
            ..Default::default()
        });
    Ok((Lane::new_seeded(cfg, spec.seed_usd, fracs)?, risk))
}
/// Rebuild the atomic runtime policy after a wallet-registry edit without changing the
/// lane's configured cap ratios. The registry owns the seed and copy percentage; the boot
/// configuration owns how that seed is partitioned into fill/market/open/daily limits.
pub fn build_reprice_policy(
    lane: &Lane,
    spec: &crate::wallets::WalletSpec,
) -> Result<crate::lanes::SizingPolicy, String> {
    crate::lanes::SizingPolicy::build(
        0,
        spec.seed_usd,
        Sizing::Pct(spec.pct),
        spec.max_effective_pct,
        spec.compound,
        &lane.budget_fracs,
    )
}
pub fn addr20(s: &str) -> Result<[u8; 20], String> {
    let b = hex::decode(s.trim_start_matches("0x")).map_err(|e| format!("{s}: {e}"))?;
    if b.len() != 20 {
        return Err(format!("{s}: expected 20 bytes, got {}", b.len()));
    }
    let mut o = [0u8; 20];
    o.copy_from_slice(&b);
    Ok(o)
}
impl Root {
    pub fn load(path: &str) -> Result<Root, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        toml::from_str(&raw).map_err(|e| format!("{path}: {e}"))
    }
    pub fn total_seed_usd(&self) -> f64 {
        self.lane
            .iter()
            .filter(|l| l.enabled)
            .filter_map(|l| l.budget.bankroll_usd)
            .sum()
    }
    pub fn wallet_specs(&self) -> Vec<crate::wallets::WalletSpec> {
        self.lane
            .iter()
            .filter(|l| l.enabled)
            .map(|l| crate::wallets::WalletSpec {
                name: l.name.clone(),
                leader: l.wallet.clone(),
                seed_usd: l.budget.bankroll_usd.unwrap_or(0.0),
                pct: l.sizing.pct,
                enabled: true,
                stop_mode: "none".into(),
                min_buy_price: l.budget.min_buy_price,
                max_buy_price: l.budget.max_buy_price,
                max_effective_pct: l.sizing.max_effective_pct,
                compound: l.sizing.compound,
                lane_id: None,
                copy_makers: l.execution.copy_makers,
                copy_maker_sells: Some(l.execution.copy_maker_sells),
                exclude_political: l.execution.exclude_political,
                leader_max_order_usd: l.leader_max_order_usd,
                leader_peak_exposure_usd: l.leader_peak_exposure_usd,
                risk: l.risk,
                buy_slippage_c: l.execution.buy_slippage_c,
                sell_slippage_c: l.execution.sell_slippage_c,
                sell_floor_frac: Some(l.execution.sell_floor_frac),
                min_order_usd: Some(l.sizing.min_order_usd),
                min_fill_floor: Some(l.sizing.min_fill_floor),
                sell_all_frac: Some(l.execution.sell_all_frac),
            })
            .collect()
    }
    pub fn build_lanes(&self) -> Result<Vec<Lane>, String> {
        let mut out = Vec::new();
        let mut seen_wallets = Vec::new();
        let mut seen_names = Vec::new();
        for l in self.lane.iter().filter(|l| l.enabled) {
            if seen_names.contains(&l.name) {
                return Err(format!("duplicate lane name {:?}", l.name));
            }
            seen_names.push(l.name.clone());
            let wallet20 = addr20(&l.wallet)?;
            if seen_wallets.contains(&wallet20) {
                return Err(format!("lane {}: duplicate wallet {}", l.name, l.wallet));
            }
            seen_wallets.push(wallet20);
            let sizing = match l.sizing.mode.as_str() {
                "shares" => Sizing::Shares(l.sizing.fixed_shares),
                "usd" => Sizing::Usd(l.sizing.flat_usd),
                "pct" => Sizing::Pct(l.sizing.pct),
                m => return Err(format!("lane {}: unknown sizing mode {m:?}", l.name)),
            };
            let execution = match l.execution.mode.as_str() {
                "taker" => Execution::Taker,
                "hybrid" => Execution::Hybrid,
                m => return Err(format!("lane {}: unknown execution mode {m:?}", l.name)),
            };
            let (caps, derived) = resolve_caps(l)?;
            let leader = crate::budget::LeaderStats {
                max_order_usd: l
                    .leader_max_order_usd
                    .ok_or_else(|| {
                        format!(
                            "lane {}: leader_max_order_usd is REQUIRED — measure it from this \
                     leader's own order flow, do NOT copy another lane's number",
                            l.name
                        )
                    })?,
                peak_exposure_usd: l
                    .leader_peak_exposure_usd
                    .ok_or_else(|| {
                        format!(
                            "lane {}: leader_peak_exposure_usd is REQUIRED — measure it from this \
                     leader's own order flow, do NOT copy another lane's number",
                            l.name
                        )
                    })?,
            };
            leader.validate(&l.name)?;
            if let Sizing::Pct(p) = sizing {
                if let Err(e) = crate::budget::check_pct_fits(
                    &l.name,
                    p,
                    &caps,
                    &leader,
                ) {
                    let aggressive = l.sizing.max_effective_pct
                        > crate::budget::MAX_EFFECTIVE_PCT;
                    if derived && !aggressive {
                        return Err(e);
                    }
                    if derived {
                        eprintln!(
                            "[{}] ⚠️  BUDGET ADVISORY (lane opted into {:.0}% sizing): {e}",
                            l.name,
                            l.sizing.max_effective_pct * 100.0
                        );
                    } else {
                        eprintln!(
                            "[{}] ⚠️  BUDGET WARNING (absolute caps, not enforced): {e}",
                            l.name
                        );
                    }
                }
                if let Some(adv) = crate::budget::open_cap_advisory(
                    &l.name,
                    p,
                    &caps,
                    &leader,
                ) {
                    eprintln!("[{}] ADVISORY: {adv}", l.name);
                }
            }
            if derived {
                eprintln!(
                    "[{}] caps DERIVED from bankroll: open ${:.2} / market ${:.2} / \
                     fill ${:.2} / daily ${:.2}  (ceiling {:.3}% copy)",
                    l.name, caps.max_open_usd, caps.per_market_usd, caps
                    .max_usd_per_fill, caps.daily_usd, crate ::budget::max_safe_pct(&
                    caps, & leader) * 100.0
                );
            }
            let cfg = LaneConfig {
                name: l.name.clone(),
                wallet20,
                sizing,
                execution,
                buy_slippage_c: l
                    .execution
                    .buy_slippage_c
                    .or(l.execution.slippage_c)
                    .unwrap_or(DEFAULT_SLIPPAGE),
                sell_slippage_c: l
                    .execution
                    .sell_slippage_c
                    .or(l.execution.slippage_c)
                    .unwrap_or(DEFAULT_SLIPPAGE),
                sell_floor_frac: l.execution.sell_floor_frac,
                min_copy_usd: l.sizing.min_copy_usd,
                min_order_usd: l.sizing.min_order_usd,
                max_usd_per_fill: caps.max_usd_per_fill,
                daily_budget_usd: caps.daily_usd,
                per_market_usd: caps.per_market_usd,
                max_open_usd: caps.max_open_usd,
                max_buy_price: l.budget.max_buy_price,
                min_buy_price: l.budget.min_buy_price,
                min_fill_floor: l.sizing.min_fill_floor,
                sell_all_frac: l.execution.sell_all_frac,
                max_effective_pct: crate::budget::effective_ceiling(
                    if let Sizing::Pct(p) = sizing {
                        p
                    } else {
                        l.sizing.max_effective_pct
                    },
                    l.sizing.max_effective_pct,
                ),
                compound: l.sizing.compound,
                copy_makers: l.execution.copy_makers,
                copy_maker_sells: l.execution.copy_maker_sells,
                exclude_political: l.execution.exclude_political,
            };
            cfg.validate()?;
            let lane = if let Some(seed_usd) = l.budget.bankroll_usd {
                let fracs = crate::budget::Fracs {
                    open: l.budget.open_frac,
                    per_market: l.budget.per_market_frac,
                    per_fill: l.budget.per_fill_frac,
                    daily: l.budget.daily_frac,
                };
                Lane::new_seeded(cfg, seed_usd, fracs)?
            } else {
                Lane::new(cfg)
            };
            out.push(lane);
        }
        if out.is_empty() {
            return Err("no enabled lanes — nothing to copy".into());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn consensus_example_parses_and_live_mode_is_rejected() {
        let text = include_str!("../../deploy/copybot-consensus.example.toml");
        let mut root: Root = toml::from_str(text).expect("consensus example TOML");
        let cfg = root.consensus.as_ref().unwrap();
        cfg.validate(&root).expect("dry consensus example");
        root.bot.mode = "live".into();
        assert!(root.consensus.as_ref().unwrap().validate(&root).is_err());
    }

    fn bankroll_root() -> Root {
        toml::from_str(
            r#"
[bot]
mode = "dry"
clob_host = "https://example.invalid"
funder = "0x1111111111111111111111111111111111111111"
signer = "0x2222222222222222222222222222222222222222"
events_path = "data/events.jsonl"
control_path = "run/operator.json"

[[lane]]
name = "alpha"
wallet = "0x3333333333333333333333333333333333333333"
enabled = true
leader_max_order_usd = 100.0
leader_peak_exposure_usd = 100.0

[lane.sizing]
mode = "pct"
pct = 0.005
compound = false
max_effective_pct = 0.05
min_order_usd = 1.0
min_copy_usd = 0.5
min_fill_floor = true

[lane.budget]
bankroll_usd = 550.0
max_buy_price = 0.95
min_buy_price = 0.02

[lane.execution]
mode = "taker"
copy_makers = false
copy_maker_sells = false
"#,
        )
        .expect("fixture must parse")
    }

    #[test]
    fn signer_is_optional_when_the_private_key_derives_the_owner() {
        let bot: Bot = toml::from_str(
            r#"
mode = "live"
clob_host = "https://clob.polymarket.com"
funder = "0x1111111111111111111111111111111111111111"
events_path = "data/events.jsonl"
control_path = "run/operator.json"
"#,
        )
        .expect("a live bot may omit signer");

        assert!(bot.signer.is_none());
    }

    #[test]
    fn second_feed_uses_an_environment_url_without_embedding_it_in_toml() {
        let feed: Feed = toml::from_str(
            "name = \"backup\"\nurl_env = \"COPYBOT_TEST_BACKUP_WSS\"\n",
        ).unwrap();
        std::env::set_var("COPYBOT_TEST_BACKUP_WSS", "wss://backup.example/rpc");
        assert_eq!(feed.resolved_url().unwrap(), "wss://backup.example/rpc");
        std::env::remove_var("COPYBOT_TEST_BACKUP_WSS");
        assert!(feed.resolved_url().is_err());
    }

    #[test]
    fn bankroll_caps_survive_boot_and_the_first_registry_reconciliation() {
        let root = bankroll_root();
        let lanes = root.build_lanes().expect("bankroll lane must build");
        let specs = root.wallet_specs();
        let lane = &lanes[0];
        let policy = lane.policy();

        assert!((policy.seed_usd - 550.0).abs() < 1e-9);
        assert!((lane.cfg.min_copy_usd - 0.50).abs() < 1e-9);
        assert!((policy.caps.max_open_usd - 467.50).abs() < 0.01);
        assert!((policy.caps.per_market_usd - 280.50).abs() < 0.01);
        assert!((policy.caps.max_usd_per_fill - 116.88).abs() < 0.01);
        assert!((policy.caps.daily_usd - 327.25).abs() < 0.01);

        let applied = vec![(
            lane.cfg.name.clone(),
            0.005,
            policy.seed_usd,
            policy.max_effective_pct,
            policy.compound,
            lane.state.rest_buys.load(Ordering::Relaxed),
            lane.state.rest_sells.load(Ordering::Relaxed),
            lane.cfg.min_buy_price,
            lane.cfg.max_buy_price,
        )];
        assert!(
            crate::wallets::repricing(&specs, &applied).is_empty(),
            "an unchanged boot lane must not be repriced on the first two-second tick"
        );
    }

    #[test]
    fn hot_seed_change_preserves_the_toml_cap_ratios() {
        let root = bankroll_root();
        let mut lanes = root.build_lanes().expect("bankroll lane must build");
        let lane = lanes.pop().unwrap();
        let mut spec = root.wallet_specs().pop().unwrap();
        spec.seed_usd = 1_100.0;

        let next = build_reprice_policy(&lane, &spec).expect("reprice must build");
        lane.apply_policy(next).expect("reprice must publish");
        let policy = lane.policy();

        assert!((policy.caps.max_open_usd - 935.00).abs() < 0.01);
        assert!((policy.caps.per_market_usd - 561.00).abs() < 0.01);
        assert!((policy.caps.max_usd_per_fill - 233.77).abs() < 0.01);
        assert!((policy.caps.daily_usd - 654.50).abs() < 0.01);
    }

    #[test]
    fn explicit_aggressive_ceiling_allows_a_capped_twenty_percent_lane() {
        let mut root = bankroll_root();
        root.lane[0].sizing.pct = 0.2;
        root.lane[0].sizing.max_effective_pct = 0.2;
        root.lane[0].budget.bankroll_usd = Some(1_000.0);

        let lanes = root
            .build_lanes()
            .expect("an explicit aggressive ceiling should allow capped sizing");
        let policy = lanes[0].policy();

        assert_eq!(lanes[0].cfg.sizing, Sizing::Pct(0.2));
        assert!((policy.seed_usd - 1_000.0).abs() < 1e-9);
        assert!((policy.caps.max_open_usd - 850.0).abs() < 0.01);
        assert!((policy.caps.max_usd_per_fill - 212.517).abs() < 0.01);
    }

}
