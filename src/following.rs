use reqwest::header::{HeaderValue, ACCEPT};
use crate::net::build_cookie_headers;
use crate::feed::{parse_medium_api_json, clean_rss_url, parse_rss_items, extract_following_from_html};
use crate::util::{get_jitter_ms, format_ts};

enum RssFetch {
    Body(String),
    RateLimited,
    Failed,
}

async fn fetch_rss_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
) -> RssFetch {
    let resp = match client.get(url).headers(headers.clone()).send().await {
        Ok(r) => r,
        Err(e) => { tracing::error!(url, error = %e, "RSS fetch failed"); return RssFetch::Failed; }
    };
    if resp.status().is_success() {
        return resp.text().await.map(RssFetch::Body).unwrap_or(RssFetch::Failed);
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
            Ok(r) if r.status().is_success() => r.text().await.map(RssFetch::Body).unwrap_or(RssFetch::Failed),
            _ => { tracing::warn!(url, "RSS still rate limited after retry, skipping"); RssFetch::RateLimited }
        };
    }
    tracing::warn!(url, status = %resp.status(), "RSS fetch returned error");
    RssFetch::Failed
}

pub async fn fetch_following_feed(
    sid: &str,
    uid: &str,
    cf_clearance: &str,
    known_authors: &[(String, String)],
) -> Result<Vec<(String, String, String, String)>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| {
            let err_msg = format!("Failed to create client: {}", e);
            tracing::error!("{}", err_msg);
            err_msg
        })?;

    let mut page_headers = build_cookie_headers(sid, uid, cf_clearance);
    page_headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));

    let (html, feed_url_used) = {
        let resp = match client
            .get("https://medium.com/?feed=following")
            .headers(page_headers.clone())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Failed to fetch feed page: {}", e);
                tracing::error!("{}", err_msg);
                return Err(err_msg);
            }
        };

        if resp.status().is_success() {
            tracing::info!("Fetched /?feed=following successfully");
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    let err_msg = format!("Failed to read feed page: {}", e);
                    tracing::error!("{}", err_msg);
                    return Err(err_msg);
                }
            };
            (text, "/?feed=following")
        } else {
            tracing::warn!(status = %resp.status(), "/?feed=following blocked, falling back to /me/feed");
            let fallback_resp = match client
                .get("https://medium.com/me/feed")
                .headers(page_headers)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = format!("Failed to fetch fallback feed page: {}", e);
                    tracing::error!("{}", err_msg);
                    return Err(err_msg);
                }
            };

            let status = fallback_resp.status();
            if !status.is_success() {
                let err_msg = format!("Fallback feed page returned error status: {}", status);
                tracing::error!("{}", err_msg);
                return Err(err_msg);
            }

            let text = match fallback_resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    let err_msg = format!("Failed to read fallback feed page: {}", e);
                    tracing::error!("{}", err_msg);
                    return Err(err_msg);
                }
            };
            (text, "/me/feed")
        }
    };

    tracing::info!(source = feed_url_used, "Parsing Apollo state");
    let (apollo_usernames, apollo_pub_slugs, apollo_posts) = extract_following_from_html(&html);

    // Use the pre-fetched following list when available; fall back to Apollo extraction
    let (usernames, pub_slugs): (Vec<String>, Vec<String>) = if !known_authors.is_empty() {
        tracing::info!(count = known_authors.len(), "Using known authors list for RSS fetching");
        let users = known_authors.iter()
            .filter(|(k, _)| k == "user")
            .map(|(_, n)| n.clone())
            .collect();
        let pubs = known_authors.iter()
            .filter(|(k, _)| k == "pub")
            .map(|(_, n)| n.clone())
            .collect();
        (users, pubs)
    } else {
        tracing::info!(
            users = apollo_usernames.len(),
            publications = apollo_pub_slugs.len(),
            "No known authors list; using Apollo state extraction"
        );
        (apollo_usernames, apollo_pub_slugs)
    };

    if usernames.is_empty() && pub_slugs.is_empty() && apollo_posts.is_empty() {
        let err_msg = "No followed users or publications found. Check your MEDIUM_SID and MEDIUM_CF_CLEARANCE.";
        tracing::error!("{}", err_msg);
        return Err(err_msg.to_string());
    }

    tracing::info!(
        users = usernames.len(),
        publications = pub_slugs.len(),
        apollo_posts = apollo_posts.len(),
        "Fetching RSS feeds"
    );

    let mut articles: Vec<(i64, String, String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (ts, title, url, author) in apollo_posts {
        let clean = clean_rss_url(&url);
        if seen.insert(clean.clone()) {
            articles.push((ts, title, clean, author));
        }
    }

    let mut inter_delay_ms: u64 = 1500;

    for (idx, username) in usernames.iter().enumerate() {
        if idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(inter_delay_ms))).await;
        }
        let feed_url = format!("https://medium.com/feed/@{}", username);
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/rss+xml, text/xml, */*"));
        match fetch_rss_with_retry(&client, &feed_url, h).await {
            RssFetch::Body(text) => {
                let items = parse_rss_items(&text);
                tracing::info!(username, count = items.len(), "Fetched user RSS feed");
                for (ts, title, url, author) in items {
                    let clean = clean_rss_url(&url);
                    if seen.insert(clean.clone()) {
                        articles.push((ts, title, clean, author));
                    }
                }
                inter_delay_ms = inter_delay_ms.max(1500);
            }
            RssFetch::RateLimited => {
                tracing::warn!(username, "User RSS still rate limited, increasing inter-request delay");
                inter_delay_ms = (inter_delay_ms * 2).min(8000);
            }
            RssFetch::Failed => {}
        }
    }

    for (idx, slug) in pub_slugs.iter().enumerate() {
        if !usernames.is_empty() || idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(inter_delay_ms))).await;
        }
        let feed_url = format!("https://medium.com/feed/{}", slug);
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/rss+xml, text/xml, */*"));
        match fetch_rss_with_retry(&client, &feed_url, h).await {
            RssFetch::Body(text) => {
                let items = parse_rss_items(&text);
                tracing::info!(slug, count = items.len(), "Fetched publication RSS feed");
                for (ts, title, url, author) in items {
                    let clean = clean_rss_url(&url);
                    if seen.insert(clean.clone()) {
                        articles.push((ts, title, clean, author));
                    }
                }
                inter_delay_ms = inter_delay_ms.max(1500);
            }
            RssFetch::RateLimited => {
                tracing::warn!(slug, "Publication RSS still rate limited, increasing inter-request delay");
                inter_delay_ms = (inter_delay_ms * 2).min(8000);
            }
            RssFetch::Failed => {}
        }
    }

    articles.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let articles: Vec<(String, String, String, String)> = articles
        .into_iter()
        .map(|(ts, t, u, a)| (t, u, format_ts(ts), a))
        .collect();

    tracing::info!(total = articles.len(), "Feed fetch complete");
    Ok(articles)
}

