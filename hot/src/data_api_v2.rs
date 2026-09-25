use serde_json::Value;
use std::collections::HashSet;

pub const BASE: &str = "https://data-api.polymarket.com";
pub const MAX_PAGES: usize = 50;

pub fn page(body: &Value) -> Result<(&[Value], Option<&str>), String> {
    let rows = body.get("data").and_then(Value::as_array)
        .ok_or("v2 response has no data array")?;
    let pagination = body.get("pagination").and_then(Value::as_object)
        .ok_or("v2 response has no pagination")?;
    let next = match pagination.get("next_cursor") {
        Some(Value::Null) => None,
        Some(Value::String(s)) if !s.is_empty() => Some(s.as_str()),
        _ => return Err("v2 response has no valid next_cursor".into()),
    };
    if pagination.get("has_more").and_then(Value::as_bool) != Some(next.is_some()) {
        return Err("v2 pagination state and cursor disagree".into());
    }
    Ok((rows, next))
}

pub async fn fetch_all(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    params: &[(&str, &str)],
) -> Result<Vec<Value>, String> {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = HashSet::new();
    for _ in 0..MAX_PAGES {
        let mut url = reqwest::Url::parse(&format!("{base}{path}"))
            .map_err(|e| format!("v2 URL: {e}"))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("limit", "500");
            for (key, value) in params {
                query.append_pair(key, value);
            }
            if let Some(c) = &cursor {
                query.append_pair("cursor", c);
            }
        }
        let response = http.get(url).send().await.map_err(|e| format!("v2 transport: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("v2 HTTP {}", response.status().as_u16()));
        }
        let body: Value = response.json().await.map_err(|e| format!("v2 JSON: {e}"))?;
        let (batch, next) = page(&body)?;
        rows.extend(batch.iter().cloned());
        match next {
            None => return Ok(rows),
            Some(next) if seen.insert(next.to_owned()) => cursor = Some(next.to_owned()),
            Some(_) => return Err("v2 cursor repeated".into()),
        }
    }
    Err(format!("v2 pagination exceeded {MAX_PAGES} pages"))
}

pub fn number_micros(value: &Value) -> Option<i64> {
    let n = value.as_f64().or_else(|| value.as_str()?.parse::<f64>().ok())?;
    if !n.is_finite() || n < 0.0 || n > (i64::MAX as f64) / 1_000_000.0 {
        return None;
    }
    Some((n * 1_000_000.0).round() as i64)
}

/// Field aliases for older read-only activity consumers.
pub fn legacy_activity(row: &Value) -> Result<Value, String> {
    let mut value = row.clone();
    let object = value.as_object_mut().ok_or("v2 activity is not an object")?;
    for (old, new) in [("conditionId", "condition_id"),
        ("transactionHash", "transaction_hash"), ("usdcSize", "usdc_size"),
        ("outcomeIndex", "outcome_index"), ("asset", "token_id")] {
        if let Some(v) = object.get(new).cloned() {
            object.insert(old.into(), v);
        }
    }
    if matches!(value["type"].as_str(), Some("SPLIT" | "MERGE"))
        && value["size"].is_null() {
        let shares = value["usdc_size"].clone();
        value["size"] = shares;
    }
    Ok(value)
}

pub fn position_book(
    rows: &[Value],
    wallet: &str,
    market: &crate::consensus::Market,
    as_of: i64,
) -> Result<crate::consensus::Book, String> {
    let (mut up, mut down) = (0i64, 0i64);
    for row in rows {
        let row_wallet = row["proxy_wallet"].as_str().ok_or("position wallet missing")?;
        if !row_wallet.eq_ignore_ascii_case(wallet) {
            return Err("position wallet mismatch".into());
        }
        let condition = row["condition_id"].as_str().ok_or("position condition missing")?;
        let token = row["token_id"].as_str().or_else(|| row["asset_id"].as_str())
            .ok_or("position token missing")?;
        let size = number_micros(&row["current_size"]).ok_or("position size invalid")?;
        if condition.eq_ignore_ascii_case(&market.condition) {
            if token == market.up_token {
                up = up.checked_add(size).ok_or("position shares overflow")?;
            } else if token == market.down_token {
                down = down.checked_add(size).ok_or("position shares overflow")?;
            } else {
                return Err("position outcome token does not match market".into());
            }
        }
    }
    Ok(crate::consensus::Book { up, down, as_of, complete: true })
}

pub fn trade(row: &Value, market: &crate::consensus::Market) -> Result<crate::consensus::Trade, String> {
    let condition = row["condition_id"].as_str().ok_or("trade condition missing")?;
    if !condition.eq_ignore_ascii_case(&market.condition) {
        return Err("trade condition mismatch".into());
    }
    let tx = row["transaction_hash"].as_str().ok_or("trade transaction hash missing")?;
    let token = row["token_id"].as_str().or_else(|| row["asset_id"].as_str())
        .ok_or("trade token missing")?;
    let side = match row["side"].as_str() {
        Some("BUY") => 0,
        Some("SELL") => 1,
        _ => return Err("trade side invalid".into()),
    };
    let shares = number_micros(&row["size"]).ok_or("trade size invalid")?;
    let price = number_micros(&row["price"]).ok_or("trade price invalid")?;
    let ts = row["timestamp"].as_i64().ok_or("trade time invalid")?;
    let key = if let Some(index) = row["log_index"].as_u64() {
        format!("{tx}:{index}")
    } else if let Some(id) = row["id"].as_str() {
        format!("{tx}:{id}")
    } else {
        format!("{tx}:{token}:{side}:{shares}:{price}:{ts}")
    };
    Ok(crate::consensus::Trade { key, token: token.into(), side, shares, price, ts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_and_incomplete_data_are_not_mistaken_for_an_empty_wallet() {
        let complete = serde_json::json!({"data":[], "pagination":{"next_cursor":null,"has_more":false}});
        assert_eq!(page(&complete).unwrap().0.len(), 0);
        assert!(page(&serde_json::json!({"data":[]})).is_err());
        assert!(page(&serde_json::json!({"data":[],"pagination":{"has_more":true,"next_cursor":null}})).is_err());
        assert!(page(&serde_json::json!({"data":[],"pagination":{"has_more":false,"next_cursor":"abc"}})).is_err());
    }
}
