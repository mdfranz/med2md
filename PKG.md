# External Dependencies

## TUI / Terminal

| Crate | Version | Role |
|---|---|---|
| `ratatui` | 0.26 | TUI framework — layouts, widgets, styling, rendering |
| `crossterm` | 0.27 | Terminal backend — raw mode, alternate screen, bracketed paste, key/event polling |

**Validation:** Both are required. `ratatui` drives every UI widget; `crossterm` provides the terminal primitives `ratatui`'s `CrosstermBackend` depends on and the event loop (`event::poll` / `event::read`).

**Feature flag note:** `crossterm` is declared with `features = ["event-stream"]`, which provides an async `EventStream`. The code does not use it — it uses `event::poll` + `event::read` instead. This feature can be dropped.

---

## HTTP / Networking

| Crate | Version | Role |
|---|---|---|
| `reqwest` | 0.12 | HTTP client — fetches article HTML and downloads images |
| `tokio` | 1 | Async runtime — `#[tokio::main]`, `tokio::spawn`, `tokio::fs`, `mpsc` channel |

**Validation:** Both are required. `reqwest` is the only HTTP client used; `tokio` is the runtime that drives all async work including file I/O (`tokio::fs::write`, `tokio::fs::create_dir_all`) and the background download task.

**Feature flag note:** `reqwest` is declared with `features = ["cookies"]`, which enables the built-in cookie jar. The code does **not** use the cookie jar — session cookies are assembled manually into a `Cookie` header string. This feature flag is unnecessary and can be removed.

---

## HTML Parsing & Conversion

| Crate | Version | Role |
|---|---|---|
| `scraper` | 0.19 | HTML parsing and CSS selector querying — parses pages, selects/mutates DOM nodes |
| `html2md` | 0.2 | Converts cleaned HTML fragment to Markdown |
| `ego-tree` | 0.6 | Arena tree — `scraper` exposes its internal `ego_tree::NodeRef` type in public APIs |
| `markup5ever` | 0.12 | HTML5 primitives — `QualName` / `Namespace` / `LocalName` needed to construct attribute keys when mutating DOM nodes via `scraper` |

**Validation:** All four are required.
- `scraper` is used for all DOM traversal, selector matching, and node mutation.
- `html2md` is the only HTML→Markdown converter called (`html2md::parse_html`).
- `ego-tree` is a direct dependency because `scraper::Html::tree` and `NodeRef` appear in function signatures (`has_key_descendants`, `get_text`). It cannot be avoided without wrapping.
- `markup5ever` types are required when inserting or looking up attributes on `scraper` elements (e.g. constructing `QualName::new(None, Namespace::from(""), LocalName::from("src"))`). There is no higher-level API in `scraper` for this mutation path.

---

## URL Handling

| Crate | Version | Role |
|---|---|---|
| `url` | 2.5 | URL parsing and query-parameter manipulation — strips tracking params (`source`, `referrer`, `gi`) from links |

**Validation:** Required. Used in `clean_article` to parse, filter query pairs, and reserialize URLs. `reqwest` re-exports a compatible `Url` type but not the query mutation API (`query_pairs_mut`), so the standalone `url` crate is needed.

---

## Summary

All 9 crates are justified. Two feature flags are unnecessary and can be removed:

```toml
# Remove `cookies` — sessions are sent as a raw Cookie header, not via jar
reqwest = { version = "0.12" }

# Remove `event-stream` — async EventStream is unused; poll/read suffices
crossterm = { version = "0.27" }
```