pub async fn fetch_following_via_api(
    client: &reqwest::Client,
    sid: &str,
    uid: &str,
    cf_clearance: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut authors: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut next_to: Option<String> = None;

    loop {
        let mut url = format!("https://medium.com/_/api/users/{}/following?limit=200", uid);
        if let Some(ref to) = next_to {
            url.push_str(&format!("&to={}", to));
        }
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let resp = client.get(&url).headers(h).send().await
            .map_err(|e| format!("API request failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("API returned {}", resp.status()));
        }
        let text = resp.text().await.map_err(|e| format!("Failed to read API response: {}", e))?;

        let payload = match parse_medium_api_json(&text) {
            Some(p) => p,
            None => {
                tracing::warn!(
                    preview = &text[..text.len().min(400)],
                    "API response could not be parsed as Medium JSON"
                );
                return Err("Could not parse API response".to_string());
            }
        };

        let refs = payload.get("references");

        let mut target_ids: Vec<String> = Vec::new();
        if let Some(social) = refs.and_then(|r| r.get("Social")).and_then(|s| s.as_object()) {
            for (target_id, entry) in social {
                let is_following = entry.get("isFollowing").and_then(|v| v.as_bool()).unwrap_or(true);
                if is_following && seen.insert(target_id.clone()) {
                    target_ids.push(target_id.clone());
                }
            }
        }

        let user_refs = refs.and_then(|r| r.get("User")).and_then(|u| u.as_object());
        tracing::info!(
            social_ids = target_ids.len(),
            user_refs = user_refs.map(|u| u.len()).unwrap_or(0),
            "API page"
        );

        let before = authors.len();
        for target_id in &target_ids {
            let username = user_refs
                .and_then(|ur| ur.get(target_id))
                .and_then(|u| u.get("username"))
                .and_then(|u| u.as_str())
                .filter(|u| !u.is_empty());
            if let Some(uname) = username {
                authors.push(("user".to_string(), uname.to_string()));
            } else {
                authors.push(("uid".to_string(), target_id.clone()));
            }
        }

        if target_ids.is_empty() {
            tracing::warn!("API page had empty Social references — stopping pagination");
            break;
        }
        if authors.len() == before {
            break;
        }

        next_to = payload
            .get("paging").and_then(|p| p.get("next"))
            .and_then(|n| n.get("to")).and_then(|t| t.as_str())
            .map(|s| s.to_string());
        if next_to.is_none() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(300))).await;
    }

    let unresolved: Vec<String> = authors.iter()
        .filter(|(k, _)| k == "uid")
        .map(|(_, v)| v.clone())
        .collect();

    if !unresolved.is_empty() {
        tracing::info!(count = unresolved.len(), "Resolving userids without User refs via profile API");
        let mut uid_to_username: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (idx, user_id) in unresolved.iter().enumerate() {
            if idx > 0 && idx % 10 == 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(400))).await;
            }
            let profile_url = format!("https://medium.com/_/api/users/{}", user_id);
            let mut h = build_cookie_headers(sid, uid, cf_clearance);
            h.insert(ACCEPT, HeaderValue::from_static("application/json"));
            if let Ok(resp) = client.get(&profile_url).headers(h).send().await {
                if resp.status().is_success() {
                    if let Ok(text) = resp.text().await {
                        if let Some(p) = parse_medium_api_json(&text) {
                            let username = p.get("value")
                                .and_then(|v| v.get("username"))
                                .and_then(|u| u.as_str())
                                .filter(|u| !u.is_empty())
                                .map(|u| u.to_string());
                            if let Some(uname) = username {
                                uid_to_username.insert(user_id.clone(), uname);
                            }
                        }
                    }
                }
            }
        }
        authors = authors.into_iter().filter_map(|(kind, val)| {
            if kind == "uid" {
                uid_to_username.get(&val).map(|uname| ("user".to_string(), uname.clone()))
            } else {
                Some((kind, val))
            }
        }).collect();
        tracing::info!(resolved = uid_to_username.len(), remaining_unresolved = unresolved.len() - uid_to_username.len(), "UID resolution complete");
    }

    Ok(authors)
}

