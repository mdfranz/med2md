# Medium Content Capture & API Mechanisms

This document describes how `med2md` authenticates with Medium, discovers content, and converts it to Markdown. The codebase is split across modules under [src/](src/).

---

## 1. Authentication & Cookie Handshake

Medium shields premium content and user feeds behind Cloudflare and cookie authentication. Three parameters are required:

- **`MEDIUM_SID`**: Core HTTP session cookie. Without it only public articles are accessible.
- **`MEDIUM_UID`**: User identity cookie. Required for API queries that identify the current user.
- **`MEDIUM_CF_CLEARANCE`**: Cloudflare clearance cookie. Required for feed endpoints and most content without triggering bot detection.

Every HTTP request is built with `build_cookie_headers` ([src/net.rs](src/net.rs)) which constructs a `HeaderMap` combining the three cookies with a modern browser `User-Agent`. Interactive cookie setup (prompting/reading env vars) lives in `setup_cookies` ([src/auth.rs](src/auth.rs)).

### Which Pages Cloudflare Actually Gates

A valid `cf_clearance` cookie does **not** unblock every page — Cloudflare's WAF applies per-route rules, verified by direct request testing:

| URL pattern | Result with valid cookies |
|---|---|
| `medium.com/me/feed` | 200 OK |
| `medium.com/feed/@{username}` (RSS) | 200 OK |
| `medium.com/feed/{publication_slug}` (RSS) | 200 OK |
| Individual article pages (incl. `{user}.medium.com/...` custom subdomains) | 200 OK |
| `medium.com/@{username}` (profile page HTML) | **403** — Cloudflare JS "Just a moment..." managed challenge |
| `medium.com/me/following-feed/all` | **403** — same managed challenge |

The practical rule of thumb: plain HTML GETs to profile/publication *listing* pages get challenged regardless of cookies, but the RSS feed for that same account does not. Every content-discovery path in this codebase (`fetch_following_feed`, `fetch_rss_for_authors`, the `--web` browser) is designed around this — falling back to RSS instead of scraping profile HTML whenever a per-author/publication article list is needed. `fetch_following_list` ([src/following.rs](src/following.rs)) still tries `/me/following-feed/all` and `/@{username}/following` as candidates since Cloudflare's behavior isn't guaranteed stable, but treat their success as opportunistic, not relied-upon.

---

## 2. Content Discovery

### A. Following List — Internal JSON APIs

`fetch_following_list` ([src/following.rs](src/following.rs)) assembles the full list of followed accounts using two private endpoints:

- **People**: `https://medium.com/_/api/users/{uid}/following?limit=200`
- **Publications**: `https://medium.com/_/api/users/{uid}/followingPublications?limit=200`

Both endpoints return paginated responses with a `next` cursor. Pagination loops until no cursor is returned.

#### XSSI Protection

Medium prefixes all JSON API responses with a looping script block to block cross-site inclusion:

```
])}while(1);</x>{"payload": ...}
```

`parse_medium_api_json` ([src/feed.rs](src/feed.rs)) strips everything before the first `{` before handing the payload to `serde_json`.

#### Apollo State Fallback

When the API returns incomplete results, `fetch_following_list` falls back to fetching `https://medium.com/@{username}/following` as HTML and scraping the embedded Apollo client state:

```html
<script>window.__APOLLO_STATE__ = { ... }</script>
```

`extract_following_from_html` ([src/feed.rs](src/feed.rs)) parses this block to harvest usernames and publication slugs. `extract_user_id_from_apollo` ([src/feed.rs](src/feed.rs)) similarly extracts numeric user IDs from Apollo state for API calls that require them.

### B. Following Feed — Apollo HTML Scraping

`fetch_following_feed` ([src/following.rs](src/following.rs)) fetches `https://medium.com/?feed=following` as HTML and extracts article metadata from the Apollo state embedded in the page. This returns the same seed articles that Medium would render in the client without requiring additional API round-trips.

### C. Per-Author Article Lists

When articles are requested for specific selected authors, `fetch_rss_for_authors` ([src/articles.rs](src/articles.rs)) fetches each author's RSS feed and `fetch_user_posts_api` ([src/articles.rs](src/articles.rs)) queries the paginated posts API:

- **User posts API**: `https://medium.com/_/api/users/{user_id}/posts?limit=20&to={cursor}`

### D. RSS Feeds

Medium exposes public RSS feeds:

- **Users**: `https://medium.com/feed/@{username}`
- **Publications**: `https://medium.com/feed/{publication_slug}`

`parse_rss_items` ([src/feed.rs](src/feed.rs)) parses these to extract `(timestamp, title, url, author)` tuples. `clean_rss_url` ([src/feed.rs](src/feed.rs)) strips Medium tracking parameters from RSS item links.

---

## 3. Author Enrichment

`enrich_authors` ([src/meta.rs](src/meta.rs)) runs as a background Tokio task to enrich the author list with last-post date and article count by fetching each author's RSS feed.

### Rate Limiting

The RSS endpoint rate-limits aggressively when many authors are queried in sequence. The enrichment loop uses two layers of protection:

- **Per-author retry**: `fetch_one` ([src/meta.rs](src/meta.rs)) retries up to 3 times on 429 responses. Each retry waits the duration from the `Retry-After` response header (parsed as integer seconds; falls back to an exponentially-growing default). The per-retry backoff caps at 5 minutes.
- **Adaptive inter-request delay**: A `delay_ms` variable starts at 400ms, doubles (up to 8 seconds) after any 429 encounter, and gradually recovers toward 400ms after clean successes.

