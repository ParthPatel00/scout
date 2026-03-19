# Scout (CodeSearch) — Comprehensive Implementation Plan

## Context

Scout is a semantic code search CLI tool being built from scratch in Rust. Two specification documents define the full scope:
- `codesearch-spec.md` — feature spec, CLI interface, data models, roadmap
- `TECHNICAL_DEEP_DIVE.md` — production risks and their solutions

The goal of this plan is to translate those specs into 9 discrete implementation phases, each with clear deliverables, testing guidelines, and success criteria. The plan also serves as a living progress tracker — `CLAUDE.md` and this file should be updated to reflect the current phase and completed phases after each phase is finished.

## Between-Phase Checklist

After completing any phase and before starting the next, do the following:

1. **Update `IMPLEMENTATION_PLAN.md`** — Mark the finished phase as `✅ Complete` in the Phase Overview table and the next phase as `🔄 In progress`
2. **Update `CLAUDE.md`** — Change the "Current Phase" line and mark the finished phase as complete in its phase table
3. **Verify success criteria** — All success criteria for the completed phase must pass before moving on
4. **Commit** — Commit all code and both updated docs together with a message like `feat: complete phase N — <name>`

---

## Phase Overview

| Phase | Name | Focus | Status |
|-------|------|--------|--------|
| 1 | Foundation | Project setup, data models, tree-sitter parsing | ✅ Complete |
| 2 | Fast Search | Tantivy BM25 + basic CLI | ✅ Complete |
| 3 | Smart Search | Call graphs, filters, fusion ranking | ✅ Complete |
| 4 | TUI & UX | Ratatui interface, syntax highlighting, output formats | ✅ Complete |
| 5 | Production Hardening | Concurrency, corruption recovery, migration, maintenance | ✅ Complete |
| 6 | Daemon & File Watching | Background indexing, incremental updates, git hooks | 🔄 In progress |
| 7 | Local AI Embeddings | Candle + UniXcoder, vector DB, hybrid search | ⬜ Not started |
| 8 | Cross-Repo & Storage Opt. | Multi-repo registry, compression, deduplication | ⬜ Not started |
| 9 | Cloud AI & Security | Voyage/OpenAI APIs, keychain, rate limiting | ⬜ Not started |

---

## Phase 1: Foundation

### Goal
Establish the core Rust project structure, data models, and tree-sitter code parsing pipeline. No search yet — just the ability to parse a codebase into structured records.

### Deliverables
- `Cargo.toml` workspace with feature flags (`default`, `local-models`, `cloud-models`, `full`)
- Core data types: `CodeUnit`, `FileRecord`, `CallEdge`, `IndexMetadata`
- SQLite schema initialized on `codesearch index` (WAL mode, all tables and indexes from spec)
- Tree-sitter integration parsing Python, TypeScript, Rust, Go, Java into `CodeUnit` records
- File hash tracking per file in `file_index` table
- Directory walker (`walkdir`) with smart exclusion filters (node_modules, .git, target, dist, build)
- `.codesearch/` directory creation and `metadata.json` with version + checksum
- `codesearch index` command: walks repo, parses files, stores code units in SQLite
- `codesearch index --verbose` shows files parsed, functions found, time taken
- Async file parsing via Tokio thread pool with **10-second timeout per file** and regex fallback
- Skip files >1 MB or >10,000 lines (per TECHNICAL_DEEP_DIVE.md)

### Key Libraries
- `clap 4.5` — CLI
- `tokio 1.36` — async runtime
- `rusqlite` (bundled, WAL mode) — metadata
- `tree-sitter` + language grammars — parsing
- `walkdir 2.4` — directory traversal
- `sha2 0.10` — file hashing
- `anyhow 1.0` — error handling
- `serde / serde_json` — serialization

### Testing Guidelines
- Unit test each language parser: given a source file fixture, assert the correct `CodeUnit` records are extracted (name, line numbers, signature, type)
- Integration test: run `codesearch index` on a small fixture repository (~50 files), assert SQLite row counts match expected functions
- Test async timeout: provide a deliberately huge/malformed file, assert parsing doesn't hang and falls back gracefully
- Test smart exclusion: assert `node_modules/` and `.git/` directories are never parsed

### Success Criteria
- `codesearch index` completes on a 1,000-file repo without panics or hangs
- SQLite contains correct `code_units` rows with accurate `file_path`, `name`, `line_number`, `unit_type`
- Indexing speed: <1 second per 1,000 functions parsed
- Zero files from excluded directories appear in the index

> **Phase transition:** Mark Phase 1 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 2.

---

## Phase 2: Fast Search (BM25)

