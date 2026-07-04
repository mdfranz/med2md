# Project History and Feature Evolution — med2md

`med2md` is a Terminal User Interface (TUI) downloader written in asynchronous Rust. It fetches Medium articles (both public and premium/paywalled via cookies), cleans up tracking parameters and DOM clutter, extracts and downloads high-quality embedded images locally, and converts the content into clean, offline-ready Markdown.

This document traces the chronological evolution of the project's features, codebase structure, and files to serve as a history log and developmental reference.

---

## 1. Project Evolution Timeline

The project has evolved through several key phases recorded in its git history:

### Phase 1: Inception & Core Download Pipeline
*   **Git Commits**: `e9fc393` (Initial commit), `20b5de7` (Initialize med2md), `cfa6d2e` (removed junk), `2c25096` (Update docs & screenshots)
*   **Key Features**:
    *   Basic asynchronous scraping of Medium article HTML using `reqwest` and `scraper`.
    *   Conversion of target article elements to Markdown text using `html2md`.
    *   Local caching and renaming of embedded image assets from Miro/Medium CDN links.
    *   Basic single-view Terminal User Interface (TUI) utilizing `ratatui` and `crossterm`.
    *   Clipboard integration for URL ingestion.
*   **Key Files Added**:
    *   [src/main.rs](src/main.rs): The entire project codebase was initially housed here (containing downloader tasks, UI renderer, event handlers, HTML DOM cleaner, etc.).
    *   [Cargo.toml](Cargo.toml): Declared initial dependencies (`tokio`, `reqwest`, `scraper`, `html2md`, `ratatui`, `crossterm`).
    *   [PKG.md](PKG.md): Dependency tracking document cataloging the purpose of each external crate.
    *   Visual assets: `link-selector.png` and `markdown-viewer.png` demonstrating UI functionality.
*   **Key Files Removed**: `examples/test_scraper.rs`, a scratch scraping script committed during initial setup, deleted a day later once its logic had been folded into `src/main.rs`.

### Phase 2: Feed Selection & Interactive Credentials
*   **Git Commits**: `5c362cd` (Add CLI args, feed selector & interactive cookie setup; remove redundant docs), `bdee939` (Fix color scheme)
*   **Key Features**:
    *   **CLI Arguments**: Introduced arguments (`--feed`, `--authors`, `--dir`, `--browse`, etc.) to control application startup views and output directory parameters.
    *   **Interactive Cookie Setup**: Enabled TUI execution to interactively query user cookies (`MEDIUM_SID`, `MEDIUM_UID`, `MEDIUM_CF_CLEARANCE`) using `rpassword` if environment variables are not pre-configured.
    *   **Feed Selector View**: Integrated a checklist selector listing articles inside target feeds for bulk choice.
    *   **Color Scheme Refinement**: Updated TUI rendering panels with enhanced block styling and theme borders.
*   **Key Files Removed**: `README.md` and `PKG.md` were deleted in `5c362cd` as "redundant docs" during the CLI-args refactor. Both were recreated shortly after — `PKG.md` in Phase 3 (`96a22f5`) and `README.md` in Phase 7 (`0bf2f4c`) — once the codebase had stabilized enough to document again.

### Phase 3: Architectural Documentation & Path Standards
*   **Git Commits**: `d0fc86f` (Add architecture docs & update TUI), `96a22f5` (Update doc references)
*   **Key Features**:
    *   Created [ARCHITECTURE.md](ARCHITECTURE.md) outlining the high-level layers (UI, State, Async Workers, Network, DOM Cleaning, and Disk Storage).
    *   Updated TUI views to handle feed metadata transitions and active load screen states.
    *   Standardized all documentation references to use strictly relative links (`[link](path)`) and comply with [AGENTS.md](AGENTS.md) rules.
*   **Key Files Added**:
    *   [ARCHITECTURE.md](ARCHITECTURE.md): High-level block and flow diagrams mapping parsing tasks.
    *   [MEDIUM.md](MEDIUM.md): Detailing the network endpoints, Apollo GraphQL state harvesting, and XSSI security bypass mechanisms.

### Phase 4: Modular Codebase Refactoring
*   **Git Commits**: `45c2106` (Refactor codebase: modularize main.rs)
*   **Key Features**:
    *   Divided the massive `src/main.rs` file (which grew to over 2000 lines) into discrete Rust modules.
    *   Decoupled TUI rendering from keyboard input controllers, and separated network fetching logic from HTML processing tasks.
