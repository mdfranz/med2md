mod util;
mod app;
use app::{App, AppView, AppEvent};
mod html;
mod feed;
mod markdown;
mod net;
mod following;
use following::{fetch_following_feed, fetch_following_list};
mod articles;
mod auth;
use auth::setup_cookies;
mod input;
use input::{handle_key, handle_paste, enter_picker_view};
mod ui;
use ui::draw_ui;

use std::io;
use std::time::Duration;
use ratatui::{backend::CrosstermBackend, Terminal};
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tokio::sync::mpsc;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("med2md — Download Medium articles as Markdown\n");
        println!("USAGE:");
        println!("  med2md                    Launch TUI downloader");
        println!("  med2md --feed             Fetch your following feed and select articles to download");
        println!("  med2md --authors          Browse followed authors, select, then fetch their articles");
        println!("  med2md --dir <path>       Output directory for downloaded articles (default: ~/.medium)");
        println!("  med2md --force            Re-download articles even if they already exist");
        println!("  med2md --log <path>       Write JSON logs to <path> (default: medium.log)\n");
        println!("ENVIRONMENT VARIABLES:");
        println!("  MEDIUM_SID          Your Medium session cookie (required for member-only content)");
        println!("  MEDIUM_UID          Your Medium user ID cookie (improves --authors completeness)");
        println!("  MEDIUM_USERNAME     Your Medium @username (helps --authors find /@user/following)");
        println!("  MEDIUM_CF_CLEARANCE Cloudflare clearance cookie (required for --feed and most content)");
        println!("  MEDIUM_DIR          Output directory for downloaded articles (default: ~/.medium)");
        return Ok(());
    }

    let log_path = args.windows(2)
        .find(|w| w[0] == "--log")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "medium.log".to_string());

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("Failed to open/create log file {}: {}", log_path, e))?;
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
    let authors_mode = args.iter().any(|a| a == "--authors");

    let mut initial_feed_articles: Vec<(String, String, String, String)> = Vec::new();
    let mut initial_authors: Vec<(String, String)> = Vec::new();

    if authors_mode {
        println!("Fetching your following list...");
        match fetch_following_list(&sid, &uid, &cf_clearance).await {
            Ok(authors) => {
                println!("Found {} followed authors/publications.", authors.len());
                initial_authors = authors;
            }
            Err(e) => eprintln!("Warning: {}", e),
        }
    } else if feed_mode {
        println!("Fetching your following feed...");
        match fetch_following_feed(&sid, &uid, &cf_clearance).await {
            Ok(articles) => {
                println!("Found {} articles from followed users and publications.", articles.len());
                initial_feed_articles = articles;
            }
            Err(e) => eprintln!("Warning: {}", e),
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let force_download = args.iter().any(|a| a == "--force");

    let mut app = App::new(sid, uid, cf_clearance, output_dir);
    app.force_download = force_download;
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    if authors_mode && !initial_authors.is_empty() {
        let n = initial_authors.len();
        app.view = AppView::AuthorBrowser {
            authors: initial_authors,
            selected: vec![false; n],
            cursor: 0,
            scroll: 0,
        };
    } else if authors_mode {
        app.log("Warning: No authors found. Check MEDIUM_SID and MEDIUM_CF_CLEARANCE.".to_string());
    } else if feed_mode && !initial_feed_articles.is_empty() {
        let n = initial_feed_articles.len();
        app.feed_articles = initial_feed_articles;
        app.feed_selected = vec![false; n];
        app.view = AppView::FeedSelector;
    } else if feed_mode {
        app.log("Warning: No articles found. Check MEDIUM_SID and MEDIUM_CF_CLEARANCE.".to_string());
    }

    loop {
        terminal.draw(|f| draw_ui(f, &mut app))?;

        while let Ok(app_event) = rx.try_recv() {
            match app_event {
                AppEvent::Log(msg) => app.log(msg),
                AppEvent::DownloadFinished => {
                    app.is_downloading = false;
                    enter_picker_view(&mut app);
                }
                AppEvent::FeedReady(articles) => {
                    let n = articles.len();
                    app.feed_articles = articles;
                    app.feed_selected = vec![false; n];
                    app.feed_cursor = 0;
                    app.feed_scroll = 0;
                    if n == 0 {
                        app.log("Warning: No articles found for selected authors.".to_string());
                        app.view = AppView::Download;
                    } else {
                        app.view = AppView::FeedSelector;
                    }
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
