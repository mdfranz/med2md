use crate::net::build_cookie_headers;

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

    let headers = build_cookie_headers(&sid, &uid, &cf_clearance);
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap_or_default();
    match client.get("https://medium.com/me/feed").send().await {
        Ok(resp) if resp.status().is_success() => {
            eprintln!("Cookies OK ({})", resp.status());
        }
        Ok(resp) => {
            eprintln!("Warning: cookie test returned {} — downloads may fail", resp.status());
        }
        Err(e) => {
            eprintln!("Warning: cookie test failed: {} — continuing anyway", e);
        }
    }

    (sid, uid, cf_clearance)
}
