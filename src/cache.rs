use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_AUTHORS: &str = "authors.json";
const CACHE_FEED: &str = "feed.json";
const CACHE_META: &str = "authors_meta.json";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache_raw(cache_dir: &str, filename: &str, max_age_secs: u64) -> Option<serde_json::Value> {
    let path = format!("{}/{}", cache_dir, filename);
    let text = std::fs::read_to_string(&path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    let fetched_at = val["fetched_at"].as_u64()?;
    if now_secs().saturating_sub(fetched_at) > max_age_secs {
        return None;
    }
    Some(val)
}

fn write_cache_raw(cache_dir: &str, filename: &str, data: &serde_json::Value) {
    let _ = std::fs::create_dir_all(cache_dir);
    let path = format!("{}/{}", cache_dir, filename);
    let payload = serde_json::json!({ "fetched_at": now_secs(), "data": data });
    if let Ok(text) = serde_json::to_string(&payload) {
        let _ = std::fs::write(path, text);
    }
}

pub fn read_authors_cache(cache_dir: &str, max_age_secs: u64) -> Option<Vec<(String, String)>> {
    let val = read_cache_raw(cache_dir, CACHE_AUTHORS, max_age_secs)?;
    serde_json::from_value(val["data"].clone()).ok()
}

pub fn write_authors_cache(cache_dir: &str, data: &[(String, String)]) {
    write_cache_raw(cache_dir, CACHE_AUTHORS, &serde_json::json!(data));
}

pub fn read_feed_cache(cache_dir: &str, max_age_secs: u64) -> Option<Vec<(String, String, String, String)>> {
    let val = read_cache_raw(cache_dir, CACHE_FEED, max_age_secs)?;
    serde_json::from_value(val["data"].clone()).ok()
}

pub fn write_feed_cache(cache_dir: &str, data: &[(String, String, String, String)]) {
    write_cache_raw(cache_dir, CACHE_FEED, &serde_json::json!(data));
}

pub fn read_meta_cache(cache_dir: &str, max_age_secs: u64) -> Option<HashMap<String, (i64, usize)>> {
    let val = read_cache_raw(cache_dir, CACHE_META, max_age_secs)?;
    serde_json::from_value(val["data"].clone()).ok()
}

pub fn write_meta_cache(cache_dir: &str, data: &HashMap<String, (i64, usize)>) {
    write_cache_raw(cache_dir, CACHE_META, &serde_json::json!(data));
}
