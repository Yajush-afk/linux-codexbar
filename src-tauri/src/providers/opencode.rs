use crate::models::{OpenCodeConfig, ProviderSnapshot, UsageWindowSnapshot};
use chrono::{DateTime, Local};
use regex::Regex;
use reqwest::header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const OPENCODE_WORKSPACES_ID: &str = "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f";
const OPENCODE_SUBSCRIPTION_ID: &str = "7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4";
const OPENCODE_SERVER_URL: &str = "https://opencode.ai/_server";
const OPENCODE_BASE_URL: &str = "https://opencode.ai";
const USER_AGENT_VALUE: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

#[derive(Clone, Debug)]
struct OpenCodeUsageData {
    rolling_usage_percent: f64,
    weekly_usage_percent: f64,
    rolling_reset_in_sec: i64,
    weekly_reset_in_sec: i64,
    workspace_id: String,
}

#[derive(Clone, Debug)]
struct WindowCandidate {
    percent: f64,
    reset_in_sec: i64,
    path_lower: String,
}

pub async fn fetch(config: &OpenCodeConfig) -> ProviderSnapshot {
    if !config.enabled {
        return ProviderSnapshot::disabled("OpenCode");
    }

    let cookie_header = match request_cookie_header(&config.cookie_header) {
        Some(header) => header,
        None => {
            return ProviderSnapshot::needs_setup(
                "OpenCode",
                "Set providers.opencode.cookie_header in config.json",
            )
        }
    };

    match fetch_usage(cookie_header, config.workspace_id.clone()).await {
        Ok(usage) => {
            let now = Local::now();
            let mut snapshot = ProviderSnapshot::ready("OpenCode");
            snapshot.primary = Some(UsageWindowSnapshot {
                label: "5h",
                used_percent: usage.rolling_usage_percent,
                resets_at: Some(now + chrono::Duration::seconds(usage.rolling_reset_in_sec)),
            });
            snapshot.secondary = Some(UsageWindowSnapshot {
                label: "7d",
                used_percent: usage.weekly_usage_percent,
                resets_at: Some(now + chrono::Duration::seconds(usage.weekly_reset_in_sec)),
            });
            snapshot.source = Some("manual cookie".to_string());
            snapshot.detail = Some(format!("workspace {}", shorten_workspace_id(&usage.workspace_id)));
            snapshot.updated_at = Some(now);
            snapshot
        }
        Err(error) => ProviderSnapshot::error("OpenCode", error),
    }
}

async fn fetch_usage(cookie_header: String, workspace_override: Option<String>) -> Result<OpenCodeUsageData, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Failed creating OpenCode HTTP client: {error}"))?;

    let workspace_id = if let Some(workspace) = normalize_workspace_id(workspace_override.as_deref()) {
        workspace
    } else {
        fetch_workspace_id(&client, &cookie_header).await?
    };

    let subscription_text = fetch_server_text(
        &client,
        OPENCODE_SUBSCRIPTION_ID,
        Some(vec![workspace_id.clone()]),
        "GET",
        &format!("https://opencode.ai/workspace/{workspace_id}/billing"),
        &cookie_header,
    )
    .await?;

    let usage = parse_usage_payload(&subscription_text)?;
    Ok(OpenCodeUsageData {
        workspace_id,
        ..usage
    })
}

async fn fetch_workspace_id(client: &Client, cookie_header: &str) -> Result<String, String> {
    let text = fetch_server_text(
        client,
        OPENCODE_WORKSPACES_ID,
        None,
        "GET",
        OPENCODE_BASE_URL,
        cookie_header,
    )
    .await?;

    if looks_signed_out(&text) {
        return Err("OpenCode cookie is invalid or expired.".to_string());
    }

    let ids = parse_workspace_ids(&text);
    ids.into_iter()
        .next()
        .ok_or_else(|| "No OpenCode workspace ID found in response.".to_string())
}

