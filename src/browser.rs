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
    match fetch_and_parse(&client, &current_url).await {
        Ok(links) => {
            let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
        }
        Err(e) => {
            let _ = tx.send(AppEvent::Log(format!("Failed to load page: {}", e)));
        }
    }

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
                match fetch_and_parse(&client, &current_url).await {
                    Ok(links) => {
                        let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Log(format!("Failed to load page: {}", e)));
                    }
                }
            }
            BrowserCommand::ScrollDown => {
                let _ = tx.send(AppEvent::Log("Scroll down is not supported in static mode".to_string()));
            }
            BrowserCommand::GoBack => {
                if let Some(prev_url) = history.pop() {
                    let _ = tx.send(AppEvent::Log(format!("Loading {}...", prev_url)));
                    current_url = prev_url.clone();
                    match fetch_and_parse(&client, &current_url).await {
                        Ok(links) => {
                            let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Log(format!("Failed to load page: {}", e)));
                        }
                    }
                } else {
                    let _ = tx.send(AppEvent::Log("Browser history is empty.".to_string()));
                }
            }
        }
    }
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
    for (_ts, title, post_url, author) in posts {
        let mut full_url = post_url.trim().to_string();
        if !full_url.starts_with("http") {
            if full_url.starts_with('/') {
                full_url = format!("https://medium.com{}", full_url);
            } else {
                full_url = format!("https://medium.com/{}", full_url);
            }
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
            });
        }
    }

    // 2. Extract followed authors and publications directory from window.__APOLLO_STATE__
    for username in usernames {
        let author_url = format!("https://medium.com/@{}", username);
        if seen.insert(author_url.clone()) {
            links.push(BrowserLink {
                text: format!("Author: @{}", username),
                url: author_url,
                kind: LinkKind::Author,
            });
        }
    }
    for slug in pub_slugs {
        let pub_url = format!("https://medium.com/{}", slug);
        if seen.insert(pub_url.clone()) {
            links.push(BrowserLink {
                text: format!("Publication: {}", slug),
                url: pub_url,
                kind: LinkKind::Author,
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

        let text = element.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if text.is_empty() || !full_url.starts_with("http") {
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
            LinkKind::Other
        };

        links.push(BrowserLink { text, url: full_url, kind });
    }

    Ok(links)
}
