use crate::config::ConsensusToml;
use crate::consensus::{self, Book, Evidence, Market, Policy, Verdict};
use crate::data_api_v2::{self, number_micros};
use crate::feeds::RawTx;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Default)]
struct SimPosition {
    cents: i64,
    shares: i64,
}

struct Journal {
    file: std::fs::File,
    positions: HashMap<String, SimPosition>,
    daily_spent: HashMap<i64, i64>,
    candidates: HashSet<String>,
    vetoed: HashSet<String>,
    exited: HashSet<String>,
    tentative: HashMap<String, i64>,
    last_verdict: HashMap<String, String>,
}

fn position_key(condition: &str, token: &str) -> String {
    format!("{condition}:{token}")
}

impl Journal {
    fn open(path: &str) -> Result<Self, String> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("consensus journal directory: {e}"))?;
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).read(true)
            .open(path).map_err(|e| format!("consensus journal: {e}"))?;
        let reader = std::io::BufReader::new(file.try_clone().map_err(|e| e.to_string())?);
        let mut journal = Self {
            file, positions: HashMap::new(), daily_spent: HashMap::new(),
            candidates: HashSet::new(), vetoed: HashSet::new(), exited: HashSet::new(),
            tentative: HashMap::new(), last_verdict: HashMap::new(),
        };
        for (line_no, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| format!("consensus journal line {}: {e}", line_no + 1))?;
            if line.is_empty() { continue; }
            let row: Value = serde_json::from_str(&line)
                .map_err(|e| format!("consensus journal line {}: {e}", line_no + 1))?;
            journal.apply(&row)?;
        }
        Ok(journal)
    }

    fn apply(&mut self, row: &Value) -> Result<(), String> {
        let ev = row["ev"].as_str().ok_or("consensus journal event missing")?;
        let condition = row["condition"].as_str().unwrap_or("");
        let token = row["token"].as_str().unwrap_or("");
        let key = position_key(condition, token);
        match ev {
            "consensus_candidate" => { self.candidates.insert(key); }
            "consensus_veto" => { self.vetoed.insert(condition.into()); }
            "consensus_tentative" => {
                let id = row["id"].as_str().ok_or("tentative id missing")?;
                let at = row["t"].as_i64().ok_or("tentative time missing")?;
                self.tentative.insert(id.into(), at);
            }
            "consensus_tentative_expired" => {
                let id = row["id"].as_str().ok_or("expired tentative id missing")?;
                self.tentative.remove(id);
            }
            "consensus_sim_fill" => {
                let cents = row["cents"].as_i64().filter(|n| *n > 0).ok_or("fill cents invalid")?;
                let shares = row["shares"].as_i64().filter(|n| *n > 0).ok_or("fill shares invalid")?;
                let day = row["t"].as_i64().ok_or("fill time missing")?.div_euclid(86400);
                let p = self.positions.entry(key).or_default();
                p.cents = p.cents.checked_add(cents).ok_or("fill amount overflow")?;
                p.shares = p.shares.checked_add(shares).ok_or("fill shares overflow")?;
                let spent = self.daily_spent.entry(day).or_default();
                *spent = spent.checked_add(cents).ok_or("daily spent overflow")?;
            }
            "consensus_sim_exit" => {
                self.positions.remove(&key);
                self.exited.insert(key);
            }
            "consensus_decision" => {
                let reason = row["reason"].as_str().ok_or("decision reason missing")?;
                self.last_verdict.insert(key, reason.into());
            }
            "consensus_unknown" | "consensus_quote_skip" => {}
            _ => return Err(format!("unknown consensus journal event {ev}")),
        }
        Ok(())
    }

    fn append(&mut self, row: Value) -> Result<(), String> {
        writeln!(self.file, "{row}").map_err(|e| format!("consensus journal append: {e}"))?;
        self.file.sync_data().map_err(|e| format!("consensus journal sync: {e}"))?;
        self.apply(&row)
    }
}

fn unknown(now: i64) -> Book {
    Book { up: 0, down: 0, as_of: now, complete: false }
}