async fn fetch_server_text(
    client: &Client,
    server_id: &str,
    args: Option<Vec<String>>,
    method: &str,
    referer: &str,
    cookie_header: &str,
) -> Result<String, String> {
    let request_id = format!("server-fn:{}", uuid_like());
    let mut request = if method.eq_ignore_ascii_case("GET") {
        client.get(server_url(server_id, args.as_ref()))
    } else {
        client.post(OPENCODE_SERVER_URL)
    };

    request = request
        .header(COOKIE, cookie_header)
        .header("X-Server-Id", server_id)
        .header("X-Server-Instance", request_id)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ORIGIN, OPENCODE_BASE_URL)
        .header(REFERER, referer)
        .header(ACCEPT, "text/javascript, application/json;q=0.9, */*;q=0.8");

    if !method.eq_ignore_ascii_case("GET") {
        request = request.header(CONTENT_TYPE, "application/json");
        if let Some(args) = args {
            request = request.json(&args);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("OpenCode request failed: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed reading OpenCode response body: {error}"))?;

    if status.is_success() {
        return Ok(text);
    }

    if status.as_u16() == 401 || status.as_u16() == 403 || looks_signed_out(&text) {
        return Err("OpenCode cookie is invalid or expired.".to_string());
    }

    Err(format!("OpenCode API returned HTTP {}", status.as_u16()))
}

fn server_url(server_id: &str, args: Option<&Vec<String>>) -> String {
    let mut url = format!("{OPENCODE_SERVER_URL}?id={server_id}");
    if let Some(args) = args {
        if !args.is_empty() {
            let args_json = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
            let encoded = urlencoding::encode(&args_json);
            url.push_str("&args=");
            url.push_str(encoded.as_ref());
        }
    }
    url
}

fn parse_usage_payload(text: &str) -> Result<OpenCodeUsageData, String> {
    if let Some(snapshot) = parse_usage_json(text) {
        return Ok(snapshot);
    }

    let rolling_percent = extract_float(text, r"rollingUsage[^}]*?usagePercent\s*:\s*([0-9]+(?:\.[0-9]+)?)");
    let rolling_reset = extract_int(text, r"rollingUsage[^}]*?resetInSec\s*:\s*([0-9]+)");
    let weekly_percent = extract_float(text, r"weeklyUsage[^}]*?usagePercent\s*:\s*([0-9]+(?:\.[0-9]+)?)");
    let weekly_reset = extract_int(text, r"weeklyUsage[^}]*?resetInSec\s*:\s*([0-9]+)");

    match (rolling_percent, rolling_reset, weekly_percent, weekly_reset) {
        (Some(rp), Some(rr), Some(wp), Some(wr)) => Ok(OpenCodeUsageData {
            rolling_usage_percent: rp,
            weekly_usage_percent: wp,
            rolling_reset_in_sec: rr,
            weekly_reset_in_sec: wr,
            workspace_id: String::new(),
        }),
        _ => Err("Failed to parse OpenCode usage windows.".to_string()),
    }
}

fn parse_usage_json(text: &str) -> Option<OpenCodeUsageData> {
    let value: Value = serde_json::from_str(text).ok()?;

    if let Some((rolling, weekly)) = find_named_windows(&value) {
        return Some(OpenCodeUsageData {
            rolling_usage_percent: rolling.percent,
            weekly_usage_percent: weekly.percent,
            rolling_reset_in_sec: rolling.reset_in_sec,
            weekly_reset_in_sec: weekly.reset_in_sec,
            workspace_id: String::new(),
        });
    }

    let candidates = collect_window_candidates(&value, Vec::new());
    if candidates.is_empty() {
        return None;
    }

    let rolling = pick_window_candidate(&candidates, true)?;
    let weekly = pick_window_candidate_excluding(&candidates, false, &rolling.path_lower)?;

    Some(OpenCodeUsageData {
        rolling_usage_percent: rolling.percent,
        weekly_usage_percent: weekly.percent,
        rolling_reset_in_sec: rolling.reset_in_sec,
        weekly_reset_in_sec: weekly.reset_in_sec,
        workspace_id: String::new(),
    })
}

