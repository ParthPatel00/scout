<div align="center">

# Scout

**Semantic code search for your codebase.**<br>
Type what code *does*. Land at the exact line in your editor.

<br>

![Python](https://img.shields.io/badge/Python-3776AB?style=flat-square&logo=python&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white)
![Go](https://img.shields.io/badge/Go-00ADD8?style=flat-square&logo=go&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-3178C6?style=flat-square&logo=typescript&logoColor=white)
![JavaScript](https://img.shields.io/badge/JavaScript-F7DF1E?style=flat-square&logo=javascript&logoColor=black)
![Java](https://img.shields.io/badge/Java-ED8B00?style=flat-square&logo=openjdk&logoColor=white)
![C](https://img.shields.io/badge/C-A8B9CC?style=flat-square&logo=c&logoColor=black)
![C++](https://img.shields.io/badge/C++-00599C?style=flat-square&logo=cplusplus&logoColor=white)

<br>

[![Rust](https://img.shields.io/badge/built_with-Rust-orange?style=flat-square)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.1.10-blue?style=flat-square)](https://github.com/ParthPatel00/scout/releases)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)]()

<br>

![macOS Apple Silicon](https://img.shields.io/badge/macOS-Apple_Silicon-black?style=flat-square&logo=apple&logoColor=white)
![macOS Intel](https://img.shields.io/badge/macOS-Intel-black?style=flat-square&logo=apple&logoColor=white)
![Linux x86](https://img.shields.io/badge/Linux-x86__64-FCC624?style=flat-square&logo=linux&logoColor=black)
![Linux ARM64](https://img.shields.io/badge/Linux-ARM64-FCC624?style=flat-square&logo=linux&logoColor=black)
![Windows](https://img.shields.io/badge/Windows-x86__64-0078D4?style=flat-square&logo=windows&logoColor=white)

<br>

**No cloud &nbsp;·&nbsp; No API keys &nbsp;·&nbsp; No Python runtime &nbsp;·&nbsp; Single binary**

</div>

<br>

```bash
scout "stripe payment retry logic"
scout "where do we handle rate limiting"
scout "function that validates JWT tokens"
```

---

## What Scout is

Scout is a code search CLI that understands **what code does**, not just what it says. It parses every function, method, and class in your codebase using AST analysis, indexes them with BM25 full-text search, and optionally layers in local AI vector embeddings. Type what you're looking for in plain English and land at the exact line in your editor in under a second.

**Everything runs on your machine.** No server, no account, no internet connection required.

### Core capabilities

- **Semantic search** — finds `check_token()` when you ask for "validate JWT", even with zero keyword overlap. Powered by a local UniXcoder model (~500 MB, one-time download, fully optional).
- **AST-aware indexing** — tree-sitter parses 8 languages into functions, methods, and classes. Results are code units, not line matches.
- **Call graph context** — see what calls a function and what it calls with `--show-context`.
- **Interactive TUI** — browse results with syntax-highlighted previews, navigate with `j`/`k`, press `Enter` to jump to that exact line in your editor.
- **Direct editor integration** — opens VS Code, Cursor, Neovim, Zed, Helix, nano, or any terminal editor at the right line. Detects your editor automatically.
- **Cross-repo search** — register multiple codebases and search all of them at once with `--all-repos`.
- **Language and path filters** — narrow results with `--lang python`, `--path-filter services/payments`, or both.
- **Background daemon and git hooks** — keep the index current automatically without ever thinking about it.
- **Incremental indexing** — re-indexes only changed files. A 10,000-function repo updates in under 2 seconds.
- **Scriptable output** — `--format json` or `--format csv` for piping into other tools.
- **Shell completions** — Zsh, Bash, and Fish, installable with one command.

### How fast

BM25 search returns in **under 10ms**. Hybrid search across 50,000 functions runs in ~30ms. First index of a 10,000-function repo takes ~8 seconds; every run after that skips unchanged files.

---

## The problem

Every developer knows this: you're dropped into a codebase and need to find something. You know *what* it does. You don't know *what it's called*.

`grep` and `ripgrep` are fast, but they find text — not intent. GitHub search requires your code to be on GitHub. AI assistants hallucinate file names. So you end up clicking through folders for 20 minutes.

**Scout fixes this.** It parses your code into functions and classes using tree-sitter AST analysis, indexes them with BM25 full-text search, and optionally fuses in local AI vector embeddings. Type what you're looking for in plain English and get results in under 10ms.

---

## What it looks like

```
$ scout "authentication with stripe"

services/payments/processor.py:310  _map_stripe_status    function · python
  def _map_stripe_status(self, stripe_status:

services/auth/service.py:110        validate_token        function · python
  def validate_token(self, token:

gateway/main.go:149                 Authenticate          method · go
  func (m *AuthMiddleware) Authenticate(next http.Handler)

services/payments/processor.py:206  capture_payment       function · python
  def capture_payment(self, payment_intent_id:
```

In a terminal, Scout launches an interactive TUI. Navigate with `j`/`k`, press `Enter` to jump to that exact line in your editor:

```
┌─ Results (10) ────────────────────────┐┌─ Preview ──────────────────────────────────────────────────────┐
│  1. function  validate_token          ││ services/auth/service.py:110                          [94.3]   │
│  2. class     AuthService             ││ ─────────────────────────────────────────────────────────────  │
│  3. method    Authenticate            ││ def validate_token(self, token: str) -> Optional[TokenPayload]:│
│  4. function  setup_mfa               ││     """Validate a JWT token and return its payload."""         │
│  5. function  change_password         ││     try:                                                       │
│  6. function  validate_api_key        ││         payload = jwt.decode(                                  │
│  7. function  login                   ││             token,                                             │
│  8. function  logout                  ││             self.secret_key,                                   │
│  9. class     AuthenticationError     ││             algorithms=["HS256"]                               │
│ 10. function  _validate_password_str… ││         )                                                      │
└───────────────────────────────────────┘└────────────────────────────────────────────────────────────────┘
 j/k: navigate   Enter: open in editor   o: open (stay)   d/u: scroll   q: quit
```

---

## How it works

Scout runs three search strategies in parallel and fuses the results:

```
Your query
    │
    ├── BM25 full-text (Tantivy)   ─────────────────────┐
    │   Always on. <10ms.                                │
    │                                                    ▼
    ├── Name-match re-ranker ──────────────► Reciprocal Rank Fusion ──► ranked results
    │   Always on.                                       ▲
    │                                                    │
    └── AI vector search (UniXcoder) ───────────────────┘
        Optional. One-time ~500 MB model download.
```

The index lives in `.scout/` at your repo root. Tree-sitter parses each file into functions, methods, and classes. Tantivy handles BM25 indexing. SQLite stores the call graph. Vector embeddings are quantized with IVF+PQ for minimal disk usage.

---

## Why Scout beats the alternatives

| | `grep` / `ripgrep` | GitHub Search | Sourcegraph | **Scout** |
|---|:---:|:---:|:---:|:---:|
| Finds code by what it *does* | No | Partial | Yes | **Yes** |
| Works offline | Yes | No | No | **Yes** |
| Private repos | Yes | Yes | Paid | **Yes** |
| No infrastructure needed | Yes | Yes | No | **Yes** |
| Call graph context | No | No | Yes | **Yes** |
| Cross-repo search | No | No | Yes | **Yes** |
| Single binary, no runtime | Yes | No | No | **Yes** |
| Query time | <1ms | Varies | ~100ms | **<10ms** |

---

## Install

### Pre-built binary (recommended)

**macOS (Apple Silicon)**
```bash
curl -L https://github.com/ParthPatel00/scout/releases/latest/download/scout-aarch64-apple-darwin.tar.gz | tar xz
sudo mv scout /usr/local/bin/
```

**macOS (Intel)**
```bash
curl -L https://github.com/ParthPatel00/scout/releases/latest/download/scout-x86_64-apple-darwin.tar.gz | tar xz
sudo mv scout /usr/local/bin/
```

**Linux (x86_64)**
```bash
curl -L https://github.com/ParthPatel00/scout/releases/latest/download/scout-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv scout /usr/local/bin/
```

**Linux (ARM64)**
```bash
curl -L https://github.com/ParthPatel00/scout/releases/latest/download/scout-aarch64-unknown-linux-gnu.tar.gz | tar xz
sudo mv scout /usr/local/bin/
```

**Windows** — download `scout-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/ParthPatel00/scout/releases), extract, and add to PATH.

### Build from source

```bash
git clone https://github.com/ParthPatel00/scout
cd scout
cargo build --release
sudo cp target/release/scout /usr/local/bin/
```

---

## Quick start

```bash
scout init      # one-time setup: indexes your repo, sets preferences, installs completions
scout "query"   # search
```

`scout init` is an 8-question wizard that sets everything up permanently. Run it once and never think about configuration again.

---

## Searching

```bash
# Natural language
scout "where do we handle payment failures"
scout "function that sends email notifications"
scout "retry logic with exponential backoff"

# Exact name
scout "AuthService"
scout "process_payment"

# Filter by language
scout "error handling" --lang python
scout "middleware" --lang go
scout "interface" --lang typescript

# Filter by path
scout "rate limit" --path-filter gateway
scout "refund" --path-filter services/payments

# Show call graph — who calls this, what does it call
scout "validate_token" --show-context

# Cross-repo: search multiple codebases at once
scout "user session" --all-repos
scout "rate limit" --repos backend,shared

# Force semantic (vector-only) search
scout "functions that expire stale sessions" --semantic

# Find functions similar to a specific one
scout search --find-similar services/auth/service.py:110

# JSON output for scripting
scout "payment" --format json | jq -r '.[].file_path' | sort -u
```

---

## Cross-repo search

Register repos once, then search all of them together:

```bash
scout repos add backend   ~/projects/backend
scout repos add frontend  ~/projects/frontend
scout repos add shared    ~/projects/shared-libs

scout "authentication" --all-repos
```

```
[backend]  gateway/main.go:149           Authenticate    method · go
[backend]  services/auth/service.py:35   AuthService     class · python
[frontend] src/hooks/useAuth.ts:12       useAuth         function · typescript
[shared]   lib/auth/validator.py:88      validate_token  function · python
```

---

## Keeping the index current

### Manual
```bash
scout index    # incremental — only re-parses changed files
```

### Background daemon
```bash
scout daemon start     # watches for file saves, updates index automatically
scout daemon status
scout daemon stop
```

### Git hooks (set and forget)
```bash
scout daemon install-hooks
# Installs post-commit, post-merge, post-checkout hooks
```

A 10,000-file codebase re-indexes in under 2 seconds on subsequent runs.

---

## Editor integration

Press `Enter` on any TUI result to open the file at the exact line in your editor. Press `o` to open without leaving the TUI.

Scout defaults to a terminal editor that is guaranteed to work (`nano`, `nvim`, or `vi` — whichever is found first). GUI editors (VS Code, Cursor, Zed) are opt-in: set them once with `scout init` or any time with:

```bash
scout config set editor.command nvim
scout config set editor.command cursor
scout config set editor.command code
```

On macOS, once set, Scout finds VS Code and Cursor through their `.app` bundles even if the CLI is not in your PATH.

Detection priority: `editor.command` config > `$SCOUT_EDITOR` > `$VISUAL` > `$EDITOR` > terminal editor from PATH > `vi` / `notepad`.

---

## Configuration

```bash
scout config list                           # view all settings
scout config set search.limit 20
scout config set search.no_tui true         # always plain text
scout config set search.exclude_tests true  # hide test files
scout config set index.auto_index true      # auto-index on first search
scout config edit                           # open config in editor
```

---

## Index maintenance

```bash
scout cleanup   # remove entries for deleted files
scout optimize  # compact database, reclaim disk space
scout rebuild   # wipe and regenerate from scratch
scout stats     # unit counts, languages, embeddings, disk usage
```

---

## Performance

Benchmarked on an M2 MacBook Pro against a 10,000-function codebase:

| Operation | Time |
|---|---|
| First index (10k functions) | ~8s |
| Incremental index (1 changed file) | <200ms |
| BM25 search | **<10ms** |
| Hybrid search (50k functions) | ~30ms |

---

## Supported languages

| Language | Functions | Methods | Classes | Call graph |
|:---|:---:|:---:|:---:|:---:|
| ![Python](https://img.shields.io/badge/Python-3776AB?style=flat-square&logo=python&logoColor=white) | ✓ | ✓ | ✓ | ✓ |
| ![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white) | ✓ | ✓ | ✓ | ✓ |
| ![Go](https://img.shields.io/badge/Go-00ADD8?style=flat-square&logo=go&logoColor=white) | ✓ | ✓ | ✓ | ✓ |
| ![TypeScript](https://img.shields.io/badge/TypeScript-3178C6?style=flat-square&logo=typescript&logoColor=white) | ✓ | ✓ | ✓ | ✓ |
| ![JavaScript](https://img.shields.io/badge/JavaScript-F7DF1E?style=flat-square&logo=javascript&logoColor=black) | ✓ | ✓ | ✓ | ✓ |
| ![Java](https://img.shields.io/badge/Java-ED8B00?style=flat-square&logo=openjdk&logoColor=white) | ✓ | ✓ | ✓ | ✓ |
| ![C](https://img.shields.io/badge/C-A8B9CC?style=flat-square&logo=c&logoColor=black) | ✓ | ✓ | ✓ | Partial |
| ![C++](https://img.shields.io/badge/C++-00599C?style=flat-square&logo=cplusplus&logoColor=white) | ✓ | ✓ | ✓ | Partial |

---

## Tech stack

Built in Rust for performance and a single distributable binary.

| Layer | Technology |
|---|---|
| CLI | clap |
| TUI | Ratatui + Crossterm |
| Code parsing | Tree-sitter (8 language grammars) |
| Full-text index | Tantivy (BM25) |
| Metadata + call graph | SQLite (WAL mode) |
| AI embeddings | Candle (pure Rust) running UniXcoder locally |
| Vector storage | Custom IVF+PQ quantization + ZSTD |
| Syntax highlighting | Syntect |
| Async runtime | Tokio + Rayon |

---

## Shell completions

```bash
scout completions zsh > ~/.zsh/completions/_scout
scout completions bash > ~/.bash_completions/scout
scout completions fish > ~/.config/fish/completions/scout.fish
```

Or let `scout init` install them automatically.

---

## FAQ

### Does Scout send my code anywhere?

No. Everything runs locally. No network requests unless you explicitly download the AI model or run `scout update`.

### How is this different from `grep` or `ripgrep`?

`grep` finds text. Scout finds functions — it understands code structure. `scout "validate JWT"` surfaces a function called `check_token` whose body handles JWT validation, even if the words "validate JWT" never appear in its source.

### How is this different from GitHub code search?

GitHub requires your code to be on GitHub. Scout works on private repos, local clones, and fully offline. No account required.

### How big can the codebase be?

Tested on repos with 100,000+ functions. Search stays under 30ms. First index of a monorepo that size takes a few minutes; subsequent runs skip unchanged files.

### Is the AI model required?

No. BM25 + name-match works immediately with no download. The AI model adds concept-level search on top — `scout "expire stale sessions"` can then surface `refresh_credentials()` without any keyword overlap.

### Is Rust required?

Only to build from source. Pre-built binaries for macOS, Linux, and Windows are on the [Releases](https://github.com/ParthPatel00/scout/releases) page — no Rust needed to use Scout.
