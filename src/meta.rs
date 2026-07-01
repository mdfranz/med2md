use std::collections::HashMap;
use std::time::Duration;
use reqwest::header::{HeaderValue, ACCEPT};
use tokio::sync::mpsc;
use crate::app::AppEvent;
use crate::cache;
use crate::feed::parse_rss_items;
use crate::net::build_cookie_headers;
use crate::util::get_jitter_ms;

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 400;
const MAX_DELAY_MS: u64 = 8000;

/// Parse `Retry-After: <seconds>` header value. Falls back to `default_secs` for
/// HTTP-date format (uncommon) or missing header.
fn retry_after_secs(resp: &reqwest::Response, default_secs: u64) -> u64 {
    resp.headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default_secs)
}

/// Fetch one author's RSS feed with exponential backoff on 429.
/// Returns `(last_post_ts, rss_count, was_rate_limited)`.
async fn fetch_one(
    client: &reqwest::Client,
    url: &str,
    name: &str,
    headers: reqwest::header::HeaderMap,
    tx: &mpsc::UnboundedSender<AppEvent>,
) -> (i64, usize, bool) {
    let mut backoff_secs = 30u64;
    let mut rate_limited = false;

    for attempt in 0..MAX_RETRIES {
        let resp = match client.get(url).headers(headers.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name, error = %e, "Enrichment: request error");
                return (0, 0, rate_limited);
            }
        };

        match resp.status().as_u16() {
            200..=299 => {
                match resp.text().await {
                    Ok(text) => {
                        let items = parse_rss_items(&text);
                        let ts = items.iter().map(|(t, _, _, _)| *t).max().unwrap_or(0);
                        tracing::info!(name, ts, rss_count = items.len(), "Enrichment: fetched");
                        return (ts, items.len(), rate_limited);
                    }
                    Err(e) => {
                        tracing::warn!(name, error = %e, "Enrichment: failed to read body");
                        return (0, 0, rate_limited);
                    }
                }
            }
            429 => {
                rate_limited = true;
                let wait = retry_after_secs(&resp, backoff_secs);
                tracing::warn!(
                    name, attempt, wait_secs = wait,
                    "Enrichment: rate limited (429), backing off"
                );
                let _ = tx.send(AppEvent::EnrichmentThrottled(wait));
                tokio::time::sleep(Duration::from_secs(wait)).await;
                backoff_secs = (backoff_secs * 2).min(300); // cap at 5 min
            }
            status => {
                tracing::warn!(name, status, "Enrichment: HTTP error");
                return (0, 0, rate_limited);
            }
        }
    }

    tracing::warn!(name, MAX_RETRIES, "Enrichment: exhausted retries");
    (0, 0, rate_limited)
}

pub async fn enrich_authors(
    sid: &str,
    uid: &str,
    cf_clearance: &str,
    authors: &[(String, String)],
    tx: mpsc::UnboundedSender<AppEvent>,
    cache_dir: &str,
    existing: &HashMap<String, (i64, usize)>,
    refresh: bool,
) {
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Enrichment: failed to create HTTP client");
            return;
        }
    };

    let total = authors.len();
    tracing::info!(total, "Author enrichment started");

    let mut collected: HashMap<String, (i64, usize)> = HashMap::new();
    let mut delay_ms = BASE_DELAY_MS;
    let mut first_fetch = true;

    for (_, (kind, name)) in authors.iter().enumerate() {
        // Reuse cached data for authors we already know about, unless forced refresh
        if !refresh {
            if let Some(&(ts, count)) = existing.get(name) {
                if ts > 0 {
                    collected.insert(name.clone(), (ts, count));
                    continue;
                }
            }
        }

        if !first_fetch {
            tokio::time::sleep(Duration::from_millis(get_jitter_ms(delay_ms))).await;
        }
        first_fetch = false;

        let feed_url = if kind == "user" {
            format!("https://medium.com/feed/@{}", name)
        } else {
            format!("https://medium.com/feed/{}", name)
        };

        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/rss+xml, text/xml, */*"));

        let (last_ts, count, rate_limited) = fetch_one(&client, &feed_url, name, h, &tx).await;

        // Widen inter-request gap after a 429; slowly recover on success
        if rate_limited {
            delay_ms = (delay_ms * 2).min(MAX_DELAY_MS);
            tracing::info!(delay_ms, "Enrichment: inter-request delay increased");
        } else if last_ts > 0 && delay_ms > BASE_DELAY_MS {
            delay_ms = (delay_ms * 3 / 4).max(BASE_DELAY_MS);
        }

        collected.insert(name.clone(), (last_ts, count));
        let _ = tx.send(AppEvent::AuthorEnriched(name.clone(), last_ts, count));
    }

    tracing::info!(total, "Author enrichment complete");
    cache::write_meta_cache(cache_dir, &collected);
    let _ = tx.send(AppEvent::EnrichmentDone);
}
