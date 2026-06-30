use std::io;
use std::time::Duration;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, List, ListItem},
    Frame, Terminal,
};
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use scraper::{Html, Selector};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use tokio::sync::mpsc;
use markup5ever::{QualName, Namespace, LocalName};
use rss::Channel as RssChannel;

enum AppView {
    Download,
    Picker {
        files: Vec<String>,
        selected_idx: usize,
        preview_lines: Vec<Line<'static>>,
        preview_scroll_y: usize,
    },
    FeedSelector,
}

enum AppEvent {
    Log(String),
    DownloadFinished,
}

struct App {
    sid: String,
    uid: String,
    cf_clearance: String,
    urls: Vec<String>,
    cursor_x: usize,
    cursor_y: usize,
    logs: Vec<String>,
    is_downloading: bool,
    force_download: bool,
    urls_scroll_y: usize,
    view: AppView,
    output_dir: String,
    feed_articles: Vec<(String, String, String, String)>,
    feed_selected: Vec<bool>,
    feed_cursor: usize,
    feed_scroll: usize,
}

impl App {
    fn new(sid: String, uid: String, cf_clearance: String, output_dir: String) -> Self {
        Self {
            sid,
            uid,
            cf_clearance,
            output_dir,
            urls: vec![String::new()],
            cursor_x: 0,
            cursor_y: 0,
            logs: vec![
                "Welcome to med2md! Paste URLs below, then Ctrl+S to download.".to_string(),
                "Ctrl+P: Markdown preview picker  |  Ctrl+L: Clear log  |  Esc: Quit".to_string(),
            ],
            is_downloading: false,
            force_download: false,
            urls_scroll_y: 0,
            view: AppView::Download,
            feed_articles: Vec::<(String, String, String, String)>::new(),
            feed_selected: Vec::new(),
            feed_cursor: 0,
            feed_scroll: 0,
        }
    }
}

fn get_char_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .map(|(byte_idx, _)| byte_idx)
        .nth(char_idx)
        .unwrap_or_else(|| s.len())
}

fn handle_multiline_key(app: &mut App, key: KeyEvent) {
    let line_count = app.urls.len();
    let y = app.cursor_y;
    let x = app.cursor_x;

    match key.code {
        KeyCode::Char(c) => {
            let line = &mut app.urls[y];
            let idx = get_char_byte_index(line, x);
            line.insert(idx, c);
            app.cursor_x += 1;
        }
        KeyCode::Enter => {
            let line = &mut app.urls[y];
            let idx = get_char_byte_index(line, x);
            let next_part = line.split_off(idx);
            app.urls.insert(y + 1, next_part);
            app.cursor_y += 1;
            app.cursor_x = 0;
        }
        KeyCode::Backspace => {
            if x > 0 {
                let line = &mut app.urls[y];
                app.cursor_x -= 1;
                let idx = get_char_byte_index(line, app.cursor_x);
                line.remove(idx);
            } else if y > 0 {
                let current_line = app.urls.remove(y);
                let prev_line = &mut app.urls[y - 1];
                let prev_len = prev_line.chars().count();
                prev_line.push_str(&current_line);
                app.cursor_y -= 1;
                app.cursor_x = prev_len;
            }
        }
        KeyCode::Delete => {
            let line = &app.urls[y];
            let line_len = line.chars().count();
            if x < line_len {
                let line_mut = &mut app.urls[y];
                let idx = get_char_byte_index(line_mut, x);
                line_mut.remove(idx);
            } else if y + 1 < line_count {
                let next_line = app.urls.remove(y + 1);
                app.urls[y].push_str(&next_line);
            }
        }
        KeyCode::Left => {
            if x > 0 {
                app.cursor_x -= 1;
            } else if y > 0 {
                app.cursor_y -= 1;
                app.cursor_x = app.urls[app.cursor_y].chars().count();
            }
        }
        KeyCode::Right => {
            let line_len = app.urls[y].chars().count();
            if x < line_len {
                app.cursor_x += 1;
            } else if y + 1 < line_count {
                app.cursor_y += 1;
                app.cursor_x = 0;
            }
        }
        KeyCode::Up => {
            if y > 0 {
                app.cursor_y -= 1;
                let next_len = app.urls[app.cursor_y].chars().count();
                app.cursor_x = app.cursor_x.min(next_len);
            }
        }
        KeyCode::Down => {
            if y + 1 < line_count {
                app.cursor_y += 1;
                let next_len = app.urls[app.cursor_y].chars().count();
                app.cursor_x = app.cursor_x.min(next_len);
            }
        }
        KeyCode::Home => {
            app.cursor_x = 0;
        }
        KeyCode::End => {
            app.cursor_x = app.urls[y].chars().count();
        }
        _ => {}
    }
}

fn handle_paste_urls(app: &mut App, pasted: &str) {
    if app.urls.is_empty() {
        app.urls.push(String::new());
    }
    let lines: Vec<String> = pasted.lines().map(|s| s.to_string()).collect();
    if lines.is_empty() {
        return;
    }
    
    let y = app.cursor_y;
    let x = app.cursor_x;
    
    let current_line = &mut app.urls[y];
    let char_idx = get_char_byte_index(current_line, x);
    let tail = current_line.split_off(char_idx);

    app.urls[y].push_str(&lines[0]);

    for i in 1..lines.len() {
        app.urls.insert(y + i, lines[i].clone());
    }

    let last_pasted_y = y + lines.len() - 1;
    app.urls[last_pasted_y].push_str(&tail);

    app.cursor_y = last_pasted_y;
    app.cursor_x = app.urls[last_pasted_y].chars().count().saturating_sub(tail.chars().count());
}

