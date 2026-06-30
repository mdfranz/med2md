use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
use crate::app::{App, AppView, AppEvent};
use crate::util::{get_char_byte_index, get_jitter_ms};
use crate::markdown::load_preview_lines;
use crate::articles::fetch_rss_for_authors;
use crate::net::perform_download;

pub fn handle_multiline_key(app: &mut App, key: KeyEvent) {
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

pub fn handle_paste_urls(app: &mut App, pasted: &str) {
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

pub fn handle_paste(app: &mut App, text: &str) {
    handle_paste_urls(app, &text.replace('\r', ""));
}

pub fn handle_feed_selector_key(app: &mut App, key: KeyEvent) -> bool {
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

pub fn handle_author_browser_key(app: &mut App, key: KeyEvent, tx: mpsc::UnboundedSender<AppEvent>) -> bool {
    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
        return true;
    }
    match key.code {
        KeyCode::Up => {
            if let AppView::AuthorBrowser { cursor, .. } = &mut app.view {
                if *cursor > 0 { *cursor -= 1; }
            }
        }
        KeyCode::Down => {
            if let AppView::AuthorBrowser { cursor, authors, .. } = &mut app.view {
                if *cursor + 1 < authors.len() { *cursor += 1; }
            }
        }
        KeyCode::Char(' ') => {
            if let AppView::AuthorBrowser { cursor, selected, .. } = &mut app.view {
                let c = *cursor;
                if c < selected.len() { selected[c] = !selected[c]; }
            }
        }
        KeyCode::Char('a') => {
            if let AppView::AuthorBrowser { selected, .. } = &mut app.view {
                let all = selected.iter().all(|&s| s);
                for s in selected.iter_mut() { *s = !all; }
            }
        }
        KeyCode::Enter => {
            let selected_authors: Vec<(String, String)> = if let AppView::AuthorBrowser { authors, selected, .. } = &app.view {
                authors.iter().zip(selected.iter())
                    .filter(|(_, sel)| **sel)
                    .map(|((kind, name), _)| (kind.clone(), name.clone()))
                    .collect()
            } else {
                return false;
            };
            if selected_authors.is_empty() { return false; }
            let sid = app.sid.clone();
            let uid = app.uid.clone();
            let cf_clearance = app.cf_clearance.clone();
            let n = selected_authors.len();
            app.view = AppView::Loading {
                message: format!("Fetching RSS feeds for {} author{}...", n, if n == 1 { "" } else { "s" }),
            };
            tokio::spawn(async move {
                match fetch_rss_for_authors(&sid, &uid, &cf_clearance, &selected_authors).await {
                    Ok(articles) => { let _ = tx.send(AppEvent::FeedReady(articles)); }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Log(format!("Error: {}", e)));
                        let _ = tx.send(AppEvent::FeedReady(Vec::new()));
                    }
                }
            });
        }
        _ => {}
    }
    false
}

pub fn handle_key(
    app: &mut App,
    key: KeyEvent,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> bool {
    if matches!(app.view, AppView::FeedSelector) {
        return handle_feed_selector_key(app, key);
    }
    if matches!(app.view, AppView::AuthorBrowser { .. }) {
        return handle_author_browser_key(app, key, tx);
    }
    if matches!(app.view, AppView::Loading { .. }) {
        if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
            return true;
        }
        return false;
    }

    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
        return true;
    }

    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        match &app.view {
            AppView::Download => {
                enter_picker_view(app);
            }
            AppView::Picker { .. } => {
                app.view = AppView::Download;
            }
            AppView::FeedSelector | AppView::AuthorBrowser { .. } | AppView::Loading { .. } => {}
        }
        return false;
    }

    match &mut app.view {
        AppView::Download => {
            if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
                app.logs.clear();
                app.log("Logs cleared.".to_string());
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
        AppView::FeedSelector | AppView::AuthorBrowser { .. } | AppView::Loading { .. } => {}
    }
    false
}

pub fn enter_picker_view(app: &mut App) {
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
        app.log(format!("Warning: No markdown files found in {}.", app.output_dir));
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

pub fn start_download(app: &mut App, tx: mpsc::UnboundedSender<AppEvent>) {
    let urls: Vec<String> = app
        .urls
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if urls.is_empty() {
        app.log("Error: No URLs to download!".to_string());
        return;
    }

    let sid = app.sid.trim().to_string();
    let uid = app.uid.trim().to_string();
    let cf_clearance = app.cf_clearance.trim().to_string();
    let output_dir = app.output_dir.clone();
    let force_download = app.force_download;

    if sid.is_empty() {
        app.log("Warning: MEDIUM_SID is not set. Fetching public version.".to_string());
    } else {
        app.log("Using provided MEDIUM_SID session cookie.".to_string());
    }

    app.is_downloading = true;
    app.log(format!("Starting download of {} articles to {}...", urls.len(), output_dir));

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
            if idx > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(get_jitter_ms(500))).await;
            }
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
