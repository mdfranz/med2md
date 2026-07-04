# RAG Chat Feature for med2md

## Context

`references/rig-lancedb-deep-research.md` is prep material (not code) for using the `rig` orchestration framework + `LanceDB` in Rust. The goal is to act on it: let med2md ingest its own downloaded article markdown into a local vector store and answer questions about them via a chat interface inside the existing TUI, using the user's choice of Anthropic, OpenAI, or Gemini for the actual chat completions (embeddings configured independently, since Anthropic has no embeddings API).

Confirmed via user Q&A:
- Chat is a new `AppView::Chat` reachable via keybinding in the existing ratatui TUI (not a separate CLI mode).
- Embedding provider/model is configured fully independently of the chat provider/model (separate env vars).
- Ingestion is a manual `--index` CLI flag/command, not automatic on download or on chat open.
- Scope is chat only, but the LLM completion code must be structured as a reusable provider abstraction (OpenAI/Anthropic/Gemini) so future non-chat features can reuse it.

Version/API research (verified against crates.io + rig source, 2026-07-04): `rig-core` 0.39.0, `rig-lancedb` 0.39.0 (locked to `rig-core ^0.39.0`), `lancedb ^0.30` (latest 0.31.0, depends on arrow `^58`, **not** the newest arrow 59.x — our own `arrow-array`/`arrow-schema` deps must be pinned to `"58"` to avoid a version clash). `Agent<M>`/`CompletionModel` are generic per-provider and use RPITIT, so they are **not** dyn-compatible — the right abstraction is enum dispatch over the three provider clients, erasing to a uniform type only at the *stream item* level (`Pin<Box<dyn Stream<Item = String> + Send>>`), which rig's own `StreamingResult<R>` type alias already does per-provider internally. `EmbeddingModel::ndims()` is a runtime instance method, which resolves the "vector dimension must match embedding model" requirement without a hardcoded table.

