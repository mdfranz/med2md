use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use scraper::{Html, Selector};
use tokio::sync::mpsc;
use crate::app::AppEvent;
use crate::util::{extract_slug, get_jitter_ms};
use crate::html::{clean_article_and_collect_images, clean_markdown, inject_source_link};
use chromiumoxide::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use futures::StreamExt;

pub fn build_cookie_headers(sid: &str, uid: &str, cf_clearance: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

    let mut cookie = String::new();
    if !sid.is_empty() {
        cookie.push_str(&format!("sid={}", sid));
    }
    if !uid.is_empty() {
        if !cookie.is_empty() { cookie.push_str("; "); }
        cookie.push_str(&format!("uid={}", uid));
    }
    if !cf_clearance.is_empty() {
        if !cookie.is_empty() { cookie.push_str("; "); }
        cookie.push_str(&format!("cf_clearance={}", cf_clearance));
    }
    if !cookie.is_empty() {
        if let Ok(val) = HeaderValue::from_str(&cookie) {
            headers.insert(reqwest::header::COOKIE, val);
        }
    }
    headers
}

pub async fn perform_download(
    client: &reqwest::Client,
    url_str: &str,
    sid: &str,
    uid: &str,
    cf_clearance: &str,
    output_dir: &str,
    force: bool,
    use_chromium: bool,
    tx: &mpsc::UnboundedSender<AppEvent>,
) -> Result<String, String> {
    let slug = extract_slug(url_str);
    let filename = format!("{}/{}.md", output_dir, slug);

    if !force && tokio::fs::metadata(&filename).await.is_ok() {
        tracing::info!(url = url_str, file = %filename, "Skipping — file already exists");
        return Ok(format!("{} (skipped, already exists)", filename));
    }

    let html_content = if use_chromium {
        let _ = tx.send(AppEvent::Log("Fetching article via headless Chromium...".to_string()));
        fetch_html_chromium(url_str, sid, uid, cf_clearance).await?
    } else {
        let mut headers = build_cookie_headers(sid, uid, cf_clearance);
        headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,image/apng,*/*;q=0.8"));

        tracing::info!(url = url_str, "Starting article download via reqwest");

        let response = client
            .get(url_str)
            .headers(headers)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(url = url_str, error = %e, "Network request failed");
                format!("Network request failed: {}", e)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            tracing::error!(url = url_str, status = %status, "HTTP error");
            return Err(format!("HTTP Error: {}", status));
        }

        response
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?
    };

    let images_dir_basename = format!("{}_images", slug);
    let images_dir_path = format!("{}/{}", output_dir, images_dir_basename);

    let (image_downloads, md_cleaned) = {
        let mut document = Html::parse_document(&html_content);

        let image_downloads = clean_article_and_collect_images(&mut document, &images_dir_basename)?;

        let article_selector = Selector::parse("article").unwrap();
        let cleaned_html = if let Some(article_ref) = document.select(&article_selector).next() {
            article_ref.html()
        } else {
            document.html()
        };

        let md = html2md::parse_html(&cleaned_html);
        let md_cleaned = clean_markdown(&md);
        let md_with_link = inject_source_link(&md_cleaned, url_str);

        (image_downloads, md_with_link)
    };

    tokio::fs::write(&filename, md_cleaned)
        .await
        .map_err(|e| format!("File write error: {}", e))?;

    tracing::info!(url = url_str, file = %filename, "Article saved");

    if !image_downloads.is_empty() {
        let _ = tx.send(AppEvent::Log(format!("Downloading {} images...", image_downloads.len())));
        if let Err(e) = tokio::fs::create_dir_all(&images_dir_path).await {
            let _ = tx.send(AppEvent::Log(format!("Warning: Failed to create images directory: {}", e)));
        } else {
            let mut img_count = 0;
            for (img_url, rel_path) in image_downloads {
                if img_count > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(250))).await;
                }
                img_count += 1;
                let local_path = format!("{}/{}", output_dir, rel_path.trim_start_matches("./"));
                let mut img_headers = HeaderMap::new();
                img_headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36"));
                match client.get(&img_url).headers(img_headers).send().await {
                    Ok(img_res) => {
                        if img_res.status().is_success() {
                            match img_res.bytes().await {
                                Ok(bytes) => {
                                    if let Err(e) = tokio::fs::write(&local_path, bytes).await {
                                        let _ = tx.send(AppEvent::Log(format!("Warning: Failed to save image {}: {}", local_path, e)));
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(AppEvent::Log(format!("Warning: Failed to read bytes for {}: {}", img_url, e)));
                                }
                            }
                        } else {
                            let _ = tx.send(AppEvent::Log(format!("Warning: Image status error {} for {}", img_res.status(), img_url)));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Log(format!("Warning: Failed to fetch image {}: {}", img_url, e)));
                    }
                }
            }
        }
    }

    Ok(filename)
}

pub async fn fetch_html_chromium(
    url_str: &str,
    sid: &str,
    uid: &str,
    cf_clearance: &str,
) -> Result<String, String> {
    let user_agent = std::env::var("MEDIUM_USER_AGENT")
        .unwrap_or_else(|_| "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36".to_string());

    let config = BrowserConfig::builder()
        .chrome_executable("/usr/bin/chromium")
        .arg("--headless")
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--host-resolver-rules=MAP challenges.cloudflare.com ~NOTFOUND")
        .arg(format!("--user-agent={}", user_agent))
        .build()
        .map_err(|e| format!("Failed to build browser config: {}", e))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("Failed to launch Chromium: {}", e))?;

    let handle = tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });

    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| format!("Failed to create new page: {}", e))?;

    let stealth_js = r#"
        try {
            delete Object.getPrototypeOf(navigator).webdriver;
        } catch (e) {}
        Object.defineProperty(navigator, 'plugins', { get: () => [1, 2, 3] });
        Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'] });
        window.chrome = { runtime: {} };
        try {
            const originalQuery = window.navigator.permissions.query;
            window.navigator.permissions.query = (parameters) => (
                parameters.name === 'notifications' ?
                    Promise.resolve({ state: Notification.permission }) :
                    originalQuery(parameters)
            );
        } catch (e) {}
        try {
            const getParameter = WebGLRenderingContext.prototype.getParameter;
            WebGLRenderingContext.prototype.getParameter = function(parameter) {
                if (parameter === 37445) return 'Google Inc. (Intel)';
                if (parameter === 37446) return 'Intel Iris OpenGL Engine';
                return getParameter(parameter);
            };
        } catch (e) {}
    "#;

    if let Err(e) = page.evaluate_on_new_document(stealth_js).await {
        tracing::warn!("Failed to inject stealth script: {}", e);
    }

    if let Err(e) = page.goto("https://medium.com/robots.txt").await {
        tracing::warn!("Initial navigation to medium.com/robots.txt failed (expected if not logged in yet): {}", e);
    }

    tracing::info!("Setting cookies in Chromium...");
    if !sid.is_empty() {
        let cookie = CookieParam::builder()
            .name("sid")
            .value(sid)
            .domain(".medium.com")
            .path("/")
            .secure(true)
            .build()
            .map_err(|e| format!("Failed to build sid cookie param: {}", e))?;
        page.set_cookie(cookie)
            .await
            .map_err(|e| format!("Failed to set sid cookie: {}", e))?;
    }
    if !uid.is_empty() {
        let cookie = CookieParam::builder()
            .name("uid")
            .value(uid)
            .domain(".medium.com")
            .path("/")
            .secure(true)
            .build()
            .map_err(|e| format!("Failed to build uid cookie param: {}", e))?;
        page.set_cookie(cookie)
            .await
            .map_err(|e| format!("Failed to set uid cookie: {}", e))?;
    }
    if !cf_clearance.is_empty() {
        let cookie = CookieParam::builder()
            .name("cf_clearance")
            .value(cf_clearance)
            .domain(".medium.com")
            .path("/")
            .secure(true)
            .build()
            .map_err(|e| format!("Failed to build cf_clearance cookie param: {}", e))?;
        page.set_cookie(cookie)
            .await
            .map_err(|e| format!("Failed to set cf_clearance cookie: {}", e))?;
    }

    match page.get_cookies().await {
        Ok(c) => {
            let names: Vec<String> = c.iter().map(|cookie| format!("{} (domain={})", cookie.name, cookie.domain)).collect();
            tracing::info!("Cookies currently in Chromium: {:?}", names);
        }
        Err(e) => {
            tracing::warn!("Failed to retrieve cookies in Chromium: {}", e);
        }
    }

    // Navigate to the target page URL
    tracing::info!("Navigating to article URL: {}", url_str);
    page.goto(url_str)
        .await
        .map_err(|e| format!("Failed to navigate to target URL: {}", e))?;

    if let Ok(Some(title)) = page.get_title().await {
        tracing::info!("Loaded page title: {}", title);
    }

    // Wait for the page content to load. Since Medium dynamically renders content,
    // we wait for the <article> element. We'll poll for it up to 5 seconds.
    let poll_start = std::time::Instant::now();
    while poll_start.elapsed() < std::time::Duration::from_secs(5) {
        if page.find_element("article").await.is_ok() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }

    // Get rendered HTML
    let html = page
        .content()
        .await
        .map_err(|e| format!("Failed to get page content: {}", e))?;

    // Clean up
    let _ = browser.close().await;
    let _ = handle.await;

    Ok(html)
}