async fn market_by_slug(http: &reqwest::Client, slug: &str, condition: &str) -> Result<Market, String> {
    if !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("market slug has invalid characters".into());
    }
    let url = format!("https://gamma-api.polymarket.com/markets/slug/{slug}");
    let response = http.get(&url).send().await.map_err(|e| format!("Gamma transport: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Gamma HTTP {}", response.status().as_u16()));
    }
    let body: Value = response.json().await.map_err(|e| format!("Gamma JSON: {e}"))?;
    consensus::market_from_gamma(&body, condition)
}

async fn confirmed_book(
    http: &reqwest::Client,
    rpc: &str,
    rows: Result<&Vec<Value>, &String>,
    wallet: &str,
    market: &Market,
) -> Result<Book, String> {
    let as_of = crate::ledger::now_secs();
    let book = data_api_v2::position_book(rows.map_err(Clone::clone)?, wallet, market, as_of)?;
    let (up, down) = tokio::try_join!(
        crate::txsend::ctf_balance(http, rpc, wallet, &market.up_token),
        crate::txsend::ctf_balance(http, rpc, wallet, &market.down_token),
    )?;
    if (number_micros(&json!(up)).ok_or("chain Up balance invalid")? - book.up).abs() > 1
        || (number_micros(&json!(down)).ok_or("chain Down balance invalid")? - book.down).abs() > 1
    {
        return Err("v2 position snapshot has not caught up with chain balances".into());
    }
    Ok(book)
}

