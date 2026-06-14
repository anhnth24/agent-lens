//! Nhận OpenTelemetry của Claude Code qua OTLP/HTTP **JSON** (POST /v1/metrics).
//! Bật trên máy dev: CLAUDE_CODE_ENABLE_TELEMETRY=1, OTEL_METRICS_EXPORTER=otlp,
//! OTEL_EXPORTER_OTLP_PROTOCOL=http/json, OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:8787
//! Trích: cost.usage, lines_of_code.count (added/removed), commit.count, pull_request.count.
//! [Unverified] temporality của metric (cumulative vs delta) — hiện cộng dồn như delta.

use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct SessionDelta {
    pub session_id: String,
    pub cost: f64,
    pub loc_added: i64,
    pub loc_removed: i64,
    pub commits: i64,
    pub prs: i64,
}

fn attr<'a>(attrs: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    attrs?
        .as_array()?
        .iter()
        .find(|a| a.get("key").and_then(|k| k.as_str()) == Some(key))
        .and_then(|a| a.get("value"))
}

fn attr_str(attrs: Option<&Value>, key: &str) -> Option<String> {
    attr(attrs, key)
        .and_then(|v| v.get("stringValue"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

fn dp_number(dp: &Value) -> f64 {
    if let Some(i) = dp.get("asInt") {
        // OTLP JSON encode int as string
        i.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| i.as_f64()).unwrap_or(0.0)
    } else if let Some(d) = dp.get("asDouble").and_then(|x| x.as_f64()) {
        d
    } else {
        0.0
    }
}

/// Parse 1 payload OTLP/HTTP JSON metrics -> các delta theo session.
pub fn parse_metrics(body: &Value) -> Vec<SessionDelta> {
    let mut out: std::collections::HashMap<String, SessionDelta> = std::collections::HashMap::new();
    let rms = match body.get("resourceMetrics").and_then(|x| x.as_array()) {
        Some(v) => v,
        None => return Vec::new(),
    };
    for rm in rms {
        let res_attrs = rm.get("resource").and_then(|r| r.get("attributes"));
        // session.id ưu tiên ở resource; fallback data-point attr
        let res_sid = attr_str(res_attrs, "session.id");
        for sm in rm.get("scopeMetrics").and_then(|x| x.as_array()).unwrap_or(&vec![]) {
            for metric in sm.get("metrics").and_then(|x| x.as_array()).unwrap_or(&vec![]) {
                let name = metric.get("name").and_then(|x| x.as_str()).unwrap_or("");
                let dps = metric
                    .get("sum")
                    .or_else(|| metric.get("gauge"))
                    .and_then(|s| s.get("dataPoints"))
                    .and_then(|x| x.as_array());
                let dps = match dps {
                    Some(d) => d,
                    None => continue,
                };
                for dp in dps {
                    let dp_attrs = dp.get("attributes");
                    let sid = res_sid
                        .clone()
                        .or_else(|| attr_str(dp_attrs, "session.id"))
                        .unwrap_or_default();
                    if sid.is_empty() {
                        continue;
                    }
                    let val = dp_number(dp);
                    let e = out.entry(sid.clone()).or_insert_with(|| SessionDelta {
                        session_id: sid.clone(),
                        ..Default::default()
                    });
                    match name {
                        "claude_code.cost.usage" => e.cost += val,
                        "claude_code.lines_of_code.count" => {
                            match attr_str(dp_attrs, "type").as_deref() {
                                Some("removed") => e.loc_removed += val as i64,
                                _ => e.loc_added += val as i64, // added (mặc định)
                            }
                        }
                        "claude_code.commit.count" => e.commits += val as i64,
                        "claude_code.pull_request.count" => e.prs += val as i64,
                        _ => {}
                    }
                }
            }
        }
    }
    out.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::parse_metrics;
    use serde_json::json;

    #[test]
    fn parses_cost_and_loc() {
        let body = json!({
          "resourceMetrics": [{
            "resource": { "attributes": [{ "key": "session.id", "value": { "stringValue": "s1" } }] },
            "scopeMetrics": [{
              "metrics": [
                { "name": "claude_code.cost.usage", "sum": { "dataPoints": [{ "asDouble": 0.42 }] } },
                { "name": "claude_code.lines_of_code.count", "sum": { "dataPoints": [
                   { "asInt": "120", "attributes": [{ "key": "type", "value": { "stringValue": "added" } }] },
                   { "asInt": "30",  "attributes": [{ "key": "type", "value": { "stringValue": "removed" } }] }
                ] } },
                { "name": "claude_code.commit.count", "sum": { "dataPoints": [{ "asInt": "2" }] } }
              ]
            }]
          }]
        });
        let d = parse_metrics(&body);
        assert_eq!(d.len(), 1);
        let s = &d[0];
        assert_eq!(s.session_id, "s1");
        assert!((s.cost - 0.42).abs() < 1e-9);
        assert_eq!(s.loc_added, 120);
        assert_eq!(s.loc_removed, 30);
        assert_eq!(s.commits, 2);
    }
}
