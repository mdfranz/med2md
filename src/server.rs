use std::collections::HashSet;
use std::path::Path;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct ArticleItem {
    slug: String,
    title: String,
    url: String,
    author: String,
    reading_time_mins: usize,
    file_size_bytes: u64,
    content_prefix: String,
}

pub async fn run_server(output_dir: &str, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("--------------------------------------------------");
    println!("med2md local reader server running!");
    println!("Open your browser and navigate to: http://localhost:{}", port);
    println!("Press Ctrl+C in this terminal to stop the server.");
    println!("--------------------------------------------------");
    
    let output_dir_str = output_dir.to_string();
    
    loop {
        let (mut socket, _) = match listener.accept().await {
            Ok(val) => val,
            Err(_) => continue,
        };
        
        let output_dir = output_dir_str.clone();
        tokio::spawn(async move {
            let mut buf = [0; 4096];
            let n = match socket.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            
            let request = String::from_utf8_lossy(&buf[..n]);
            let mut lines = request.lines();
            let req_line = match lines.next() {
                Some(line) => line,
                None => return,
            };
            
            let parts: Vec<&str> = req_line.split_whitespace().collect();
            if parts.len() < 2 { return; }
            let method = parts[0];
            let raw_path = parts[1];
            
            if method != "GET" {
                let response = "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(response.as_bytes()).await;
                return;
            }
            
            // Extract and percent decode the path
            let clean_path = raw_path.split('?').next().unwrap_or(raw_path).split('#').next().unwrap_or(raw_path);
            let decoded_path = match percent_encoding::percent_decode_str(clean_path).decode_utf8_lossy() {
                std::borrow::Cow::Borrowed(s) => s.to_string(),
                std::borrow::Cow::Owned(s) => s,
            };
            
            // 1. Static images requests: /<slug>_images/<img_file> or paths containing _images/
            if decoded_path.contains("_images/") {
                let rel_path = decoded_path.trim_start_matches('/');
                let file_path = Path::new(&output_dir).join(rel_path);
                
                // Security check: ensure path does not escape output_dir
                if !file_path.starts_with(Path::new(&output_dir)) {
                    let response = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = socket.write_all(response.as_bytes()).await;
                    return;
                }
                
                if let Ok(mut file) = tokio::fs::File::open(&file_path).await {
                    let mut contents = Vec::new();
                    if file.read_to_end(&mut contents).await.is_ok() {
                        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        let content_type = match ext.to_lowercase().as_str() {
                            "png" => "image/png",
                            "jpg" | "jpeg" => "image/jpeg",
                            "webp" => "image/webp",
                            "gif" => "image/gif",
                            "svg" => "image/svg+xml",
                            _ => "application/octet-stream",
                        };
                        let response_header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            content_type,
                            contents.len()
                        );
                        let _ = socket.write_all(response_header.as_bytes()).await;
                        let _ = socket.write_all(&contents).await;
                        return;
                    }
                }
                
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(response.as_bytes()).await;
                return;
            }
            
            // 2. Dashboard requests
            if decoded_path == "/" || decoded_path == "/index.html" {
                serve_dashboard(&mut socket, &output_dir).await;
                return;
            }
            
            // 3. Article view requests: /<slug>
            let slug = decoded_path.trim_start_matches('/');
            let md_file_path = Path::new(&output_dir).join(format!("{}.md", slug));
            
            if md_file_path.exists() && md_file_path.starts_with(Path::new(&output_dir)) {
                serve_article(&mut socket, &md_file_path, slug).await;
                return;
            }
            
            // 4. Fallback 404
            let body = "<h1>404 Not Found</h1><p>The requested page or article could not be found.</p>";
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}

async fn serve_dashboard(socket: &mut tokio::net::TcpStream, output_dir: &str) {
    let mut articles = Vec::new();
    let mut authors = HashSet::new();
    let mut total_est_read = 0;
    
    if let Ok(mut entries) = tokio::fs::read_dir(output_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if filename == "README.md" || filename == "ARCHITECTURE.md" || filename == "PROJECT.md" || filename == "PKG.md" || filename == "AGENTS.md" || filename == "CLAUDE.md" {
                    continue;
                }
                
                let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    let mut title = slug.replace('-', " ");
                    let mut source_url = String::new();
                    
                    for line in content.lines().take(5) {
                        if line.starts_with("# [") {
                            if let Some(end_title) = line.find("](") {
                                title = strip_markdown_emphasis(&line[3..end_title]);
                                source_url = line[end_title + 2 .. line.len() - 1].to_string();
                                break;
                            }
                        } else if line.starts_with("[Source](") {
                            source_url = line[9..line.len()-1].to_string();
                        }
                    }
                    
                    let author = if !source_url.is_empty() {
                        let auth = extract_author_from_url(&source_url);
                        authors.insert(auth.clone());
                        auth
                    } else {
                        "Unknown".to_string()
                    };
                    
                    let word_count = content.split_whitespace().count();
                    let read_time = (word_count / 200).max(1);
                    total_est_read += read_time;

                    let file_size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);

                    articles.push(ArticleItem {
                        slug,
                        title,
                        url: source_url,
                        author,
                        reading_time_mins: read_time,
                        file_size_bytes: file_size,
                        content_prefix: get_content_prefix(&content, 150),
                    });
                }
            }
        }
    }
    
    articles.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let cards_html: String = articles.iter().map(render_card).collect();

    let mut page = DASHBOARD_HTML_TEMPLATE.replace("{total_count}", &articles.len().to_string());
    page = page.replace("{author_count}", &authors.len().to_string());
    page = page.replace("{total_est_read}", &total_est_read.to_string());
    page = page.replace("{cards}", &cards_html);
    
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        page.len(),
        page
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

