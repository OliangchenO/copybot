use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const OHIO: &str = "0x0c7c5204404e9d5402d258fedac59c7212bae4cb";
pub const STD0: &str = "0xdf7930e89a2c47560165331863c31deca0733dcd";
pub const ANTS: &str = "0x3c58ef422754ff22c7e806336feba0064d8b776b";

#[derive(Clone, Debug)]
pub struct Policy {
    pub symbols: Vec<String>,
    pub purity_ppm: i128,
    pub primary_cents: i64,
    pub min_price: i64,
    pub max_price: i64,
    pub two_wallet_cents: i64,
    pub three_wallet_cents: i64,
    pub confirm_secs: i64,
    pub min_remaining_secs: i64,
    pub max_age_secs: i64,
}

impl From<&crate::config::ConsensusToml> for Policy {
    fn from(c: &crate::config::ConsensusToml) -> Self {
        Self {
            symbols: c.symbols.clone(),
            purity_ppm: (c.min_purity * 1_000_000.0).round() as i128,
            primary_cents: (c.min_primary_directional_usd * 100.0).round() as i64,
            min_price: (c.min_buy_price * 1_000_000.0).round() as i64,
            max_price: (c.max_buy_price * 1_000_000.0).round() as i64,
            two_wallet_cents: (c.two_wallet_target_usd * 100.0).round() as i64,
            three_wallet_cents: (c.three_wallet_target_usd * 100.0).round() as i64,
            confirm_secs: c.confirm_window_seconds,
            min_remaining_secs: c.min_remaining_seconds,
            max_age_secs: c.max_state_age_seconds,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Direction {
    Up,
    Down,
    Neutral,
    Empty,
    Unknown,
}

impl Direction {
    fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            x => x,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    pub up: i64,
    pub down: i64,
    pub as_of: i64,
    pub complete: bool,
}

impl Book {
    pub fn direction(&self, now: i64, max_age: i64) -> Direction {
        self.direction_with_purity(now, max_age, 600_000)
    }

    pub fn direction_with_purity(&self, now: i64, max_age: i64, purity_ppm: i128) -> Direction {
        if !self.complete || self.up < 0 || self.down < 0 || self.as_of > now
            || now - self.as_of > max_age
        {
            return Direction::Unknown;
        }
        let (up, down) = (self.up as i128, self.down as i128);
        let total = up + down;
        if total == 0 {
            return Direction::Empty;
        }
        let net = up - down;
        if net.abs() * 1_000_000 < total * purity_ppm {
            Direction::Neutral
        } else if net > 0 {
            Direction::Up
        } else {
            Direction::Down
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Market {
    pub condition: String,
    pub symbol: String,
    pub start: i64,
    pub end: i64,
    pub up_token: String,
    pub down_token: String,
    pub active: bool,
    pub accepting_orders: bool,
}

impl Market {
    pub fn valid(&self) -> bool {
        (self.symbol == "BTC" || self.symbol == "ETH")
            && self.condition.len() == 66
            && self.condition.starts_with("0x")
            && self.condition[2..].bytes().all(|b| b.is_ascii_hexdigit())
            && self.end.checked_sub(self.start) == Some(300)
            && !self.up_token.is_empty()
            && !self.down_token.is_empty()
            && self.up_token.bytes().all(|b| b.is_ascii_digit())
            && self.down_token.bytes().all(|b| b.is_ascii_digit())
            && self.up_token != self.down_token
    }

    pub fn token(&self, direction: Direction) -> Option<&str> {
        match direction {
            Direction::Up => Some(&self.up_token),
            Direction::Down => Some(&self.down_token),
            _ => None,
        }
    }
}

fn utc_seconds(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T') || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':') || !s.ends_with('Z')
    {
        return None;
    }
    let part = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (year, month, day) = (part(0, 4)?, part(5, 7)?, part(8, 10)?);
    let (hour, minute, second) = (part(11, 13)?, part(14, 16)?, part(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day)
        || hour > 23 || minute > 59 || second > 59
        || !(b.len() == 20 || b.get(19) == Some(&b'.'))
    {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) * 86400 + hour * 3600 + minute * 60 + second)
}

/// Gamma identifies outcomes and token IDs by the same array index.
pub fn market_from_gamma(body: &serde_json::Value, condition: &str) -> Result<Market, String> {
    let actual = body["conditionId"].as_str().ok_or("Gamma conditionId missing")?;
    if !actual.eq_ignore_ascii_case(condition) {
        return Err("Gamma conditionId mismatch".into());
    }
    let slug = body["slug"].as_str().ok_or("Gamma slug missing")?;
    let (symbol, suffix) = if let Some(s) = slug.strip_prefix("btc-updown-5m-") {
        ("BTC", s)
    } else if let Some(s) = slug.strip_prefix("eth-updown-5m-") {
        ("ETH", s)
    } else {
        return Err("market is not BTC/ETH 5m".into());
    };
    let start: i64 = suffix.parse().map_err(|_| "market slug has no start epoch")?;
    let end = utc_seconds(body["endDate"].as_str().ok_or("Gamma endDate missing")?)
        .ok_or("Gamma endDate invalid")?;
    if end - start != 300 {
        return Err("market period is not exactly five minutes".into());
    }
    let parse_array = |key: &str| -> Result<Vec<String>, String> {
        let value = &body[key];
        let parsed = if let Some(s) = value.as_str() {
            serde_json::from_str::<serde_json::Value>(s).map_err(|e| format!("Gamma {key}: {e}"))?
        } else {
            value.clone()
        };
        let items = parsed.as_array().ok_or_else(|| format!("Gamma {key} is not an array"))?;
        items.iter().map(|v| v.as_str().map(str::to_owned)
            .ok_or_else(|| format!("Gamma {key} contains a non-string"))).collect()
    };
    let labels = parse_array("outcomes")?;
    let tokens = parse_array("clobTokenIds")?;
    if labels.len() != 2 || tokens.len() != 2 {
        return Err("market must have exactly two outcomes and tokens".into());
    }
    let up = labels.iter().position(|s| s.eq_ignore_ascii_case("Up"))
        .ok_or("market has no Up outcome")?;
    let down = labels.iter().position(|s| s.eq_ignore_ascii_case("Down"))
        .ok_or("market has no Down outcome")?;
    if up == down {
        return Err("market outcome labels collide".into());
    }
    let market = Market {
        condition: actual.to_ascii_lowercase(),
        symbol: symbol.into(),
        start, end,
        up_token: tokens[up].clone(),
        down_token: tokens[down].clone(),
        active: body["active"].as_bool().unwrap_or(false) && !body["closed"].as_bool().unwrap_or(true),
        accepting_orders: body["acceptingOrders"].as_bool().unwrap_or(false),
    };
    if market.valid() { Ok(market) } else { Err("market metadata invalid".into()) }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub key: String,
    pub token: String,
    pub side: u8, // 0 = BUY, 1 = SELL
    pub shares: i64, // micro-shares
    pub price: i64, // micro-USDC per share
    pub ts: i64,
}

fn ambiguous_order(trades: &[Trade]) -> bool {
    let mut sides: HashMap<(i64, &str), u8> = HashMap::new();
    for t in trades {
        if t.side > 1 { return true; }
        let seen = sides.entry((t.ts, &t.token)).or_default();
        *seen |= 1 << t.side;
        if *seen == 3 { return true; }
    }
    false
}

/// Rebuild Ohio's still-open qualifying cost from a complete, confirmed trade history.
/// A non-trade balance change or an ambiguous duplicate makes this result unknown.
pub fn eligible_directional_cents(
    trades: &[Trade],
    market: &Market,
    direction: Direction,
    book: &Book,
    min_price: i64,
    max_price: i64,
) -> Option<i64> {
    let token = market.token(direction)?;
    let mut ordered = trades.to_vec();
    ordered.sort_by(|a, b| (a.ts, &a.key).cmp(&(b.ts, &b.key)));
    // The feed exposes second-level timestamps. Opposite fills in one second
    // cannot be safely ordered by a hash or an opaque ID.
    if ambiguous_order(&ordered) {
        return None;
    }
    let mut seen = HashSet::new();
    let mut held: HashMap<&str, i128> = HashMap::new();
    let mut eligible_shares = 0i128;
    let mut eligible_cost = 0i128; // micro-USDC
    for trade in &ordered {
        if !seen.insert(&trade.key)
            || trade.key.is_empty()
            || trade.ts < market.start
            || trade.ts > market.end
            || trade.shares <= 0
            || !(1..1_000_000).contains(&trade.price)
            || (trade.token != market.up_token && trade.token != market.down_token)
        {
            return None;
        }
        let before = *held.get(trade.token.as_str()).unwrap_or(&0);
        let shares = trade.shares as i128;
        match trade.side {
            0 => {
                *held.entry(&trade.token).or_default() = before.checked_add(shares)?;
                if trade.token == token && (min_price..=max_price).contains(&trade.price) {
                    eligible_shares = eligible_shares.checked_add(shares)?;
                    eligible_cost = eligible_cost.checked_add(
                        shares.checked_mul(trade.price as i128)? / 1_000_000,
                    )?;
                }
            }
            1 if before >= shares => {
                if trade.token == token && eligible_shares > 0 {
                    let removed = eligible_shares.checked_mul(shares)? / before;
                    let cost_removed = eligible_cost.checked_mul(shares)? / before;
                    eligible_shares -= removed;
                    eligible_cost -= cost_removed;
                }
                *held.entry(&trade.token).or_default() = before - shares;
            }
            _ => return None,
        }
    }
    if held.get(market.up_token.as_str()).copied().unwrap_or(0) != book.up as i128
        || held.get(market.down_token.as_str()).copied().unwrap_or(0) != book.down as i128
        || eligible_shares == 0
    {
        return None;
    }
    let net = (book.up as i128 - book.down as i128).abs();
    let directional_cost = eligible_cost.min(eligible_cost.checked_mul(net)? / eligible_shares);
    i64::try_from(directional_cost / 10_000).ok()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub condition: String,
    pub direction: Direction,
    pub trigger_ts: i64,
    pub trigger_price: i64, // micro-USDC
    pub directional_cents: Option<i64>,
}

pub fn candidate_from_history(
    policy: &Policy,
    market: &Market,
    trades: &[Trade],
) -> Result<Option<Candidate>, String> {
    let mut ordered = trades.to_vec();
    ordered.sort_by(|a, b| (a.ts, &a.key).cmp(&(b.ts, &b.key)));
    if ambiguous_order(&ordered) {
        return Err("same-second opposite fills have unknown order".into());
    }
    let (mut up, mut down) = (0i64, 0i64);
    for (index, trade) in ordered.iter().enumerate() {
        let amount = if trade.side == 0 { trade.shares } else if trade.side == 1 {
            -trade.shares
        } else { return Err("trade side invalid".into()) };
        if trade.token == market.up_token {
            up = up.checked_add(amount).ok_or("up shares overflow")?;
        } else if trade.token == market.down_token {
            down = down.checked_add(amount).ok_or("down shares overflow")?;
        } else {
            return Err("trade outcome token mismatch".into());
        }
        if up < 0 || down < 0 {
            return Err("trade history sells more than it holds".into());
        }
        if trade.side != 0 || !(policy.min_price..=policy.max_price).contains(&trade.price) {
            continue;
        }
        let book = Book { up, down, as_of: trade.ts, complete: true };
        let direction = book.direction_with_purity(trade.ts, 0, policy.purity_ppm);
        if market.token(direction) != Some(trade.token.as_str()) {
            continue;
        }
        // ponytail: the 5-minute market history is small; use an incremental cost ledger if it grows.
        let Some(amount) = eligible_directional_cents(
            &ordered[..=index], market, direction, &book, policy.min_price, policy.max_price,
        ) else { return Err("eligible cost history is incomplete".into()) };
        if amount >= policy.primary_cents {
            return Ok(Some(Candidate {
                condition: market.condition.clone(), direction, trigger_ts: trade.ts,
                trigger_price: trade.price, directional_cents: Some(amount),
            }));
        }
    }
    Ok(None)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub ohio: Book,
    pub std0: Book,
    pub ants: Book,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Wait(&'static str),
    Skip(&'static str),
    Veto,
    Exit,
    Target { total_cents: i64, new_cents: i64 },
}

pub fn decide(
    policy: &Policy,
    market: &Market,
    candidate: &Candidate,
    evidence: &Evidence,
    now: i64,
    our_filled_cents: i64,
    our_pending_cents: i64,
) -> Verdict {
    if !market.valid() || !policy.symbols.contains(&market.symbol)
        || market.condition != candidate.condition {
        return Verdict::Skip("unknown_market");
    }
    if !market.active || !market.accepting_orders || now < market.start
        || market.end - now < policy.min_remaining_secs || candidate.trigger_ts < market.start
        || candidate.trigger_ts > now || now - candidate.trigger_ts > policy.confirm_secs
    {
        return Verdict::Skip("expired");
    }
    if !(policy.min_price..=policy.max_price).contains(&candidate.trigger_price) {
        return Verdict::Skip("trigger_price");
    }
    let ohio = evidence.ohio.direction_with_purity(now, policy.max_age_secs, policy.purity_ppm);
    if ohio == Direction::Unknown || candidate.direction == Direction::Unknown
        || candidate.directional_cents.is_none()
    {
        return Verdict::Wait("ohio_unknown");
    }
    if ohio != candidate.direction || candidate.directional_cents.unwrap_or(0) < policy.primary_cents {
        return Verdict::Exit;
    }
    let std0 = evidence.std0.direction_with_purity(now, policy.max_age_secs, policy.purity_ppm);
    if std0 == ohio.opposite() {
        return Verdict::Veto;
    }
    if std0 == Direction::Unknown {
        return Verdict::Wait("std0_unknown");
    }
    if std0 != ohio {
        return Verdict::Wait("std0_confirmation");
    }
    let ants = evidence.ants.direction_with_purity(now, policy.max_age_secs, policy.purity_ppm);
    if ants == ohio.opposite() {
        return Verdict::Veto;
    }
    if ants == Direction::Unknown {
        return Verdict::Wait("ants_unknown");
    }
    let total_cents = if ants == ohio { policy.three_wallet_cents } else { policy.two_wallet_cents };
    Verdict::Target {
        total_cents,
        new_cents: (total_cents - our_filled_cents.max(0) - our_pending_cents.max(0)).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(up: i64, down: i64) -> Book {
        Book { up, down, as_of: 100, complete: true }
    }

    fn market() -> Market {
        Market { condition: format!("0x{}", "a".repeat(64)), symbol: "BTC".into(),
            start: 0, end: 300, up_token: "11".into(), down_token: "22".into(),
            active: true, accepting_orders: true }
    }

    fn policy() -> Policy { Policy::from(&crate::config::ConsensusToml::default()) }

    #[test]
    fn purity_uses_both_sides_and_includes_the_boundary() {
        assert_eq!(book(30_000_000, 8_000_000).direction(100, 10), Direction::Neutral);
        assert_eq!(book(30_000_000, 7_500_000).direction(100, 10), Direction::Up);
        assert_eq!(book(35_300_000, 135_000_000).direction(100, 10), Direction::Neutral);
        assert_eq!(book(0, 0).direction(100, 10), Direction::Empty);
        assert_eq!(book(0, 0).direction(111, 10), Direction::Unknown);
    }

    #[test]
    fn selling_removes_qualifying_cost_and_a_hedge_reduces_directional_risk() {
        let m = market();
        let trades = vec![
            Trade { key: "a".into(), token: "11".into(), side: 0, shares: 20_000_000, price: 600_000, ts: 10 },
            Trade { key: "b".into(), token: "11".into(), side: 1, shares: 15_000_000, price: 500_000, ts: 20 },
        ];
        assert_eq!(eligible_directional_cents(&trades, &m, Direction::Up, &book(5_000_000, 0), 200_000, 700_000), Some(300));
        let mut hedged = trades;
        hedged.push(Trade { key: "c".into(), token: "22".into(), side: 0, shares: 4_000_000, price: 800_000, ts: 30 });
        assert_eq!(eligible_directional_cents(&hedged, &m, Direction::Up, &book(5_000_000, 4_000_000), 200_000, 700_000), Some(60));
    }

    #[test]
    fn opposite_fills_in_the_same_second_are_unknown() {
        let m = market();
        let trades = vec![
            Trade { key: "a".into(), token: "11".into(), side: 0,
                shares: 20_000_000, price: 600_000, ts: 10 },
            Trade { key: "b".into(), token: "22".into(), side: 0,
                shares: 1_000_000, price: 500_000, ts: 10 },
            Trade { key: "c".into(), token: "11".into(), side: 1,
                shares: 5_000_000, price: 500_000, ts: 10 },
        ];
        assert_eq!(eligible_directional_cents(&trades, &m, Direction::Up,
            &book(15_000_000, 1_000_000), 200_000, 700_000), None);
        assert!(candidate_from_history(&policy(), &m, &trades).is_err());
    }

    #[test]
    fn upgrades_only_the_unfilled_remainder_and_unknown_blocks_buying() {
        let m = market();
        let c = Candidate { condition: m.condition.clone(), direction: Direction::Up,
            trigger_ts: 90, trigger_price: 600_000, directional_cents: Some(1_200) };
        let mut e = Evidence { ohio: book(20_000_000, 0), std0: book(10_000_000, 0), ants: book(0, 0) };
        assert_eq!(decide(&policy(), &m, &c, &e, 100, 0, 0), Verdict::Target { total_cents: 500, new_cents: 500 });
        e.ants = book(10_000_000, 0);
        assert_eq!(decide(&policy(), &m, &c, &e, 100, 500, 300), Verdict::Target { total_cents: 1_000, new_cents: 200 });
        e.ants.complete = false;
        assert_eq!(decide(&policy(), &m, &c, &e, 100, 500, 0), Verdict::Wait("ants_unknown"));
    }

    #[test]
    fn market_metadata_rejects_other_intervals_and_maps_outcomes_by_label() {
        let m = market();
        let mut row = serde_json::json!({
            "conditionId": m.condition, "slug":"btc-updown-5m-0",
            "endDate":"1970-01-01T00:05:00Z", "active":true, "closed":false,
            "acceptingOrders":true, "outcomes":"[\"Down\",\"Up\"]",
            "clobTokenIds":"[\"22\",\"11\"]"
        });
        let parsed = market_from_gamma(&row, row["conditionId"].as_str().unwrap()).unwrap();
        assert_eq!((&*parsed.up_token, &*parsed.down_token), ("11", "22"));
        row["endDate"] = serde_json::json!("1970-01-01T00:06:00Z");
        assert!(market_from_gamma(&row, row["conditionId"].as_str().unwrap()).is_err());
        row["slug"] = serde_json::json!("btc-updown-15m-0");
        assert!(market_from_gamma(&row, row["conditionId"].as_str().unwrap()).is_err());
    }

    #[test]
    fn candidate_is_the_first_buy_crossing_ten_dollars() {
        let m = market();
        let t = vec![
            Trade { key: "a".into(), token: "11".into(), side: 0, shares: 10_000_000, price: 600_000, ts: 10 },
            Trade { key: "b".into(), token: "11".into(), side: 0, shares: 10_000_000, price: 600_000, ts: 20 },
            Trade { key: "c".into(), token: "11".into(), side: 0, shares: 10_000_000, price: 600_000, ts: 30 },
        ];
        assert_eq!(candidate_from_history(&policy(), &m, &t).unwrap().unwrap().trigger_ts, 20);
    }
}
