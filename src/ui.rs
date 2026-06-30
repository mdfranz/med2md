use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use crate::app::PickerPane;
use crate::app::{App, AppView};
use crate::util::extract_slug;

pub fn draw_urls_field(
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
        f.set_cursor_position((
            inner_rect.x + screen_x as u16,
            inner_rect.y + screen_y as u16,
        ));
    }
}

pub fn draw_ui(f: &mut Frame, app: &mut App) {
    let size = f.area();

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
        .border_style(Style::default().fg(Color::White));
    let title_p = Paragraph::new(Line::from(vec![
        Span::styled(" 📚 Medium Article Markdown Downloader ", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        Span::raw(" (Rust TUI Edition)"),
    ]))
    .block(title_block)
    .alignment(Alignment::Center);
    f.render_widget(title_p, chunks[0]);

    if matches!(app.view, AppView::FeedSelector) {
        let n_total = app.feed_articles.len();
        let n_sel = app.feed_selected.iter().filter(|&&s| s).count();
        let list_block = Block::default()
            .title(format!(" Following Feed — {} articles, {} selected ", n_total, n_sel))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::White));

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
                    Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)
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
            Style::default().fg(Color::Black).bg(Color::White),
        )))
        .block(footer_block);
        f.render_widget(footer_p, chunks[2]);
        return;
    }

    if let AppView::AuthorBrowser { authors, selected, cursor, scroll } = &mut app.view {
        let n_total = authors.len();
        let n_sel = selected.iter().filter(|&&s| s).count();
        let list_block = Block::default()
            .title(format!(" Followed Authors & Publications — {}, {} selected ", n_total, n_sel))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = list_block.inner(chunks[1]);
        let height = inner.height as usize;

        if *cursor >= *scroll + height.max(1) {
            *scroll = cursor.saturating_sub(height - 1);
        }
        if *cursor < *scroll {
            *scroll = *cursor;
        }

        let start = *scroll;
        let end = (start + height).min(n_total);

        let items: Vec<ListItem> = authors[start..end].iter().enumerate()
            .map(|(rel, (kind, name))| {
                let abs = start + rel;
                let checked = selected.get(abs).copied().unwrap_or(false);
                let prefix = if checked { "[x] " } else { "[ ] " };
                let label = if kind == "user" {
                    format!("@{}", name)
                } else {
                    format!("pub: {}", name)
                };
                let style = if abs == *cursor {
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else if checked {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(format!("{}{}", prefix, label)).style(style)
            })
            .collect();

        let list = List::new(items).block(list_block);
        f.render_widget(list, chunks[1]);

        let footer_text = format!(
            "  [↑↓] Navigate  [Space] Toggle  [A] Select All  [Enter] Fetch {} selected  [Esc/Ctrl+C] Quit",
            n_sel
        );
        let footer_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        let footer_p = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(Color::Black).bg(Color::Cyan),
        )))
        .block(footer_block);
        f.render_widget(footer_p, chunks[2]);
        return;
    }

    if let AppView::Loading { message } = &app.view {
        let msg = message.clone();
        let loading_block = Block::default()
            .title(" Loading ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));
        let p = Paragraph::new(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::White),
        )))
        .block(loading_block)
        .alignment(Alignment::Center);
        f.render_widget(p, chunks[1]);

        let footer_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        let footer_p = Paragraph::new(Line::from(Span::styled(
            "  Please wait...  [Esc/Ctrl+C] Quit",
            Style::default().fg(Color::Black).bg(Color::Cyan),
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
                        Line::from(Span::styled(log, Style::default().fg(Color::White)))
                    } else {
                        Line::from(Span::raw(log))
                    }
                })
                .collect();

            let logs_p = Paragraph::new(log_lines).block(logs_block);
            f.render_widget(logs_p, main_chunks[1]);

            let status_style = Style::default().fg(Color::Black).bg(Color::White);
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
            preview_content,
            preview_scroll_y,
            active_pane,
            preview_height,
        } => {
            let picker_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30),
                    Constraint::Percentage(70),
                ])
                .split(chunks[1]);

            let files_active = matches!(active_pane, PickerPane::Files);
            let file_list_block = Block::default()
                .title(" Markdown Files ")
                .borders(Borders::ALL)
                .border_type(if files_active { BorderType::Double } else { BorderType::Rounded })
                .border_style(Style::default().fg(if files_active { Color::Yellow } else { Color::DarkGray }));

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
                    let fname = std::path::Path::new(name)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(name);
                    let display_name = if idx == *selected_idx {
                        format!(" > {}", fname)
                    } else {
                        format!("   {}", fname)
                    };
                    ListItem::new(display_name).style(style)
                })
                .collect();

            let file_list = List::new(list_items).block(file_list_block);
            f.render_widget(file_list, picker_chunks[0]);

            let preview_active = matches!(active_pane, PickerPane::Preview);
            let preview_block = Block::default()
                .title(format!(" Preview: {} ", files.get(*selected_idx).cloned().unwrap_or_default()))
                .borders(Borders::ALL)
                .border_type(if preview_active { BorderType::Double } else { BorderType::Rounded })
                .border_style(Style::default().fg(if preview_active { Color::Green } else { Color::DarkGray }));

            let inner_preview_rect = preview_block.inner(picker_chunks[1]);
            *preview_height = inner_preview_rect.height as usize;

            let text = tui_markdown::from_str(preview_content);
            let max_scroll = text.lines.len().saturating_sub(*preview_height);
            *preview_scroll_y = (*preview_scroll_y).min(max_scroll);

            let preview_p = Paragraph::new(text)
                .block(preview_block)
                .wrap(Wrap { trim: false })
                .scroll((*preview_scroll_y as u16, 0));
            f.render_widget(preview_p, picker_chunks[1]);

            let status_style = Style::default().fg(Color::Black).bg(Color::Yellow);
            let footer_text = "  [Tab] Switch Pane | [↑↓/j/k] Navigate/Scroll | [PgUp/PgDn/w/s] Page Scroll | [Ctrl+P] Back | [Esc] Exit";
            let footer_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::DarkGray));
            let footer_p = Paragraph::new(Line::from(Span::styled(footer_text, status_style))).block(footer_block);
            f.render_widget(footer_p, chunks[2]);
        }
        AppView::FeedSelector => unreachable!(),
        AppView::AuthorBrowser { .. } => unreachable!(),
        AppView::Loading { .. } => unreachable!(),
    }
}