### Goal
Implement BM25 full-text search via Tantivy so that `codesearch <query>` returns relevant results from the parsed index. This is the "fast mode" baseline — no AI, no embeddings.

### Deliverables
- Tantivy index stored at `.codesearch/tantivy/` with schema matching `CodeUnit` fields
- Tantivy index populated during `codesearch index`
- `codesearch <query>` command: queries Tantivy, returns top-N results with file path, line number, function name, and a snippet
- `codesearch <query> --fast` explicit flag (fast mode)
- `--limit N` flag (default 10)
- Plain text output with syntax-highlighted snippet (syntect, basic)
- Incremental reindex: re-parse only files whose `file_hash` has changed since last index

### Key Libraries
- `tantivy 0.21` — BM25
- `syntect 5.2` — syntax highlighting

### Testing Guidelines
- Search accuracy test: index a known codebase (e.g. small Rust project), search for function names that exist, assert they appear in top-3 results
- Incremental update test: modify one file, reindex, assert the modified function appears with updated content while unchanged files are skipped
- Relevance test: search for a query with no matches, assert graceful "no results" output
- Performance test: index 10,000 functions, measure `codesearch <query>` latency — must be <100ms

### Success Criteria
- Search returns correct results for exact function name queries
- Incremental reindex skips unchanged files (verify with parse timing logs)
- Search latency: <100ms on an index of 10,000 functions
- No panics on queries with special characters or very long queries

> **Phase transition:** Mark Phase 2 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 3.

---

## Phase 3: Smart Search & Filters

### Goal
Improve search quality with call graph analysis, import tracking, advanced filters, and fusion ranking (RRF). This corresponds to "smart mode" in the spec.

### Deliverables
- Call graph extraction: populate `call_graph` table during indexing (`caller_id → callee_name`)
- Import/dependency tracking stored per `CodeUnit`
- `--show-context` flag: display callers and callees of a result
- Filter flags: `--lang <language>`, `--path <subpath>`, `--modified-last <Nd>`, `--exclude-tests`
- `--find-similar <file:line>` command: find functions similar to a given one (structural similarity via BM25 on AST features)
- Reciprocal Rank Fusion (RRF) combining name match, BM25 score, and call-graph relevance
- `codesearch report --unused-functions`: detect functions with no callers
- Fuzzy matching for typos (Levenshtein distance)

### Key Libraries
- `rayon 1.8` — parallel rank fusion computation

### Testing Guidelines
- Call graph test: index a fixture where function A calls B calls C; assert `call_graph` table correctly records edges
- Filter test: index a mixed Python/Rust fixture; `--lang python` returns only Python results
- Path filter test: `--path src/models` returns only results from that path
- Unused function test: assert `--unused-functions` correctly identifies functions with zero callers
- RRF test: create a fixture where the top BM25 result is wrong but call-graph context makes the correct result rank higher; assert RRF improves order

### Success Criteria
- Call graph correctly extracted for all 5 supported languages
- All filter flags work and reduce result set appropriately
- `--show-context` displays accurate callers/callees
- Fusion ranking improves precision over raw BM25 on a curated test set (manual evaluation)

> **Phase transition:** Mark Phase 3 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 4.

---

## Phase 4: TUI & UX Polish

### Goal
Replace plain-text output with an interactive Ratatui TUI. Add export formats and improve the overall developer experience.

### Deliverables
- Ratatui TUI: scrollable result list, preview pane with syntax-highlighted code, keyboard navigation (j/k, enter to expand, q to quit)
- Non-interactive fallback when stdout is not a TTY (for piping)
- `--format json` and `--format csv` export flags
- `codesearch <query>` without flags defaults to TUI when in a terminal, plain text when piped
- Progress bar during `codesearch index` with ETA (number of files remaining)
- `--no-tui` flag for always plain text
- Color themes respecting terminal color scheme
- C++ language support added to tree-sitter parsing

### Key Libraries
- `ratatui 0.26` — TUI
- `crossterm 0.27` — terminal control

### Testing Guidelines
- Smoke test TUI renders without panicking on various terminal sizes (80x24, 120x40, 40x10)
- JSON output test: `codesearch "query" --format json` produces valid JSON matching the result schema
- CSV output test: output can be opened in a spreadsheet (correct columns, no encoding issues)
- TTY detection test: verify plain-text fallback when stdout is piped
- Progress display test: verify progress bar appears and updates during long indexing

### Success Criteria
- TUI navigable with keyboard, no visual artifacts
- JSON output schema is stable and documented
- `--format json` output can be piped to `jq` for further filtering
- Progress bar shows accurate ETA during indexing