fn render_card(art: &ArticleItem) -> String {
    let size_str = format!("{:.1} KB", art.file_size_bytes as f64 / 1024.0);

    let original_link = if !art.url.is_empty() {
        format!("<a href=\"{}\" target=\"_blank\" class=\"original-link\" onclick=\"event.stopPropagation();\">Medium ↗</a>", html_escape(&art.url))
    } else {
        String::new()
    };

    format!(
        r#"<a href="/{slug}" class="article-card">
            <div class="card-content">
                <div class="card-meta">
                    <span class="author-badge">{author}</span>
                    {original_link}
                </div>
                <h3 class="card-title">{title}</h3>
                <p class="card-desc">{desc}</p>
                <div class="card-footer">
                    <span class="read-time">
                        <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <circle cx="12" cy="12" r="10"></circle>
                            <polyline points="12 6 12 12 16 14"></polyline>
                        </svg>
                        {read_time} min read ({size})
                    </span>
                    <span class="read-btn">
                        Read
                        <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <line x1="5" y1="12" x2="19" y2="12"></line>
                            <polyline points="12 5 19 12 12 19"></polyline>
                        </svg>
                    </span>
                </div>
            </div>
        </a>"#,
        slug = art.slug,
        author = html_escape(&art.author),
        original_link = original_link,
        title = html_escape(&art.title),
        desc = html_escape(&art.content_prefix),
        read_time = art.reading_time_mins,
        size = size_str
    )
}

async fn serve_article(socket: &mut tokio::net::TcpStream, path: &Path, slug: &str) {
    if let Ok(content) = tokio::fs::read_to_string(path).await {
        let mut title = slug.replace('-', " ");
        for line in content.lines().take(5) {
            if line.starts_with("# [") {
                if let Some(end_title) = line.find("](") {
                    title = strip_markdown_emphasis(&line[3..end_title]);
                    break;
                }
            }
        }
        
        let json_content = serde_json::to_string(&content).unwrap_or_else(|_| "[]".to_string());
        
        let mut page = ARTICLE_HTML_TEMPLATE.replace("{title}", &html_escape(&title));
        page = page.replace("{raw_markdown_json}", &json_content);
        
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            page.len(),
            page
        );
        let _ = socket.write_all(response.as_bytes()).await;
    }
}

fn html_escape(s: &str) -> String {
    let mut escaped = String::new();
    for c in s.chars() {
        match c {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn extract_author_from_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            if host != "medium.com" && host.ends_with(".medium.com") {
                return host.trim_end_matches(".medium.com").to_string();
            }
        }
        if parsed.path().starts_with("/@") {
            if let Some(username) = parsed.path().split('/').nth(1) {
                return username.to_string();
            }
        }
    }
    "Medium".to_string()
}

fn strip_markdown_emphasis(s: &str) -> String {
    s.replace("**", "")
        .replace('*', "")
        .replace('_', "")
        .replace('`', "")
}

