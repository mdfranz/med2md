use tokio::sync::mpsc;
use chromiumoxide::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use futures::StreamExt;
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
    let config = match BrowserConfig::builder()
        .chrome_executable("/usr/bin/chromium")
        .arg("--headless")
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .build()
    {
        Ok(cfg) => cfg,
        Err(e) => {
            let _ = tx.send(AppEvent::Log(format!("Error: Failed to build browser config: {}", e)));
            return;
        }
    };

    let (mut browser, mut handler) = match Browser::launch(config).await {
        Ok(res) => res,
        Err(e) => {
            let _ = tx.send(AppEvent::Log(format!("Error: Failed to launch Chromium: {}", e)));
            return;
        }
    };

    let _handler_handle = tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });

    let page = match browser.new_page("about:blank").await {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(AppEvent::Log(format!("Error: Failed to create page: {}", e)));
            let _ = browser.close().await;
            return;
        }
    };

    if let Err(e) = page.goto("https://medium.com").await {
        tracing::warn!("Browser task initial goto failed: {}", e);
    }

    let mut cookies = Vec::new();
    if !sid.is_empty() {
        cookies.push(("sid", sid));
    }
    if !uid.is_empty() {
        cookies.push(("uid", uid));
    }
    if !cf_clearance.is_empty() {
        cookies.push(("cf_clearance", cf_clearance));
    }

    for (name, val) in cookies {
        if let Ok(cookie) = CookieParam::builder()
            .name(name)
            .value(val)
            .domain(".medium.com")
            .path("/")
            .secure(true)
            .build()
        {
            let _ = page.set_cookie(cookie).await;
        }
    }

    let mut history: Vec<String> = Vec::new();
    let mut current_url = initial_url.clone();

    let _ = tx.send(AppEvent::Log(format!("Browser navigating to {}...", current_url)));
    if let Err(e) = page.goto(&current_url).await {
        let _ = tx.send(AppEvent::Log(format!("Navigation error: {}", e)));
    } else {
        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
        match extract_links(&page).await {
            Ok(links) => {
                let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Log(format!("Failed to extract links: {}", e)));
            }
        }
    }

    while let Some(cmd) = rx.recv().await {
        match cmd {
            BrowserCommand::Navigate(url) => {
                let _ = tx.send(AppEvent::Log(format!("Navigating to {}...", url)));
                history.push(current_url.clone());
                current_url = url.clone();
                if let Err(e) = page.goto(&current_url).await {
                    let _ = tx.send(AppEvent::Log(format!("Navigation error: {}", e)));
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                    match extract_links(&page).await {
                        Ok(links) => {
                            let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Log(format!("Failed to extract links: {}", e)));
                        }
                    }
                }
            }
            BrowserCommand::ScrollDown => {
                let _ = tx.send(AppEvent::Log("Scrolling down to load more...".to_string()));
                let _ = page.evaluate("window.scrollBy(0, window.innerHeight);").await;
                tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;
                match extract_links(&page).await {
                    Ok(links) => {
                        let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Log(format!("Failed to extract links: {}", e)));
                    }
                }
            }
            BrowserCommand::GoBack => {
                if let Some(prev_url) = history.pop() {
                    let _ = tx.send(AppEvent::Log(format!("Going back to {}...", prev_url)));
                    current_url = prev_url.clone();
                    if let Err(e) = page.goto(&current_url).await {
                        let _ = tx.send(AppEvent::Log(format!("Navigation error: {}", e)));
                    } else {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                        match extract_links(&page).await {
                            Ok(links) => {
                                let _ = tx.send(AppEvent::BrowserReady { url: current_url.clone(), links });
                            }
                            Err(e) => {
                                let _ = tx.send(AppEvent::Log(format!("Failed to extract links: {}", e)));
                            }
                        }
                    }
                } else {
                    let _ = tx.send(AppEvent::Log("Browser history is empty.".to_string()));
                }
            }
        }
    }

    let _ = browser.close().await;
}

#[derive(serde::Deserialize, Debug)]
struct RawLink {
    text: String,
    href: String,
}

async fn extract_links(page: &chromiumoxide::Page) -> Result<Vec<BrowserLink>, String> {
    let eval_res = page.evaluate(r#"
        () => {
            return Array.from(document.querySelectorAll('a')).map(a => {
                return {
                    text: a.innerText.trim(),
                    href: a.href
                };
            });
        }
    "#).await.map_err(|e| format!("JS evaluation error: {}", e))?;

    let raw_links: Vec<RawLink> = eval_res.into_value().map_err(|e| format!("Failed to deserialize links: {}", e))?;

    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let slug_pattern = regex::Regex::new(r"-[a-f0-9]{10,12}$").unwrap();

    for raw in raw_links {
        let url = raw.href.trim().to_string();
        let text = raw.text.replace("\n", " ").trim().to_string();

        if url.is_empty() || !url.starts_with("http") || text.is_empty() {
            continue;
        }

        if !seen.insert(url.clone()) {
            continue;
        }

        let kind = if url.contains("feed=following") || url.contains("/me/feed") {
            LinkKind::Feed
        } else if url.contains("medium.com/@") || url.contains("/@") {
            if slug_pattern.is_match(&url) {
                LinkKind::Article
            } else {
                LinkKind::Author
            }
        } else if slug_pattern.is_match(&url) {
            LinkKind::Article
        } else {
            LinkKind::Other
        };

        links.push(BrowserLink { text, url, kind });
    }

    Ok(links)
}
