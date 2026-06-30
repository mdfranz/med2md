# Medium Content Capture & API Mechanisms

This document explains the technical findings gathered from [src/main.rs](src/main.rs) regarding how Medium content is structured, fetched, authenticated, parsed, and converted.

---

## 1. Authentication & Cookie Handshake

Medium shields premium content and user feeds behind Cloudflare and cookie authentication. To retrieve paywalled or personalized feeds, three parameters are required:

*   **`MEDIUM_SID`**: The core HTTP session cookie. If not provided, only public-facing articles are accessible.
*   **`MEDIUM_UID`**: The user identity cookie. Required to identify the current user and resolve API queries.
*   **`MEDIUM_CF_CLEARANCE`**: The Cloudflare bypass clearance cookie. Vital for querying feed endpoints without getting blocked.

These cookies are sent with every HTTP request using headers built by `build_cookie_headers` (defined in [src/main.rs#L1674](src/main.rs#L1674)), combined with a modern browser user agent to avoid bot-detection headers.

---

## 2. API Endpoints & Harvesting Techniques

Medium uses a mix of internal JSON endpoints, RSS feeds, and embedded Apollo client state blocks to serve data. `med2md` interfaces with all three techniques:

### A. Internal JSON APIs (with XSSI Protection)
To fetch the list of followed accounts, the following private endpoints are queried:
*   **Followed People**: `https://medium.com/_/api/users/{uid}/following?limit=200`
*   **Followed Publications**: `https://medium.com/_/api/users/{uid}/followingPublications?limit=200`

#### XSSI Bypass:
Medium protects its JSON APIs from Cross-Site Script Inclusion (XSSI) attacks by prefixing responses with a looping script block:
```javascript
])}while(1);</x>{"payload": ...}
```
To parse this, `parse_medium_api_json` (defined in [src/main.rs#L1317](src/main.rs#L1317)) searches for the first `{` character to strip the protection block before passing the payload to `serde_json`.

### B. Apollo GraphQL HTML Harvesting
When fetching HTML pages (such as profile pages or `https://medium.com/?feed=following`), Medium embeds its client state directly into the HTML to prevent secondary database requests on the client side. This is initialized as a JavaScript global variable inside a `<script>` tag:
```html
<script>window.__APOLLO_STATE__ = { ... }</script>
```
The function `extract_following_from_html` (defined in [src/main.rs#L1051](src/main.rs#L1051)) scrapes this HTML block, extracts the JSON-like Apollo state, and harvests:
1.  Usernames of followed authors.
2.  Slugs of followed publications.
3.  Recent posts showing on the client's feed (which serves as an immediate seed of article metadata).

### C. Standard RSS Feeds
Medium exposes public-facing RSS feeds which can be fetched without Cloudflare issues if browser cookies are appended:
*   **User Feeds**: `https://medium.com/feed/@{username}`
*   **Publication Feeds**: `https://medium.com/feed/{publication_slug}`

These are parsed in `parse_rss_items` (defined in [src/main.rs#L1029](src/main.rs#L1029)) to extract metadata (timestamps, titles, article URLs, and author names).

---

## 3. DOM Scraping & Clean-up Pipeline

To produce clean Markdown text, Medium's HTML structure must be stripped of trackers, banners, and layout widgets.

### A. Highest-Quality Image Resolution
Medium serves responsive layouts using `<picture>` elements that contain nested `<source>` tags specifying differing resolutions in `srcset`:
```html
<picture>
  <source srcset="https://miro.medium.com/v2/resize:fit:640/... 640w, https://miro.medium.com/v2/resize:fit:1200/... 1200w" />
  <img src="https://miro.medium.com/v2/resize:fit:640/..." />
</picture>
```
The scraper implementation `clean_article_and_collect_images` (defined in [src/main.rs#L902](src/main.rs#L902)):
1.  Identifies `<picture>` nodes.
2.  Iterates through `<source>` tags to extract the last URL in the `srcset` (the largest resolution).
3.  Injects this high-resolution URL into the inner `<img>` tag's `src` attribute.
4.  Deletes the `<source>` nodes to avoid redundant image markup.
5.  Rewrites remote CDN image references to local folder references: `./[slug]_images/img_{counter}.{ext}`.

### B. Tracking Parameters Removal
Medium links contain tracking query variables. In `clean_article` (defined in [src/main.rs#L707](src/main.rs#L707)), URLs are parsed and cleaned of query keys such as `source*`, `referrer`, and `gi` using the `url` crate.

### C. DOM Clutter Deconstruction
The following nodes are systematically detached in `clean_article` to isolate content:
1.  **Tag Pruning**: `button`, `svg`, `style`, and `script` elements.
2.  **Avatar Images**: Images pointing to small-size parameters (e.g. `resize:fill:64:64`, `resize:fill:32:32`).
3.  **UI Metadata Text**: Paragraphs or spans containing static string snippets like:
    *   `member-only story`
    *   `press enter or click to view image in full size`
    *   `min read` (such as `5 min read`)
    *   `follow` / `mute` / `share` / `listen`
    *   Separators like `·` or `--` or `—`
