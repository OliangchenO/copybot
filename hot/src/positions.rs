use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, PartialEq)]
pub enum Completeness {
    Complete,
    Truncated { pages: usize, cap: usize },
    Failed { after_pages: usize, why: String },
}
impl Completeness {
    pub fn is_complete(&self) -> bool {
        matches!(self, Completeness::Complete)
    }
    pub fn reason(&self) -> String {
        match self {
            Completeness::Complete => "complete".into(),
            Completeness::Truncated { pages, cap } => {
                format!("truncated after {pages} page(s) at the {cap}-page budget")
            }
            Completeness::Failed { after_pages, why } => {
                format!("page {} failed: {why}", after_pages + 1)
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct Positions {
    pub rows: Vec<serde_json::Value>,
    pub completeness: Completeness,
    pub as_of: i64,
}
pub const PAGE: usize = 1000;
pub const MAX_PAGES: usize = 80;
impl Positions {
    /// A malformed balance row cannot establish that any token is absent.
    pub fn confirms_zero(&self, token: &str, not_before: i64) -> bool {
        if !self.may_act_destructively() || self.as_of < not_before {
            return false;
        }
        for row in &self.rows {
            let Some(asset) = row["asset"].as_str().filter(|s| !s.is_empty()) else {
                return false;
            };
            let Some(size) = row["size"].as_f64()
                .or_else(|| row["size"].as_str().and_then(|s| s.parse().ok())) else {
                return false;
            };
            if !size.is_finite() || size < 0.0 || (asset == token && size > 1e-9) {
                return false;
            }
        }
        true
    }
    pub fn empty(as_of: i64) -> Self {
        Self {
            rows: Vec::new(),
            completeness: Completeness::Complete,
            as_of,
        }
    }
    pub fn may_act_destructively(&self) -> bool {
        self.completeness.is_complete()
    }
    pub fn by_asset(&self) -> HashMap<String, f64> {
        let mut out = HashMap::with_capacity(self.rows.len());
        for r in &self.rows {
            let Some(a) = r["asset"].as_str() else { continue };
            let sz = r["size"]
                .as_f64()
                .or_else(|| r["size"].as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0.0);
            if sz.is_finite() {
                *out.entry(a.to_string()).or_insert(0.0) += sz;
            }
        }
        out
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}
#[derive(serde::Deserialize)]
struct V2Page {
    data: Vec<serde_json::Value>,
    pagination: V2Pagination,
}
#[derive(serde::Deserialize)]
struct V2Pagination {
    has_more: bool,
    next_cursor: Option<String>,
}
fn adapt_v2_row(mut row: serde_json::Value, user: &str) -> Result<serde_json::Value, String> {
    let obj = row.as_object_mut().ok_or("position row is not an object")?;
    if !obj.get("proxy_wallet").and_then(|v| v.as_str())
        .is_some_and(|v| v.eq_ignore_ascii_case(user))
    {
        return Err("position wallet does not match requested user".into());
    }
    if !obj.get("token_id").and_then(|v| v.as_str()).is_some_and(|v| !v.is_empty()) {
        return Err("position has no token_id".into());
    }
    let size = obj.get("current_size")
        .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .ok_or("position has no current_size")?;
    if !size.is_finite() || size < 0.0 {
        return Err("position has invalid current_size".into());
    }
    // Keep the v2 fields and expose the v1 names used by existing consumers.
    for (from, to) in [
        ("proxy_wallet", "proxyWallet"), ("token_id", "asset"),
        ("condition_id", "conditionId"), ("current_size", "size"),
        ("avg_price", "avgPrice"), ("entry_cost_usdc", "initialValue"),
        ("total_cost_usdc", "grossInitialValue"),
        ("entry_fees_usdc", "entryFeesUsdc"),
        ("current_value", "currentValue"), ("unrealized_pnl", "cashPnl"),
        ("percent_pnl", "percentPnl"), ("total_size", "totalBought"),
        ("realized_pnl", "realizedPnl"),
        ("percent_realized_pnl", "percentRealizedPnl"),
        ("current_price", "curPrice"), ("event_slug", "eventSlug"),
        ("outcome_index", "outcomeIndex"),
        ("opposite_outcome", "oppositeOutcome"),
        ("opposite_token_id", "oppositeAsset"),
        ("end_date", "endDate"), ("negative_risk", "negativeRisk"),
    ] {
        if let Some(value) = obj.get(from).cloned() {
            obj.insert(to.into(), value);
        }
    }
    Ok(row)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(asset: &str, size: &str) -> serde_json::Value {
        serde_json::json!({ "asset" : asset, "size" : size })
    }
    #[test]
    fn v2_row_maps_the_balance_fields_and_rejects_wrong_wallet() {
        let raw = serde_json::json!({
            "proxy_wallet": "0xabc", "token_id": "T", "current_size": 2.5,
            "current_price": 0.6, "redeemable": true
        });
        let row = adapt_v2_row(raw.clone(), "0xAbC").unwrap();
        assert_eq!(row["asset"], "T");
        assert_eq!(row["size"], 2.5);
        assert_eq!(row["curPrice"], 0.6);
        assert_eq!(row["redeemable"], true);
        assert!(adapt_v2_row(raw, "0xdef").is_err());
    }
    #[test]
    fn DESTRUCTIVE_action_is_refused_on_anything_but_a_complete_read() {
        let mk = |c: Completeness| Positions {
            rows: vec![],
            completeness: c,
            as_of: 0,
        };
        assert!(mk(Completeness::Complete).may_act_destructively());
        assert!(
            ! mk(Completeness::Truncated { pages : 50, cap : 50 })
            .may_act_destructively()
        );
        assert!(
            ! mk(Completeness::Failed { after_pages : 1, why : "x".into() })
            .may_act_destructively()
        );
    }
    #[test]
    fn by_asset_parses_BOTH_numeric_and_string_sizes_and_sums_duplicates() {
        let p = Positions {
            rows: vec![
                row("T", "10.5"), serde_json::json!({ "asset" : "U", "size" : 3.25 }),
                row("T", "1.5"), serde_json::json!({ "nope" : 1 })
            ],
            completeness: Completeness::Complete,
            as_of: 0,
        };
        let m = p.by_asset();
        assert!((m["T"] - 12.0).abs() < 1e-9, "duplicate rows for one asset must SUM");
        assert!((m["U"] - 3.25).abs() < 1e-9);
        assert_eq!(m.len(), 2, "a row with no asset is skipped, not fatal");
    }
}
pub async fn fetch(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    size_threshold: &str,
    extra: &str,
    now: i64,
) -> Positions {
    let include_archived = match extra {
        "" => "false",
        "&includeArchived=true" => "true",
        _ => return Positions {
            rows: Vec::new(),
            completeness: Completeness::Failed {
                after_pages: 0,
                why: format!("unsupported positions filter: {extra}"),
            },
            as_of: now,
        },
    };
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut seen_tokens = HashSet::new();
    let mut completeness = Completeness::Truncated { pages: MAX_PAGES, cap: MAX_PAGES };
    for page in 0..MAX_PAGES {
        let mut request = http.get(format!("{base}/v2/positions")).query(&[
            ("user", user), ("limit", "1000"), ("filter_type", "TOKENS"),
            ("filter_amount", size_threshold), ("sort_by", "TOKENS"),
            ("include_archived", include_archived),
        ]);
        if let Some(value) = &cursor {
            request = request.query(&[("cursor", value)]);
        }
        let got = match request.send().await {
            Ok(r) if r.status().is_success() => {
                r.json::<V2Page>()
                    .await
                    .map_err(|e| format!("decode: {e}"))
            }
            Ok(r) => Err(format!("http {}", r.status().as_u16())),
            Err(e) => Err(format!("transport: {e}")),
        };
        let result = got.and_then(|batch| {
            if batch.data.len() > PAGE || (batch.pagination.has_more && batch.data.is_empty()) {
                return Err("invalid page size".into());
            }
            for raw in batch.data {
                let row = adapt_v2_row(raw, user)?;
                let token = row["asset"].as_str().unwrap();
                if !seen_tokens.insert(token.to_owned()) {
                    return Err(format!("duplicate token in positions: {token}"));
                }
                rows.push(row);
            }
            if batch.pagination.has_more {
                let next = batch.pagination.next_cursor
                    .filter(|s| !s.is_empty())
                    .ok_or("has_more without next_cursor")?;
                if !seen_cursors.insert(next.clone()) {
                    return Err("repeated positions cursor".into());
                }
                cursor = Some(next);
            } else {
                completeness = Completeness::Complete;
            }
            Ok(())
        });
        if let Err(why) = result {
            completeness = Completeness::Failed { after_pages: page, why };
            break;
        }
        if completeness.is_complete() {
            break;
        }
    }
    Positions {
        rows,
        completeness,
        as_of: now,
    }
}