*   **Key Files Created (Sub-modules under `src/`)**:
    *   [src/app.rs](src/app.rs): Manages the state machine, view state variants, and UI pane trackers.
    *   [src/articles.rs](src/articles.rs): Queries user-specific articles via the Medium JSON API and handles async RSS crawler mappings for followed authors.
    *   [src/auth.rs](src/auth.rs): Handles interactive credential loading and cookie tests.
    *   [src/cache.rs](src/cache.rs): Implements cache reads/writes to serialize author and feed data onto disk.
    *   [src/feed.rs](src/feed.rs): Extracts feeds, handles Apollo client state JSON harvesting, and strips XSSI headers.
    *   [src/following.rs](src/following.rs): Retrieves and resolves authors and publications lists.
    *   [src/html.rs](src/html.rs): Contains DOM traversal pipelines that remove clutter, clean markdown, and extract high-res images.
    *   [src/input.rs](src/input.rs): Binds key/paste event dispatchers.
    *   [src/markdown.rs](src/markdown.rs): Resolves markdown file content for local display.
    *   [src/meta.rs](src/meta.rs): Records and tracks metadata files of downloaded articles.
    *   [src/net.rs](src/net.rs): Prepares cookie validation and network header profiles.
    *   [src/ui.rs](src/ui.rs): Assembles and paints the Ratatui UI components.
    *   [src/util.rs](src/util.rs): Bundles formatting, jitter sleep calculations, and string slug operations.

### Phase 5: Inline Markdown Previews & Modern Terminal Upgrades
*   **Git Commits**: `d527688` (Integrate tui-markdown & update ratatui to 0.30)
*   **Key Features**:
    *   **tui-markdown Integration**: Swapped raw text viewing of markdown files in the preview pane for a fully formatted rendered layout utilizing the `tui-markdown` crate.
    *   **Crate Upgrades**: Upgraded `ratatui` to `0.30` and `crossterm` to `0.28` to maintain compatibility with modern terminal standards.

### Phase 6: Caching, Author Enrichment, and Architecture Updates
*   **Git Commits**: `4c2f080` (Add PROJECT.md), `9dc462a` (Add cache, author enrichment, download pipeline), `a8d8a0a` (Update system architecture and package dependencies docs), `5ecb673` (doc: update PROJECT.md with Phase 6 details)
*   **Key Features**:
    *   **Local Cache Store**: Introduced a JSON-serialized local cache under `<output_dir>/.cache` for followed creators, feed details, and publication metadata using `serde_json` to reduce redundant network hits on startup.
    *   **Background Author Enrichment**: Implemented an async background enrichment pipeline (`src/meta.rs`) that queries creators' RSS feeds sequentially with jitter and exponential backoff to populate post counts and last-active timestamps.
    *   **Documentation Refactoring**: Created `PROJECT.md` to trace feature evolution, and updated `ARCHITECTURE.md` and `PKG.md` to align with the modularized sub-modules structure and upgraded dependency packages.

### Phase 7: README Restoration
*   **Git Commits**: `0bf2f4c` (fixed readme)
*   **Key Features**:
    *   Recreated `README.md` from scratch (it had been deleted as a "redundant doc" back in Phase 2 and never replaced), rebuilding it around the Features/Installation/Usage/Environment Variables/Documentation-links structure it has today.

### Phase 8: Headless Chromium Experiment
*   **Git Commits**: `235ed99` (Add headless Chromium scraper backend using chromiumoxide), `e8abd8b` (Implement interactive TUI web browser interface powered by headless Chromium)
*   **Key Features**:
    *   **`--chromium` Download Flag**: Added `fetch_html_chromium` (`src/net.rs`) using the `chromiumoxide` crate to launch a real headless Chrome/Chromium process per article — with stealth JS injection (masking `navigator.webdriver`, spoofing WebGL renderer strings) and CDP-based cookie injection — as an alternative to the plain `reqwest` fetch, aimed at defeating Cloudflare challenges that blocked the simpler HTTP client.
    *   **`--web` Interactive Browser (v1)**: Introduced `src/browser.rs` and the `AppView::Browser` TUI view, initially built on top of the same headless-Chromium machinery to render and navigate Medium pages interactively, extracting links for the user to drill into or download.
*   **Dependencies Added**: `chromiumoxide`, `futures`.

### Phase 9: Pivot to a Reqwest-Based Browser
*   **Git Commits**: `92bf5da` (Implement reqwest-based TUI browser with feed directory navigation)
*   **Key Features**:
    *   Replaced the Chromium-driven `--web` browser with a plain `reqwest` + HTML/Apollo-state scrape implementation (`fetch_and_parse`), plus a URL-shape dispatcher that routes landing-page and author/publication URLs to RSS feeds instead of gated HTML wherever possible — avoiding the need to launch a browser process just to navigate Medium interactively.
    *   `--chromium` remained in place for the article *download* pipeline only; the interactive browser no longer depended on it.