pub async fn fetch_following_publications_via_api(
    client: &reqwest::Client,
    sid: &str,
    uid: &str,
    cf_clearance: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut pubs: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut next_to: Option<String> = None;

    loop {
        let mut url = format!("https://medium.com/_/api/users/{}/following-publications?limit=200", uid);
        if let Some(ref to) = next_to {
            url.push_str(&format!("&to={}", to));
        }
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let resp = client.get(&url).headers(h).send().await
            .map_err(|e| format!("Publications API request failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Publications API returned {}", resp.status()));
        }
        let text = resp.text().await.map_err(|e| format!("Failed to read publications API response: {}", e))?;
        let payload = match parse_medium_api_json(&text) {
            Some(p) => p,
            None => return Err("Could not parse publications API response".to_string()),
        };

        let refs = payload.get("references");
        let mut found = 0usize;
        for collection_key in &["Collection", "Publication"] {
            if let Some(coll) = refs.and_then(|r| r.get(collection_key)).and_then(|c| c.as_object()) {
                for (_, entry) in coll {
                    let slug = entry.get("slug").and_then(|s| s.as_str()).unwrap_or("");
                    if !slug.is_empty() && seen.insert(slug.to_string()) {
                        pubs.push(("pub".to_string(), slug.to_string()));
                        found += 1;
                    }
                }
            }
        }

        tracing::info!(found, total = pubs.len(), "Publications API page");

        if found == 0 {
            break;
        }

        next_to = payload
            .get("paging").and_then(|p| p.get("next"))
            .and_then(|n| n.get("to")).and_then(|t| t.as_str())
            .map(|s| s.to_string());
        if next_to.is_none() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(300))).await;
    }

    Ok(pubs)
}

