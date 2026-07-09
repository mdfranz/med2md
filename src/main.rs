mod util;
mod app;
use app::{App, AppView, AppEvent};
mod cache;
mod meta;
mod html;
mod feed;
mod markdown;
mod net;
mod following;
use following::{fetch_following_feed, fetch_following_list};
mod articles;
mod auth;
use auth::{setup_cookies, check_session};
mod input;
use input::{handle_key, handle_paste, enter_picker_view};
mod ui;
use ui::draw_ui;
mod browser;
mod llm_config;
mod embed;
mod chat;
mod rag;
mod server;
mod checkmd;
use chat::ChatProvider;
use embed::EmbedProvider;

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
        println!("  med2md --dir <path>       Output directory for downloaded articles (default: ~/.local/med2md)");
        println!("  med2md --browse           Browse already-downloaded markdown files");
        println!("  med2md --serve            Launch HTTP server to browse downloaded articles in a browser");
        println!("  med2md --port <number>    Port to run the HTTP server on (default: 3000)");
        println!("  med2md --force            Re-download articles even if they already exist");
        println!("  med2md --refresh          Ignore cache and re-fetch authors/feed from Medium");
        println!("  med2md --web              Launch TUI web browser to browse and select articles");
        println!("  med2md --log <path>       Write JSON logs to <path> (default: medium.log)");
        println!("  med2md --index            Index downloaded markdown into the local RAG vector store, then exit");
        println!("  med2md --checkmd          Scan downloaded markdown for known issues and fix them in place");
        println!("  med2md --checkmd --dry-run   Report what --checkmd would fix, without writing files");
        println!("  med2md --chat-provider <p>   Chat completion provider: openai, anthropic, gemini (default: anthropic)");
        println!("  med2md --chat-model <m>      Chat completion model (default: claude-sonnet-4-5)");
        println!("  med2md --embed-provider <p>  Embedding provider: openai, gemini (default: openai)");
        println!("  med2md --embed-model <m>     Embedding model (default: text-embedding-3-small)\n");
        println!("ENVIRONMENT VARIABLES:");
        println!("  MEDIUM_SID          Your Medium session cookie (required for member-only content)");
        println!("  MEDIUM_UID          Your Medium user ID cookie (improves --authors completeness)");
        println!("  MEDIUM_USERNAME     Your Medium @username (helps --authors find /@user/following)");
        println!("  MEDIUM_CF_CLEARANCE Cloudflare clearance cookie (required for --feed and most content)");
        println!("  MEDIUM_DIR          Output directory for downloaded articles (default: ~/.local/med2md)");
        println!("  MED2MD_CHAT_PROVIDER   Chat completion provider (default: anthropic)");
        println!("  MED2MD_CHAT_MODEL      Chat completion model (default: claude-sonnet-4-5)");
        println!("  MED2MD_EMBED_PROVIDER  Embedding provider (default: openai)");
        println!("  MED2MD_EMBED_MODEL     Embedding model (default: text-embedding-3-small)");
        println!("  OPENAI_API_KEY / ANTHROPIC_API_KEY / GEMINI_API_KEY   API key for the selected provider(s)");
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
    let filter = tracing_subscriber::EnvFilter::new("info,tui_markdown=error");
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_env_filter(filter)
        .with_target(false)
        .try_init()
        .ok();

    tracing::info!(log = %log_path, "med2md started");

    let feed_mode = args.iter().any(|a| a == "--feed");
    let authors_mode = args.iter().any(|a| a == "--authors");
    let browse_mode = args.iter().any(|a| a == "--browse");
    let refresh = args.iter().any(|a| a == "--refresh");
    let web_mode = args.iter().any(|a| a == "--web");
    let index_mode = args.iter().any(|a| a == "--index");
    let serve_mode = args.iter().any(|a| a == "--serve");
    let checkmd_mode = args.iter().any(|a| a == "--checkmd");
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let chat_provider_arg = args.windows(2).find(|w| w[0] == "--chat-provider").map(|w| w[1].clone());
    let chat_model_arg = args.windows(2).find(|w| w[0] == "--chat-model").map(|w| w[1].clone());
    let embed_provider_arg = args.windows(2).find(|w| w[0] == "--embed-provider").map(|w| w[1].clone());
    let embed_model_arg = args.windows(2).find(|w| w[0] == "--embed-model").map(|w| w[1].clone());

    let output_dir_early = args.windows(2)
        .find(|w| w[0] == "--dir")
        .map(|w| w[1].clone())
        .or_else(|| std::env::var("MEDIUM_DIR").ok())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{}/.local/med2md", home)
        });

    let port = args.windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse::<u16>().ok())
        .unwrap_or(3000);

    if index_mode {
        let embed_config = match llm_config::load_embed_config(embed_provider_arg, embed_model_arg) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        };
        if let Err(e) = rag::ingest::run_index(&output_dir_early, &embed_config).await {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    if serve_mode {
        if let Err(e) = server::run_server(&output_dir_early, port).await {
            eprintln!("Server Error: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    if checkmd_mode {
        if let Err(e) = checkmd::run_checkmd(&output_dir_early, dry_run).await {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    let (sid, uid, cf_clearance) = setup_cookies().await;

    if feed_mode || authors_mode || (!browse_mode && !web_mode && args.len() == 1) {
        if let Err(e) = check_session(&sid, &uid, &cf_clearance).await {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    let output_dir = output_dir_early;

    let cache_dir = format!("{}/.cache", output_dir);

    const CACHE_TTL: u64 = 86400;

    let mut initial_feed_articles: Vec<(String, String, String, String)> = Vec::new();
    let mut initial_authors: Vec<(String, String)> = Vec::new();

    if authors_mode {
        if !refresh {
            if let Some(cached) = cache::read_authors_cache(&cache_dir, CACHE_TTL) {
                println!("Loaded {} authors from cache.", cached.len());
                initial_authors = cached;
            }
        }
        if initial_authors.is_empty() {
            println!("Fetching your following list...");
            match fetch_following_list(&sid, &uid, &cf_clearance).await {
                Ok(authors) => {
                    println!("Found {} followed authors/publications.", authors.len());
                    cache::write_authors_cache(&cache_dir, &authors);
                    initial_authors = authors;
                }
                Err(e) => eprintln!("Warning: {}", e),
            }
        }
    } else if feed_mode {
        if !refresh {
            if let Some(cached) = cache::read_feed_cache(&cache_dir, CACHE_TTL) {
                println!("Loaded {} articles from cache.", cached.len());
                initial_feed_articles = cached;
            }
        }
        if initial_feed_articles.is_empty() {
            // Use cached authors list if available; otherwise feed falls back to
            // Apollo state extraction (~10 authors from the page). Run --authors
            // first to populate the cache and get full coverage.
            if initial_authors.is_empty() {
                if let Some(cached) = cache::read_authors_cache(&cache_dir, u64::MAX) {
                    initial_authors = cached;
                }
            }
            println!("Fetching your following feed ({} authors)...", initial_authors.len());
            match fetch_following_feed(&sid, &uid, &cf_clearance, &initial_authors).await {
                Ok(articles) => {
                    println!("Found {} articles from followed users and publications.", articles.len());
                    cache::write_feed_cache(&cache_dir, &articles);
                    initial_feed_articles = articles;
                }
                Err(e) => eprintln!("Warning: {}", e),
            }
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

    match llm_config::load_chat_config(chat_provider_arg, chat_model_arg) {
        Ok(chat_config) => match ChatProvider::build(&chat_config) {
            Ok(provider) => {
                app.chat_model = chat_config.model;
                app.chat_provider = Some(std::sync::Arc::new(provider));
            }
            Err(e) => app.log(format!("Warning: Chat unavailable: {}", e)),
        },
        Err(e) => app.log(format!("Warning: Chat unavailable: {}", e)),
    }

    match llm_config::load_embed_config(embed_provider_arg, embed_model_arg) {
        Ok(embed_config) => match EmbedProvider::build(&embed_config) {
            Ok(embed_provider) => {
                if let Some(table) = rag::schema::open_existing_table(&app.output_dir).await {
                    match rig_lancedb::LanceDbVectorIndex::new(
                        table,
                        embed_provider,
                        "id",
                        rig_lancedb::SearchParams::default(),
                    )
                    .await
                    {
                        Ok(index) => app.rag_index = Some(std::sync::Arc::new(index)),
                        Err(e) => app.log(format!("Warning: Failed to open RAG index: {}", e)),
                    }
                }
            }
            Err(e) => app.log(format!("Warning: Embeddings unavailable: {}", e)),
        },
        Err(e) => app.log(format!("Warning: Embeddings unavailable: {}", e)),
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    if web_mode {
        input::enter_browser_mode(&mut app, tx.clone());
    } else if authors_mode && !initial_authors.is_empty() {
        // Always load meta cache for immediate display, even if stale
        if let Some(meta) = cache::read_meta_cache(&cache_dir, u64::MAX) {
            app.author_meta = meta;
        }

        let n = initial_authors.len();
        app.view = AppView::AuthorBrowser {
            authors: initial_authors.clone(),
            selected: vec![false; n],
            cursor: 0,
            scroll: 0,
        };

        // Always spawn enrichment; existing meta is passed so known authors are skipped
        let authors_e = initial_authors.clone();
        let tx_e = tx.clone();
        let sid_e = app.sid.clone();
        let uid_e = app.uid.clone();
        let cf_e = app.cf_clearance.clone();
        let cache_dir_e = cache_dir.clone();
        let existing_e = app.author_meta.clone();
        tokio::spawn(async move {
            meta::enrich_authors(&sid_e, &uid_e, &cf_e, &authors_e, tx_e, &cache_dir_e, &existing_e, refresh).await;
        });
    } else if authors_mode {
        app.log("Warning: No authors found. Check MEDIUM_SID and MEDIUM_CF_CLEARANCE.".to_string());
    } else if feed_mode && !initial_feed_articles.is_empty() {
        let n = initial_feed_articles.len();
        app.feed_articles = initial_feed_articles;
        app.feed_selected = vec![false; n];
        app.following_authors = initial_authors;
        app.view = AppView::FeedSelector;
    } else if feed_mode {
        app.log("Warning: No articles found. Check MEDIUM_SID and MEDIUM_CF_CLEARANCE.".to_string());
    } else if browse_mode {
        enter_picker_view(&mut app);
    }

    loop {
        terminal.draw(|f| draw_ui(f, &mut app))?;

        while let Ok(app_event) = rx.try_recv() {
            match app_event {
                AppEvent::Log(msg) => app.log(msg),
                AppEvent::AuthorEnriched(name, ts, count) => {
                    app.author_meta.insert(name, (ts, count));
                    app.enrichment_throttle = None;
                }
                AppEvent::EnrichmentThrottled(secs) => {
                    app.enrichment_throttle = Some(secs);
                }
                AppEvent::EnrichmentDone => {
                    app.enrichment_throttle = None;
                }
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
                        if let Some((authors, selected)) = app.prev_authors.take() {
                            app.view = AppView::AuthorBrowser { authors, selected, cursor: 0, scroll: 0 };
                        } else {
                            app.view = AppView::Download;
                        }
                    } else {
                        app.view = AppView::FeedSelector;
                    }
                }
                AppEvent::BrowserReady { url, links } => {
                    app.view = AppView::Browser {
                        current_url: url,
                        links,
                        selected_idx: 0,
                        scroll_offset: 0,
                        selected: std::collections::HashSet::new(),
                    };
                }
                AppEvent::BrowserLinksUpdated { url, links } => {
                    if let AppView::Browser { current_url, links: cur_links, selected_idx, scroll_offset, .. } = &mut app.view {
                        if *current_url == url {
                            *cur_links = links;
                            let max_idx = cur_links.len().saturating_sub(1);
                            *selected_idx = (*selected_idx).min(max_idx);
                            *scroll_offset = (*scroll_offset).min(max_idx);
                        }
                    }
                }
                AppEvent::ChatToken(token) => {
                    if let AppView::Chat { messages, .. } = &mut app.view {
                        match messages.last_mut() {
                            Some(app::ChatMessage { role: app::ChatRole::Assistant, content }) => {
                                content.push_str(&token);
                            }
                            _ => {
                                messages.push(app::ChatMessage { role: app::ChatRole::Assistant, content: token });
                            }
                        }
                    }
                }
                AppEvent::ChatTurnDone(history) => {
                    if let AppView::Chat { is_streaming, rag_history, .. } = &mut app.view {
                        *is_streaming = false;
                        *rag_history = history;
                    }
                }
                AppEvent::ChatError(err) => {
                    app.log(format!("Error: {}", err));
                    if let AppView::Chat { is_streaming, .. } = &mut app.view {
                        *is_streaming = false;
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