> **Phase transition:** Mark Phase 4 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 5.

---

## Phase 5: Production Hardening

### Goal
Make the index safe for concurrent access, resilient to corruption, and upgradeable across versions. This phase has no user-visible features but is critical before daemon mode.

### Deliverables
- File-based locking: `index.lock` with shared locks for readers, exclusive lock for writers
- SQLite WAL mode enabled + all writes in transactions
- Index checksum validation on open: compare stored checksum in `metadata.json` vs computed hash
- Automatic backup: copy `metadata.db` to `.codesearch.backup/` before any write batch
- Graceful fallback: if primary index is corrupted, fall back to backup with warning
- `codesearch rebuild`: delete and regenerate entire index from scratch
- `codesearch optimize`: runs `VACUUM`, `ANALYZE`, deletes orphaned embeddings, rotates logs
- `codesearch cleanup`: reclaims disk space from deleted functions
- Version migration system: `metadata.json` stores index version, migration functions run on version mismatch
- On startup: validate index version compatibility, auto-migrate minor versions, error clearly for major version incompatibility with instructions to rebuild

### Testing Guidelines
- Concurrent access test: spawn two processes simultaneously (one reading, one writing), assert no data corruption
- Corruption recovery test: manually corrupt `metadata.db` (truncate it), run a search command, assert it falls back to backup or gives a clear rebuild instruction
- Migration test: create a v1 format index fixture, open it with v2 code, assert migration runs and index is valid
- Checksum test: modify a byte in the index after writing, assert startup detects the mismatch
- Optimize test: run `codesearch optimize` and verify SQLite page count decreases on a fragmented index

### Success Criteria
- No data corruption occurs under simulated concurrent access (50 concurrent reads + 1 write)
- Corrupted index triggers clear error with recovery instructions, does not panic
- Migrations run automatically and silently on minor version bumps
- `codesearch optimize` measurably reduces disk usage on a fragmented index

> **Phase transition:** Mark Phase 5 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 6.

---

## Phase 6: Daemon & File Watching

### Goal
Enable background indexing with a daemon that watches for file changes and incrementally updates the index. Includes git hooks integration.

### Deliverables
- File watching strategy (in priority order):
  1. Git-based: watch `.git/index` (single file, works on monorepos with millions of files)
  2. Native notify: OS file events (fallback, limited to repos <10k files)
  3. Polling: hash-based every 5 seconds (last resort)
- `codesearch daemon start`: spawn background process, write PID file
- `codesearch daemon stop`: send SIGTERM to daemon
- `codesearch daemon status`: show daemon PID, uptime, last update time, files queued
- Incremental update pipeline on change: parse file → diff old vs new code units → update SQLite transaction → update Tantivy → (optional) queue embedding generation
- `codesearch install-hooks`: install post-commit, post-merge, post-checkout git hooks that trigger incremental reindex
- `codesearch update --batch`: manual batch update (all changed files since last index)
- Smart exclusion filter applied to watched paths (same exclusions as Phase 1)

### Key Libraries
- `notify 6.1` — native file watching
- `tokio` — async daemon event loop

### Testing Guidelines
- Git watcher test: make a git commit in a test repo, assert daemon picks up the change and updates the index within 5 seconds
- Fallback test: simulate inotify limit exhaustion, assert graceful fallback to polling
- Incremental update test: modify one function in a file, assert only that file is re-parsed (not full reindex)
- Daemon lifecycle test: start daemon, assert PID file exists; stop daemon, assert PID file removed
- Git hooks test: run `codesearch install-hooks`, make a commit, assert index is updated

### Success Criteria
- Incremental update triggered within 5 seconds of a file change
- Daemon handles a 50,000-file repo without exceeding OS inotify limits
- Incremental update: <5 seconds per changed file
- Git hooks trigger index updates automatically on commit/merge/checkout

> **Phase transition:** Mark Phase 6 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 7.

---

## Phase 7: Local AI Embeddings

### Goal
Add semantic search via local ML models (UniXcoder running in Candle). Implement hybrid search combining all three backends with RRF, and compress vectors with Product Quantization.

### Deliverables
- Candle ML framework integration with UniXcoder model
- `codesearch index --download-model`: download UniXcoder (~350 MB) to `~/.config/codesearch/models/`
- Embedding generation with batch size 64 and parallel batch processing (Rayon)
- **Lazy embedding strategy**: AST + BM25 index built first, embeddings generated in background and on-demand
- Vector storage: Qdrant or custom implementation with:
  - Product Quantization (768-dim → 96 bytes = 32x compression)
  - f32 → u8 scalar quantization (4x reduction)
  - ZSTD compression (level 3)
  - mmap-based disk access (memory-mapped, no full load into RAM)
  - IVF index for fast approximate nearest-neighbor search
  - LRU hot cache (~100 MB)
