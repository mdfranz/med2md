use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub fn load_preview_lines(filename: &str) -> Vec<Line<'static>> {
    match std::fs::read_to_string(filename) {
        Ok(content) => parse_markdown_to_lines(&content),
        Err(e) => vec![Line::from(Span::styled(
            format!("Error loading file: {}", e),
            Style::default().fg(Color::Red),
        ))],
    }
}

pub fn create_span(text: String, bold: bool, italic: bool, code: bool) -> Span<'static> {
    let mut style = Style::default();
    if code {
        style = style.fg(Color::Green).bg(Color::Rgb(40, 40, 40));
    } else {
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
    }
    Span::styled(text, style)
}

pub fn parse_inline_formatting(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();

    let mut is_bold = false;
    let mut is_italic = false;
    let mut is_code = false;

    while let Some(c) = chars.next() {
        if c == '`' {
            if !current.is_empty() {
                spans.push(create_span(current.clone(), is_bold, is_italic, is_code));
                current.clear();
            }
            is_code = !is_code;
            continue;
        }

        if c == '*' {
            if chars.peek() == Some(&'*') {
                chars.next();
                if !current.is_empty() {
                    spans.push(create_span(current.clone(), is_bold, is_italic, is_code));
                    current.clear();
                }
                is_bold = !is_bold;
                continue;
            } else {
                if !current.is_empty() {
                    spans.push(create_span(current.clone(), is_bold, is_italic, is_code));
                    current.clear();
                }
                is_italic = !is_italic;
                continue;
            }
        }

        current.push(c);
    }

    if !current.is_empty() {
        spans.push(create_span(current, is_bold, is_italic, is_code));
    }

    spans
}

pub fn parse_markdown_to_lines(content: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();

        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            let style = Style::default().fg(Color::DarkGray);
            lines.push(Line::from(Span::styled("--- Code Block ---", style)));
            continue;
        }

        if in_code_block {
            let style = Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 30));
            lines.push(Line::from(Span::styled(format!("  {}", raw_line), style)));
            continue;
        }

        if trimmed.starts_with("# ") {
            let text = trimmed[2..].to_string();
            let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(text, style)));
            lines.push(Line::from(Span::styled("========================================", Style::default().fg(Color::Yellow))));
            continue;
        }
        if trimmed.starts_with("## ") {
            let text = trimmed[3..].to_string();
            let style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(text, style)));
            lines.push(Line::from(Span::styled("----------------------------------------", Style::default().fg(Color::White))));
            continue;
        }
        if trimmed.starts_with("### ") {
            let text = trimmed[4..].to_string();
            let style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(text, style)));
            continue;
        }

        if trimmed.starts_with('>') {
            let text = trimmed.trim_start_matches('>').trim().to_string();
            let style = Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC);
            lines.push(Line::from(vec![
                Span::styled("▍ ", Style::default().fg(Color::Yellow)),
                Span::styled(text, style),
            ]));
            continue;
        }

        if trimmed == "---" || trimmed == "***" || trimmed == "=====" {
            lines.push(Line::from(Span::styled("────────────────────────────────────────", Style::default().fg(Color::DarkGray))));
            continue;
        }

        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let text = trimmed[2..].to_string();
            let spans = parse_inline_formatting(&text);
            let mut line_spans = vec![Span::styled("• ", Style::default().fg(Color::Green))];
            line_spans.extend(spans);
            lines.push(Line::from(line_spans));
            continue;
        }

        let spans = parse_inline_formatting(raw_line);
        lines.push(Line::from(spans));
    }

    lines
}