async fn top_price(http: &reqwest::Client, clob: &str, token: &str, buy: bool)
    -> Result<(i64, i64), String> {
    let mut url = reqwest::Url::parse(&format!("{}/book", clob.trim_end_matches('/')))
        .map_err(|e| format!("CLOB book URL: {e}"))?;
    url.query_pairs_mut().append_pair("token_id", token);
    let response = http.get(url).send().await.map_err(|e| format!("CLOB book transport: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("CLOB book HTTP {}", response.status().as_u16()));
    }
    let body: Value = response.json().await.map_err(|e| format!("CLOB book JSON: {e}"))?;
    let rows = body[if buy { "asks" } else { "bids" }].as_array().ok_or("CLOB side missing")?;
    rows.iter().filter_map(|v| Some((number_micros(&v["price"])?, number_micros(&v["size"])?)))
        .filter(|(price, size)| *price > 0 && *size > 0)
        .min_by_key(|(price, _)| if buy { *price } else { -*price })
        .ok_or("CLOB side empty".into())
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub daily_cents: i64,
    pub max_open_cents: i64,
    pub per_fill_cents: i64,
    pub min_order_cents: i64,
    pub buy_slippage_micro: i64,
}

async fn poll(
    http: &reqwest::Client, rpc: &str, clob: &str, policy: &Policy,
    limits: Limits, journal: &mut Journal,
) -> Result<(), String> {
    let now = crate::ledger::now_secs();
    let start = (now - 360).to_string();
    let recent = data_api_v2::fetch_all(http, data_api_v2::BASE, "/v2/trades",
        &[("user", consensus::OHIO), ("start", &start)]).await?;
    let mut markets: HashMap<(String, String), Vec<Value>> = HashMap::new();
    for row in recent {
        let slug = row["slug"].as_str().unwrap_or("");
        if !slug.starts_with("btc-updown-5m-") && !slug.starts_with("eth-updown-5m-") {
            continue;
        }
        let condition = row["condition_id"].as_str().ok_or("recent trade condition missing")?;
        markets.entry((condition.to_ascii_lowercase(), slug.into())).or_default().push(row);
    }
    if markets.len() > 10 { return Err("too many recent target markets".into()); }
    if markets.is_empty() { return Ok(()); }
    let (ohio, std0, ants) = tokio::join!(
        data_api_v2::fetch_all(http, data_api_v2::BASE, "/v2/positions",
            &[("user", consensus::OHIO), ("status", "OPEN"), ("filter_type", "TOKENS"), ("filter_amount", "0")]),
        data_api_v2::fetch_all(http, data_api_v2::BASE, "/v2/positions",
            &[("user", consensus::STD0), ("status", "OPEN"), ("filter_type", "TOKENS"), ("filter_amount", "0")]),
        data_api_v2::fetch_all(http, data_api_v2::BASE, "/v2/positions",
            &[("user", consensus::ANTS), ("status", "OPEN"), ("filter_type", "TOKENS"), ("filter_amount", "0")]),
    );
    for ((condition, slug), _) in markets {
        let market = match market_by_slug(http, &slug, &condition).await {
            Ok(m) => m,
            Err(why) => {
                journal.append(json!({"t":now,"ev":"consensus_unknown","condition":condition,"why":why}))?;
                continue;
            }
        };
        if !policy.symbols.contains(&market.symbol) || now >= market.end { continue; }
        let history = data_api_v2::fetch_all(http, data_api_v2::BASE, "/v2/trades",
            &[("user", consensus::OHIO), ("condition", &market.condition)]).await;
        let trades: Vec<consensus::Trade> = match history {
            Ok(rows) => rows.iter().map(|v| data_api_v2::trade(v, &market)).collect::<Result<_,_>>()?,
            Err(e) => { journal.append(json!({"t":now,"ev":"consensus_unknown","condition":condition,"why":e}))?; continue; }
        };
        let Some(mut candidate) = consensus::candidate_from_history(policy, &market, &trades)? else { continue };
        let token = market.token(candidate.direction).ok_or("candidate outcome unknown")?.to_string();
        let key = position_key(&market.condition, &token);
        if !journal.candidates.contains(&key) {
            journal.append(json!({"t":now,"ev":"consensus_candidate","condition":market.condition,
                "token":token,"trigger_ts":candidate.trigger_ts,"trigger_price":candidate.trigger_price,
                "direction":candidate.direction,"symbol":market.symbol}))?;
        }
        let (o, s, a) = tokio::join!(
            confirmed_book(http, rpc, ohio.as_ref(), consensus::OHIO, &market),
            confirmed_book(http, rpc, std0.as_ref(), consensus::STD0, &market),
            confirmed_book(http, rpc, ants.as_ref(), consensus::ANTS, &market),
        );
        let evidence = Evidence { ohio: o.unwrap_or_else(|_| unknown(now)),
            std0: s.unwrap_or_else(|_| unknown(now)), ants: a.unwrap_or_else(|_| unknown(now)) };
        candidate.directional_cents = if evidence.ohio.complete {
            consensus::eligible_directional_cents(&trades, &market, candidate.direction,
                &evidence.ohio, policy.min_price, policy.max_price)
        } else { None };
        let ohio_direction = evidence.ohio.direction_with_purity(
            now, policy.max_age_secs, policy.purity_ppm);
        if journal.positions.get(&key).is_some_and(|p| p.cents > 0)
            && ohio_direction != consensus::Direction::Unknown
            && ohio_direction != candidate.direction
        {
            if let Ok((bid, _)) = top_price(http, clob, &token, false).await {
                let p = &journal.positions[&key];
                let proceeds = (p.shares as i128 * bid as i128 / 10_000_000_000) as i64;
                journal.append(json!({"t":now,"ev":"consensus_sim_exit","condition":market.condition,
                    "token":token,"proceeds_cents":proceeds,"bid":bid}))?;
            }
            continue;
        }
        if journal.vetoed.contains(&market.condition) || journal.exited.contains(&key) { continue; }
        let filled = journal.positions.get(&key).map(|p| p.cents).unwrap_or(0);
        let verdict = consensus::decide(policy, &market, &candidate, &evidence, now, filled, 0);
        let reason = format!("{verdict:?}");
        if journal.last_verdict.get(&key) != Some(&reason) {
            journal.append(json!({"t":now,"ev":"consensus_decision","condition":market.condition,
                "token":token,"reason":reason,"candidate":candidate,"market":market,
                "evidence":evidence,"our_filled_cents":filled,"our_pending_cents":0}))?;
        }
        match verdict {
            Verdict::Veto => {
                journal.append(json!({"t":now,"ev":"consensus_veto","condition":market.condition,"token":token}))?;
            }
            Verdict::Target { new_cents, .. } if new_cents > 0 => {
                let (ask, available) = match top_price(http, clob, &token, true).await {
                    Ok(v) => v,
                    Err(e) => { journal.append(json!({"t":now,"ev":"consensus_quote_skip",
                        "condition":market.condition,"token":token,"why":e}))?; continue; }
                };
                let proposed = ask.saturating_add(limits.buy_slippage_micro);
                if proposed < policy.min_price || proposed > policy.max_price {
                    journal.append(json!({"t":now,"ev":"consensus_quote_skip","condition":market.condition,
                        "token":token,"why":"price_out_of_band","ask":ask,"proposed":proposed}))?;
                    continue;
                }
                let open: i64 = journal.positions.values().map(|p| p.cents).sum();
                let day = now.div_euclid(86400);
                let budgeted = new_cents.min(limits.per_fill_cents)
                    .min(limits.max_open_cents.saturating_sub(open))
                    .min(limits.daily_cents.saturating_sub(*journal.daily_spent.get(&day).unwrap_or(&0)));
                let available_cents = (available as i128 * ask as i128 / 10_000_000_000) as i64;
                let cents = budgeted.min(available_cents);
                if cents < limits.min_order_cents {
                    journal.append(json!({"t":now,"ev":"consensus_quote_skip","condition":market.condition,
                        "token":token,"why":"size_or_budget","budgeted_cents":budgeted,
                        "available_cents":available_cents}))?;
                    continue;
                }
                let shares = (cents as i128 * 10_000_000_000 / ask as i128) as i64;
                journal.append(json!({"t":now,"ev":"consensus_sim_fill","condition":market.condition,
                    "token":token,"cents":cents,"shares":shares,"ask":ask,"proposed":proposed}))?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub async fn run(
    cfg: ConsensusToml, clob: String, rpc: String, limits: Limits,
    mut tentative_rx: mpsc::UnboundedReceiver<RawTx>,
) {
    let mut journal = match Journal::open(&cfg.events_path) {
        Ok(j) => j,
        Err(e) => { eprintln!("[consensus] REFUSING observation: {e}"); return; }
    };
    let http = reqwest::Client::builder().timeout(Duration::from_secs(8))
        .build().unwrap_or_default();
    let policy = Policy::from(&cfg);
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let mut last_error = String::new();
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let now = crate::ledger::now_secs();
                let expired: Vec<String> = journal.tentative.iter()
                    .filter(|(_, at)| now - **at > policy.confirm_secs)
                    .map(|(id, _)| id.clone()).collect();
                for id in expired {
                    if let Err(e) = journal.append(json!({"t":now,
                        "ev":"consensus_tentative_expired","id":id})) {
                        eprintln!("[consensus] REFUSING observation: {e}"); return;
                    }
                }
                match poll(&http, &rpc, &clob, &policy, limits, &mut journal).await {
                    Ok(()) => last_error.clear(),
                    Err(e) if e != last_error => {
                        eprintln!("[consensus] observation UNKNOWN: {e}");
                        if journal.append(json!({"t":crate::ledger::now_secs(),
                            "ev":"consensus_unknown","why":e})).is_err() { return; }
                        last_error = e;
                    }
                    Err(_) => {}
                }
            }
            raw = tentative_rx.recv() => {
                let Some(raw) = raw else { return };
                for (role, wallet) in [("ohio", consensus::OHIO), ("std0", consensus::STD0),
                    ("ants", consensus::ANTS)] {
                    let Ok(address) = crate::config::addr20(wallet) else { continue };
                    for d in crate::calldata::decode_all(&raw.input, &address) {
                        let id = format!("{}:{role}:{}:{}", raw.hash.to_ascii_lowercase(),
                            d.occurrence, d.token_id);
                        if journal.tentative.contains_key(&id) { continue; }
                        if let Err(e) = journal.append(json!({"t":crate::ledger::now_secs(),
                            "ev":"consensus_tentative","id":id,"role":role,"tx":raw.hash,
                            "condition":format!("0x{}",hex::encode(d.condition_id)),
                            "token":d.token_id,"side":d.side,"price":d.price,"shares":d.fill_size})) {
                            eprintln!("[consensus] REFUSING observation: {e}"); return;
                        }
                    }
                }
            }
        }
    }
}