fn get_content_prefix(md: &str, limit: usize) -> String {
    let link_re = regex::Regex::new(r"\[([^\[\]]*)\]\([^()]*\)").unwrap();
    let mut text = String::new();
    for line in md.lines() {
        let trimmed = line.trim();
        let is_underline = !trimmed.is_empty()
            && (trimmed.chars().all(|c| c == '=') || trimmed.chars().all(|c| c == '-'));
        if trimmed.starts_with("[Source](")
            || trimmed.starts_with("# ")
            || is_underline
            || trimmed.starts_with("<img")
            || trimmed.is_empty()
        {
            continue;
        }

        let delinked = link_re.replace_all(trimmed, "$1");
        let cleaned_line = strip_markdown_emphasis(&delinked);

        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&cleaned_line);
        
        if text.len() >= limit {
            break;
        }
    }
    
    if text.len() > limit {
        let mut truncated = text.chars().take(limit).collect::<String>();
        truncated.push_str("...");
        truncated
    } else if text.is_empty() {
        "No description available.".to_string()
    } else {
        text
    }
}

const DASHBOARD_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>med2md - Local Article Reader</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&family=Outfit:wght@400;600;700;800&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-gradient: linear-gradient(135deg, #0b0f19 0%, #111827 50%, #1e1b4b 100%);
            --text-primary: #f3f4f6;
            --text-secondary: #9ca3af;
            --accent-primary: #6366f1;
            --accent-secondary: #a855f7;
            --card-bg: rgba(31, 41, 55, 0.4);
            --card-border: rgba(255, 255, 255, 0.08);
            --card-hover-bg: rgba(31, 41, 55, 0.7);
            --card-hover-border: rgba(99, 102, 241, 0.4);
            --glass-bg: rgba(17, 24, 39, 0.7);
            --glass-border: rgba(255, 255, 255, 0.1);
        }

        [data-theme="light"] {
            --bg-gradient: linear-gradient(135deg, #f8fafc 0%, #f1f5f9 100%);
            --text-primary: #0f172a;
            --text-secondary: #475569;
            --accent-primary: #4f46e5;
            --accent-secondary: #9333ea;
            --card-bg: rgba(255, 255, 255, 0.7);
            --card-border: rgba(0, 0, 0, 0.08);
            --card-hover-bg: rgba(255, 255, 255, 0.95);
            --card-hover-border: rgba(79, 70, 229, 0.4);
            --glass-bg: rgba(255, 255, 255, 0.8);
            --glass-border: rgba(0, 0, 0, 0.1);
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            transition: background-color 0.3s ease, border-color 0.3s ease;
        }

        body {
            background: var(--bg-gradient);
            background-attachment: fixed;
            color: var(--text-primary);
            font-family: 'Inter', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            min-height: 100vh;
            padding-bottom: 4rem;
        }

        header {
            position: sticky;
            top: 0;
            z-index: 100;
            background: var(--glass-bg);
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            border-bottom: 1px solid var(--glass-border);
            padding: 1.25rem 2rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .header-left {
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }

        .logo {
            font-family: 'Outfit', sans-serif;
            font-weight: 800;
            font-size: 1.5rem;
            background: linear-gradient(90deg, var(--accent-primary), var(--accent-secondary));
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            letter-spacing: -0.025em;
        }

        .logo-dot {
            color: var(--accent-secondary);
        }

        .header-right {
            display: flex;
            align-items: center;
            gap: 1.5rem;
        }

        .search-box {
            position: relative;
            width: 300px;
        }

        .search-input {
            width: 100%;
            background: rgba(0, 0, 0, 0.2);
            border: 1px solid var(--card-border);
            border-radius: 9999px;
            padding: 0.6rem 1.2rem;
            color: var(--text-primary);
            font-family: inherit;
            font-size: 0.9rem;
            outline: none;
            transition: all 0.2s ease;
        }

        .search-input:focus {
            border-color: var(--accent-primary);
            box-shadow: 0 0 0 3px rgba(99, 102, 241, 0.2);
            background: rgba(0, 0, 0, 0.3);
        }

        .theme-toggle-btn {
            background: none;
            border: 1px solid var(--card-border);
            color: var(--text-primary);
            padding: 0.5rem;
            border-radius: 50%;
            cursor: pointer;
            display: flex;
            align-items: center;
            justify-content: center;
            width: 2.25rem;
            height: 2.25rem;
            transition: all 0.2s ease;
        }

        .theme-toggle-btn:hover {
            border-color: var(--accent-primary);
            background: rgba(255, 255, 255, 0.05);
        }

        .container {
            max-width: 1200px;
            margin: 0 auto;
            padding: 2rem;
        }

        .stats-panel {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 1.5rem;
            margin-bottom: 3rem;
        }

        .stat-card {
            background: var(--card-bg);
            border: 1px solid var(--card-border);
            border-radius: 1rem;
            padding: 1.5rem;
            text-align: center;
            backdrop-filter: blur(8px);
        }

        .stat-val {
            font-family: 'Outfit', sans-serif;
            font-weight: 700;
            font-size: 2.25rem;
            color: var(--accent-primary);
            margin-bottom: 0.25rem;
        }

        .stat-label {
            font-size: 0.85rem;
            color: var(--text-secondary);
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }

        .grid {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 2rem;
        }

        @media (max-width: 768px) {
            .grid {
                grid-template-columns: 1fr;
            }
        }

        .grid .card-content {
            padding: 2rem;
        }

        .grid .card-title {
            font-size: 1.5rem;
        }

        .grid .card-desc {
            font-size: 1rem;
            -webkit-line-clamp: 4;
        }

        .article-card {
            background: var(--card-bg);
            border: 1px solid var(--card-border);
            border-radius: 1rem;
            overflow: hidden;
            display: flex;
            flex-direction: column;
            height: 100%;
            text-decoration: none;
            color: inherit;
            transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1), border-color 0.2s ease, box-shadow 0.2s ease;
            backdrop-filter: blur(8px);
        }

        .article-card:hover {
            transform: translateY(-4px);
            border-color: var(--card-hover-border);
            background: var(--card-hover-bg);
            box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.3), 0 0 15px rgba(99, 102, 241, 0.15);
        }

        .card-content {
            padding: 1.5rem;
            display: flex;
            flex-direction: column;
            flex-grow: 1;
        }

        .card-meta {
            display: flex;
            align-items: center;
            gap: 0.75rem;
            margin-bottom: 0.75rem;
            font-size: 0.8rem;
            color: var(--text-secondary);
        }

        .author-badge {
            background: rgba(99, 102, 241, 0.1);
            color: var(--accent-primary);
            padding: 0.2rem 0.6rem;
            border-radius: 9999px;
            font-weight: 600;
            font-size: 0.75rem;
        }

        .original-link {
            color: var(--text-secondary);
            text-decoration: none;
            font-size: 0.75rem;
            margin-left: auto;
            transition: color 0.2s ease;
            font-weight: 500;
        }

        .original-link:hover {
            color: var(--accent-primary);
        }

        .card-title {
            font-family: 'Outfit', sans-serif;
            font-weight: 600;
            font-size: 1.2rem;
            line-height: 1.4;
            margin-bottom: 0.75rem;
            display: -webkit-box;
            -webkit-line-clamp: 2;
            -webkit-box-orient: vertical;
            overflow: hidden;
        }

        .card-desc {
            font-size: 0.9rem;
            color: var(--text-secondary);
            line-height: 1.5;
            margin-bottom: 1.5rem;
            display: -webkit-box;
            -webkit-line-clamp: 3;
            -webkit-box-orient: vertical;
            overflow: hidden;
            flex-grow: 1;
        }

        .card-footer {
            display: flex;
            justify-content: space-between;
            align-items: center;
            font-size: 0.8rem;
            color: var(--text-secondary);
            border-top: 1px solid var(--card-border);
            padding-top: 1rem;
            margin-top: auto;
        }

        .read-time {
            display: flex;
            align-items: center;
            gap: 0.25rem;
        }

        .read-btn {
            color: var(--accent-primary);
            font-weight: 600;
            display: flex;
            align-items: center;
            gap: 0.25rem;
        }

        .read-btn svg {
            transition: transform 0.2s ease;
        }

        .article-card:hover .read-btn svg {
            transform: translateX(3px);
        }

        .empty-state {
            grid-column: 1 / -1;
            text-align: center;
            padding: 4rem 2rem;
            background: var(--card-bg);
            border: 1px solid var(--card-border);
            border-radius: 1rem;
            display: none;
        }

        .empty-state-title {
            font-family: 'Outfit', sans-serif;
            font-size: 1.5rem;
            font-weight: 600;
            margin-bottom: 0.5rem;
        }

        .empty-state-desc {
            color: var(--text-secondary);
        }
    </style>