fn handle_paste(app: &mut App, text: &str) {
    handle_paste_urls(app, &text.replace('\r', ""));
}

fn handle_feed_selector_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
        return true;
    }
    match key.code {
        KeyCode::Up => {
            if app.feed_cursor > 0 {
                app.feed_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.feed_cursor + 1 < app.feed_articles.len() {
                app.feed_cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            if app.feed_cursor < app.feed_selected.len() {
                app.feed_selected[app.feed_cursor] = !app.feed_selected[app.feed_cursor];
            }
        }
        KeyCode::Enter => {
            let urls: Vec<String> = app.feed_articles.iter().enumerate()
                .filter(|(i, _)| app.feed_selected.get(*i).copied().unwrap_or(false))
                .map(|(_, (_, url, _, _))| url.clone())
                .collect();
            if !urls.is_empty() {
                app.urls = urls;
                app.cursor_y = 0;
                app.cursor_x = 0;
            }
            app.view = AppView::Download;
        }
        _ => {}
    }
    false
}

fn handle_key(
    app: &mut App,
    key: KeyEvent,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> bool {
    if matches!(app.view, AppView::FeedSelector) {
        return handle_feed_selector_key(app, key);
    }

    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
        return true;
    }

    // Toggle Preview Picker view with Ctrl+P
    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        match &app.view {
            AppView::Download => {
                enter_picker_view(app);
            }
            AppView::Picker { .. } => {
                app.view = AppView::Download;
            }
            AppView::FeedSelector => {}
        }
        return false;
    }

    match &mut app.view {
        AppView::Download => {
            if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
                app.logs.clear();
                app.logs.push("Logs cleared.".to_string());
                return false;
            }

            if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                if !app.is_downloading {
                    start_download(app, tx);
                }
                return false;
            }

            handle_multiline_key(app, key);
        }
        AppView::Picker {
            files,
            selected_idx,
            preview_lines,
            preview_scroll_y,
        } => {
            match key.code {
                KeyCode::Up => {
                    if !files.is_empty() && *selected_idx > 0 {
                        *selected_idx -= 1;
                        *preview_lines = load_preview_lines(&files[*selected_idx]);
                        *preview_scroll_y = 0;
                    }
                }
                KeyCode::Down => {
                    if !files.is_empty() && *selected_idx + 1 < files.len() {
                        *selected_idx += 1;
                        *preview_lines = load_preview_lines(&files[*selected_idx]);
                        *preview_scroll_y = 0;
                    }
                }
                KeyCode::PageUp | KeyCode::Char('w') | KeyCode::Char('k') => {
                    *preview_scroll_y = preview_scroll_y.saturating_sub(1);
                }
                KeyCode::PageDown | KeyCode::Char('s') | KeyCode::Char('j') => {
                    *preview_scroll_y += 1;
                }
                _ => {}
            }
        }
        AppView::FeedSelector => {}
    }
    false
}

fn enter_picker_view(app: &mut App) {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&app.output_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "md" {
                        if let Some(p) = path.to_str() {
                            files.push(p.to_string());
                        }
                    }
                }
            }
        }
    }
    files.sort();

    if files.is_empty() {
        app.logs.push(format!("Warning: No markdown files found in {}.", app.output_dir));
        return;
    }

    let preview_lines = load_preview_lines(&files[0]);
    app.view = AppView::Picker {
        files,
        selected_idx: 0,
        preview_lines,
        preview_scroll_y: 0,
    };
}

fn load_preview_lines(filename: &str) -> Vec<Line<'static>> {
    match std::fs::read_to_string(filename) {
        Ok(content) => parse_markdown_to_lines(&content),
        Err(e) => vec![Line::from(Span::styled(
            format!("Error loading file: {}", e),
            Style::default().fg(Color::Red),
        ))],
    }
}

fn create_span(text: String, bold: bool, italic: bool, code: bool) -> Span<'static> {
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

fn parse_inline_formatting(text: &str) -> Vec<Span<'static>> {
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

