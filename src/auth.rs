use reqwest::header::HeaderValue;
use crate::net::build_cookie_headers;

pub async fn check_session(sid: &str, uid: &str, cf_clearance: &str) -> Result<(), String> {
    let headers = build_cookie_headers(sid, uid, cf_clearance);
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let resp = client
        .get("https://medium.com/me")
        .headers(headers)
        .header(reqwest::header::ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,*/*"))
        .send()
        .await
        .map_err(|e| format!("Session check network error: {}", e))?;

    let status = resp.status();
    let final_url = resp.url().to_string();

    if status.as_u16() == 429 {
        return Err("Rate limited (429) — your cf_clearance cookie has likely expired. Refresh it from your browser.".to_string());
    }
    if status.as_u16() == 403 {
        return Err("Access denied (403) — your cf_clearance cookie has likely expired. Refresh it from your browser.".to_string());
    }
    if final_url.contains("/login") || final_url.contains("/signin") {
        return Err("Session expired — your MEDIUM_SID cookie is invalid. Get a fresh one from your browser.".to_string());
    }
    if !status.is_success() {
        return Err(format!("Session check returned HTTP {} (url: {}) — cookies may be invalid.", status, final_url));
    }

    tracing::info!(status = %status, url = %final_url, "Session check passed");
    Ok(())
}

pub async fn setup_cookies() -> (String, String, String) {
    let sid = std::env::var("MEDIUM_SID").unwrap_or_default();
    let uid = std::env::var("MEDIUM_UID").unwrap_or_default();
    let cf_clearance = std::env::var("MEDIUM_CF_CLEARANCE").unwrap_or_default();

    let sid = if sid.is_empty() {
        eprint!("MEDIUM_SID (session cookie): ");
        rpassword::prompt_password("").unwrap_or_default()
    } else {
        sid
    };

    let cf_clearance = if cf_clearance.is_empty() {
        eprint!("MEDIUM_CF_CLEARANCE (Cloudflare cookie): ");
        rpassword::prompt_password("").unwrap_or_default()
    } else {
        cf_clearance
    };

    (sid, uid, cf_clearance)
}
