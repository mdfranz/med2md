use rss::Channel as RssChannel;

pub fn clean_rss_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut u) => {
            let cleaned: Vec<(String, String)> = u.query_pairs()
                .filter(|(k, _)| !k.starts_with("source") && k != "referrer" && k != "gi")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            u.set_query(None);
            if !cleaned.is_empty() {
                let mut qs = u.query_pairs_mut();
                for (k, v) in cleaned {
                    qs.append_pair(&k, &v);
                }
            }
            u.to_string()
        }
        Err(_) => url.to_string(),
    }
}

pub fn parse_rss_items(content: &str) -> Vec<(i64, String, String, String)> {
    match RssChannel::read_from(content.as_bytes()) {
        Ok(channel) => channel.items().iter()
            .filter_map(|item| {
                let title = item.title()?.to_string();
                let link = item.link()?.to_string();
                if link.is_empty() { return None; }
                let ts = item.pub_date()
                    .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);
                let author = item.dublin_core_ext()
                    .and_then(|dc| dc.creators().first().cloned())
                    .or_else(|| item.author().map(|s| s.to_string()))
                    .unwrap_or_default();
                Some((ts, title, link, author))
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn extract_following_from_html(
    html: &str,
) -> (Vec<String>, Vec<String>, Vec<(i64, String, String, String)>) {
    let marker = "window.__APOLLO_STATE__ = ";
    let start = match html.find(marker) {
        Some(i) => i + marker.len(),
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };
    let end = match html[start..].find("</script>") {
        Some(i) => start + i,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };
    let json_str = &html[start..end];

    let state: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Vec::new(), Vec::new()),
    };

    let mut usernames = Vec::new();
    let mut pub_slugs = Vec::new();
    let mut posts: Vec<(i64, String, String, String)> = Vec::new();

    let obj = match state.as_object() {
        Some(o) => o,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };

    let mut user_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (key, value) in obj {
        if key.starts_with("User:") {
            let display = value.get("name").and_then(|v| v.as_str())
                .or_else(|| value.get("username").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            if !display.is_empty() {
                user_names.insert(key.clone(), display);
            }
            if let Some(u) = value.get("username").and_then(|v| v.as_str()) {
                if !u.is_empty() && !usernames.contains(&u.to_string()) {
                    usernames.push(u.to_string());
                }
            }
        } else if key.starts_with("Publication:") {
            if let Some(s) = value.get("slug").and_then(|v| v.as_str()) {
                if !s.is_empty() && !pub_slugs.contains(&s.to_string()) {
                    pub_slugs.push(s.to_string());
                }
            }
        }
    }

    for (key, value) in obj {
        if !key.starts_with("Post:") { continue; }
        let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let url = value.get("mediumUrl").and_then(|v| v.as_str())
            .or_else(|| value.get("uniqueSlug").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        if title.is_empty() || url.is_empty() { continue; }
        let ts = value.get("firstPublishedAt")
            .or_else(|| value.get("publishedAt"))
            .or_else(|| value.get("createdAt"))
            .and_then(|v| v.as_i64())
            .map(|ms| ms / 1000)
            .unwrap_or(0);
        let author = value.get("creator")
            .and_then(|c| c.get("__ref"))
            .and_then(|r| r.as_str())
            .and_then(|r| user_names.get(r))
            .cloned()
            .unwrap_or_default();
        posts.push((ts, title, url, author));
    }

    posts.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    (usernames, pub_slugs, posts)
}

pub fn parse_medium_api_json(text: &str) -> Option<serde_json::Value> {
    let start = text.find('{')?;
    let parsed: serde_json::Value = serde_json::from_str(&text[start..]).ok()?;
    parsed.get("payload").cloned()
}

pub fn extract_user_id_from_apollo(html: &str, username: &str) -> Option<String> {
    let marker = "window.__APOLLO_STATE__ = ";
    let start = html.find(marker)? + marker.len();
    let end = start + html[start..].find("</script>")?;
    let state: serde_json::Value = serde_json::from_str(&html[start..end]).ok()?;
    let obj = state.as_object()?;
    for (key, value) in obj {
        if key.starts_with("User:") {
            if value.get("username").and_then(|u| u.as_str()) == Some(username) {
                return Some(key.trim_start_matches("User:").to_string());
            }
        }
    }
    None
}