fn parse_markdown_to_lines(content: &str) -> Vec<Line<'static>> {
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
            let style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(text, style)));
            lines.push(Line::from(Span::styled("----------------------------------------", Style::default().fg(Color::Cyan))));
            continue;
        }
        if trimmed.starts_with("### ") {
            let text = trimmed[4..].to_string();
            let style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
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

fn start_download(app: &mut App, tx: mpsc::UnboundedSender<AppEvent>) {
    let urls: Vec<String> = app
        .urls
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if urls.is_empty() {
        app.logs.push("Error: No URLs to download!".to_string());
        return;
    }

    let sid = app.sid.trim().to_string();
    let uid = app.uid.trim().to_string();
    let cf_clearance = app.cf_clearance.trim().to_string();
    let output_dir = app.output_dir.clone();
    let force_download = app.force_download;

    if sid.is_empty() {
        app.logs.push("Warning: MEDIUM_SID is not set. Fetching public version.".to_string());
    } else {
        app.logs.push("Using provided MEDIUM_SID session cookie.".to_string());
    }

    app.is_downloading = true;
    app.logs.push(format!("Starting download of {} articles to {}...", urls.len(), output_dir));

    tokio::spawn(async move {
        if let Err(e) = tokio::fs::create_dir_all(&output_dir).await {
            let _ = tx.send(AppEvent::Log(format!("Error: Failed to create output directory {}: {}", output_dir, e)));
            let _ = tx.send(AppEvent::DownloadFinished);
            return;
        }

        let client = match reqwest::Client::builder().build() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(AppEvent::Log(format!("Error: Failed to create client: {}", e)));
                let _ = tx.send(AppEvent::DownloadFinished);
                return;
            }
        };

        for (idx, url_str) in urls.iter().enumerate() {
            let num = idx + 1;
            let total = urls.len();
            let _ = tx.send(AppEvent::Log(format!("[{}/{}] Downloading {}...", num, total, url_str)));

            match perform_download(&client, url_str, &sid, &uid, &cf_clearance, &output_dir, force_download, &tx).await {
                Ok(filename) => {
                    let _ = tx.send(AppEvent::Log(format!("[{}/{}] Success! Saved to {}", num, total, filename)));
                }
                Err(err) => {
                    let _ = tx.send(AppEvent::Log(format!("[{}/{}] Error: {}", num, total, err)));
                }
            }
        }

        let _ = tx.send(AppEvent::Log("All tasks completed.".to_string()));
        let _ = tx.send(AppEvent::DownloadFinished);
    });
}

fn clean_markdown(md_text: &str) -> String {
    let lines = md_text.lines();
    let mut cleaned_lines = Vec::new();
    let mut prev_blank = false;
    for line in lines {
        let is_blank = line.trim().is_empty();
        if is_blank {
            if !prev_blank {
                cleaned_lines.push("");
                prev_blank = true;
            }
        } else {
            cleaned_lines.push(line.trim_end());
            prev_blank = false;
        }
    }
    cleaned_lines.join("\n")
}

fn has_key_descendants(node: ego_tree::NodeRef<'_, scraper::Node>) -> bool {
    for child in node.children() {
        if let Some(el) = child.value().as_element() {
            let name = el.name.local.as_ref();
            if ["p", "h1", "h2", "h3", "h4", "h5", "h6", "img", "pre", "ul", "ol"].contains(&name) {
                return true;
            }
        }
        if has_key_descendants(child) {
            return true;
        }
    }
    false
}

fn get_text(node: ego_tree::NodeRef<'_, scraper::Node>) -> String {
    let mut text = String::new();
    for child in node.children() {
        if let scraper::Node::Text(ref t) = *child.value() {
            text.push_str(&t.text);
        } else {
            text.push_str(&get_text(child));
        }
    }
    text
}