- `codesearch <query> --semantic`: vector search via embeddings
- `codesearch <query>` (default): hybrid search = AST + BM25 + Embeddings fused via RRF
- `codesearch <query> --best`: same as hybrid with highest quality settings
- Background progress bar during initial embedding generation showing ETA
- `has_embedding` and `embedding_model` fields in `code_units` table tracked correctly

### Key Libraries
- `candle-core`, `candle-nn`, `candle-transformers 0.4`
- `tokenizers 0.15`
- `zstd 0.13`
- `memmap2 0.9`
- `rayon 1.8`

### Testing Guidelines
- Semantic search test: index a codebase, search for "function that handles user authentication" — assert auth-related functions rank in top 5
- Memory test: load embeddings for 10,000 functions, assert RAM usage stays <500 MB
- Storage test: verify 10,000 functions use <20 MB on disk after PQ + compression
- Batch embedding test: measure time for 10,000 embeddings with batch size 64 + parallel — must be <10 minutes
- Lazy embedding test: assert `codesearch index` completes fast (<30s for 1k files), embeddings generated in background
- RRF fusion test: create a case where BM25 ranks wrong answer first but embeddings rank correct answer first — assert hybrid search returns correct answer

### Success Criteria
- Semantic search finds conceptually related functions not reachable by keyword
- Hybrid search latency: <500ms on 10,000 functions
- Memory usage: <500 MB RAM during search
- Storage: <20 MB per 10,000 functions (including vectors)
- Initial embedding generation: <10 minutes for 10,000 functions (with batching + parallelism)

> **Phase transition:** Mark Phase 7 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 8.

---

## Phase 8: Cross-Repo & Storage Optimization

### Goal
Enable search across multiple registered repositories. Add code similarity detection, deduplication, and full export/reporting capabilities.

### Deliverables
- `codesearch repos add <path>`: register a repo in `~/.config/codesearch/repos.json`
- `codesearch repos list`: show all registered repos with status
- `codesearch repos remove <name>`: unregister a repo
- `codesearch <query> --all-repos`: federated search across all registered repos
- `codesearch <query> --repos backend,frontend`: search specific repos
- Results display repo name alongside file path
- Cross-repo deduplication: vectors from identical code units shared (SHA2 content hash → reuse embedding)
- `codesearch --find-similar <file:line>`: cross-repo similarity detection
- `codesearch report --unused-functions`: report functions with no callers (per-repo or cross-repo)
- `codesearch <query> --format json > results.json` and `--format csv` work for all search modes

### Testing Guidelines
- Multi-repo test: register 3 repos, search across all with `--all-repos`, assert results from all 3 appear
- Repo filter test: `--repos backend` returns only results from the backend repo
- Deduplication test: index two repos with shared utility code, assert shared functions use the same embedding (no duplicate storage)
- Similarity test: `--find-similar` returns functions structurally similar to the given function, across repos
- Export test: `--format json` output for multi-repo search includes a `repo` field per result

### Success Criteria
- Cross-repo search works across 10+ registered repos in <500ms (fast mode)
- Deduplication measurably reduces storage (>20% reduction on repos with shared code)
- `--find-similar` identifies similar utility functions across repos
- All export formats work correctly for cross-repo results

> **Phase transition:** Mark Phase 8 complete in `IMPLEMENTATION_PLAN.md` and `CLAUDE.md`, then commit before starting Phase 9.

---

## Phase 9: Cloud AI & Security

### Goal
Add optional cloud embedding APIs (Voyage-code-3, OpenAI) with secure API key management, rate limiting, cost tracking, and privacy-first defaults.

### Deliverables
- Voyage-code-3 API integration: batch requests up to API limit
- OpenAI text-embedding-3-large API integration
- Rate limiting via `governor` crate (1,000 req/min for Voyage)
- Exponential backoff with 5 retries on 429 errors
- `codesearch config --set-api-key voyage <KEY>`: stores key in OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service via `keyring` crate)
- `codesearch config --model voyage-code-3`: switch embedding model
- Local caching of cloud embeddings (avoid re-calling API for unchanged code)
- Cost estimation: show estimated API cost before running cloud indexing
- Privacy defaults: `--cloud` flag required to use cloud APIs; explicit opt-in with warning message
- API key never written to disk in plaintext
- Cross-platform static binary builds via GitHub Actions matrix:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
  - `x86_64-pc-windows-msvc`

