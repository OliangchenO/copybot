use crate::feeds::{RawTx, CTF_EXCHANGE_V2, NEG_RISK_CTF_EXCHANGE_V2};
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Semaphore};

pub const MAX_AGE_MS: i64 = 10_000;

#[derive(Debug, Clone, Deserialize)]
pub struct Signal {
    pub t: i64,
    pub tx: String,
    pub token: String,
    pub side: String,
    pub size: f64,
    pub lane: String,
    #[serde(skip)]
    pub quote: f64,
    #[serde(skip)]
    pub quote_at: i64,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

impl Signal {
    pub fn valid_at(&self, now: i64) -> bool {
        self.t > 0 && now >= self.t && now - self.t <= MAX_AGE_MS
            && self.tx.len() == 66 && self.tx.starts_with("0x")
            && self.tx[2..].bytes().all(|b| b.is_ascii_hexdigit())
            && !self.token.is_empty() && self.token.bytes().all(|b| b.is_ascii_digit())
            && matches!(self.side.as_str(), "BUY" | "SELL")
            && self.size.is_finite() && self.size > 0.0 && !self.lane.is_empty()
    }
    pub fn matches(&self, lane: &str, tx: &str, token: &str, side: u8, size: f64) -> bool {
        self.lane == lane && self.tx.eq_ignore_ascii_case(tx) && self.token == token
            && (self.side == "BUY") == (side == 0)
            && (self.size - size).abs() <= 0.5_f64.max(self.size.max(size) * 0.01)
    }
    pub fn quote_ok(&self, now: i64, limit: f64) -> bool {
        self.valid_at(now) && self.quote_at > 0 && now >= self.quote_at
            && now - self.quote_at <= 2_000 && self.quote > 0.0
            && if self.side == "BUY" { self.quote <= limit + 1e-9 }
               else { self.quote + 1e-9 >= limit }
    }
}

fn number(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok()).filter(|x| x.is_finite() && *x > 0.0)
}

pub fn executable_quote(book: &serde_json::Value, side: &str) -> Option<f64> {
    let levels = book[if side == "BUY" { "asks" } else { "bids" }].as_array()?;
    if side == "BUY" { levels.iter().filter_map(|v| number(&v["price"])).reduce(f64::min) }
    else { levels.iter().filter_map(|v| number(&v["price"])).reduce(f64::max) }
}

fn raw_from_rpc(v: &serde_json::Value, signal: Signal) -> Option<RawTx> {
    let r = &v["result"];
    let hash = r["hash"].as_str()?;
    if !hash.eq_ignore_ascii_case(&signal.tx) { return None; }
    let to = r["to"].as_str()?.trim_start_matches("0x").to_ascii_lowercase();
    if to != CTF_EXCHANGE_V2 && to != NEG_RISK_CTF_EXCHANGE_V2 { return None; }
    let input = hex::decode(r["input"].as_str()?.trim_start_matches("0x")).ok()?;
    let seen_ns = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some(RawTx {
        source: "watcher_recovery".into(), hash: hash.into(), input,
        to_neg_risk: to == NEG_RISK_CTF_EXCHANGE_V2, to, seen_ns,
        recovery: Some(signal),
    })
}

