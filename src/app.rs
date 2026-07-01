use std::collections::HashMap;

pub enum PickerPane {
    Files,
    Preview,
}

pub enum AuthorSort {
    Alpha,
    LastPost,
}

pub fn compute_display_order(
    authors: &[(String, String)],
    meta: &HashMap<String, (i64, usize)>,
    sort: &AuthorSort,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..authors.len()).collect();
    match sort {
        AuthorSort::Alpha => order.sort_by(|&a, &b| authors[a].1.cmp(&authors[b].1)),
        AuthorSort::LastPost => order.sort_by(|&a, &b| {
            let ts_a = meta.get(&authors[a].1).map(|(ts, _)| *ts).unwrap_or(0);
            let ts_b = meta.get(&authors[b].1).map(|(ts, _)| *ts).unwrap_or(0);
            ts_b.cmp(&ts_a)
        }),
    }
    order
}

pub enum AppView {
    Download,
    Picker {
        files: Vec<String>,
        selected_idx: usize,
        preview_content: String,
        preview_lines: Vec<ratatui::text::Line<'static>>,
        preview_scroll_y: usize,
        active_pane: PickerPane,
        preview_height: usize,
    },
    FeedSelector,
    AuthorBrowser {
        authors: Vec<(String, String)>,
        selected: Vec<bool>,
        cursor: usize,
        scroll: usize,
    },
    Loading { message: String },
}

pub enum AppEvent {
    Log(String),
    DownloadFinished,
    FeedReady(Vec<(String, String, String, String)>),
    AuthorEnriched(String, i64, usize),
    EnrichmentThrottled(u64),
    EnrichmentDone,
}

pub struct App {
    pub sid: String,
    pub uid: String,
    pub cf_clearance: String,
    pub urls: Vec<String>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub logs: Vec<String>,
    pub is_downloading: bool,
    pub force_download: bool,
    pub urls_scroll_y: usize,
    pub view: AppView,
    pub output_dir: String,
    pub feed_articles: Vec<(String, String, String, String)>,
    pub feed_selected: Vec<bool>,
    pub feed_cursor: usize,
    pub feed_scroll: usize,
    pub prev_authors: Option<(Vec<(String, String)>, Vec<bool>)>,
    pub following_authors: Vec<(String, String)>,
    pub author_meta: HashMap<String, (i64, usize)>,
    pub author_sort: AuthorSort,
    pub enrichment_throttle: Option<u64>,
}

impl App {
    pub fn new(sid: String, uid: String, cf_clearance: String, output_dir: String) -> Self {
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
            prev_authors: None,
            following_authors: Vec::new(),
            author_meta: HashMap::new(),
            author_sort: AuthorSort::Alpha,
            enrichment_throttle: None,
        }
    }

    pub fn log(&mut self, msg: String) {
        if msg.starts_with("Error") {
            tracing::error!("{}", msg);
        } else if msg.starts_with("Warning") {
            tracing::warn!("{}", msg);
        } else {
            tracing::info!("{}", msg);
        }
        self.logs.push(msg);
    }
}
