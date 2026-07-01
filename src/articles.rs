use reqwest::header::{HeaderValue, ACCEPT};
use crate::net::build_cookie_headers;
use crate::feed::{parse_medium_api_json, clean_rss_url, parse_rss_items, extract_following_from_html, extract_user_id_from_apollo};
use crate::util::{get_jitter_ms, format_ts};

async fn fetch_rss_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
) -> Option<String> {
    let resp = match client.get(url).headers(headers.clone()).send().await {
        Ok(r) => r,
        Err(e) => { tracing::error!(url, error = %e, "RSS fetch failed"); return None; }
    };
    if resp.status().is_success() {
        return resp.text().await.ok();
    }
    if resp.status().as_u16() == 429 {
        let wait_secs: u64 = resp.headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        tracing::warn!(url, wait_secs, "RSS rate limited (429), backing off");
        tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
        return match client.get(url).headers(headers).send().await {
            Ok(r) if r.status().is_success() => r.text().await.ok(),
            _ => { tracing::warn!(url, "RSS still rate limited after retry, skipping"); None }
        };
    }
    tracing::warn!(url, status = %resp.status(), "RSS fetch returned error");
    None
}

pub async fn fetch_user_posts_api(
    client: &reqwest::Client,
    sid: &str,
    viewer_uid: &str,
    cf_clearance: &str,
    author_user_id: &str,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<(i64, String, String, String)> {
    let mut posts: Vec<(i64, String, String, String)> = Vec::new();
    let mut next_to: Option<String> = None;

    loop {
        let mut url = format!("https://medium.com/_/api/users/{}/posts?limit=20", author_user_id);
        if let Some(ref to) = next_to {
            url.push_str(&format!("&to={}", to));
        }
        let mut h = build_cookie_headers(sid, viewer_uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let resp = match client.get(&url).headers(h).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => { tracing::warn!(status = %r.status(), "User posts API error"); break; }
            Err(e) => { tracing::error!(error = %e, "User posts API request failed"); break; }
        };
        let text = match resp.text().await {
            Ok(t) => t,
            Err(_) => break,
        };
        let payload = match parse_medium_api_json(&text) {
            Some(p) => p,
            None => break,
        };

        let before = posts.len();
        if let Some(post_refs) = payload.get("references")
            .and_then(|r| r.get("Post"))
            .and_then(|p| p.as_object())
        {
            let user_refs = payload.get("references").and_then(|r| r.get("User"));
            for (_, post) in post_refs {
                let title = post.get("title").and_then(|t| t.as_str()).unwrap_or("");
                let post_url = post.get("mediumUrl")
                    .or_else(|| post.get("uniqueSlug"))
                    .and_then(|u| u.as_str()).unwrap_or("");
                if title.is_empty() || post_url.is_empty() { continue; }
                let ts = post.get("firstPublishedAt").or_else(|| post.get("publishedAt"))
                    .and_then(|v| v.as_i64()).map(|ms| ms / 1000).unwrap_or(0);
                let author_name = post.get("creator").and_then(|c| c.get("__ref"))
                    .and_then(|r| r.as_str())
                    .and_then(|r| user_refs.and_then(|u| u.get(r.trim_start_matches("User:"))))
                    .and_then(|u| u.get("name")).and_then(|n| n.as_str())
                    .unwrap_or("").to_string();
                let clean = clean_rss_url(post_url);
                if seen.insert(clean.clone()) {
                    posts.push((ts, title.to_string(), clean, author_name));
                }
            }
        }

        let payload_keys: Vec<_> = payload.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        tracing::info!(new = posts.len() - before, total = posts.len(), keys = ?payload_keys, "User posts API page");

        if posts.len() == before { break; }

        next_to = payload.get("paging").and_then(|p| p.get("next"))
            .and_then(|n| n.get("to")).and_then(|t| t.as_str())
            .map(|s| s.to_string());
        if next_to.is_none() { break; }
        tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(400))).await;
    }

    posts
}

pub async fn fetch_rss_for_authors(
    sid: &str,
    uid: &str,
    cf_clearance: &str,
    authors: &[(String, String)],
) -> Result<Vec<(String, String, String, String)>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let mut articles: Vec<(i64, String, String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut inter_delay_ms: u64 = 1500;

    for (idx, (kind, name)) in authors.iter().enumerate() {
        if idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(inter_delay_ms))).await;
        }

        let feed_url = if kind == "user" {
            format!("https://medium.com/feed/@{}", name)
        } else {
            format!("https://medium.com/feed/{}", name)
        };
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/rss+xml, text/xml, */*"));
        let rss_ok = if let Some(text) = fetch_rss_with_retry(&client, &feed_url, h).await {
            let items = parse_rss_items(&text);
            tracing::info!(name, rss = items.len(), "Fetched author RSS");
            for (ts, title, url, author) in items {
                let clean = clean_rss_url(&url);
                if seen.insert(clean.clone()) {
                    articles.push((ts, title, clean, author));
                }
            }
            inter_delay_ms = inter_delay_ms.max(1500);
            true
        } else {
            inter_delay_ms = (inter_delay_ms * 2).min(8000);
            false
        };

        if !rss_ok { continue; }

        if kind == "user" {
            tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(250))).await;
            let profile_url = format!("https://medium.com/@{}", name);
            let mut ph = build_cookie_headers(sid, uid, cf_clearance);
            ph.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));

            if let Ok(resp) = client.get(&profile_url).headers(ph).send().await {
                if resp.status().is_success() {
                    if let Ok(html) = resp.text().await {
                        let (_, _, profile_posts) = extract_following_from_html(&html);
                        let before = articles.len();
                        for (ts, title, url, author) in profile_posts {
                            let clean = clean_rss_url(&url);
                            if seen.insert(clean.clone()) {
                                articles.push((ts, title, clean, author));
                            }
                        }
                        tracing::info!(name, profile_extra = articles.len() - before, "Profile page posts");

                        if let Some(author_uid) = extract_user_id_from_apollo(&html, name) {
                            tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(300))).await;
                            let api_posts = fetch_user_posts_api(&client, sid, uid, cf_clearance, &author_uid, &mut seen).await;
                            let api_count = api_posts.len();
                            articles.extend(api_posts);
                            tracing::info!(name, api_posts = api_count, "User posts API");
                        }
                    }
                }
            }
        }
    }

    articles.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    Ok(articles.into_iter().map(|(ts, t, u, a)| (t, u, format_ts(ts), a)).collect())
}
