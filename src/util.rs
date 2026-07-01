pub fn get_char_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .map(|(byte_idx, _)| byte_idx)
        .nth(char_idx)
        .unwrap_or_else(|| s.len())
}

pub fn get_extension(url: &str) -> Option<&str> {
    let path = url.split('?').next()?;
    let segment = path.split('/').last()?;
    let ext = segment.split('.').last()?;
    if ext.len() <= 4 && !ext.is_empty() {
        Some(ext)
    } else {
        None
    }
}

pub fn extract_slug(url: &str) -> String {
    let slug = url
        .split('/')
        .last()
        .and_then(|s| s.split('?').next())
        .unwrap_or("medium_article");
    if slug.is_empty() {
        "medium_article".to_string()
    } else {
        slug.to_string()
    }
}

pub fn format_ts(ts: i64) -> String {
    if ts == 0 { return String::new(); }
    match chrono::DateTime::from_timestamp(ts, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

pub fn format_date(ts: i64) -> String {
    if ts == 0 { return String::new(); }
    match chrono::DateTime::from_timestamp(ts, 0) {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => String::new(),
    }
}

pub fn get_jitter_ms(base_ms: u64) -> u64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let factor = 0.75 + ((nanos % 75) as f64 / 100.0);
    (base_ms as f64 * factor) as u64
}