Verified against actual code: `AppView`/`AppEvent` live in `src/app.rs`; `handle_key` in `src/input.rs` dispatches to per-view handlers (`handle_browser_key`, `handle_feed_selector_key`, `handle_author_browser_key`) *before* the global `Esc`/`Ctrl+C` = quit check at line 303 — a `Chat` view needs the same early-dispatch treatment so `Esc` inside chat returns to `Download` instead of quitting the app (mirrors `handle_browser_key`, `src/input.rs:525-533`). `Ctrl+W` (web browser) is checked at `src/input.rs:298`, right before the global quit check — the new `Ctrl+G` (unused) chat-entry binding goes in that same spot. The `Ctrl+P` no-op exhaustive match at `src/input.rs:315` and the final `unreachable!()` exhaustive match in `src/ui.rs:519-522` both need `AppView::Chat { .. }` added. `src/markdown.rs`'s `render_markdown()` (wraps `tui_markdown::from_str`) is reusable as-is for rendering assistant responses. `src/auth.rs::setup_cookies()` is the precedent for env-var-based secrets, but its interactive-prompt fallback is Medium-cookie-specific; LLM API keys should just read the provider's conventional env var names (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY` — matching what `rig`'s own `Client::from_env()` looks for) and error clearly if missing, not prompt interactively.

## New Cargo dependencies

```toml
rig-core = "0.39"
rig-lancedb = "0.39"
lancedb = "0.31"
arrow-array = "=58.0.0"   # pinned to match lancedb 0.31's transitive requirement, not newest 59.x
arrow-schema = "=58.0.0"
futures = "0.3"           # StreamExt::boxed()/map/filter_map for the provider-agnostic stream
```

`rig-core` default features (`reqwest`, `derive`, `rustls`) are what we want — `derive` brings the `#[derive(Embed)]` macro, `rustls` avoids a new system OpenSSL dependency for what's meant to be an easy-to-`cargo install` CLI.

**Keep manual CLI arg parsing** (no clap): the new flags (`--index`, `--chat-provider`, `--chat-model`, `--embed-provider`, `--embed-model`) are all either bare toggles or `--flag value` pairs, identical in shape to the existing `--dir`/`--log` handling in `main.rs`. Introducing clap for this marginal ergonomic gain would be inconsistent with the rest of the 293-line `main.rs` and adds a dependency for no real benefit.

## New modules

```
src/llm_config.rs   -- env/CLI config resolution for chat + embedding provider/model/keys
src/chat.rs          -- ChatProvider enum + boxed streaming dispatch
src/embed.rs         -- EmbedProvider enum wrapping OpenAI/Gemini embedding models
src/rag/mod.rs
src/rag/schema.rs     -- Arrow schema (Chunk struct w/ #[derive(Embed)]), LanceDB connect/table creation
src/rag/ingest.rs     -- walk output_dir, chunk markdown, embed, merge_insert upsert (the --index path)
src/rag/query.rs      -- retrieval (top-n via LanceDbVectorIndex) + preamble/prompt assembly
```

### `src/llm_config.rs`

```rust
pub enum ChatProviderKind { OpenAI, Anthropic, Gemini }
pub enum EmbedProviderKind { OpenAI, Gemini }  // Anthropic has no embeddings API

pub struct ChatConfig { pub provider: ChatProviderKind, pub model: String, pub api_key: String }
pub struct EmbedConfig { pub provider: EmbedProviderKind, pub model: String, pub api_key: String }

pub fn load_chat_config(cli_provider: Option<String>, cli_model: Option<String>) -> Result<ChatConfig, String>;
pub fn load_embed_config(cli_provider: Option<String>, cli_model: Option<String>) -> Result<EmbedConfig, String>;
```

Precedence: CLI flag > env var > default (same precedence order `main.rs` already uses for `--dir`/`MEDIUM_DIR`).

| Purpose | Env var | CLI flag | Default |
|---|---|---|---|
| Chat provider | `MED2MD_CHAT_PROVIDER` | `--chat-provider` | `anthropic` |
| Chat model | `MED2MD_CHAT_MODEL` | `--chat-model` | `claude-sonnet-4-5` |
| Embedding provider | `MED2MD_EMBED_PROVIDER` | `--embed-provider` | `openai` |
| Embedding model | `MED2MD_EMBED_MODEL` | `--embed-model` | `text-embedding-3-small` |
| API keys | `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GEMINI_API_KEY` | — | required for the selected provider(s) |

No interactive prompt for missing API keys (unlike Medium cookies) — fail with a clear error message, since every other Anthropic/OpenAI/Gemini CLI tool expects these standard env vars already set.

### `src/chat.rs` — provider enum, dyn-safe only at the stream-item boundary

```rust
pub enum ChatProvider {
    OpenAI(rig::providers::openai::Client),
    Anthropic(rig::providers::anthropic::Client),
    Gemini(rig::providers::gemini::Client),
}

pub enum ChatStreamEvent { Delta(String), Done { history: Vec<rig::message::Message> } }
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<ChatStreamEvent, String>> + Send>>;

impl ChatProvider {
    pub async fn stream_chat(&self, model: &str, preamble: &str, prompt: String, history: Vec<rig::message::Message>) -> Result<TokenStream, String>;
}
```

Implementation: match on the enum once per call to get a concrete, monomorphized `Agent<M>` (`client.agent(model).preamble(preamble).build()`), call `.stream_chat(prompt, &history).await`, then `.map(...).boxed()` to erase the provider-specific stream item type down to `ChatStreamEvent`. Map `MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(t))` → `Delta(t.text)`, `MultiTurnStreamItem::FinalResponse(fin)` → `Done { history: fin.history() }`; ignore/empty-string any tool/reasoning stream items (no tools are registered). Do **not** attempt to make `Agent<M>`/`CompletionModel` itself `dyn` — that's the part that isn't object-safe.

### `src/embed.rs`

```rust
pub enum EmbedProvider { OpenAI(rig::providers::openai::Client), Gemini(rig::providers::gemini::Client) }
```

Simpler than chat: no streaming, so each call site (ingestion, query-time question embedding) matches on the enum directly and gets a `Vec<f32>` back — no boxing needed.

### `src/rag/schema.rs`

```rust
#[derive(rig::Embed, Clone, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    pub id: String,          // "{slug}#{chunk_idx}"
    pub slug: String,
    pub source_path: String,
    pub heading: String,     // nearest preceding heading, "" if none
    #[embed]
    pub content: String,
}
```

- LanceDB lives at `{output_dir}/.rag/lancedb` (sibling to the existing `.cache/` dir, same naming convention).
- A sidecar `{output_dir}/.rag/meta.json` (`{"embed_provider", "embed_model", "dims"}`) records the dimensionality used to build the table (obtained once via `embed_model.ndims()`). If `--index` detects a provider/model/dims mismatch on re-run, drop and recreate the table rather than risk an Arrow schema mismatch — this is a deliberately simple, file-based stand-in for full ACID schema evolution, right-sized for a single-user tool.
- **No ANN/IVF_PQ index is built.** At this project's scale (a personal article collection — dozens to hundreds of docs, not >100k vectors), brute-force ENN (LanceDB's default with no index) is correct and simpler; skip `create_index()`, GPU acceleration, and the S3/DynamoDB production-scale material from the reference doc entirely — none of it applies to a local desktop CLI.

