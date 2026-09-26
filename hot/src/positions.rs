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
pub const PAGE: usize = 500;
pub const MAX_PAGES: usize = 50;
const V2_PAGE: usize = 1000;
const V2_MAX_PAGES: usize = 100;
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
pub fn page_url(
    base: &str,
    user: &str,
    size_threshold: &str,
    offset: usize,
    extra: &str,
) -> String {
    format!(
        "{base}/positions?user={user}&sizeThreshold={size_threshold}\
             &limit={PAGE}&offset={offset}{extra}"
    )
}
pub fn judge(
    page_lens: &[usize],
    failed_at: Option<(usize, String)>,
    cap: usize,
) -> Completeness {
    if let Some((idx, why)) = failed_at {
        return Completeness::Failed {
            after_pages: idx,
            why,
        };
    }
    match page_lens.last() {
        Some(&n) if n < PAGE => Completeness::Complete,
        None => Completeness::Complete,
        Some(_) if page_lens.len() >= cap => {
            Completeness::Truncated {
                pages: page_lens.len(),
                cap,
            }
        }
        Some(_) => {
            Completeness::Truncated {
                pages: page_lens.len(),
                cap,
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(asset: &str, size: &str) -> serde_json::Value {
        serde_json::json!({ "asset" : asset, "size" : size })
    }
    #[test]
    fn a_SHORT_final_page_proves_completeness() {
        assert_eq!(judge(& [500, 500, 12], None, MAX_PAGES), Completeness::Complete);
        assert_eq!(judge(& [3], None, MAX_PAGES), Completeness::Complete);
        assert_eq!(judge(& [0], None, MAX_PAGES), Completeness::Complete);
        assert_eq!(judge(& [], None, MAX_PAGES), Completeness::Complete);
    }
    #[test]
    fn a_FULL_final_page_at_the_budget_is_TRUNCATED() {
        let lens = vec![500usize; MAX_PAGES];
        assert!(
            matches!(judge(& lens, None, MAX_PAGES), Completeness::Truncated { .. })
        );
    }
    #[test]
    fn a_FAILED_page_is_never_COMPLETE_however_much_we_got() {
        let c = judge(&[500, 500], Some((2, "503".into())), MAX_PAGES);
        assert!(! c.is_complete());
        assert!(c.reason().contains("page 3 failed"));
        assert!(! judge(& [7], Some((1, "timeout".into())), MAX_PAGES).is_complete());
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
    #[test]
    fn page_url_advances_the_offset_and_keeps_extras() {
        let u = page_url("https://x", "0xabc", "0.0001", 1000, "&redeemable=true");
        assert!(u.contains("offset=1000"));
        assert!(u.contains("limit=500"));
        assert!(u.contains("redeemable=true"));
    }
    #[test]
    fn v2_boot_page_maps_the_fields_used_by_reconciliation() {
        let body = serde_json::json!({
            "data": [{"token_id": "123", "current_size": 2.5, "avg_price": 0.4, "redeemable": false}],
            "pagination": {"has_more": true, "next_cursor": "next"}
        });
        let (rows, cursor) = decode_v2_page(&body).unwrap();
        assert_eq!(rows, vec![serde_json::json!({"asset": "123", "size": 2.5, "avgPrice": 0.4, "redeemable": false})]);
        assert_eq!(cursor.as_deref(), Some("next"));
    }
    #[test]
    fn v2_boot_page_refuses_a_partial_or_malformed_snapshot() {
        let no_cursor = serde_json::json!({"data": [], "pagination": {"has_more": true}});
        assert!(decode_v2_page(&no_cursor).is_err());
        let bad_size = serde_json::json!({
            "data": [{"token_id": "123", "current_size": -1.0, "redeemable": false}],
            "pagination": {"has_more": false}
        });
        assert!(decode_v2_page(&bad_size).is_err());
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
    let mut rows = Vec::new();
    let mut lens = Vec::new();
    let mut failed = None;
    for page in 0..MAX_PAGES {
        let url = page_url(base, user, size_threshold, page * PAGE, extra);
        let got = match http.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                r.json::<Vec<serde_json::Value>>()
                    .await
                    .map_err(|e| format!("decode: {e}"))
            }
            Ok(r) => Err(format!("http {}", r.status().as_u16())),
            Err(e) => Err(format!("transport: {e}")),
        };
        match got {
            Ok(batch) => {
                let n = batch.len();
                rows.extend(batch);
                lens.push(n);
                if n < PAGE {
                    break;
                }
            }
            Err(why) => {
                failed = Some((page, why));
                break;
            }
        }
    }
    let completeness = judge(&lens, failed, MAX_PAGES);
    Positions {
        rows,
        completeness,
        as_of: now,
    }
}

fn decode_v2_page(body: &serde_json::Value) -> Result<(Vec<serde_json::Value>, Option<String>), String> {
    let data = body["data"].as_array().ok_or("missing data array")?;
    if data.len() > V2_PAGE {
        return Err("page exceeds requested limit".into());
    }
    let more = body["pagination"]["has_more"].as_bool().ok_or("missing has_more")?;
    let next = body["pagination"]["next_cursor"].as_str().filter(|s| !s.is_empty());
    if more && next.is_none() {
        return Err("has_more without next_cursor".into());
    }
    let mut rows = Vec::with_capacity(data.len());
    for item in data {
        let asset = item["token_id"].as_str().filter(|s| !s.is_empty()).ok_or("missing token_id")?;
        let size = item["current_size"].as_f64().ok_or("missing current_size")?;
        if !size.is_finite() || size < 0.0 {
            return Err("invalid current_size".into());
        }
        let avg = item["avg_price"].as_f64().unwrap_or(0.0);
        if !avg.is_finite() || avg < 0.0 {
            return Err("invalid avg_price".into());
        }
        let redeemable = item["redeemable"].as_bool().ok_or("missing redeemable")?;
        rows.push(serde_json::json!({
            "asset": asset, "size": size, "avgPrice": avg, "redeemable": redeemable,
        }));
    }
    Ok((rows, if more { next.map(str::to_string) } else { None }))
}

/// Boot reconciliation needs a complete current-position snapshot. The legacy
/// offset endpoint repeats its last page above offset 10000 for large wallets.
pub async fn fetch_v2_boot(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    now: i64,
) -> Positions {
    let mut rows = Vec::new();
    let mut assets = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut completeness = Completeness::Truncated { pages: V2_MAX_PAGES, cap: V2_MAX_PAGES };
    for page in 0..V2_MAX_PAGES {
        let mut url = match reqwest::Url::parse(&format!("{base}/v2/positions")) {
            Ok(url) => url,
            Err(e) => {
                completeness = Completeness::Failed { after_pages: page, why: format!("url: {e}") };
                break;
            }
        };
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("user", user)
                .append_pair("limit", &V2_PAGE.to_string())
                .append_pair("filter_type", "TOKENS")
                .append_pair("filter_amount", "0.01")
                .append_pair("include_archived", "true");
            if let Some(value) = &cursor {
                query.append_pair("cursor", value);
            }
        }
        let got = match http.get(url).send().await {
            Ok(response) if response.status().is_success() => response.json::<serde_json::Value>()
                .await.map_err(|e| format!("decode: {e}")),
            Ok(response) => Err(format!("http {}", response.status().as_u16())),
            Err(e) => Err(format!("transport: {e}")),
        }.and_then(|body| decode_v2_page(&body));
        match got {
            Ok((batch, next)) => {
                if batch.iter().any(|row| !assets.insert(row["asset"].as_str().unwrap().to_string())) {
                    completeness = Completeness::Failed { after_pages: page, why: "duplicate token across pages".into() };
                    break;
                }
                rows.extend(batch);
                match next {
                    None => {
                        completeness = Completeness::Complete;
                        break;
                    }
                    Some(value) if cursors.insert(value.clone()) => cursor = Some(value),
                    Some(_) => {
                        completeness = Completeness::Failed { after_pages: page, why: "repeated cursor".into() };
                        break;
                    }
                }
            }
            Err(why) => {
                completeness = Completeness::Failed { after_pages: page, why };
                break;
            }
        }
    }
    Positions { rows, completeness, as_of: now }
}