</head>
<body>
    <header>
        <div class="header-left">
            <span class="logo">med2md<span class="logo-dot">.</span>reader</span>
        </div>
        <div class="header-right">
            <div class="search-box">
                <input type="text" id="search-input" class="search-input" placeholder="Search articles...">
            </div>
            <button id="theme-toggle" class="theme-toggle-btn" title="Toggle theme">
                <svg id="theme-icon" xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <circle cx="12" cy="12" r="5"></circle>
                    <line x1="12" y1="1" x2="12" y2="3"></line>
                    <line x1="12" y1="21" x2="12" y2="23"></line>
                    <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"></line>
                    <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"></line>
                    <line x1="1" y1="12" x2="3" y2="12"></line>
                    <line x1="21" y1="12" x2="23" y2="12"></line>
                    <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"></line>
                    <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"></line>
                </svg>
            </button>
        </div>
    </header>
    <div class="container">
        <div class="stats-panel">
            <div class="stat-card">
                <div class="stat-val">{total_count}</div>
                <div class="stat-label">Total Articles</div>
            </div>
            <div class="stat-card">
                <div class="stat-val">{author_count}</div>
                <div class="stat-label">Authors</div>
            </div>
            <div class="stat-card">
                <div class="stat-val">{total_est_read}m</div>
                <div class="stat-label">Est. Read Time</div>
            </div>
        </div>

        <div class="grid" id="articles-grid">
            {cards}
            <div class="empty-state" id="empty-state">
                <div class="empty-state-title">No articles found</div>
                <div class="empty-state-desc">Try modifying your search query</div>
            </div>
        </div>
    </div>

    <script>
        const themeToggle = document.getElementById('theme-toggle');
        const themeIcon = document.getElementById('theme-icon');
        
        const setLightMode = () => {
            document.documentElement.setAttribute('data-theme', 'light');
            localStorage.setItem('theme', 'light');
            themeIcon.innerHTML = '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"></path>';
        };

        const setDarkMode = () => {
            document.documentElement.removeAttribute('data-theme');
            localStorage.setItem('theme', 'dark');
            themeIcon.innerHTML = '<circle cx="12" cy="12" r="5"></circle><line x1="12" y1="1" x2="12" y2="3"></line><line x1="12" y1="21" x2="12" y2="23"></line><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"></line><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"></line><line x1="1" y1="12" x2="3" y2="12"></line><line x1="21" y1="12" x2="23" y2="12"></line><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"></line><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"></line>';
        };

        themeToggle.addEventListener('click', () => {
            if (document.documentElement.getAttribute('data-theme') === 'light') {
                setDarkMode();
            } else {
                setLightMode();
            }
        });

        const storedTheme = localStorage.getItem('theme');
        if (storedTheme === 'light') {
            setLightMode();
        } else {
            setDarkMode();
        }

        const searchInput = document.getElementById('search-input');
        const emptyState = document.getElementById('empty-state');
        
        searchInput.addEventListener('input', (e) => {
            const query = e.target.value.toLowerCase().trim();
            const cards = document.querySelectorAll('.article-card');
            let visibleCount = 0;
            
            cards.forEach(card => {
                const title = card.querySelector('.card-title').textContent.toLowerCase();
                const author = card.querySelector('.author-badge').textContent.toLowerCase();
                const desc = card.querySelector('.card-desc') ? card.querySelector('.card-desc').textContent.toLowerCase() : '';
                
                if (title.includes(query) || author.includes(query) || desc.includes(query)) {
                    card.style.display = 'flex';
                    visibleCount++;
                } else {
                    card.style.display = 'none';
                }
            });
            
            if (visibleCount === 0) {
                emptyState.style.display = 'block';
            } else {
                emptyState.style.display = 'none';
            }
        });
    </script>
