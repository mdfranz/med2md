pub fn load_preview_content(filename: &str) -> String {
    match std::fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => format!("Error loading file: {}", e),
    }
}