### Phase 10: Multi-Select & RSS Discovery Bypasses
*   **Git Commits**: `77ede6e` (feat: multi-article selection in TUI browser and RSS feed discovery bypasses)
*   **Key Features**:
    *   **Multi-Article Selection**: Added checkbox-style multi-select inside the `--web` browser view, letting users tick several articles across a session and hand them all to the downloader at once (`BrowserAction::PopulateDownloader`).
    *   **RSS Discovery Bypasses**: Extended `load_full_feed` to walk every followed author/publication's RSS feed from inside the browser (not just the landing page's own scrape), with jittered delays between requests to avoid rate-limiting.
    *   Expanded `MEDIUM.md` with a full write-up of the `--web` dispatch logic (Section 6).

### Phase 11: Headless Chromium Removal
*   **Git Commits**: `28b1976` (Remove headless Chromium download support)
*   **Key Features**:
    *   Removed `--chromium` and `fetch_html_chromium` entirely, along with the `chromiumoxide`/`futures` dependencies. The feature never worked reliably in practice: it hardcoded `/usr/bin/chromium` as the executable path (unusable on macOS and most non-Debian Linux layouts), never validated the HTTP status of the page it rendered (silently saving Cloudflare challenge/error pages as if they were articles), and leaked the spawned Chromium process on any cookie or navigation error.
    *   With this removal, both the download pipeline and the `--web` browser are back to depending solely on `reqwest` — closing out the two-phase Chromium detour started in Phase 8.

---

## 2. Current Codebase Structure

The modern modularized files in `src/` serve distinct roles in the application lifecycle:

| File | Primary Role & Functionality |
| :--- | :--- |
| [src/main.rs](src/main.rs) | Application entry point; handles log initialization, CLI argument parsing, TUI frame loop setup, and cookie authentication routing. |
| [src/app.rs](src/app.rs) | Contains structural data models: `App` (global state), `AppView` (views: `Download`, `Picker`, `FeedSelector`, `AuthorBrowser`, `Loading`, `Browser`), and `AppEvent` (async communication messages). |
| [src/articles.rs](src/articles.rs) | Queries user-specific articles via the Medium JSON API and handles async RSS crawler mappings for followed authors. |
| [src/auth.rs](src/auth.rs) | Interactively prompts users for session identifiers and validates credentials against Medium network requests. |
| [src/browser.rs](src/browser.rs) | Drives the interactive `--web` TUI browser: fetches a URL, dispatches to the landing-page/RSS/HTML-scrape strategy that best avoids Cloudflare gating, extracts links, and streams incremental updates back to the UI. |
| [src/cache.rs](src/cache.rs) | Reads and writes JSON-serialized details of followed users, feed configurations, and metadata lists to `~/.local/med2md/.cache`. |
| [src/feed.rs](src/feed.rs) | Parses RSS feeds, cleans URL formatting, strips XSSI security envelopes from response payloads, and parses Apollo GraphQL HTML script variables. |
| [src/following.rs](src/following.rs) | Coordinates followed user and publication list retrievals. |
| [src/html.rs](src/html.rs) | Implements the DOM cleanup pipeline. Traverses elements to prune scripts, tracking parameters, banners, and map CDN image sources to local targets. |
| [src/input.rs](src/input.rs) | Processes keypress mappings and clipboard copy-paste hooks depending on the current active `AppView`. |
| [src/markdown.rs](src/markdown.rs) | Reads downloaded documents and converts them to formatted Ratatui lines using `tui-markdown`. |
| [src/meta.rs](src/meta.rs) | Coordinates asynchronous background author enrichment to fetch the latest post timestamps and post counts for all creators. |
| [src/net.rs](src/net.rs) | Assembles user-agents and browser-like cookie headers to bypass basic automated scrape blocks. |
| [src/ui.rs](src/ui.rs) | Translates the application view states into layout block definitions, displaying the URL inputs, logs, checklists, and formatted files. |
| [src/util.rs](src/util.rs) | Helper functions for generating delay jitter, parsing filesystem file types, formatting time signatures, and sanitizing article URLs into file slugs. |

---

## 3. Reference Documentation Mapping
For deeper technical discussions on specific areas of the `med2md` project, see:
*   [ARCHITECTURE.md](ARCHITECTURE.md) — Explains the high-level diagrams, asynchronous execution layers, and article parsing pipeline.
*   [MEDIUM.md](MEDIUM.md) — Details Medium API structures, cookie bypass methods, and DOM scraping logic.
*   [PKG.md](PKG.md) — Focuses on the role and usage of external dependencies declared in `Cargo.toml`.
*   [AGENTS.md](AGENTS.md) / [CLAUDE.md](CLAUDE.md) — Focuses on instruction guidelines for AI agents interacting with the codebase.