### Key Libraries
- `reqwest 0.11` (with `json` feature)
- `governor` — rate limiting
- `keyring` — OS keychain access

### Testing Guidelines
- API integration test (with mock server): assert correct batch request format, handle 429 with retry
- Rate limit test: simulate rapid requests, assert governor throttles to stay under 1000/min
- Keychain test: store and retrieve API key via `keyring`, assert it is not written to any config file
- Opt-in test: assert `codesearch <query> --semantic` without `--cloud` uses local model, not API
- Cost display test: assert cost estimate is shown before any cloud API call
- CI matrix test: verify GitHub Actions builds produce working binaries for all 5 targets (smoke test each binary)

### Success Criteria
- Cloud APIs produce higher-quality search results than local model (measurable on benchmark set)
- API key never appears in any file on disk
- Rate limiting prevents API quota violations
- Privacy: zero network requests without explicit `--cloud` flag
- All 5 platform binaries produce working search results on their respective platforms

---

## Language Rationale

Rust was chosen over Python and Go for the following non-negotiable reasons:
- **Single binary distribution**: critical for a CLI tool — users download one file, no runtime deps
- **Memory safety without GC**: no crashes or pauses in a long-running daemon
- **Concurrency**: Tokio async + Rayon parallelism needed for <100ms search across multiple backends
- **ML inference**: Candle (pure Rust) enables local model inference without Python FFI

Development speed concern (addressed): Phases 1–4 are straightforward Rust with no complex lifetimes. Use `cargo check` (not `cargo build`) for fast feedback loops. `cargo watch -x check` for continuous checking. The detailed spec means no design uncertainty slowing implementation.

---

## Cross-Cutting Concerns (Apply to All Phases)

### Error Handling Standards
- All errors use `anyhow::Result` with `.context("...")` for clear messages
- User-facing errors include a recovery suggestion (what to run next)
- Internal panics are not acceptable in any user-facing path

### Code Organization
```
src/
├── main.rs              # CLI entry point (clap)
├── cli/                 # Command handlers
├── index/               # Tree-sitter parsing, indexing pipeline
├── search/              # BM25, vector, hybrid search, RRF
├── storage/             # SQLite, Tantivy, vector DB adapters
├── watch/               # File watcher strategies
├── ml/                  # Candle embeddings, model management
├── api/                 # Cloud API clients (Voyage, OpenAI)
├── tui/                 # Ratatui components
└── types.rs             # Core data types shared across modules
```

### Performance Invariants
- Search must never block on embedding generation
- Indexing must run in background threads, not blocking CLI
- All file I/O must go through the exclusion filter

### CLAUDE.md Updates
After each phase is completed, update `CLAUDE.md` to:
1. Mark the phase as complete in the Phase Overview table
2. Add a "Current Phase" line pointing to the next phase
3. List any architectural decisions made during the phase that future Claude instances should know

---

## Files to Create/Modify

### Phase 1
- `Cargo.toml` (new)
- `src/main.rs` (new)
- `src/types.rs` (new)
- `src/storage/sqlite.rs` (new)
- `src/index/parser.rs` (new)
- `src/index/walker.rs` (new)
- `src/cli/index.rs` (new)

### Phase 2
- `src/storage/tantivy.rs` (new)
- `src/search/bm25.rs` (new)
- `src/cli/search.rs` (new)

### Phase 3
- `src/index/call_graph.rs` (new)
- `src/search/rrf.rs` (new)
- `src/cli/filters.rs` (new)

### Phase 4
- `src/tui/mod.rs` (new)
- `src/tui/results.rs` (new)
- `src/tui/preview.rs` (new)

### Phase 5
- `src/storage/lock.rs` (new)
- `src/storage/backup.rs` (new)
- `src/storage/migration.rs` (new)
- `src/cli/maintenance.rs` (new)

### Phase 6
- `src/watch/git.rs` (new)
- `src/watch/native.rs` (new)
- `src/watch/polling.rs` (new)
- `src/watch/daemon.rs` (new)

### Phase 7
- `src/ml/model.rs` (new)
- `src/ml/embeddings.rs` (new)
- `src/storage/vectors.rs` (new)
- `src/search/hybrid.rs` (new)

### Phase 8
- `src/repo/registry.rs` (new)
- `src/search/cross_repo.rs` (new)
- `src/cli/repos.rs` (new)

### Phase 9
- `src/api/voyage.rs` (new)
- `src/api/openai.rs` (new)
- `src/api/cache.rs` (new)
- `src/cli/config.rs` (new)
- `.github/workflows/release.yml` (new)
