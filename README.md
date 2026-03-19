<div align="center">

# Scout

**Search your codebase the way you think about it.**

```
scout "stripe payment retry logic"
scout "function that validates JWT tokens"
scout "where do we handle rate limiting"
```

![Rust](https://img.shields.io/badge/built_with-Rust-orange?style=flat-square)
![Platform](https://img.shields.io/badge/platform-macOS_·_Linux_·_Windows-lightgrey?style=flat-square)
![Version](https://img.shields.io/badge/version-0.1.4-blue?style=flat-square)

</div>

---

> **No cloud. No API keys. Single binary.** Scout indexes your code locally using tree-sitter AST parsing, BM25 full-text search, and optional AI vector embeddings — all fused together for the best results. BM25 mode works immediately with no setup. The AI model is a one-time ~350 MB download that unlocks concept-level search.

---

## What it looks like

```
$ scout "authentication with stripe"

services/payments/processor.py:310  _map_stripe_status     function · python
  def _map_stripe_status(self, stripe_status:

services/auth/service.py:110        validate_token         function · python
  def validate_token(self, token:

gateway/main.go:149                 Authenticate           method · go
  func (m *AuthMiddleware) Authenticate(next http.Handler)

services/auth/service.py:35         AuthService            class · python
services/payments/processor.py:206  capture_payment        function · python
  def capture_payment(self, payment_intent_id:
```

When stdout is a terminal, Scout launches an interactive TUI. Navigate with `j`/`k`, press `Enter` to open the result in your editor at the exact line:

```
┌─ Results (10) ─────────────────────────┐┌─ Preview ────────────────────────────────────────────────────────┐
│  1. function  validate_token           ││ services/auth/service.py:110                            [94.3]   │
│  2. class     AuthService              ││ ─────────────────────────────────────────────────────────────    │
│  3. method    Authenticate             ││ def validate_token(self, token: str) -> Optional[TokenPayload]:  │
│  4. function  setup_mfa                ││     """Validate a JWT token and return its payload."""           │
│  5. function  change_password          ││     try:                                                         │
│  6. function  validate_api_key         ││         payload = jwt.decode(                                    │
│  7. function  login                    ││             token,                                               │
│  8. function  logout                   ││             self.secret_key,                                     │
│  9. class     AuthenticationError      ││             algorithms=["HS256"]                                 │
│ 10. function  _validate_password_str…  ││         )                                                        │
└────────────────────────────────────────┘└──────────────────────────────────────────────────────────────────┘
 j/k: navigate   Enter: open in editor   o: open (stay)   d/u: scroll   q: quit
```

---

## Install

### Option 1 — Pre-built binary (recommended)

Download the latest binary for your platform from [Releases](https://github.com/ParthPatel00/scout/releases):

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

**Windows**
Download `scout-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/ParthPatel00/scout/releases), extract, and add to your `PATH`.

Verify:
```bash
scout --version
# scout 0.1.4
```

---

### Option 2 — Build from source (requires Rust)

```bash
git clone https://github.com/ParthPatel00/scout
cd scout
cargo build --release
sudo cp target/release/scout /usr/local/bin/
```

Don't have Rust? Install it in one command:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## Quick start

```bash
scout init       # one-time setup wizard — does everything for you
scout "query"    # search
```

That's it. `scout init` walks you through your preferences once, then takes care of every setup step automatically: indexes the current directory, starts the daemon or installs git hooks, downloads the AI model, and installs shell completions. Nothing to run manually after.

---

## Setup wizard — `scout init`

`scout init` asks eight questions, all multiple-choice. Every answer is saved permanently so you never have to pass flags again. Each setting can be changed later with `scout config set <key> <value>`.

**1 — How many results to show by default?**
```
  5 / 10 (default) / 15 / 20 / 30 / 50
```
Sets `search.limit`. Override per-search with `--limit N`.

---

**2 — Default output format**
```
  Plain text with TUI when interactive (recommended)
  Always plain text  (good for scripts / piping)
  JSON
  CSV
```
In a terminal Scout launches the TUI. When piped it always outputs plain text. "Always plain text" gives consistent output regardless of context.

---

**3 — Show test files in search results?**
```
  No  — include test files in results
  Yes — hide test files from results
```
Sets `search.exclude_tests`.

---

**4 — Keep the index fresh automatically?**
```
  No  — I'll run `scout index` manually
  Yes — via daemon   (background process, watches file changes)
  Yes — via git hooks  (updates on commit / merge / checkout)
```
Choosing **daemon** or **git hooks** causes `init` to immediately build the index, then set up the chosen method — nothing to run manually after.

- **Daemon** — background process watching for file saves, updates the index as you work.
- **Git hooks** — installs `post-commit`, `post-merge`, `post-checkout` hooks; the index updates on every git operation.

---

**5 — Enable AI-powered semantic search?**
```
  Yes — download the model now  (~350 MB)
  Yes — download in the background
  No  — keyword search is fine for now
```
Choosing "download now" fetches the UniXcoder model (~350 MB) with a live progress bar. "Download in the background" spawns a detached process that fetches it without blocking. After the model is present, hybrid search (BM25 + name-match + vectors) runs automatically on every query — no flags needed.

---

**6 — Editor for opening results**
```
  Auto-detect  (currently: cursor)
  code   — VS Code
  cursor — Cursor
  zed    — Zed
  nvim   — Neovim / vim / hx / nano / Other
```
Sets `editor.command`. On macOS, Scout finds VS Code and Cursor through their app bundles automatically — even if the `code`/`cursor` CLI is not in your PATH.

---

**7 — Install shell completions?**
```
  Skip for now / Zsh / Bash / Fish
```
`init` writes the completion file to the right location for your shell and adds the `source` line to your rc file — no manual steps.

---

**8 — Add other repos for cross-repo search now?**
```
  Yes / No  (add later with `scout repos add <name> <path>`)
```

---

## Configuration

```bash
scout config list                           # view all settings + current values
scout config set search.limit 20
scout config set search.no_tui true         # always plain text
scout config set search.exclude_tests true  # hide test files
scout config set search.format json
scout config set editor.command nvim
scout config set index.auto_index true      # auto-index on first search
scout config edit                           # open config file in editor
```

Config file locations:
- **macOS:** `~/Library/Application Support/scout/config.toml`
- **Linux:** `~/.config/scout/config.toml`
- **Windows:** `%APPDATA%\scout\config.toml`

---

## Indexing

```bash
scout index              # index current directory (incremental)
scout index --verbose    # show each file as it's parsed
scout index /path/to/repo
```

Scout re-parses only files whose content has changed (tracked by SHA2 hash). A 10,000-file codebase re-indexes in under 2 seconds on subsequent runs.

```
$ scout index --verbose

Found 312 source files to consider
  skip (unchanged): src/auth/middleware.ts
  parsing:          src/users/service.py     → 23 units
  parsing:          src/api/routes.go        → 41 units
Indexed 8 files (147 new units, 304 unchanged) in 0.84s
Index totals: 312 files, 4,821 units — /your/project/.scout
```

The index lives in `.scout/` at your project root. Add it to `.gitignore`:
```
.scout/
```

---

## Searching

### Natural language
```bash
scout "where do we handle payment failures"
scout "function that sends email notifications"
scout "JWT token validation"
scout "retry logic with exponential backoff"
scout "database connection pool management"
```

### Exact name
```bash
scout "AuthService"
scout "process_payment"
scout "handleWebhook"
```

### Filter by language
```bash
scout "error handling" --lang python
scout "middleware" --lang go
scout "interface" --lang typescript
scout "serialize" --lang rust
```
Supported: `python`, `rust`, `go`, `typescript`, `javascript`, `java`, `cpp`

### Filter by path
```bash
scout "rate limit" --path-filter gateway
scout "refund" --path-filter services/payments
```

### Recent files only
```bash
scout "new feature" --modified-last 7    # files changed in last 7 days
```

### Exclude tests
```bash
scout "validate_card" --exclude-tests
```

### Show call graph context
```
$ scout "validate_token" --show-context

services/auth/service.py:110  validate_token  function · python
  def validate_token(self, token:
  Calls:    AuthenticationError (service.py:25)  is_valid (models.py:81)
  Callers:  login (service.py:48)  verify_request (middleware.go:89)
```

### JSON output
```bash
scout "payment" --format json | jq -r '.[].name'
scout "payment" --format json | jq -r '.[].file_path' | sort -u
scout "auth" --format json | jq '[.[] | select(.score > 150)]'
```

### Plain text (no TUI)
```bash
scout "auth" --no-tui
scout "auth" --no-tui | grep python
```

### Force semantic (vector-only) search
```bash
scout "functions that expire stale sessions" --semantic
```

### Find similar functions
```bash
scout search --find-similar services/auth/service.py:110
```

---

## Index statistics

```bash
scout stats
```

```
Index stats  /your/project

    4,821 functions/methods      312 files      1,203 call edges

  Unit types
    function            3,891
    method                712
    class                 183
    module                 35
    with docstrings       1,204

  Languages
    python              1,823  [████████████░░░░░░░░]  37%
    typescript          1,401  [█████████░░░░░░░░░░░]  29%
    go                    891  [██████░░░░░░░░░░░░░░]  18%
    rust                  521  [███░░░░░░░░░░░░░░░░░]  10%
    java                  185  [█░░░░░░░░░░░░░░░░░░░]   3%

  Embeddings
    4,209/4,821 units  [█████████████████░░░]  87%
    model: microsoft/unixcoder-base

  Storage
    database (metadata.db)    4.2 MB
    vectors (vectors.bin)     2.1 MB
    tantivy index             8.7 MB
    total                    15.0 MB

  Status
    last indexed              3m ago
    index version             v1
    daemon                    running  PID 71926  uptime 3h12m ago
```

---

## Cross-repo search

Search across multiple codebases as if they were one:

```bash
# Register repos once
scout repos add backend   ~/projects/backend
scout repos add frontend  ~/projects/frontend
scout repos add shared    ~/projects/shared-libs

# Search all of them
scout "user session" --all-repos

# Or specific ones
scout "rate limit" --repos backend,shared
```

```
$ scout "authentication" --all-repos --no-tui

[backend]  gateway/main.go:149           Authenticate    method · go
[backend]  services/auth/service.py:35   AuthService     class · python
[frontend] src/hooks/useAuth.ts:12       useAuth         function · typescript
[shared]   lib/auth/validator.py:88      validate_token  function · python
```

```bash
scout repos list     # show registered repos and their status
scout repos remove backend
```

---

## Keeping the index current

### Option A — Manual
```bash
scout index    # re-parses only changed files
```

### Option B — Background daemon
```bash
scout daemon start              # start watching for file changes
scout daemon status             # running  PID 48291  uptime 4h12m
scout daemon stop
```

### Option C — Git hooks (set and forget)
```bash
scout daemon install-hooks
# Installs post-commit, post-merge, post-checkout hooks
```

After this, the index is updated automatically on every git operation.

---

## Editor integration

Press `Enter` on any TUI result to open the file **at the exact line** in your editor. Press `o` to open without leaving the TUI.

Editor detection priority:

| Priority | Source |
|----------|--------|
| 1st | `editor.command` in config |
| 2nd | `$SCOUT_EDITOR` env var |
| 3rd | `$VISUAL` env var |
| 4th | `$EDITOR` env var |
| 5th | Auto-detect from PATH, then macOS app bundles |

**Terminal editors** (nvim, vim, helix, nano) take over the terminal and return you to Scout when you close them.
**GUI editors** (VS Code, Cursor, Zed) open in the background — Scout stays running.

On macOS, Scout finds VS Code and Cursor through their `.app` bundles even if the CLI is not in PATH.

---

## Semantic search

Scout's default search uses **BM25 + name-match** — fast and accurate for keyword queries. When the AI model is present, Scout upgrades to **hybrid search**: all three backends fused via Reciprocal Rank Fusion.

**Without model:** keyword search — works immediately, no download needed.
**With model:** hybrid search — finds code by what it _does_, even when the words don't match.

Example: `scout "handle token expiry"` will surface `refresh_credentials()` even though it never says "token expiry".

`scout init` handles the model download automatically. To do it manually:
```bash
scout index --download-model    # shows download options
```

---

## Ignoring files

Scout respects `.gitignore` automatically. For Scout-specific exclusions:

```
# .scoutignore
tests/fixtures/
vendor/
src/generated/
**/*.snapshot.ts
```

Scout also automatically skips binary files, minified files, generated-code headers, `.d.ts` files, and protobuf stubs. Files over 1 MB or 10,000 lines are skipped.

---

## Index maintenance

```bash
scout cleanup   # remove entries for deleted files
scout optimize  # compact database, reclaim disk space
scout rebuild   # wipe and regenerate from scratch
```

---

## CI / scripting

```bash
# Always plain text
scout "database migration" --no-tui --format json | jq '.[0]'

# All files containing auth logic
scout "authentication" --format json | jq -r '.[].file_path' | sort -u

# Fail CI if unused function count exceeds threshold
scout report unused-functions --format json | jq 'length' | xargs -I{} test {} -lt 50
```

---

## Shell completions

```bash
# Zsh
scout completions zsh > ~/.zsh/completions/_scout
# add to ~/.zshrc: fpath=(~/.zsh/completions $fpath) && autoload -Uz compinit && compinit

# Bash
scout completions bash > ~/.bash_completions/scout
# add to ~/.bashrc: source ~/.bash_completions/scout

# Fish
scout completions fish > ~/.config/fish/completions/scout.fish
```

Or let `scout init` install them automatically.

---

## Performance

Benchmarked on an M2 MacBook Pro against a 10,000-function codebase:

| Operation | Time |
|-----------|------|
| First index (10k functions) | ~8s |
| Incremental index (1 changed file) | <200ms |
| Search query (BM25) | **<10ms** |
| Search query (hybrid, 50k functions) | ~30ms |

---

## Supported languages

| Language | Functions | Methods | Classes | Call graph |
|----------|:---------:|:-------:|:-------:|:----------:|
| Python | ✓ | ✓ | ✓ | ✓ |
| Rust | ✓ | ✓ | ✓ | ✓ |
| Go | ✓ | ✓ | ✓ | ✓ |
| TypeScript | ✓ | ✓ | ✓ | ✓ |
| JavaScript | ✓ | ✓ | ✓ | ✓ |
| Java | ✓ | ✓ | ✓ | ✓ |
| C / C++ | ✓ | ✓ | ✓ | Partial |

---

## How it works

```
Your code
    │
    ▼
┌─────────────────────────────────────────────────┐
│  .gitignore + .scoutignore + content heuristics │  ← skip generated/minified/binary
└─────────────────────────────────────────────────┘
    │
    ▼
┌────────────────────┐
│  Tree-sitter AST   │  ← parse functions, classes, call edges per language
└────────────────────┘
    │
    ├──────────────────► SQLite (metadata.db)   functions, call graph, file hashes
    └──────────────────► Tantivy (tantivy/)     BM25 full-text index


scout "query"
    │
    ├── BM25 search ─────────────────────┐
    │   (always)                         │
    │                                    ▼
    ├── Name-match re-rank ─────► Reciprocal Rank Fusion ──► ranked results
    │   (always)                         ▲
    │                                    │
    └── Vector search ──────────────────┘
        (when model is downloaded)
```

---

## FAQ

**Does Scout send my code anywhere?**
No. Everything runs locally. No network requests are made unless you explicitly download the AI model or run `scout update` to check for a new binary.

**How is this different from `grep` or `ripgrep`?**
`grep` finds text. Scout finds _functions_ — it understands code structure. `scout "validate JWT"` surfaces a function called `check_token` whose body handles JWT validation, even if the words "validate JWT" never appear in its source.

**How is this different from GitHub code search?**
GitHub requires your code to be on GitHub. Scout works on private repos, local clones, and fully offline.

**How big can the codebase be?**
Tested on repos with 100,000+ functions. Search stays under 30ms. First index of a monorepo that size takes a few minutes; subsequent runs skip unchanged files.

**Can I search multiple repos at once?**
Yes — see [Cross-repo search](#cross-repo-search).

**The index is out of date after a pull. What do I do?**
Run `scout index`, or use the daemon (`scout daemon start`) or git hooks (`scout daemon install-hooks`) to keep it current automatically.

**VS Code / Cursor isn't opening when I press Enter.**
On macOS, Scout finds VS Code and Cursor through their `.app` bundles automatically — you don't need the `code` CLI in PATH. If it's still not working, set the editor explicitly: `scout config set editor.command cursor`.

**Is Rust required to use Scout?**
Only to build from source. Pre-built binaries for all platforms are on the [Releases](https://github.com/ParthPatel00/scout/releases) page.