fn find_named_windows(value: &Value) -> Option<(WindowCandidate, WindowCandidate)> {
    match value {
        Value::Object(map) => {
            let direct = parse_named_windows_from_map(map);
            if direct.is_some() {
                return direct;
            }

            for nested in map.values() {
                if let Some(found) = find_named_windows(nested) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_named_windows),
        _ => None,
    }
}

fn parse_named_windows_from_map(map: &serde_json::Map<String, Value>) -> Option<(WindowCandidate, WindowCandidate)> {
    let rolling_keys = ["rollingUsage", "rolling", "rolling_usage", "rollingWindow", "rolling_window"];
    let weekly_keys = ["weeklyUsage", "weekly", "weekly_usage", "weeklyWindow", "weekly_window"];

    let rolling = rolling_keys
        .iter()
        .find_map(|key| map.get(*key))
        .and_then(parse_window);
    let weekly = weekly_keys
        .iter()
        .find_map(|key| map.get(*key))
        .and_then(parse_window);

    match (rolling, weekly) {
        (Some(rolling), Some(weekly)) => Some((rolling, weekly)),
        _ => None,
    }
}

fn collect_window_candidates(value: &Value, path: Vec<String>) -> Vec<WindowCandidate> {
    let mut out = Vec::new();
    match value {
        Value::Object(map) => {
            if let Some(window) = parse_window(value) {
                out.push(WindowCandidate {
                    path_lower: path.join(".").to_lowercase(),
                    ..window
                });
            }
            for (key, nested) in map {
                let mut next_path = path.clone();
                next_path.push(key.clone());
                out.extend(collect_window_candidates(nested, next_path));
            }
        }
        Value::Array(items) => {
            for (index, nested) in items.iter().enumerate() {
                let mut next_path = path.clone();
                next_path.push(format!("[{index}]"));
                out.extend(collect_window_candidates(nested, next_path));
            }
        }
        _ => {}
    }
    out
}

fn parse_window(value: &Value) -> Option<WindowCandidate> {
    let map = value.as_object()?;
    let percent_keys = [
        "usagePercent",
        "usedPercent",
        "percentUsed",
        "percent",
        "usage_percent",
        "used_percent",
        "utilization",
        "utilizationPercent",
        "utilization_percent",
        "usage",
    ];
    let reset_in_keys = [
        "resetInSec",
        "resetInSeconds",
        "resetSeconds",
        "reset_sec",
        "reset_in_sec",
        "resetsInSec",
        "resetsInSeconds",
        "resetIn",
        "resetSec",
    ];

    let percent = percent_keys.iter().find_map(|key| map.get(*key)).and_then(value_to_f64)?;
    let reset_in_sec = reset_in_keys.iter().find_map(|key| map.get(*key)).and_then(value_to_i64)?;

    Some(WindowCandidate {
        percent,
        reset_in_sec,
        path_lower: String::new(),
    })
}

fn pick_window_candidate(candidates: &[WindowCandidate], shorter_reset: bool) -> Option<WindowCandidate> {
    let preferred: Vec<WindowCandidate> = candidates
        .iter()
        .filter(|candidate| {
            if shorter_reset {
                candidate.path_lower.contains("rolling")
                    || candidate.path_lower.contains("hour")
                    || candidate.path_lower.contains("5h")
            } else {
                candidate.path_lower.contains("weekly") || candidate.path_lower.contains("week")
            }
        })
        .cloned()
        .collect();

    pick_window_candidate_impl(if preferred.is_empty() { candidates } else { &preferred }, shorter_reset)
}

fn pick_window_candidate_excluding(
    candidates: &[WindowCandidate],
    shorter_reset: bool,
    excluded_path: &str,
) -> Option<WindowCandidate> {
    let filtered: Vec<WindowCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.path_lower != excluded_path)
        .cloned()
        .collect();
    pick_window_candidate(&filtered, shorter_reset)
}