### `src/rag/ingest.rs` — the `--index` path

Walk `output_dir` for `*.md` files, skipping anything under a `*_images/` directory.

**Chunking** (concrete, for already-clean article markdown with no frontmatter):
1. Split on markdown headings (`^#{1,6}\s`) into heading-scoped sections, tracking heading text per chunk (empty for content before the first heading).
2. Within each section, split on blank-line-delimited paragraphs and greedily accumulate into chunks up to ~1500 characters; a single paragraph longer than that is kept whole rather than mid-split.
3. Drop chunks under ~40 characters (stray captions/headings with no body).
4. `id = format!("{slug}#{idx}")` where `slug` is the file stem (`Path::file_stem()`) — this is exactly the same slug `net.rs::perform_download` used to name `{slug}.md`, so ids stay stable across re-indexing.

Flow: resolve embed config → connect LanceDB → check `meta.json`, rebuild table if mismatched/absent → for each file, read + chunk → batch (~96 chunks) through `rig::embeddings::EmbeddingsBuilder` → `table.merge_insert(&["id"]).when_matched_update_all().when_not_matched_insert_all().execute(...)` so re-running `--index` after re-downloading updated articles is safe (updates changed rows instead of duplicating). Print progress/summary to stdout; this runs before the TUI starts (like `--browse`) and exits — it is not an `AppView`.

### `src/rag/query.rs`

```rust
pub async fn retrieve(index: &LanceDbVectorIndex<EmbedModel>, question: &str, n: usize) -> Vec<Chunk>;
pub fn build_preamble(chunks: &[Chunk]) -> String;
```

`n = 5` by default. Preamble is rebuilt fresh each turn from that turn's retrieval (instructs the model to answer only from the given excerpts and cite the source slug); conversation history is carried separately through rig's own `stream_chat(prompt, &history)` parameter.

## `AppView::Chat` wiring

### `src/app.rs`

```rust
pub enum AppView {
    // ...existing variants...
    Chat {
        input: String,
        cursor_x: usize,
        messages: Vec<ChatMessage>,
        scroll_y: usize,
        is_streaming: bool,
        rag_history: Vec<rig::message::Message>,
    },
}
pub enum ChatRole { User, Assistant }
pub struct ChatMessage { pub role: ChatRole, pub content: String }
```

`AppEvent` gains `ChatToken(String)`, `ChatTurnDone(Vec<rig::message::Message>)`, `ChatError(String)`.

`App` gains `chat_provider: Option<ChatProvider>`, `chat_model: String`, `embed_provider: Option<EmbedProvider>`, `embed_model: String`, `rag_index: Option<LanceDbVectorIndex<...>>` — all `Option` because construction can fail on a missing key, and the app must still launch for plain downloading with no LLM keys configured; entering Chat with `None` shows a log error instead of failing startup.

### `src/main.rs`

- Build `ChatProvider`/`EmbedProvider` once at startup (next to the existing `setup_cookies().await` call), store in `App`.
- Add two `AppEvent` match arms in the event-drain loop (~line 213-266): `ChatToken` appends to (or starts) the last assistant message; `ChatTurnDone` clears `is_streaming` and updates `rag_history`; `ChatError` logs and clears `is_streaming`.
- New CLI flags, same manual style as existing ones:
  ```rust
  let index_mode = args.iter().any(|a| a == "--index");
  let chat_provider_override = args.windows(2).find(|w| w[0] == "--chat-provider").map(|w| w[1].clone());
  // ...and --chat-model, --embed-provider, --embed-model similarly
  ```
  `--index` short-circuits before terminal/raw-mode setup (like `--help`): run ingestion, print a summary, `return Ok(())`. Add corresponding lines to the `--help` text and the `ENVIRONMENT VARIABLES` block.

### `src/input.rs`