### Cache-Aware Skipping

`enrich_authors` accepts the existing `author_meta` map. Authors with a known non-zero last-post timestamp are skipped unless `--refresh` was passed, so repeat runs only fetch new or previously-failed authors.

---

## 4. Local Cache

`src/cache.rs` provides a JSON cache with TTL enforcement. All cache files live in `{output_dir}/.cache/`.

| File | Contents | TTL |
|---|---|---|
| `authors.json` | `Vec<(kind, name)>` — the following list | 24 hours |
| `feed.json` | `Vec<(title, url, date, author)>` — feed articles | 24 hours |
| `authors_meta.json` | `HashMap<name, (last_post_ts, rss_count)>` | 24 hours |

Cache format:
```json
{ "fetched_at": 1719792000, "data": [...] }
```

The meta cache is always loaded at startup regardless of TTL so authors display immediately with their last-known data. Enrichment then runs in the background to fill in any gaps.

`--refresh` bypasses the authors and feed caches entirely and forces enrichment to re-fetch all authors.

---

## 5. Article Download Pipeline

`perform_download` ([src/net.rs](src/net.rs)) orchestrates a full article download:

1. Fetches the article HTML with auth cookies.
2. Passes the document through `clean_article` ([src/html.rs](src/html.rs)) which removes buttons, SVGs, style/script tags, avatar images, and Medium UI chrome (tracking links, `min read`, `follow`, separators, etc.).
3. Calls `clean_article_and_collect_images` ([src/html.rs](src/html.rs)) which rewrites `<picture>` elements to use the highest-resolution `srcset` URL, collects all image URLs, and replaces remote CDN references with local paths (`./[slug]_images/img_N.ext`).
4. Converts the cleaned HTML to Markdown via `htmd`.
5. Passes the Markdown through `clean_markdown` ([src/html.rs](src/html.rs)) to strip any residual HTML artifacts.
6. Calls `inject_source_link` ([src/html.rs](src/html.rs)) to embed the source URL as a Markdown link in the `# Title` heading.
7. Downloads images concurrently into `{output_dir}/{slug}_images/`.
8. Writes the final Markdown to `{output_dir}/{slug}.md`.

### Source Link Injection

`inject_source_link` ([src/html.rs](src/html.rs)) rewrites the first `# Heading` line:

```markdown
# Title                          →   # [Title](https://medium.com/...)
```

If no heading is found, a `[Source](url)` line is prepended.

### Tracking Parameter Removal

Medium embeds tracking query parameters (`source*`, `referrer`, `gi`) in internal links. These are stripped during `clean_article` using the `url` crate.

### Image Resolution

Medium serves responsive images using `<picture>` / `<source srcset="...">` markup. The scraper selects the last (largest) URL from each `srcset` and injects it into the inner `<img src>` before converting to Markdown, ensuring downloaded images are full resolution.

---

## 6. Interactive Web Browser (`--web`)

`run_browser_task` ([src/browser.rs](src/browser.rs)) drives the `AppView::Browser` TUI view: it fetches a URL, extracts links, and waits for the next `BrowserCommand` (`Navigate`, `GoBack`, `ScrollDown`) sent by keyboard input in [src/input.rs](src/input.rs).

Because of the Cloudflare gating described above, this module cannot simply GET whatever URL the user navigates to — it dispatches based on URL shape:

- **`https://medium.com/me/feed`** (the landing page): handled by `load_full_feed`, which first scrapes the page's own Apollo state/`<a>` tags via `fetch_and_parse` (a handful of server-rendered articles plus the list of followed authors/publications as `Author`-kind links), then walks every followed author/publication found and pulls their RSS feed too (`fetch_rss_links`). The RSS-derived articles are recency-sorted among themselves, but they're appended after the initial landing-page articles rather than merged into one fully sorted list — the ordering isn't identical to `--feed`'s single global sort, just built lazily inside the browser. A 1.2s-jittered delay separates each RSS request to avoid rate-limiting; author/publication links stay in the list afterward for optional drill-down, and progress is streamed to the log pane (`[n/total] +k articles`).
- **Author/publication profile URLs** (`medium.com/@user`, `medium.com/{pub-slug}`, `{user}.medium.com/`): `rss_url_for_profile` maps the URL to its RSS feed, and `fetch_rss_links` is used instead of scraping the (Cloudflare-gated) HTML. Falls back to the HTML scrape only if the RSS fetch fails or is empty.
- **Everything else** (individual articles, nav/footer links, tag pages): falls through to `fetch_and_parse`, the original raw-HTML + Apollo-state scrape.

`is_boilerplate_url` filters out known non-content links (sitemap, careers, store, etc.) before they ever become candidate `Author`/`Feed` entries.

---

## 7. Storage Layout

Default output directory: `~/.local/med2md/` (XDG-compliant; overridable via `--dir` or `MEDIUM_DIR`).

```
~/.local/med2md/
├── {slug}.md                   # downloaded articles
├── {slug}_images/
│   └── img_1.jpg               # locally cached images
└── .cache/
    ├── authors.json
    ├── feed.json
    └── authors_meta.json
```