async fn fetch_signal(
    client: &reqwest::Client, rpc: &str, clob: &str, mut signal: Signal,
) -> Option<RawTx> {
    let request = serde_json::json!({"jsonrpc":"2.0","id":1,
        "method":"eth_getTransactionByHash","params":[signal.tx]});
    let mut attempts = 0;
    while signal.valid_at(now_ms()) && attempts < 5 {
        attempts += 1;
        let transaction: Option<serde_json::Value> = match client.post(rpc).json(&request).send().await {
            Ok(resp) if resp.status().is_success() => resp.json().await.ok(),
            _ => None,
        };
        if let Some(v) = transaction {
            if v["result"].is_object() {
                let book: Option<serde_json::Value> = match client.get(format!("{clob}/book"))
                    .query(&[("token_id", &signal.token)]).send().await {
                    Ok(resp) if resp.status().is_success() => resp.json().await.ok(),
                    _ => None,
                };
                if let Some(quote) = book.as_ref().and_then(|b| executable_quote(b, &signal.side)) {
                    signal.quote = quote;
                    signal.quote_at = now_ms();
                    return raw_from_rpc(&v, signal);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    None
}

pub async fn run(
    path: String, rpc: String, clob: String, out: mpsc::UnboundedSender<RawTx>,
    events: mpsc::UnboundedSender<serde_json::Value>,
) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().expect("recovery HTTP");
    let mut offset = 0u64;
    let mut partial = String::new();
    let mut seen = std::collections::HashMap::<(String, String, String, u64), i64>::new();
    let slots = Arc::new(Semaphore::new(16));
    loop {
        if let Ok(mut file) = std::fs::File::open(Path::new(&path)) {
            if let Ok(len) = file.metadata().map(|m| m.len()) {
                if len < offset { offset = 0; partial.clear(); }
                if offset == 0 && len > 1_000_000 {
                    offset = len - 1_000_000;
                    partial.clear();
                }
                if file.seek(SeekFrom::Start(offset)).is_ok() {
                    let mut bytes = Vec::new();
                    if file.take(1_000_000).read_to_end(&mut bytes).is_ok() {
                        offset += bytes.len() as u64;
                        partial.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(end) = partial.find('\n') {
                            let line: String = partial.drain(..=end).collect();
                            let Ok(signal) = serde_json::from_str::<Signal>(&line) else { continue };
                            let now = now_ms();
                            if !signal.valid_at(now) { continue; }
                            let key = (signal.tx.to_ascii_lowercase(), signal.token.clone(),
                                signal.side.clone(), (signal.size * 1e6) as u64);
                            seen.retain(|_, at| now - *at <= MAX_AGE_MS);
                            if seen.insert(key, now).is_some() { continue; }
                            let (client, rpc, clob, out, events, slots) =
                                (client.clone(), rpc.clone(), clob.clone(), out.clone(), events.clone(), slots.clone());
                            tokio::spawn(async move {
                                let tx = signal.tx.clone();
                                let token = signal.token.clone();
                                let lane = signal.lane.clone();
                                let result = if let Ok(_slot) = slots.acquire().await {
                                    if signal.valid_at(now_ms()) {
                                        fetch_signal(&client, &rpc, &clob, signal).await
                                    } else { None }
                                } else { None };
                                let ok = result.as_ref().is_some_and(|raw| out.send(raw.clone()).is_ok());
                                let _ = events.send(serde_json::json!({"t":now_ms(),
                                    "ev":if ok {"recovery_queued"} else {"recovery_unavailable"},
                                    "tx":tx,"tok":token,"lane":lane}));
                            });
                        }
                        if partial.len() > 1_000_000 { partial.clear(); }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signal(now: i64) -> Signal {
        Signal { t: now, tx: format!("0x{}", "a".repeat(64)), token: "123".into(),
            side: "BUY".into(), size: 5.0, lane: "lane".into(), quote: 0.55, quote_at: now }
    }
    #[test]
    fn stale_or_unpriced_recovery_cannot_trade() {
        let now = 100_000;
        let mut s = signal(now);
        assert!(s.quote_ok(now + 1, 0.56));
        assert!(!s.quote_ok(now + 1, 0.54));
        assert!(!s.quote_ok(now + 2_001, 0.56));
        s.quote_at = now + 9_000;
        assert!(!s.quote_ok(now + MAX_AGE_MS + 1, 0.56));
    }
    #[test]
    fn identity_requires_same_transaction_lane_token_side_and_size() {
        let s = signal(100_000);
        assert!(s.matches("lane", &s.tx, "123", 0, 5.0));
        assert!(!s.matches("other", &s.tx, "123", 0, 5.0));
        assert!(!s.matches("lane", &s.tx, "123", 1, 5.0));
        assert!(!s.matches("lane", &s.tx, "123", 0, 7.0));
    }
    #[test]
    fn book_selects_executable_side_not_midpoint() {
        let b = serde_json::json!({"bids":[{"price":"0.4"},{"price":"0.42"}],
            "asks":[{"price":"0.57"},{"price":"0.55"}]});
        assert_eq!(executable_quote(&b, "BUY"), Some(0.55));
        assert_eq!(executable_quote(&b, "SELL"), Some(0.42));
    }
}
