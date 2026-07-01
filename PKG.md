# External Dependencies — med2md

This document summarizes, classifies, and describes the external libraries (crates) used in the `med2md` project, as defined in [Cargo.toml](Cargo.toml).

---

## 1. Dependency Classification

The dependencies can be grouped into the following functional categories:

| Category | Crate Name | Description |
| :--- | :--- | :--- |
| **Async Runtime** | `tokio` | Async runtime for non-blocking I/O execution. |
| **Network & Protocol** | `reqwest`, `rss` | HTTP client execution and RSS feed extraction. |
| **Terminal UI (TUI)** | `ratatui`, `crossterm`, `tui-markdown` | Terminal drawing, event loop management, and formatted preview rendering. |
| **HTML Parsing & DOM** | `scraper`, `ego-tree`, `markup5ever` | Scraping Web DOM components and querying selectors. |
| **Markup Conversion** | `html2md` | HTML-to-Markdown translation engine. |
| **Data Serialization** | `serde_json`, `url` | JSON payload analysis and URL query cleaning. |
| **Utilities & Logging** | `chrono`, `rpassword`, `tracing`, `tracing-subscriber` | Time formatting, hidden input, and JSON structured logs. |

---

## 2. Library Details & Usage in Code

### A. Async Runtime & Core Utilities

#### `tokio` (v1)
*   **Purpose**: The central asynchronous runtime.
*   **Usage**: Spawns concurrent downloader tasks via `tokio::spawn` in `start_download` (defined in [src/input.rs#L423](src/input.rs#L423)), implements jitter sleep delays with `tokio::time::sleep` (defined in [src/util.rs](src/util.rs)), and performs non-blocking disk writes via `tokio::fs::write` (defined in [src/net.rs#L99](src/net.rs#L99)).

#### `chrono` (v0.4)
*   **Purpose**: Date and time parsing and formatting.
*   **Usage**: Formats epoch timestamps into human-readable date strings for feed indexing using `chrono::DateTime` (defined in [src/util.rs#L34](src/util.rs#L34)) and parses RSS publication dates (defined in [src/feed.rs#L31](src/feed.rs#L31)).

---

### B. Network & Protocol Layer

#### `reqwest` (v0.12)
*   **Purpose**: Async HTTP client for sending network requests.
*   **Usage**: Executed with `cookies` enabled to handle session validation. Downloads raw HTML text of articles in `perform_download` (defined in [src/net.rs#L33](src/net.rs#L33)), queries Medium JSON APIs in [src/articles.rs](src/articles.rs) and [src/following.rs](src/following.rs), and downloads images from CDNs sequentially.

#### `rss` (v2)
*   **Purpose**: RSS feed parser.
*   **Usage**: Parses feed strings fetched from author pages or publication RSS endpoints (`/feed/@username`) into structured Rust channels (defined in [src/feed.rs#L1](src/feed.rs#L1)).

---

### C. Terminal Graphics (TUI)

#### `ratatui` (v0.30)
*   **Purpose**: Frame drawing and UI layout library for terminal interfaces.
*   **Usage**: Controls state layout, handles styling of files and selected authors in `draw_ui` (defined in [src/ui.rs#L78](src/ui.rs#L78)), renders active logs, and manages cursor indicators.

#### `crossterm` (v0.28)
*   **Purpose**: Terminal input/event polling and alternate screen manipulation.
*   **Usage**: Handles terminal raw mode initialization, bracketed paste triggers in [src/main.rs](src/main.rs), and translates keystrokes into control events parsed in `handle_key` (defined in [src/input.rs#L277](src/input.rs#L277)).

#### `tui-markdown` (v0.3)
*   **Purpose**: Formatted Markdown renderer for Ratatui.
*   **Usage**: Swaps raw text viewing of markdown files in the preview pane for a formatted rendered layout using `tui_markdown::from_str` (defined in [src/markdown.rs#L8](src/markdown.rs#L8)).

---

### D. HTML DOM Cleaning & Scraping

#### `scraper` (v0.19)
*   **Purpose**: HTML parsing and CSS selector engine based on `html5ever`.
*   **Usage**: Parses article HTML into a tree structure. Queries selectors (like `article`, `p`, `span`, `img`, `picture`) to process elements in [src/html.rs](src/html.rs).

#### `ego-tree` (v0.6)
*   **Purpose**: The underlying tree data structure utilized by the `scraper` crate.
*   **Usage**: Used in DOM pruning actions (such as `node.detach()` inside `clean_article` defined in [src/html.rs#L70](src/html.rs#L70)) to edit elements from the HTML DOM recursively.

#### `markup5ever` (v0.12)
*   **Purpose**: Common XML/HTML definitions and types used by HTML parsers.
*   **Usage**: Provides namespace and local name structures (`QualName`, `Namespace`, `LocalName`) needed to manipulate DOM attributes dynamically in [src/html.rs](src/html.rs).

---

### E. Text & Markup Transformation

#### `html2md` (v0.2)
*   **Purpose**: Straightforward HTML to Markdown converter.
*   **Usage**: Converts cleaned HTML DOM fragments (often targeted from the `<article>` tag) directly into standard Markdown text strings in `perform_download` (defined in [src/net.rs#L92](src/net.rs#L92)).

#### `url` (v2.5)
*   **Purpose**: URL parsing, serialization, and query manipulation.
*   **Usage**: Used in `clean_article` (defined in [src/html.rs#L70](src/html.rs#L70)) and `fetch_following_feed` (defined in [src/following.rs#L41](src/following.rs#L41)) to parse feed URLs and strip Medium tracking query parameters (like `source`, `referrer`, and `gi`) from links inside the DOM.

#### `serde_json` (v1)
*   **Purpose**: JSON parsing and serialization.
*   **Usage**: Used to strip XSSI security prefixes from Medium's API JSON payloads, extract GraphQL Apollo state nodes in [src/feed.rs](src/feed.rs), and serialize/deserialize cache data (defined in [src/cache.rs](src/cache.rs)).

---

### F. Observability & Security

#### `tracing` (v0.1) & `tracing-subscriber` (v0.3)
*   **Purpose**: Diagnostics, instrumentation, and structured event logging.
*   **Usage**: Writes application actions, networking statuses, API warnings, and background worker progress to `medium.log` in JSON format. Enabled and configured with EnvFilter filtering in [src/main.rs](src/main.rs).

#### `rpassword` (v7)
*   **Purpose**: Prompts the terminal user for sensitive credentials.
*   **Usage**: Interactively requests the session cookies `MEDIUM_SID` and `MEDIUM_CF_CLEARANCE` securely during setup in `setup_cookies` (defined in [src/auth.rs#L38](src/auth.rs#L38)) if they are not pre-configured as environment variables.