fn clean_article(document: &mut Html) {
    let decompose_selector = Selector::parse("button, svg, style, script").unwrap();
    let ids: Vec<_> = document.select(&decompose_selector).map(|el| el.id()).collect();
    for id in ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }

    let a_selector = Selector::parse("a").unwrap();
    let a_ids: Vec<_> = document.select(&a_selector).map(|el| el.id()).collect();
    for id in a_ids {
        let mut detach = false;
        let mut new_href = None;
        let mut should_remove_href = false;

        if let Some(node) = document.tree.get(id) {
            if let Some(element) = node.value().as_element() {
                if let Some(href) = element.attr("href") {
                    let href_lower = href.to_lowercase();
                    if href_lower.contains("signin")
                        || href_lower.contains("signup")
                        || href_lower.contains("plans?dimension")
                        || href_lower.contains("upgrade")
                    {
                        detach = true;
                    } else if let Ok(mut url) = url::Url::parse(href) {
                        let cleaned_query: Vec<(String, String)> = url
                            .query_pairs()
                            .filter(|(k, _)| {
                                !k.starts_with("source") && k != "referrer" && k != "gi"
                            })
                            .map(|(k, v)| (k.into_owned(), v.into_owned()))
                            .collect();
                        url.set_query(None);
                        if !cleaned_query.is_empty() {
                            let mut query_serializer = url.query_pairs_mut();
                            for (k, v) in cleaned_query {
                                query_serializer.append_pair(&k, &v);
                            }
                        }
                        new_href = Some(url.to_string());
                    } else if href.starts_with('/') {
                        if let Ok(mut url) = url::Url::parse(&format!("https://medium.com{}", href)) {
                            let cleaned_query: Vec<(String, String)> = url
                                .query_pairs()
                                .filter(|(k, _)| {
                                    !k.starts_with("source") && k != "referrer" && k != "gi"
                                })
                                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                                .collect();
                            url.set_query(None);
                            if !cleaned_query.is_empty() {
                                let mut query_serializer = url.query_pairs_mut();
                                for (k, v) in cleaned_query {
                                    query_serializer.append_pair(&k, &v);
                                }
                            }
                            new_href = Some(url.path().to_string() + url.query().map(|q| format!("?{}", q)).as_deref().unwrap_or(""));
                        }
                    }

                    if !detach {
                        let check_href = new_href.as_deref().unwrap_or(href);
                        if check_href.is_empty()
                            || check_href == "/"
                            || check_href == "javascript:void(0)"
                            || check_href.starts_with('?')
                        {
                            should_remove_href = true;
                        }
                    }
                }
            }
        }

        if detach {
            if let Some(mut node) = document.tree.get_mut(id) {
                node.detach();
            }
            continue;
        }

        if let Some(mut node) = document.tree.get_mut(id) {
            if let scraper::Node::Element(ref mut element) = *node.value() {
                if should_remove_href {
                    let mut keys = Vec::new();
                    for k in element.attrs.keys() {
                        if k.local.as_ref() == "href" {
                            keys.push(k.clone());
                        }
                    }
                    for k in keys {
                        element.attrs.remove(&k);
                    }
                } else if let Some(href_val) = new_href {
                    for (k, v) in &mut element.attrs {
                        if k.local.as_ref() == "href" {
                            *v = href_val.clone().into();
                        }
                    }
                }
            }
        }
    }

    let img_selector = Selector::parse("img").unwrap();
    let img_ids: Vec<_> = document.select(&img_selector).map(|el| el.id()).collect();
    for id in img_ids {
        let mut detach = false;
        if let Some(node) = document.tree.get(id) {
            if let Some(element) = node.value().as_element() {
                if let Some(src) = element.attr("src") {
                    if src.contains("resize:fill:64:64")
                        || src.contains("resize:fill:32:32")
                        || src.contains("resize:fill:40:40")
                        || src.contains("resize:fill:48:48")
                    {
                        detach = true;
                    }
                }
            }
        }
        if detach {
            if let Some(mut node) = document.tree.get_mut(id) {
                node.detach();
            }
        }
    }

    let text_container_selector = Selector::parse("p, span, div, a").unwrap();
    let tc_ids: Vec<_> = document.select(&text_container_selector).map(|el| el.id()).collect();
    
    let target_texts = [
        "member-only story",
        "listen",
        "share",
        "follow",
        "mute",
        "--",
        "·",
        "read",
        "press enter or click to view image in full size",
    ];

    for id in tc_ids {
        let mut detach = false;
        if let Some(node) = document.tree.get(id) {
            let has_key_elements = if let Some(el) = node.value().as_element() {
                if el.name.local.as_ref() == "div" {
                    has_key_descendants(node)
                } else {
                    false
                }
            } else {
                false
            };

            if !has_key_elements {
                let text = get_text(node).trim().to_lowercase();
                if target_texts.contains(&text.as_str())
                    || (text.len() == 1 && (text == "·" || text == "-" || text == "—"))
                {
                    detach = true;
                } else if text.ends_with("min read") || text.contains("min read") {
                    if let Some(el) = node.value().as_element() {
                        let name = el.name.local.as_ref();
                        if name == "span" || name == "p" || name == "div" {
                            detach = true;
                        }
                    }
                }
            }
        }

        if detach {
            if let Some(mut node) = document.tree.get_mut(id) {
                node.detach();
            }
        }
    }

}

fn get_extension(url: &str) -> Option<&str> {
    let path = url.split('?').next()?;
    let segment = path.split('/').last()?;
    let ext = segment.split('.').last()?;
    if ext.len() <= 4 && !ext.is_empty() {
        Some(ext)
    } else {
        None
    }
}

fn clean_article_and_collect_images(
    document: &mut Html,
    images_dir_name: &str,
) -> Result<Vec<(String, String)>, String> {
    let picture_selector = Selector::parse("picture").unwrap();
    let source_selector = Selector::parse("source").unwrap();
    let img_selector = Selector::parse("img").unwrap();

    let pic_ids: Vec<_> = document.select(&picture_selector).map(|el| el.id()).collect();

    for pic_id in pic_ids {
        let mut image_url = None;
        if let Some(pic_node) = document.tree.get(pic_id) {
            let pic_ref = scraper::ElementRef::wrap(pic_node).unwrap();
            for src_ref in pic_ref.select(&source_selector) {
                if let Some(srcset) = src_ref.value().attr("srcset").or_else(|| src_ref.value().attr("srcSet")) {
                    if let Some(last_src) = srcset.split(',').last() {
                        let url_part = last_src.trim().split(' ').next().unwrap_or("");
                        if !url_part.is_empty() {
                            image_url = Some(url_part.to_string());
                            break;
                        }
                    }
                }
            }
        }

        if let Some(url) = image_url {
            let mut img_ids = Vec::new();
            if let Some(pic_node) = document.tree.get(pic_id) {
                let pic_ref = scraper::ElementRef::wrap(pic_node).unwrap();
                for img_ref in pic_ref.select(&img_selector) {
                    img_ids.push(img_ref.id());
                }
            }
            for img_id in img_ids {
                if let Some(mut img_node) = document.tree.get_mut(img_id) {
                    if let scraper::Node::Element(ref mut element) = *img_node.value() {
                        let src_key = QualName::new(None, Namespace::from(""), LocalName::from("src"));
                        element.attrs.insert(src_key, url.clone().into());
                    }
                }
            }
        }
    }

    let source_ids: Vec<_> = document.select(&source_selector).map(|el| el.id()).collect();
    for id in source_ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }

    clean_article(document);

    let mut image_downloads = Vec::new();
    let img_ids: Vec<_> = document.select(&img_selector).map(|el| el.id()).collect();
    let mut img_counter = 0;

    for id in img_ids {
        let mut original_src = None;
        if let Some(node) = document.tree.get(id) {
            if let Some(element) = node.value().as_element() {
                if let Some(src) = element.attr("src") {
                    if src.starts_with("http") {
                        original_src = Some(src.to_string());
                    }
                }
            }
        }

        if let Some(src) = original_src {
            img_counter += 1;
            let ext = get_extension(&src).unwrap_or("jpg");
            let local_filename = format!("img_{}.{}", img_counter, ext);
            let local_relative_path = format!("./{}/{}", images_dir_name, local_filename);

            image_downloads.push((src.clone(), local_relative_path.clone()));

            if let Some(mut node) = document.tree.get_mut(id) {
                if let scraper::Node::Element(ref mut element) = *node.value() {
                    for (k, v) in &mut element.attrs {
                        if k.local.as_ref() == "src" {
                            *v = local_relative_path.clone().into();
                        }
                    }
                }
            }
        }
    }

    Ok(image_downloads)
}

