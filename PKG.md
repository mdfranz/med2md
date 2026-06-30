# External Dependencies — med2md

This document summarizes, classifies, and describes the external libraries (crates) used in the `med2md` project, as defined in [Cargo.toml](Cargo.toml).

---

## 1. Dependency Classification

The dependencies can be grouped into the following functional categories:

| Category | Crate Name | Description |
| :--- | :--- | :--- |
| **Async Runtime** | `tokio` | Async runtime for non-blocking I/O execution. |
| **Network & Protocol** | `reqwest`, `rss` | HTTP client execution and RSS feed extraction. |
| **Terminal UI (TUI)** | `ratatui`, `crossterm` | Terminal drawing and event loop management. |
| **HTML Parsing & DOM** | `scraper`, `ego-tree`, `markup5ever` | Scraping Web DOM components and querying selectors. |
| **Markup Conversion** | `html2md` | HTML-to-Markdown translation engine. |
| **Data Serialization** | `serde_json`, `url` | JSON payload analysis and URL query cleaning. |
| **Utilities & Logging** | `chrono`, `rpassword`, `tracing`, `tracing-subscriber` | Time formatting, hidden input, and JSON structured logs. |

---

## 2. Library Details & Usage in Code

### A. Async Runtime & Core Utilities

#### `tokio` (v1)
*   **Purpose**: The central asynchronous runtime.
*   **Usage**: Spawns concurrent downloader tasks via `tokio::spawn` in `start_download` (defined in [src/main.rs#L594](src/main.rs#L594)), implements jitter sleep delays with `tokio::time::sleep`, and performs non-blocking disk writes via `tokio::fs::write`.

#### `chrono` (v0.4)
*   **Purpose**: Date and time parsing and formatting.
*   **Usage**: Formats epoch timestamps into human-readable date strings for feed indexing using `chrono::DateTime`.

---

### B. Network & Protocol Layer

#### `reqwest` (v0.12)
*   **Purpose**: Async HTTP client for sending network requests.
*   **Usage**: Executed with `cookies` enabled to handle session validation. Downloads raw HTML text of articles and downloads images from CDNs sequentially.

#### `rss` (v2)
*   **Purpose**: RSS feed parser.
*   **Usage**: Parses feed strings fetched from author pages or publication RSS endpoints (`/feed/@username`) into structured Rust channels.

---

### C. Terminal Graphics (TUI)

#### `ratatui` (v0.26)
*   **Purpose**: Frame drawing and UI layout library for terminal interfaces.
*   **Usage**: Controls state layout, handles styling of files and selected authors in `draw_ui` (defined in [src/main.rs#L1880](src/main.rs#L1880)), renders active logs, and manages cursor indicators.

#### `crossterm` (v0.27)
*   **Purpose**: Terminal input/event polling and alternate screen manipulation.
*   **Usage**: Handles terminal raw mode initialization, bracketed paste triggers, and translates keystrokes into control events parsed in `handle_key` (defined in [src/main.rs#L326](src/main.rs#L326)).

---

### D. HTML DOM Cleaning & Scraping

#### `scraper` (v0.19)
*   **Purpose**: HTML parsing and CSS selector engine based on `html5ever`.
*   **Usage**: Parses article HTML into a tree structure. Queries selectors (like `article`, `p`, `span`, `img`, `picture`) to process elements.

#### `ego-tree` (v0.6)
*   **Purpose**: The underlying tree data structure utilized by the `scraper` crate.
*   **Usage**: Used in DOM pruning actions (such as `node.detach()` inside `clean_article` defined in [src/main.rs#L707](src/main.rs#L707)) to edit elements from the HTML DOM recursively.

#### `markup5ever` (v0.12)
*   **Purpose**: Common XML/HTML definitions and types used by HTML parsers.
*   **Usage**: Provides namespace and local name structures (`QualName`, `Namespace`, `LocalName`) needed to manipulate DOM attributes dynamically.

---

### E. Text & Markup Transformation

#### `html2md` (v0.2)
*   **Purpose**: Straightforward HTML to Markdown converter.
*   **Usage**: Converts cleaned HTML DOM fragments (often targeted from the `<article>` tag) directly into standard Markdown text strings.

#### `url` (v2.5)
*   **Purpose**: URL parsing, serialization, and query manipulation.
*   **Usage**: Used in `clean_article` to strip Medium tracking query parameters (like `source`, `referrer`, and `gi`) from links inside the DOM.

#### `serde_json` (v1)
*   **Purpose**: JSON parsing and serialization.
*   **Usage**: Used to strip XSSI security prefixes from Medium's API JSON payloads and extract GraphQL node states.

---

### F. Observability & Security

#### `tracing` (v0.1) & `tracing-subscriber` (v0.3)
*   **Purpose**: Diagnostics, instrumentation, and structured event logging.
*   **Usage**: Writes application actions, networking statuses, and API warnings to `medium.log` in JSON format.

#### `rpassword` (v7)
*   **Purpose**: Prompts the terminal user for sensitive credentials.
*   **Usage**: Interactively requests the session cookies `MEDIUM_SID` and `MEDIUM_CF_CLEARANCE` securely during setup if they are not pre-configured as environment variables.