pub async fn fetch_following_list(
    sid: &str,
    uid: &str,
    cf_clearance: &str,
) -> Result<Vec<(String, String)>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    if !uid.is_empty() {
        let mut api_authors: Vec<(String, String)> = Vec::new();

        match fetch_following_via_api(&client, sid, uid, cf_clearance).await {
            Ok(people) => {
                tracing::info!(count = people.len(), "Fetched people following via API");
                api_authors.extend(people);
            }
            Err(e) => tracing::warn!(error = %e, "People following API failed"),
        }

        match fetch_following_publications_via_api(&client, sid, uid, cf_clearance).await {
            Ok(pubs) => {
                tracing::info!(count = pubs.len(), "Fetched publication following via API");
                api_authors.extend(pubs);
            }
            Err(e) => tracing::warn!(error = %e, "Publication following API failed"),
        }

        if !api_authors.is_empty() {
            tracing::info!(total = api_authors.len(), "Full following list from API");
            return Ok(api_authors);
        }
        tracing::warn!("Both API endpoints returned empty, falling back to HTML");
    }

    let mut html_headers = build_cookie_headers(sid, uid, cf_clearance);
    html_headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));

    let viewer_username: Option<String> = {
        match client.get("https://medium.com/me").headers(html_headers.clone()).send().await {
            Ok(resp) => {
                let final_url = resp.url().to_string();
                final_url.find("/@").map(|pos| {
                    final_url[pos + 2..].split('/').next().unwrap_or("").to_string()
                }).filter(|s| !s.is_empty())
            }
            Err(_) => None,
        }
    }.or_else(|| std::env::var("MEDIUM_USERNAME").ok().filter(|s| !s.is_empty()));

    let mut candidates: Vec<String> = Vec::new();
    if let Some(ref uname) = viewer_username {
        candidates.push(format!("https://medium.com/@{}/following", uname));
    }
    candidates.push("https://medium.com/me/following".to_string());
    candidates.push("https://medium.com/me/following-feed/all".to_string());
    candidates.push("https://medium.com/?feed=following".to_string());
    candidates.push("https://medium.com/me/feed".to_string());

    let mut seen_users: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_pubs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, url) in candidates.iter().enumerate() {
        if idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(400))).await;
        }
        match client.get(url.as_str()).headers(html_headers.clone()).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(text) = resp.text().await {
                    let (usernames, pub_slugs, _) = extract_following_from_html(&text);
                    let new_u = usernames.iter().filter(|u| seen_users.insert((*u).clone())).count();
                    let new_p = pub_slugs.iter().filter(|p| seen_pubs.insert((*p).clone())).count();
                    tracing::info!(source = url.as_str(), new_users = new_u, new_pubs = new_p,
                        total = seen_users.len() + seen_pubs.len(), "HTML following scrape");
                }
            }
            Ok(resp) => tracing::warn!(url = url.as_str(), status = %resp.status(), "HTML following endpoint error"),
            Err(e) => tracing::error!(url = url.as_str(), error = %e, "Failed to fetch HTML following endpoint"),
        }
    }

    if seen_users.is_empty() && seen_pubs.is_empty() {
        return Err("Could not fetch following list. Check MEDIUM_SID, MEDIUM_UID, and MEDIUM_CF_CLEARANCE.".to_string());
    }

    let mut authors: Vec<(String, String)> = seen_users.into_iter()
        .map(|u| ("user".to_string(), u)).collect();
    authors.extend(seen_pubs.into_iter().map(|p| ("pub".to_string(), p)));
    tracing::info!(total = authors.len(), "Combined following list from all HTML sources");
    Ok(authors)
}