</body>
</html>
"#;

const ARTICLE_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - med2md Reader</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:ital,wght@0,300;0,400;0,500;0,600;0,700;1,400&family=Outfit:wght@400;600;700;800&family=Fira+Code:wght@400;500&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-gradient: linear-gradient(135deg, #0b0f19 0%, #111827 50%, #1e1b4b 100%);
            --text-primary: #f3f4f6;
            --text-secondary: #9ca3af;
            --accent-primary: #6366f1;
            --accent-secondary: #a855f7;
            --card-bg: rgba(31, 41, 55, 0.4);
            --card-border: rgba(255, 255, 255, 0.08);
            --glass-bg: rgba(17, 24, 39, 0.7);
            --glass-border: rgba(255, 255, 255, 0.1);
            --article-bg: rgba(17, 24, 39, 0.5);
        }

        [data-theme="light"] {
            --bg-gradient: linear-gradient(135deg, #f8fafc 0%, #f1f5f9 100%);
            --text-primary: #0f172a;
            --text-secondary: #475569;
            --accent-primary: #4f46e5;
            --accent-secondary: #9333ea;
            --card-bg: rgba(255, 255, 255, 0.7);
            --card-border: rgba(0, 0, 0, 0.08);
            --glass-bg: rgba(255, 255, 255, 0.8);
            --glass-border: rgba(0, 0, 0, 0.1);
            --article-bg: #ffffff;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }

        body {
            background: var(--bg-gradient);
            background-attachment: fixed;
            color: var(--text-primary);
            font-family: 'Inter', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            min-height: 100vh;
            padding-bottom: 6rem;
            line-height: 1.7;
        }

        header {
            position: sticky;
            top: 0;
            z-index: 100;
            background: var(--glass-bg);
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            border-bottom: 1px solid var(--glass-border);
            padding: 1rem 2rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .back-btn {
            display: inline-flex;
            align-items: center;
            gap: 0.5rem;
            color: var(--text-primary);
            text-decoration: none;
            font-weight: 500;
            font-size: 0.95rem;
            border: 1px solid var(--card-border);
            background: var(--card-bg);
            padding: 0.5rem 1rem;
            border-radius: 9999px;
            transition: all 0.2s ease;
        }

        .back-btn:hover {
            border-color: var(--accent-primary);
            background: rgba(99, 102, 241, 0.1);
            transform: translateX(-2px);
        }

        .theme-toggle-btn {
            background: none;
            border: 1px solid var(--card-border);
            color: var(--text-primary);
            padding: 0.5rem;
            border-radius: 50%;
            cursor: pointer;
            display: flex;
            align-items: center;
            justify-content: center;
            width: 2.25rem;
            height: 2.25rem;
            transition: all 0.2s ease;
        }

        .theme-toggle-btn:hover {
            border-color: var(--accent-primary);
            background: rgba(255, 255, 255, 0.05);
        }

        .container {
            max-width: 1100px;
            margin: 3rem auto;
            padding: 0 1.5rem;
        }

        article {
            background: var(--article-bg);
            border: 1px solid var(--card-border);
            border-radius: 1.5rem;
            padding: 3rem;
            box-shadow: 0 10px 30px rgba(0, 0, 0, 0.15);
            backdrop-filter: blur(8px);
        }

        @media (max-width: 768px) {
            article {
                padding: 1.5rem;
                border-radius: 1rem;
            }
        }

        .markdown-body h1, .markdown-body h2, .markdown-body h3, .markdown-body h4 {
            font-family: 'Outfit', sans-serif;
            font-weight: 700;
            color: var(--text-primary);
            margin-top: 2rem;
            margin-bottom: 1rem;
            line-height: 1.25;
        }

        .markdown-body h1 {
            font-size: 2.25rem;
            border-bottom: 1px solid var(--card-border);
            padding-bottom: 0.75rem;
            margin-top: 0;
        }

        .markdown-body h1 a {
            color: inherit;
            text-decoration: none;
        }

        .markdown-body h1 a:hover {
            text-decoration: underline;
            color: var(--accent-primary);
        }

        .markdown-body h2 {
            font-size: 1.75rem;
            border-bottom: 1px solid var(--card-border);
            padding-bottom: 0.5rem;
        }

        .markdown-body h3 { font-size: 1.4rem; }
        .markdown-body h4 { font-size: 1.15rem; }

        .markdown-body p {
            margin-bottom: 1.5rem;
            font-size: 1.05rem;
            letter-spacing: -0.011em;
        }

        .markdown-body a {
            color: var(--accent-primary);
            text-decoration: none;
            font-weight: 500;
            border-bottom: 1px solid transparent;
            transition: border-color 0.2s ease;
        }

        .markdown-body a:hover {
            border-color: var(--accent-primary);
        }

        .markdown-body ul, .markdown-body ol {
            margin-bottom: 1.5rem;
            padding-left: 2rem;
        }

        .markdown-body li {
            margin-bottom: 0.5rem;
            font-size: 1.05rem;
        }

        .markdown-body blockquote {
            border-left: 4px solid var(--accent-primary);
            padding: 0.5rem 0 0.5rem 1.5rem;
            margin: 1.5rem 0;
            color: var(--text-secondary);
            font-style: italic;
            background: rgba(99, 102, 241, 0.03);
            border-radius: 0 0.5rem 0.5rem 0;
        }

        .markdown-body pre {
            background: #1e1e3f !important;
            border-radius: 0.75rem;
            padding: 1.25rem;
            overflow-x: auto;
            margin: 1.5rem 0;
            border: 1px solid rgba(255, 255, 255, 0.05);
        }

        .markdown-body code {
            font-family: 'Fira Code', monospace;
            font-size: 0.9rem;
            background: rgba(99, 102, 241, 0.1);
            color: var(--accent-secondary);
            padding: 0.2rem 0.4rem;
            border-radius: 0.25rem;
        }

        .markdown-body pre code {
            background: none;
            color: #f8f8f2;
            padding: 0;
            border-radius: 0;
            font-size: 0.85rem;
        }

        .markdown-body img {
            max-width: 100%;
            height: auto;
            border-radius: 0.75rem;
            margin: 1.5rem 0;
            display: block;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
        }

        .markdown-body table {
            width: 100%;
            border-collapse: collapse;
            margin: 1.5rem 0;
            font-size: 0.95rem;
        }

        .markdown-body th, .markdown-body td {
            border: 1px solid var(--card-border);
            padding: 0.75rem;
            text-align: left;
        }

        .markdown-body th {
            background: rgba(99, 102, 241, 0.05);
            font-weight: 600;
        }

        .markdown-body tr:nth-child(even) {
            background: rgba(255, 255, 255, 0.01);
        }
    </style>
    <script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"></script>
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/themes/prism-tomorrow.min.css">
    <script src="https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/prism.min.js"></script>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/components/prism-rust.min.js"></script>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/components/prism-javascript.min.js"></script>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/components/prism-bash.min.js"></script>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/components/prism-json.min.js"></script>
</head>
<body>
    <header>
        <a href="/" class="back-btn">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                <line x1="19" y1="12" x2="5" y2="12"></line>
                <polyline points="12 19 5 12 12 5"></polyline>
            </svg>
            Back to Articles
        </a>
        <button id="theme-toggle" class="theme-toggle-btn" title="Toggle theme">
            <svg id="theme-icon" xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="5"></circle>
                <line x1="12" y1="1" x2="12" y2="3"></line>
                <line x1="12" y1="21" x2="12" y2="23"></line>
                <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"></line>
                <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"></line>
                <line x1="1" y1="12" x2="3" y2="12"></line>
                <line x1="21" y1="12" x2="23" y2="12"></line>
                <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"></line>
                <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"></line>
            </svg>
        </button>
    </header>
    <div class="container">
        <article class="markdown-body" id="article-body">
        </article>
    </div>

    <script>
        const rawMarkdown = {raw_markdown_json};
        
        document.getElementById('article-body').innerHTML = marked.parse(rawMarkdown);
        Prism.highlightAll();

        const themeToggle = document.getElementById('theme-toggle');
        const themeIcon = document.getElementById('theme-icon');
        
        const setLightMode = () => {
            document.documentElement.setAttribute('data-theme', 'light');
            localStorage.setItem('theme', 'light');
            themeIcon.innerHTML = '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"></path>';
        };

        const setDarkMode = () => {
            document.documentElement.removeAttribute('data-theme');
            localStorage.setItem('theme', 'dark');
            themeIcon.innerHTML = '<circle cx="12" cy="12" r="5"></circle><line x1="12" y1="1" x2="12" y2="3"></line><line x1="12" y1="21" x2="12" y2="23"></line><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"></line><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"></line><line x1="1" y1="12" x2="3" y2="12"></line><line x1="21" y1="12" x2="23" y2="12"></line><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"></line><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"></line>';
        };

        themeToggle.addEventListener('click', () => {
            if (document.documentElement.getAttribute('data-theme') === 'light') {
                setDarkMode();
            } else {
                setLightMode();
            }
        });

        const storedTheme = localStorage.getItem('theme');
        if (storedTheme === 'light') {
            setLightMode();
        } else {
            setDarkMode();
        }
    </script>
</body>
</html>
"#;