fn extract_slug(url: &str) -> String {
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

fn clean_rss_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut u) => {
            let cleaned: Vec<(String, String)> = u.query_pairs()
                .filter(|(k, _)| !k.starts_with("source") && k != "referrer" && k != "gi")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            u.set_query(None);
            if !cleaned.is_empty() {
                let mut qs = u.query_pairs_mut();
                for (k, v) in cleaned {
                    qs.append_pair(&k, &v);
                }
            }
            u.to_string()
        }
        Err(_) => url.to_string(),
    }
}

fn parse_rss_items(content: &str) -> Vec<(i64, String, String, String)> {
    match RssChannel::read_from(content.as_bytes()) {
        Ok(channel) => channel.items().iter()
            .filter_map(|item| {
                let title = item.title()?.to_string();
                let link = item.link()?.to_string();
                if link.is_empty() { return None; }
                let ts = item.pub_date()
                    .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);
                let author = item.dublin_core_ext()
                    .and_then(|dc| dc.creators().first().cloned())
                    .or_else(|| item.author().map(|s| s.to_string()))
                    .unwrap_or_default();
                Some((ts, title, link, author))
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn extract_following_from_html(
    html: &str,
) -> (Vec<String>, Vec<String>, Vec<(i64, String, String, String)>) {
    let marker = "window.__APOLLO_STATE__ = ";
    let start = match html.find(marker) {
        Some(i) => i + marker.len(),
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };
    let end = match html[start..].find("</script>") {
        Some(i) => start + i,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };
    let json_str = &html[start..end];

    let state: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Vec::new(), Vec::new()),
    };

    let mut usernames = Vec::new();
    let mut pub_slugs = Vec::new();
    let mut posts: Vec<(i64, String, String, String)> = Vec::new();

    let obj = match state.as_object() {
        Some(o) => o,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };

    // First pass: build user-key → display name map
    let mut user_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (key, value) in obj {
        if key.starts_with("User:") {
            let display = value.get("name").and_then(|v| v.as_str())
                .or_else(|| value.get("username").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            if !display.is_empty() {
                user_names.insert(key.clone(), display);
            }
            if let Some(u) = value.get("username").and_then(|v| v.as_str()) {
                if !u.is_empty() && !usernames.contains(&u.to_string()) {
                    usernames.push(u.to_string());
                }
            }
        } else if key.starts_with("Publication:") {
            if let Some(s) = value.get("slug").and_then(|v| v.as_str()) {
                if !s.is_empty() && !pub_slugs.contains(&s.to_string()) {
                    pub_slugs.push(s.to_string());
                }
            }
        }
    }

    // Second pass: extract Post objects with resolved author
    for (key, value) in obj {
        if !key.starts_with("Post:") { continue; }
        let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let url = value.get("mediumUrl").and_then(|v| v.as_str())
            .or_else(|| value.get("uniqueSlug").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        if title.is_empty() || url.is_empty() { continue; }
        let ts = value.get("firstPublishedAt")
            .or_else(|| value.get("publishedAt"))
            .or_else(|| value.get("createdAt"))
            .and_then(|v| v.as_i64())
            .map(|ms| ms / 1000)
            .unwrap_or(0);
        let author = value.get("creator")
            .and_then(|c| c.get("__ref"))
            .and_then(|r| r.as_str())
            .and_then(|r| user_names.get(r))
            .cloned()
            .unwrap_or_default();
        posts.push((ts, title, url, author));
    }

    posts.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    (usernames, pub_slugs, posts)
}

fn format_ts(ts: i64) -> String {
    if ts == 0 { return String::new(); }
    match chrono::DateTime::from_timestamp(ts, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

async fn fetch_following_feed(
    sid: &str,
    uid: &str,
    cf_clearance: &str,
) -> Result<Vec<(String, String, String, String)>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let mut page_headers = build_cookie_headers(sid, uid, cf_clearance);
    page_headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));

    // Try the actual following feed first; fall back to /me/feed if Cloudflare blocks it.
    let (html, feed_url_used) = {
        let resp = client
            .get("https://medium.com/?feed=following")
            .headers(page_headers.clone())
            .send()
            .await
            .map_err(|e| format!("Failed to fetch feed page: {}", e))?;

        if resp.status().is_success() {
            tracing::info!("Fetched /?feed=following successfully");
            let text = resp.text().await.map_err(|e| format!("Failed to read feed page: {}", e))?;
            (text, "/?feed=following")
        } else {
            tracing::warn!(status = %resp.status(), "/?feed=following blocked, falling back to /me/feed");
            let text = client
                .get("https://medium.com/me/feed")
                .headers(page_headers)
                .send()
                .await
                .map_err(|e| format!("Failed to fetch fallback feed page: {}", e))?
                .text()
                .await
                .map_err(|e| format!("Failed to read fallback feed page: {}", e))?;
            (text, "/me/feed")
        }
    };

    tracing::info!(source = feed_url_used, "Parsing Apollo state");
    let (usernames, pub_slugs, apollo_posts) = extract_following_from_html(&html);

    if usernames.is_empty() && pub_slugs.is_empty() && apollo_posts.is_empty() {
        return Err("No followed users or publications found. Check your MEDIUM_SID and MEDIUM_CF_CLEARANCE.".to_string());
    }

    tracing::info!(
        users = usernames.len(),
        publications = pub_slugs.len(),
        apollo_posts = apollo_posts.len(),
        "Fetching RSS feeds"
    );

    let mut articles: Vec<(i64, String, String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Seed with posts found directly in Apollo state (these match what the web UI shows)
    for (ts, title, url, author) in apollo_posts {
        let clean = clean_rss_url(&url);
        if seen.insert(clean.clone()) {
            articles.push((ts, title, clean, author));
        }
    }

    for username in &usernames {
        let feed_url = format!("https://medium.com/feed/@{}", username);
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/rss+xml, text/xml, */*"));
        match client.get(&feed_url).headers(h).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(text) = resp.text().await {
                    let items = parse_rss_items(&text);
                    tracing::info!(username, count = items.len(), "Fetched user RSS feed");
                    for (ts, title, url, author) in items {
                        let clean = clean_rss_url(&url);
                        if seen.insert(clean.clone()) {
                            articles.push((ts, title, clean, author));
                        }
                    }
                }
            }
            Ok(resp) => tracing::warn!(username, status = %resp.status(), "User RSS feed returned error"),
            Err(e) => tracing::error!(username, error = %e, "Failed to fetch user RSS feed"),
        }
    }

    for slug in &pub_slugs {
        let feed_url = format!("https://medium.com/feed/{}", slug);
        let mut h = build_cookie_headers(sid, uid, cf_clearance);
        h.insert(ACCEPT, HeaderValue::from_static("application/rss+xml, text/xml, */*"));
        match client.get(&feed_url).headers(h).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(text) = resp.text().await {
                    let items = parse_rss_items(&text);
                    tracing::info!(slug, count = items.len(), "Fetched publication RSS feed");
                    for (ts, title, url, author) in items {
                        let clean = clean_rss_url(&url);
                        if seen.insert(clean.clone()) {
                            articles.push((ts, title, clean, author));
                        }
                    }
                }
            }
            Ok(resp) => tracing::warn!(slug, status = %resp.status(), "Publication RSS feed returned error"),
            Err(e) => tracing::error!(slug, error = %e, "Failed to fetch publication RSS feed"),
        }
    }

    articles.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let articles: Vec<(String, String, String, String)> = articles
        .into_iter()
        .map(|(ts, t, u, a)| (t, u, format_ts(ts), a))
        .collect();

    tracing::info!(total = articles.len(), "Feed fetch complete");
    Ok(articles)
}

fn build_cookie_headers(sid: &str, uid: &str, cf_clearance: &str) -> HeaderMap {
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

async fn perform_download(
    client: &reqwest::Client,
    url_str: &str,
    sid: &str,
    uid: &str,
    cf_clearance: &str,
    output_dir: &str,
    force: bool,
    tx: &mpsc::UnboundedSender<AppEvent>,
) -> Result<String, String> {
    let slug = extract_slug(url_str);
    let filename = format!("{}/{}.md", output_dir, slug);

    if !force && tokio::fs::metadata(&filename).await.is_ok() {
        tracing::info!(url = url_str, file = %filename, "Skipping — file already exists");
        return Ok(format!("{} (skipped, already exists)", filename));
    }

    let mut headers = build_cookie_headers(sid, uid, cf_clearance);
    headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,image/apng,*/*;q=0.8"));

    tracing::info!(url = url_str, "Starting article download");

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

    let html_content = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

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

        (image_downloads, md_cleaned)
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
            for (img_url, rel_path) in image_downloads {
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


fn draw_urls_field(
    rect: Rect,
    f: &mut Frame,
    app: &mut App,
    is_active: bool,
) {
    let style = if is_active {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    let border_type = if is_active {
        BorderType::Double
    } else {
        BorderType::Plain
    };

    let block = Block::default()
        .title(" URLs to Download (One per line) ")
        .borders(Borders::ALL)
        .border_style(style)
        .border_type(border_type);

    let inner_rect = block.inner(rect);
    let height = inner_rect.height as usize;
    let width = inner_rect.width as usize;

    if app.cursor_y < app.urls_scroll_y {
        app.urls_scroll_y = app.cursor_y;
    } else if app.cursor_y >= app.urls_scroll_y + height {
        app.urls_scroll_y = app.cursor_y - height + 1;
    }

    let start_line = app.urls_scroll_y;
    let end_line = (start_line + height).min(app.urls.len());

    let mut lines = Vec::new();
    for i in start_line..end_line {
        let line = &app.urls[i];
        let char_count = line.chars().count();
        let display_line: String = if char_count > width {
            let chars: Vec<char> = line.chars().collect();
            chars[0..width].iter().collect()
        } else {
            line.clone()
        };
        lines.push(Line::from(Span::raw(display_line)));
    }

    while lines.len() < height {
        lines.push(Line::from(Span::raw("")));
    }

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, rect);

    if is_active {
        let screen_y = app.cursor_y.saturating_sub(app.urls_scroll_y);
        let screen_x = app.cursor_x.min(width);
        f.set_cursor(
            inner_rect.x + screen_x as u16,
            inner_rect.y + screen_y as u16,
        );
    }
}

fn draw_ui(f: &mut Frame, app: &mut App) {
    let size = f.size();

    if size.width < 20 || size.height < 10 {
        f.render_widget(
            Paragraph::new("Screen too small!").style(Style::default().fg(Color::Red)),
            size,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(size);

    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    let title_p = Paragraph::new(Line::from(vec![
        Span::styled(" 📚 Medium Article Markdown Downloader ", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        Span::raw(" (Rust TUI Edition)"),
    ]))
    .block(title_block)
    .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(title_p, chunks[0]);

    if matches!(app.view, AppView::FeedSelector) {
        let n_total = app.feed_articles.len();
        let n_sel = app.feed_selected.iter().filter(|&&s| s).count();
        let list_block = Block::default()
            .title(format!(" Following Feed — {} articles, {} selected ", n_total, n_sel))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Magenta));

        let inner = list_block.inner(chunks[1]);
        let height = inner.height as usize;

        if app.feed_cursor >= app.feed_scroll + height.max(1) {
            app.feed_scroll = app.feed_cursor - height + 1;
        }
        if app.feed_cursor < app.feed_scroll {
            app.feed_scroll = app.feed_cursor;
        }

        let start = app.feed_scroll;
        let end = (start + height).min(n_total);

        let items: Vec<ListItem> = app.feed_articles[start..end].iter().enumerate()
            .map(|(rel, (title, url, date, author))| {
                let abs = start + rel;
                let checked = app.feed_selected.get(abs).copied().unwrap_or(false);
                let slug = extract_slug(url);
                let already_downloaded = std::fs::metadata(
                    format!("{}/{}.md", app.output_dir, slug)
                ).is_ok();
                let prefix = if checked { "[x] " } else { "[ ] " };
                let style = if abs == app.feed_cursor {
                    Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
                } else if checked {
                    Style::default().fg(Color::Green)
                } else if already_downloaded {
                    Style::default().fg(Color::Gray).add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(Color::White)
                };
                let date_part = if date.is_empty() { String::new() } else { format!("[{}] ", date) };
                let author_part = if author.is_empty() { String::new() } else { format!(" — {}", author) };
                ListItem::new(format!("{}{}{}{}", prefix, date_part, title, author_part)).style(style)
            })
            .collect();

        let list = List::new(items).block(list_block);
        f.render_widget(list, chunks[1]);

        let footer_text = format!(
            "  [Up/Down] Navigate | [Space] Toggle | [Enter] Load {} selected into downloader | [Esc/Ctrl+C] Quit",
            n_sel
        );
        let footer_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        let footer_p = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(Color::Black).bg(Color::Magenta),
        )))
        .block(footer_block);
        f.render_widget(footer_p, chunks[2]);
        return;
    }

    match &mut app.view {
        AppView::Download => {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(65),
                    Constraint::Percentage(35),
                ])
                .split(chunks[1]);

            draw_urls_field(
                main_chunks[0],
                f,
                app,
                true,
            );

            let logs_block = Block::default()
                .title(" Console Log / Progress ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Blue));
            
            let inner_logs_rect = logs_block.inner(main_chunks[1]);
            let log_height = inner_logs_rect.height as usize;
            let start_idx = app.logs.len().saturating_sub(log_height);
            
            let log_lines: Vec<Line> = app.logs[start_idx..]
                .iter()
                .map(|log| {
                    if log.contains("Error") {
                        Line::from(Span::styled(log, Style::default().fg(Color::Red)))
                    } else if log.contains("Success") {
                        Line::from(Span::styled(log, Style::default().fg(Color::Green)))
                    } else if log.contains("Warning") {
                        Line::from(Span::styled(log, Style::default().fg(Color::Yellow)))
                    } else if log.starts_with('[') {
                        Line::from(Span::styled(log, Style::default().fg(Color::Cyan)))
                    } else {
                        Line::from(Span::raw(log))
                    }
                })
                .collect();

            let logs_p = Paragraph::new(log_lines).block(logs_block);
            f.render_widget(logs_p, main_chunks[1]);

            let status_style = Style::default().fg(Color::Black).bg(Color::Cyan);
            let footer_text = if app.is_downloading {
                "  📥 Downloading... Please wait. Ctrl+C to Force Quit."
            } else {
                "  [Tab/Shift+Tab] Move Focus | [Ctrl+S] Save & Download | [Ctrl+P] Preview Picker | [Esc/Ctrl+C] Quit"
            };
            let footer_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::DarkGray));
            let footer_p = Paragraph::new(Line::from(Span::styled(footer_text, status_style))).block(footer_block);
            f.render_widget(footer_p, chunks[2]);
        }
        AppView::Picker {
            files,
            selected_idx,
            preview_lines,
            preview_scroll_y,
        } => {
            let picker_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30),
                    Constraint::Percentage(70),
                ])
                .split(chunks[1]);

            let file_list_block = Block::default()
                .title(" Markdown Files ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));

            let list_items: Vec<ListItem> = files
                .iter()
                .enumerate()
                .map(|(idx, name)| {
                    let style = if idx == *selected_idx {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let display_name = if idx == *selected_idx {
                        format!(" > {}", name)
                    } else {
                        format!("   {}", name)
                    };
                    ListItem::new(display_name).style(style)
                })
                .collect();

            let file_list = List::new(list_items).block(file_list_block);
            f.render_widget(file_list, picker_chunks[0]);

            let preview_block = Block::default()
                .title(format!(" Preview: {} ", files.get(*selected_idx).cloned().unwrap_or_default()))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green));

            let inner_preview_rect = preview_block.inner(picker_chunks[1]);
            let preview_height = inner_preview_rect.height as usize;

            let max_scroll = preview_lines.len().saturating_sub(preview_height);
            let scroll_y = (*preview_scroll_y).min(max_scroll);

            let display_lines = if preview_lines.is_empty() {
                vec![Line::from(Span::styled("Empty file.", Style::default().fg(Color::DarkGray)))]
            } else {
                let end = (scroll_y + preview_height).min(preview_lines.len());
                preview_lines[scroll_y..end].to_vec()
            };

            let preview_p = Paragraph::new(display_lines).block(preview_block);
            f.render_widget(preview_p, picker_chunks[1]);

            let status_style = Style::default().fg(Color::Black).bg(Color::Yellow);
            let footer_text = "  [Up/Down] Select File | [W/S] or [K/J] Scroll Preview | [Ctrl+P] Back to Downloader | [Esc] Exit";
            let footer_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::DarkGray));
            let footer_p = Paragraph::new(Line::from(Span::styled(footer_text, status_style))).block(footer_block);
            f.render_widget(footer_p, chunks[2]);
        }
        AppView::FeedSelector => unreachable!(),
    }
}

async fn setup_cookies() -> (String, String, String) {
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

    // Test cookies with a real request
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("med2md — Download Medium articles as Markdown\n");
        println!("USAGE:");
        println!("  med2md                    Launch TUI downloader");
        println!("  med2md --feed             Fetch your following feed and select articles to download");
        println!("  med2md --dir <path>       Output directory for downloaded articles (default: ~/.medium)");
        println!("  med2md --force            Re-download articles even if they already exist");
        println!("  med2md --log <path>       Write JSON logs to <path> (default: medium.log)\n");
        println!("ENVIRONMENT VARIABLES:");
        println!("  MEDIUM_SID          Your Medium session cookie (required for member-only content)");
        println!("  MEDIUM_UID          Your Medium user cookie (optional)");
        println!("  MEDIUM_CF_CLEARANCE Cloudflare clearance cookie (required for --feed and most content)");
        println!("  MEDIUM_DIR          Output directory for downloaded articles (default: ~/.medium)");
        return Ok(());
    }

    let log_path = args.windows(2)
        .find(|w| w[0] == "--log")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "medium.log".to_string());

    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("Failed to open log file {}: {}", log_path, e))?;
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init()
        .ok();

    tracing::info!(log = %log_path, "med2md started");

    let (sid, uid, cf_clearance) = setup_cookies().await;

    let output_dir = args.windows(2)
        .find(|w| w[0] == "--dir")
        .map(|w| w[1].clone())
        .or_else(|| std::env::var("MEDIUM_DIR").ok())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{}/.medium", home)
        });

    let feed_mode = args.iter().any(|a| a == "--feed");

    let feed_articles = if feed_mode {
        println!("Fetching your following feed...");
        match fetch_following_feed(&sid, &uid, &cf_clearance).await {
            Ok(articles) => {
                println!("Found {} articles from followed users and publications.", articles.len());
                articles
            }
            Err(e) => {
                eprintln!("Warning: {}", e);
                Vec::<(String, String, String, String)>::new()
            }
        }
    } else {
        Vec::new()
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let force_download = args.iter().any(|a| a == "--force");

    let mut app = App::new(sid, uid, cf_clearance, output_dir);
    app.force_download = force_download;
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    if feed_mode && !feed_articles.is_empty() {
        let n = feed_articles.len();
        app.feed_articles = feed_articles;
        app.feed_selected = vec![false; n];
        app.view = AppView::FeedSelector;
    } else if feed_mode {
        app.logs.push("Warning: No articles found. Check MEDIUM_SID and MEDIUM_CF_CLEARANCE.".to_string());
    }

    loop {
        terminal.draw(|f| draw_ui(f, &mut app))?;

        while let Ok(app_event) = rx.try_recv() {
            match app_event {
                AppEvent::Log(msg) => app.logs.push(msg),
                AppEvent::DownloadFinished => {
                    app.is_downloading = false;
                    enter_picker_view(&mut app);
                }
            }
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(&mut app, key, tx.clone()) {
                        break;
                    }
                }
                Event::Paste(text) => {
                    handle_paste(&mut app, &text);
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    Ok(())
}