fn pick_window_candidate_impl(candidates: &[WindowCandidate], shorter_reset: bool) -> Option<WindowCandidate> {
    let mut candidates = candidates.to_vec();
    if shorter_reset {
        candidates.sort_by(|left, right| left.reset_in_sec.cmp(&right.reset_in_sec));
    } else {
        candidates.sort_by(|left, right| right.reset_in_sec.cmp(&left.reset_in_sec));
    }
    candidates.into_iter().next()
}

fn parse_workspace_ids(text: &str) -> Vec<String> {
    let regex = Regex::new(r#"id\s*:\s*\"(wrk_[^\"]+)\""#).ok();
    regex
        .map(|regex| {
            regex
                .captures_iter(text)
                .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn request_cookie_header(raw: &str) -> Option<String> {
    let pairs: Vec<String> = raw
        .split(';')
        .filter_map(|segment| {
            let trimmed = segment.trim();
            let mut parts = trimmed.splitn(2, '=');
            let name = parts.next()?.trim();
            let value = parts.next()?.trim();
            if name == "auth" || name == "__Host-auth" {
                Some(format!("{name}={value}"))
            } else {
                None
            }
        })
        .collect();

    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

fn normalize_workspace_id(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("wrk_") {
        return Some(raw.to_string());
    }
    Regex::new(r"wrk_[A-Za-z0-9]+")
        .ok()?
        .find(raw)
        .map(|value| value.as_str().to_string())
}

fn looks_signed_out(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("login")
        || lower.contains("sign in")
        || lower.contains("auth/authorize")
        || lower.contains("not associated with an account")
        || lower.contains("actor of type \"public\"")
}

fn value_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn extract_float(text: &str, pattern: &str) -> Option<f64> {
    let regex = Regex::new(pattern).ok()?;
    regex
        .captures(text)
        .and_then(|capture| capture.get(1).and_then(|value| value.as_str().parse().ok()))
}

fn extract_int(text: &str, pattern: &str) -> Option<i64> {
    let regex = Regex::new(pattern).ok()?;
    regex
        .captures(text)
        .and_then(|capture| capture.get(1).and_then(|value| value.as_str().parse().ok()))
}

fn shorten_workspace_id(workspace_id: &str) -> String {
    if workspace_id.len() <= 16 {
        workspace_id.to_string()
    } else {
        format!("{}..{}", &workspace_id[..8], &workspace_id[workspace_id.len() - 6..])
    }
}

fn uuid_like() -> String {
    format!(
        "{:08x}{:08x}",
        rand_seed(17),
        rand_seed(53),
    )
}

fn rand_seed(offset: u32) -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    ((now >> offset) & 0xffff_ffff) as u32
}

#[cfg(test)]
mod tests {
    use super::{normalize_workspace_id, parse_workspace_ids, request_cookie_header};

    #[test]
    fn keeps_only_supported_cookie_names() {
        let header = request_cookie_header("foo=bar; auth=one; __Host-auth=two");
        assert_eq!(header.as_deref(), Some("auth=one; __Host-auth=two"));
    }

    #[test]
    fn parses_workspace_ids_from_text() {
        let ids = parse_workspace_ids(r#"[{ id: \"wrk_123\" }, { id: \"wrk_456\" }]"#);
        assert_eq!(ids, vec!["wrk_123".to_string(), "wrk_456".to_string()]);
    }

    #[test]
    fn normalizes_workspace_from_url_or_raw_value() {
        assert_eq!(normalize_workspace_id(Some("wrk_abc123")), Some("wrk_abc123".to_string()));
        assert_eq!(
            normalize_workspace_id(Some("https://opencode.ai/workspace/wrk_abc123/billing")),
            Some("wrk_abc123".to_string())
        );
    }
}
