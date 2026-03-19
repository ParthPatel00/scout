# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

Scout (CodeSearch) is a semantic code search CLI tool currently in the **specification/design phase**. No implementation code exists yet. The two specification documents are the authoritative source of truth for all design decisions.

## Specification Documents

- **`codesearch-spec.md`** — Main product spec: features, API design, CLI interface, data models, tech stack, implementation roadmap
- **`TECHNICAL_DEEP_DIVE.md`** — Production architecture: identified failure modes and their solutions (must-read before implementation)

## Planned Tech Stack (Rust)

**Core runtime:** Tokio (async), Rayon (parallelism)
**Parsing:** Tree-sitter with grammars for Python, TypeScript, Rust, Go, Java, C/C++
**Search backends:**
- Tantivy — BM25 full-text index
- SQLite (rusqlite + WAL mode) — metadata, call graphs
- Qdrant or custom — vector embeddings
- ZSTD — compression

**ML:** Candle (pure Rust) running UniXcoder/GraphCodeBERT/StarEncoder locally; optional Voyage/OpenAI APIs
**CLI/TUI:** clap (CLI), Ratatui + Crossterm (TUI), Syntect (syntax highlighting)
**Infrastructure:** notify (file watching), governor (rate limiting), keyring (secure API key storage)

## Build Commands (once implementation begins)

```bash
cargo build --release    # Production binary
cargo build              # Debug build
cargo test               # All tests
cargo test <test_name>   # Single test
cargo bench              # Performance benchmarks
cargo clippy             # Lint
cargo fmt                # Format
```

## Architecture

### Three-Tier Hybrid Search

Search runs three backends in parallel, results fused via Reciprocal Rank Fusion (RRF):
1. **Fast mode** (always free): AST parsing + BM25 (Tantivy) — zero-cost baseline
2. **Smart mode**: Local AI embeddings (one-time model download ~350–500 MB)
3. **Cloud mode** (opt-in): Voyage-code-3 or OpenAI APIs

Graceful degradation: works fully offline at tier 1.

### Index Storage Layout

Per-repo index stored in `.codesearch/` at repo root:
```
.codesearch/
├── metadata.db     # SQLite: file metadata, function definitions, call graphs
├── vectors.db      # Qdrant: embedding vectors (quantized)
├── tantivy/        # BM25 index shards
├── metadata.json   # Index version + checksum
└── index.lock      # Concurrency file lock
```

Global config at `~/.config/codesearch/`:
```
config.toml         # Global settings
models/             # Downloaded ML models
repos.json          # Cross-repo registry
cache/              # Query cache, API response cache
```

### File Watching Strategy (priority order)

1. Git-based watching (`.git/index`) — primary, handles large repos
2. Native OS events (notify crate) — fallback
3. Polling (5s interval) — last resort

### Storage Optimization

Vector quantization pipeline: f32 → u8 (4x) → product quantization (32x) → sparse (2x) → ZSTD (3x) = ~136x total reduction.

### Key Production Concerns (from TECHNICAL_DEEP_DIVE.md)

- Vector DB memory: use IVF + PQ + mmap, not in-memory loading
- Tree-sitter: must run with async + timeout (can hang on large/malformed files)
- Embedding generation: batch aggressively, use lazy embedding (index AST first, embed on demand)
- Concurrency: SQLite WAL mode + file locks for concurrent access
- Index corruption: checksums + auto-backup + rebuild capability
- Cross-platform: static linking required for distribution; CI matrix across Linux/macOS/Windows

## Planned CLI Interface

```bash
codesearch index                          # Build index for current repo
codesearch <query>                        # Default search
codesearch --semantic <query>             # Force semantic search
codesearch repos add/list/remove          # Multi-repo management
codesearch daemon start/stop/status       # Background indexing daemon
codesearch config --set-api-key <key>     # Secure API key storage
codesearch optimize/cleanup/rebuild       # Index maintenance
```

## Implementation Plan

**Current Phase: Phase 6 — Daemon & File Watching**

Full plan with testing guidelines and success criteria: `IMPLEMENTATION_PLAN.md`

| Phase | Name | Focus | Status |
|-------|------|--------|--------|
| 1 | Foundation | Project setup, data models, tree-sitter parsing | ✅ Complete |
| 2 | Fast Search | Tantivy BM25 + basic CLI | ✅ Complete |
| 3 | Smart Search | Call graphs, filters, RRF fusion ranking | ✅ Complete |
| 4 | TUI & UX | Ratatui interface, syntax highlighting, export formats | ✅ Complete |
| 5 | Production Hardening | Concurrency, corruption recovery, migration | ✅ Complete |
| 6 | Daemon & File Watching | Background indexing, incremental updates, git hooks | 🔄 In progress |
| 7 | Local AI Embeddings | Candle + UniXcoder, vector DB (IVF+PQ), hybrid search | ⬜ Not started |
| 8 | Cross-Repo & Storage | Multi-repo registry, deduplication, compression | ⬜ Not started |
| 9 | Cloud AI & Security | Voyage/OpenAI APIs, OS keychain, rate limiting, CI matrix | ⬜ Not started |

### Planned Source Layout
```
src/
├── main.rs          # CLI entry point (clap)
├── types.rs         # Core data types shared across modules
├── cli/             # Command handlers per subcommand
├── index/           # Tree-sitter parsing, indexing pipeline
├── search/          # BM25, vector, hybrid search, RRF
├── storage/         # SQLite, Tantivy, vector DB adapters
├── watch/           # File watcher strategies (git/native/polling)
├── ml/              # Candle embeddings, model management
├── api/             # Cloud API clients (Voyage, OpenAI)
└── tui/             # Ratatui components
```

### After Each Phase
Update the Status column above and add a "Current Phase" note pointing to the next phase.
