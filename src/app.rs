use ratatui::text::Line;

pub enum AppView {
    Download,
    Picker {
        files: Vec<String>,
        selected_idx: usize,
        preview_lines: Vec<Line<'static>>,
        preview_scroll_y: usize,
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
