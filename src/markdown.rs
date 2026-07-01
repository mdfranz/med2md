pub fn load_preview_content(filename: &str) -> String {
    match std::fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => format!("Error loading file: {}", e),
    }
}

pub fn render_markdown(content: &str) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::text::{Line, Span};
    tui_markdown::from_str(content)
        .lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.spans
                    .into_iter()
                    .map(|span| Span::styled(span.content.into_owned(), span.style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}