- Add `handle_chat_key` dispatched at the *top* of `handle_key`, alongside the existing `matches!(app.view, AppView::Browser { .. })` early-dispatch checks (`src/input.rs:282-296`) — this is required so `Esc` inside Chat returns to `AppView::Download` rather than hitting the global `Esc`/`Ctrl+C` = quit check at line 303 (mirrors `handle_browser_key`, `src/input.rs:525-533`).
- Add a `Ctrl+G` check (unused today; existing bindings are `Ctrl+C` quit, `Ctrl+W` web browser, `Ctrl+P` picker, `Ctrl+L` clear log, `Ctrl+S` download) right next to the `Ctrl+W` check at `src/input.rs:298`, to enter `AppView::Chat` from `Download`.
- `handle_chat_key`: single-line text editing on `input`/`cursor_x` (same char-index editing style as `handle_multiline_key`, single-line variant); `Enter` when not streaming and input non-empty → push a `ChatMessage::User`, clear input, set `is_streaming = true`, `tokio::spawn` the retrieve+stream task sending `AppEvent::ChatToken`/`ChatTurnDone`/`ChatError` back over `tx.clone()` (same spawn shape as `input::enter_browser_mode`).
- Add `AppView::Chat { .. }` to the `Ctrl+P` no-op exhaustive match at `src/input.rs:315`.

### `src/ui.rs`

- Add an `if let AppView::Chat { .. } = &app.view { ... }` block alongside the existing `Browser`/`AuthorBrowser` blocks (`src/ui.rs:291` area): scrollable message pane (assistant messages via `render_markdown` from `src/markdown.rs`, reused unmodified; user messages as plain styled text), plus a single-line input box at the bottom (reuse the cursor-positioning logic from the existing single-line-adapted portion of the urls-field drawing).
- Add `AppView::Chat { .. } => unreachable!()` to the final exhaustive match at `src/ui.rs:519-522` (Chat is handled by its own `if let` block earlier, same as `Browser`).

## Build ordering

1. `Cargo.toml` deps (get arrow/lancedb/rig-core pinning right first — highest dependency-resolution risk) + `src/llm_config.rs`.
2. `src/rag/schema.rs` + `src/embed.rs` — connect to LanceDB, create an empty table; confirms the arrow version pinning actually links.
3. `src/rag/ingest.rs` + `--index` CLI flag — get ingestion working and manually verified via CLI alone, before touching any TUI code.
4. `src/chat.rs` — provider enum + streaming, confirm it compiles against all three provider client types.
5. `src/rag/query.rs` — glue ingestion output to chat input (retrieval + preamble).
6. `AppView::Chat` in `app.rs` → dispatch/keys in `input.rs` → rendering in `ui.rs` → the two new `AppEvent` arms in `main.rs`. This is the most mechanical step once 1-5 work, since it closely follows the existing `Browser` view precedent.

## Verification

**Ingestion**: run `med2md --index --dir <test_dir>` against a small existing set of downloaded `.md` files; confirm progress output and a final chunk/article count. Inspect `{test_dir}/.rag/lancedb` and `meta.json` (dims matching the embedding model, e.g. 1536 for `text-embedding-3-small`). Re-run `--index` unchanged and confirm no row duplication (`merge_insert` upsert working). Edit one article and re-run, confirm only that article's chunks update. Switch `MED2MD_EMBED_MODEL` to a different-dimension model and re-run, confirm the mismatch is detected and the table rebuilds instead of erroring on an Arrow type mismatch.

**Chat**: launch `med2md --dir <test_dir>` normally with relevant API keys set, press `Ctrl+G` to enter Chat. Ask a question clearly answerable from an indexed article; confirm tokens stream in incrementally and the response reflects actual article content (verify retrieval is gating the answer — ask something *not* in any article and confirm the model says it doesn't know, rather than hallucinating from general knowledge). Ask a follow-up referencing "it" from the prior turn to confirm history round-trips correctly. Repeat with `MED2MD_CHAT_PROVIDER` set to each of `anthropic`/`openai`/`gemini` to prove the enum dispatch actually works at runtime for all three. Unset an API key and confirm entering Chat shows a clear log error, not a panic/hang.

**Unit tests** (add near their modules, pure/sync, no network needed): `chunk_markdown()` in `src/rag/ingest.rs` — synthetic markdown with multiple headings, one long paragraph, one short boilerplate line, asserting chunk boundaries/heading attribution/short-chunk dropping. Config precedence resolution in `src/llm_config.rs` (CLI > env > default).
