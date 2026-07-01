# med2md

A terminal UI for downloading Medium articles (including member-only content) as clean, offline-ready Markdown — with images saved locally and Medium's tracking/clutter stripped out.

![Markdown viewer](markdown-viewer.png)

## Features

- Download individual article URLs or bulk-select from your following feed
- Browse followed authors and fetch their recent articles
- Full-resolution images extracted and saved alongside each article
- Local JSON cache for authors/feeds to avoid redundant network calls
- In-app Markdown preview with syntax-aware rendering

![Link selector](link-selector.png)

## Installation

Requires the Rust toolchain (edition 2024).

```sh
cargo build --release
./target/release/med2md
```

## Usage

```
med2md                    Launch TUI downloader
med2md --feed             Fetch your following feed and select articles to download
med2md --authors          Browse followed authors, select, then fetch their articles
med2md --dir <path>       Output directory for downloaded articles (default: ~/.local/med2md)
med2md --browse           Browse already-downloaded markdown files
med2md --force            Re-download articles even if they already exist
med2md --refresh          Ignore cache and re-fetch authors/feed from Medium
med2md --log <path>       Write JSON logs to <path> (default: medium.log)
```

### Environment variables

| Variable | Purpose |
| :--- | :--- |
| `MEDIUM_SID` | Session cookie, required for member-only content |
| `MEDIUM_UID` | User ID cookie, improves `--authors` completeness |
| `MEDIUM_USERNAME` | Your Medium `@username`, helps `--authors` discovery |
| `MEDIUM_CF_CLEARANCE` | Cloudflare clearance cookie, required for `--feed` and most content |
| `MEDIUM_DIR` | Output directory (default: `~/.local/med2md`) |

If unset, `med2md` will prompt interactively for cookies on startup.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — system diagrams, async layers, and the article parsing pipeline
- [MEDIUM.md](MEDIUM.md) — how med2md authenticates with and scrapes Medium
- [PKG.md](PKG.md) — external crates and why each is used
- [PROJECT.md](PROJECT.md) — feature history and codebase evolution
- [AGENTS.md](AGENTS.md) — instructions for AI coding agents working in this repo
