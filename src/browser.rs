use tokio::sync::mpsc;
use reqwest::header::{HeaderValue, ACCEPT};
use crate::app::AppEvent;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum LinkKind {
    Article,
    Author,
    Feed,
    Other,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BrowserLink {
    pub text: String,
    pub url: String,
    pub kind: LinkKind,
    pub date: String,
}

pub enum BrowserCommand {
    Navigate(String),
    ScrollDown,
    GoBack,
}

pub async fn run_browser_task(
    sid: String,
    uid: String,
    cf_clearance: String,
    initial_url: String,
    tx: mpsc::UnboundedSender<AppEvent>,
    mut rx: mpsc::UnboundedReceiver<BrowserCommand>,
) {
    let mut headers = crate::net::build_cookie_headers(&sid, &uid, &cf_clearance);
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,image/apng,*/*;q=0.8"),
    );

    let client = match reqwest::Client::builder().default_headers(headers).build() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AppEvent::Log(format!("Error: Failed to build HTTP client: {}", e)));
            return;
        }
    };

    let mut history: Vec<String> = Vec::new();
    let mut current_url = initial_url.clone();

    if current_url == "https://medium.com" || current_url == "https://medium.com/" {
        current_url = "https://medium.com/me/feed".to_string();
    }

    let _ = tx.send(AppEvent::Log(format!("Loading {}...", current_url)));
    load_and_report(&client, &current_url, &tx).await;

    while let Some(cmd) = rx.recv().await {
        match cmd {
            BrowserCommand::Navigate(url) => {
                let mut target_url = url.clone();
                if target_url == "https://medium.com"
                    || target_url == "https://medium.com/"
                    || (target_url.starts_with("https://medium.com/?") && target_url.contains("source=home"))
                    || (target_url.starts_with("https://medium.com?") && target_url.contains("source=home"))
                {
                    target_url = "https://medium.com/me/feed".to_string();
                }
                let _ = tx.send(AppEvent::Log(format!("Loading {}...", target_url)));
                history.push(current_url.clone());
                current_url = target_url;
                load_and_report(&client, &current_url, &tx).await;
            }
            BrowserCommand::ScrollDown => {
                let _ = tx.send(AppEvent::Log("Scroll down is not supported in static mode".to_string()));
            }
            BrowserCommand::GoBack => {
                if let Some(prev_url) = history.pop() {
                    let _ = tx.send(AppEvent::Log(format!("Loading {}...", prev_url)));
                    current_url = prev_url.clone();
                    load_and_report(&client, &current_url, &tx).await;
                } else {
                    let _ = tx.send(AppEvent::Log("Browser history is empty.".to_string()));
                }
            }
        }
    }
}

/// Loads a URL and sends the resulting links (or an error) to the UI.
///
/// Medium's Cloudflare WAF blocks plain HTML GETs to profile/publication
/// pages (e.g. `medium.com/@user`) with a JS challenge, even with valid
/// session cookies — but the RSS feed for that same author/publication
/// (`medium.com/feed/@user`) is not gated. So for those URLs we fetch RSS
/// instead of scraping HTML; everything else keeps using the HTML scrape.
async fn load_and_report(client: &reqwest::Client, url: &str, tx: &mpsc::UnboundedSender<AppEvent>) {
    if url == "https://medium.com/me/feed" {
        // Streams its own BrowserReady/BrowserLinksUpdated events as RSS feeds
        // complete, so the TUI populates progressively instead of blocking.
        if let Err(e) = load_full_feed(client, url, tx).await {
            let _ = tx.send(AppEvent::Log(format!("Failed to load page: {}", e)));
        }
        return;
    }

    let result = if let Some(rss_url) = rss_url_for_profile(url) {
        match fetch_rss_links(client, &rss_url).await {
            Ok(links) if !links.is_empty() => Ok(links.into_iter().map(|(_ts, link)| link).collect()),
            Ok(_) => {
                let _ = tx.send(AppEvent::Log(format!("RSS feed for {} was empty, falling back to HTML...", url)));
                fetch_and_parse(client, url).await
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Log(format!("RSS fetch failed ({}), falling back to HTML...", e)));
                fetch_and_parse(client, url).await
            }
        }
    } else {
        fetch_and_parse(client, url).await
    };

    match result {
        Ok(links) => {
            let _ = tx.send(AppEvent::BrowserReady { url: url.to_string(), links });
        }
        Err(e) => {
            let _ = tx.send(AppEvent::Log(format!("Failed to load page: {}", e)));
        }
    }
}

/// Loads the /me/feed landing page, then walks every followed author/publication
/// link found on it and pulls their RSS feed too, so the landing screen shows a
/// full, pickable article list (like `--feed` does) instead of just the handful
/// of articles Medium happens to server-render plus a list of authors to drill into.
///
/// Sends an initial `BrowserReady` right away with whatever was on the landing
/// page, then a `BrowserLinksUpdated` after each author/publication's RSS feed
/// comes back, so the list grows in the TUI in near real time rather than the
/// UI sitting on "Loading..." for the ~20-30s it takes to walk every feed.
async fn load_full_feed(client: &reqwest::Client, url: &str, tx: &mpsc::UnboundedSender<AppEvent>) -> Result<(), String> {
    let links = fetch_and_parse(client, url).await?;

    let mut articles: Vec<BrowserLink> = Vec::new();
    let mut rest: Vec<BrowserLink> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut author_feeds: Vec<String> = Vec::new();

    for link in links {
        seen.insert(link.url.clone());
        if link.kind == LinkKind::Article {
            articles.push(link);
        } else {
            if link.kind == LinkKind::Author {
                if let Some(rss_url) = rss_url_for_profile(&link.url) {
                    author_feeds.push(rss_url);
                }
            }
            rest.push(link);
        }
    }

    let mut combined: Vec<BrowserLink> = articles.clone();
    combined.extend(rest.iter().cloned());
    let _ = tx.send(AppEvent::BrowserReady { url: url.to_string(), links: combined });

    if author_feeds.is_empty() {
        return Ok(());
    }

    let total = author_feeds.len();
    let _ = tx.send(AppEvent::Log(format!("Fetching articles from {} followed authors/publications...", total)));

    let mut fetched: Vec<(i64, BrowserLink)> = Vec::new();
    for (idx, rss_url) in author_feeds.iter().enumerate() {
        if idx > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(crate::util::get_jitter_ms(1200))).await;
        }
        match fetch_rss_links(client, rss_url).await {
            Ok(items) => {
                let mut added = 0;
                for (ts, link) in items {
                    if seen.insert(link.url.clone()) {
                        fetched.push((ts, link));
                        added += 1;
                    }
                }
                fetched.sort_unstable_by(|a, b| b.0.cmp(&a.0));

                let mut combined: Vec<BrowserLink> = articles.clone();
                combined.extend(fetched.iter().map(|(_ts, link)| link.clone()));
                combined.extend(rest.iter().cloned());

                let _ = tx.send(AppEvent::Log(format!("[{}/{}] +{} articles ({} total so far)", idx + 1, total, added, combined.len())));
                let _ = tx.send(AppEvent::BrowserLinksUpdated { url: url.to_string(), links: combined });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Log(format!("[{}/{}] RSS fetch failed: {}", idx + 1, total, e)));
            }
        }
    }

    Ok(())
}

/// Maps an author/publication profile URL to its RSS feed URL, if applicable.
fn rss_url_for_profile(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;

    if host == "medium.com" {
        let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
        if segments.len() != 1 {
            return None;
        }
        let seg = segments[0];
        if let Some(username) = seg.strip_prefix('@') {
            return Some(format!("https://medium.com/feed/@{}", username));
        }
        if is_boilerplate_url(url) || seg == "me" || seg == "search" || seg == "feed" {
            return None;
        }
        return Some(format!("https://medium.com/feed/{}", seg));
    }

    if host.ends_with(".medium.com") && host != "api.medium.com" {
        let root_path = parsed.path_segments().map(|mut s| s.next().unwrap_or("").is_empty()).unwrap_or(true);
        if root_path {
            return Some(format!("https://{}/feed", host));
        }
    }

    None
}

async fn fetch_rss_links(client: &reqwest::Client, rss_url: &str) -> Result<Vec<(i64, BrowserLink)>, String> {
    let res = client
        .get(rss_url)
        .header(ACCEPT, "application/rss+xml, text/xml, */*")
        .send()
        .await
        .map_err(|e| format!("RSS request failed: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("RSS HTTP status error: {}", res.status()));
    }
    let text = res.text().await.map_err(|e| format!("Failed to read RSS body: {}", e))?;

    let items = crate::feed::parse_rss_items(&text);
    let links = items
        .into_iter()
        .map(|(ts, title, url, author)| {
            let clean_url = crate::feed::clean_rss_url(&url);
            let text = if author.is_empty() { title } else { format!("{} (by {})", title, author) };
            let date = crate::util::format_date(ts);
            (ts, BrowserLink { text, url: clean_url, kind: LinkKind::Article, date })
        })
        .collect();
    Ok(links)
}

async fn fetch_and_parse(client: &reqwest::Client, url: &str) -> Result<Vec<BrowserLink>, String> {
    let res = client.get(url).send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("HTTP status error: {}", res.status()));
    }
    let html = res.text().await.map_err(|e| format!("Failed to read response body: {}", e))?;

    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Extract feed articles and following data from window.__APOLLO_STATE__ (which contains client-side rendered feed content)
    let (usernames, pub_slugs, posts) = crate::feed::extract_following_from_html(&html);
    for (ts, title, post_url, author) in posts {
        let mut full_url = post_url.trim().to_string();
        if !full_url.starts_with("http") {
            if full_url.starts_with('/') {
                full_url = format!("https://medium.com{}", full_url);
            } else {
                full_url = format!("https://medium.com/{}", full_url);
            }
        }
        if let Some(pos) = full_url.find('?') {
            full_url.truncate(pos);
        }
        if is_boilerplate_url(&full_url) {
            continue;
        }
        if seen.insert(full_url.clone()) {
            let display_text = if author.is_empty() {
                title
            } else {
                format!("{} (by {})", title, author)
            };
            links.push(BrowserLink {
                text: display_text,
                url: full_url,
                kind: LinkKind::Article,
                date: crate::util::format_date(ts),
            });
        }
    }

    // 2. Extract followed authors and publications directory from window.__APOLLO_STATE__
    for username in usernames {
        let mut author_url = format!("https://medium.com/@{}", username);
        if let Some(pos) = author_url.find('?') {
            author_url.truncate(pos);
        }
        if is_boilerplate_url(&author_url) {
            continue;
        }
        if seen.insert(author_url.clone()) {
            links.push(BrowserLink {
                text: format!("Author: @{}", username),
                url: author_url,
                kind: LinkKind::Author,
                date: String::new(),
            });
        }
    }
    for slug in pub_slugs {
        let mut pub_url = format!("https://medium.com/{}", slug);
        if let Some(pos) = pub_url.find('?') {
            pub_url.truncate(pos);
        }
        if is_boilerplate_url(&pub_url) {
            continue;
        }
        if seen.insert(pub_url.clone()) {
            links.push(BrowserLink {
                text: format!("Publication: {}", slug),
                url: pub_url,
                kind: LinkKind::Author,
                date: String::new(),
            });
        }
    }

    // 3. Parse regular <a> tags in the HTML body (for sidebars, headers, footer links)
    let document = scraper::Html::parse_document(&html);
    let a_selector = scraper::Selector::parse("a").map_err(|e| format!("Failed to parse selector: {:?}", e))?;
    let slug_pattern = regex::Regex::new(r"-[a-f0-9]{10,12}$").unwrap();

    for element in document.select(&a_selector) {
        let href = element.value().attr("href").unwrap_or_default().trim().to_string();
        if href.is_empty() {
            continue;
        }

        let mut full_url = href.clone();
        if href.starts_with('/') {
            full_url = format!("https://medium.com{}", href);
        }

        if let Some(pos) = full_url.find('?') {
            full_url.truncate(pos);
        }

        let text = element.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if text.is_empty() || !full_url.starts_with("http") {
            continue;
        }

        if is_boilerplate_url(&full_url) {
            continue;
        }

        if !seen.insert(full_url.clone()) {
            continue;
        }

        let kind = if full_url.contains("feed=following") || full_url.contains("/me/feed") {
            LinkKind::Feed
        } else if full_url.contains("medium.com/@") || full_url.contains("/@") {
            if slug_pattern.is_match(&full_url) {
                LinkKind::Article
            } else {
                LinkKind::Author
            }
        } else if slug_pattern.is_match(&full_url) {
            LinkKind::Article
        } else {
            // Check if it's a publication or generic feed link
            let is_medium_subdomain = if let Ok(parsed) = url::Url::parse(&full_url) {
                if let Some(host) = parsed.host_str() {
                    host.ends_with(".medium.com") && host != "medium.com" && host != "api.medium.com"
                } else {
                    false
                }
            } else {
                false
            };

            let is_publication_path = if let Ok(parsed) = url::Url::parse(&full_url) {
                if parsed.host_str() == Some("medium.com") {
                    let segments: Vec<&str> = parsed.path_segments().map(|s| s.collect()).unwrap_or_default();
                    segments.len() == 1 && !segments[0].is_empty()
                } else {
                    false
                }
            } else {
                false
            };

            if is_medium_subdomain || is_publication_path {
                LinkKind::Author
            } else {
                LinkKind::Other
            }
        };

        links.push(BrowserLink { text, url: full_url, kind, date: String::new() });
    }

    Ok(links)
}

fn is_boilerplate_url(url: &str) -> bool {
    let u = url.to_lowercase();
    u.contains("/sitemap")
        || u.contains("play.google.com")
        || u.contains("apps.apple.com")
        || u.contains("/about")
        || u.contains("/jobs-at-medium")
        || u.contains("/careers")
        || u.contains("status.medium.com")
        || u.contains("blog.medium.com")
        || u.contains("policy.medium.com")
        || u.contains("speechify.com")
        || u.contains("/new-story")
        || u.contains("/search")
        || u.contains("/notifications")
        || u.contains("/store")
        || u.contains("/gift-plans")
        || u.contains("/membership")
        || u.contains("/verified-creators")
        || u.contains("/creators")
        || u.contains("/press")
        || u.contains("/m/signin")
        || u.contains("/m/callback")
}

