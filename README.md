<div align="center">

# Scout

**Search your codebase the way you think about it.**

```
scout "how does authentication work"
scout "stripe payment retry logic"
scout "function that validates JWT tokens"
```

![Rust](https://img.shields.io/badge/built_with-Rust-orange?style=flat-square)
![Platform](https://img.shields.io/badge/platform-macOS_·_Linux_·_Windows-lightgrey?style=flat-square)

</div>

---

> **No cloud. No API keys. Single binary.** Scout indexes your code locally using tree-sitter AST parsing, BM25 full-text search, and AI vector embeddings — all fused together for the best result. The AI model is a one-time ~350 MB download; BM25-only mode works immediately without it.

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

When your terminal is interactive, Scout launches a full TUI — navigate with `j/k`, press `Enter` to open the result in your editor at the exact line:

```
┌─ Results (10) ─────────────────────────┐┌─ Preview ────────────────────────────────────────────────────────┐
│  1. function  validate_token           ││ services/auth/service.py:110                                     │
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

### Option 1 — Pre-built binary (no Rust required)

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

**Windows**
Download `scout-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/ParthPatel00/scout/releases), extract, and add to your `PATH`.

Verify the install:
```bash
scout --version
# scout 0.1.0
```

---

### Option 2 — Homebrew (macOS / Linux)

```bash
brew install ParthPatel00/tap/scout
```

---

### Option 3 — cargo install (requires Rust)

If you have Rust installed:
```bash
cargo install --git https://github.com/ParthPatel00/scout
```

Don't have Rust? Install it in one command — it takes about 60 seconds:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Then restart your terminal and run `cargo install` above.

---

### Option 4 — Build from source

```bash
git clone https://github.com/ParthPatel00/scout
cd scout
cargo build --release
# Binary is at: ./target/release/scout

# Add to PATH (macOS / Linux)
sudo cp target/release/scout /usr/local/bin/
```

---

## Quick start

### First time? Run the setup wizard

```bash
scout init
```

`scout init` walks you through your preferences once — everything is a menu choice, nothing to type. Each setting is saved permanently so you never have to pass flags again. **Everything can be changed later with `scout config set <key> <value>`.**

Here is every question the wizard asks and what it means:

---

**1 — How many results to show by default?**
```
  5 / 10 (default) / 15 / 20 / 30 / 50
```
Sets `search.limit`. You can always override per-search with `--limit N`.

---

**2 — Default output format**
```
  Plain text with TUI when interactive (recommended)
  Always plain text  (good for scripts / piping)
  JSON
  CSV
```
In a terminal, Scout launches an interactive TUI. Piped or redirected, it always outputs plain text. Choose "Always plain text" if you want consistent output regardless of context.

---

**3 — Show test files in search results?**
```
  No  — include test files in results
  Yes — hide test files from results
```
Sets `search.exclude_tests`. Useful if your tests are noisy — you can still include them per-search with `--no-exclude-tests`.

---

**4 — Keep the index fresh automatically?**
```
  No  — I'll run `scout index` manually when I want to update
  Yes — via daemon   (background process, watches file changes)
  Yes — via git hooks  (re-indexes on commit / merge / checkout)
```
If you pick **No**, you run `scout index` yourself whenever you want the index updated.

If you pick **daemon** or **git hooks**, Scout builds the initial index right now, then sets up the chosen method to keep it current automatically. Nothing to run manually after that.

- **Daemon** — starts a background process that watches for file changes and updates the index as you work.
- **Git hooks** — installs `post-commit`, `post-merge`, and `post-checkout` hooks so the index updates on every commit and pull.

---

**5 — Enable AI-powered semantic search?**
```
  Yes — download the model now  (~350 MB)
  Yes — I'll download it later with `scout index --download-model`
  No  — keyword search is fine for now
```
Scout's default search uses BM25 + name-match — fast and accurate for most queries. The AI model adds a third component (vector embeddings) that understands *concepts*, not just keywords. With it, `scout "functions that handle token expiry"` finds code that never literally says those words.

The model is ~350 MB and downloaded once to `~/.config/scout/models/`. After that, hybrid search (BM25 + name-match + vectors) runs automatically every time — no flags needed.

---

**6 — Editor for opening results**
```
  Auto-detect  (currently: nvim)
  code   — VS Code
  cursor — Cursor
  zed    — Zed
  nvim   — Neovim
  vim    — Vim
  hx     — Helix
  nano   — Nano
  Other  — enter path
```
Sets `editor.command`. This is the editor that opens when you press `Enter` in the TUI. Terminal editors (nvim, vim, helix) take over the screen and return to Scout when you close them. GUI editors (VS Code, Cursor, Zed) open in the background — Scout stays running.

---

**7 — Install shell completions?**
```
  Skip for now / Zsh / Bash / Fish
```
Prints the command to install tab-completion for your shell. You can run this any time with `scout completions <shell>`.

---

**8 — Add other repos for cross-repo search now?**
```
  Yes / No  (you can add them later with `scout repos add <name> <path>`)
```
If yes, enter one or more repository paths and short names. Scout registers them so you can search across all of them at once with `--all-repos`.

---

After the wizard, run:

```bash
scout index              # index the current directory
scout "your query"       # search
```

---

## Configuration

All preferences live in `~/.config/scout/config.toml`. Use `scout init` to set them interactively, or change individual values any time:

```bash
scout config list                           # view all settings + current values
scout config set search.limit 20            # show 20 results by default
scout config set search.no_tui true         # always plain text (good for scripts)
scout config set search.exclude_tests true  # hide test files
scout config set search.format json         # default JSON output
scout config set editor.command nvim        # override editor detection
scout config set index.auto_index true      # auto-index on first search
scout config edit                           # open config file in your editor
```

**Config file** (`~/.config/scout/config.toml`):
```toml
[search]
limit = 20
exclude_tests = true

[editor]
command = "code"  # use VS Code even if nvim is in PATH

[index]
auto_index = true  # no need to run scout index first
```

### Shell completions

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

---

## How indexing works

```
$ scout index --verbose

Found 312 source files to consider
  skip (unchanged): src/auth/middleware.ts
  skip (unchanged): src/payments/stripe.ts
  parsing:          src/users/service.py         → 23 units
  parsing:          src/api/routes.go             → 41 units
  ...
Indexed 8 files (147 new units, 304 unchanged) in 0.84s
Index totals: 312 files, 4,821 units — /your/project/.scout
```

Scout only re-parses files whose content has changed (tracked by SHA2 hash). On subsequent runs, unchanged files are skipped instantly. **A 10,000-file codebase re-indexes in under 2 seconds.**

The index lives in `.scout/` at your project root. Add it to `.gitignore`:
```
.scout/
```

---

## Examples

### Natural language search

```bash
scout "where do we handle payment failures"
scout "function that sends email notifications"
scout "how is the database connection pool managed"
scout "JWT token validation"
scout "retry logic with exponential backoff"
```

### Exact name search

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
# Only files whose path contains "gateway"
scout "rate limit" --path-filter gateway

# Only the payments service
scout "refund" --path-filter services/payments
```

### Only recent files

```bash
# Files indexed in the last 7 days
scout "new feature" --modified-last 7
```

### Exclude test files

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

### JSON output — pipe to jq

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

---

## Editor integration

Press `Enter` on any TUI result to open the file **at the exact line** in your editor. Press `o` to open without leaving the TUI.

Scout detects your editor automatically:

| Priority | Source |
|----------|--------|
| 1st | `editor.command` in `~/.config/scout/config.toml` |
| 2nd | `$SCOUT_EDITOR` environment variable |
| 3rd | `$VISUAL` environment variable |
| 4th | `$EDITOR` environment variable |
| 5th | Auto-detect from PATH: `nvim` → `vim` → `hx` → `nano` → `emacs` → `code` → `zed` |

**Terminal editors** (nvim, vim, helix, nano) take over the terminal and return you to Scout when you close them.
**GUI editors** (VS Code, Zed, Cursor) open in the background — Scout stays running.

---

## Semantic search (AI-powered)

Scout's default search uses **BM25 + name-match** — fast and accurate for keyword-style queries. When the AI model is present, Scout automatically upgrades to **hybrid search**: BM25 + name-match + vector embeddings, all three fused via Reciprocal Rank Fusion.

**Without model:** keyword search only — works immediately, no setup.
**With model:** concept-level search — finds code by what it *does*, even when the words don't match.

Example: `scout "functions that handle token expiry"` will surface token refresh logic even if the source code says `refresh_credentials()`, not "token expiry".

### Getting the model

`scout init` asks whether you want semantic search and handles the download. Or do it manually:

```bash
# Step 1 — see download instructions
scout index --download-model

# Step 2 — download the model (~350 MB, one-time)
# Place files in ~/.config/scout/models/unixcoder-base/

# Step 3 — re-index to generate embeddings
scout index
```

From this point, every search automatically uses the best available method. No flags needed.

**Force pure vector search** (skips BM25 entirely — requires model):
```bash
scout "retry failed network requests" --semantic
```

---

## Cross-repo search

Search across multiple codebases as if they were one:

```bash
# Register your repos (once, or during scout init)
scout repos add backend   ~/projects/backend
scout repos add frontend  ~/projects/frontend
scout repos add shared    ~/projects/shared-libs

# Index each one
scout index --path ~/projects/backend
scout index --path ~/projects/frontend
scout index --path ~/projects/shared-libs

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

---

## Find similar functions

Find functions structurally similar to one you already know — useful for spotting copy-paste debt:

```bash
scout --find-similar services/auth/service.py:110
```

---

## Keep the index current

### Option A — Manual (simplest)

```bash
scout index
# Indexed 3 files (41 new units, 309 unchanged) in 0.18s
```

### Option B — Background daemon

```bash
scout daemon start
scout daemon status   # Daemon running  PID 48291  uptime 4h12m
scout daemon stop
```

### Option C — Git hooks (set and forget)

```bash
scout daemon install-hooks
# Installs post-commit, post-merge, post-checkout hooks
```

After this, the index is always in sync with your working tree.

---

## Ignoring files and directories

Scout respects your existing `.gitignore` — anything git ignores, Scout ignores too. `node_modules`, `dist`, `.next`, `venv`, and all build output are skipped automatically.

For Scout-specific exclusions, create a `.scoutignore` at your project root:

```
# .scoutignore
tests/fixtures/
tests/snapshots/
vendor/
src/generated/
**/*.snapshot.ts
```

Scout also automatically skips binary files, minified files, generated-code headers (`// Code generated`, `// DO NOT EDIT`), TypeScript declaration files (`.d.ts`), and protobuf stubs (`.pb.go`, `.pb.ts`).

---

## Index maintenance

```bash
scout cleanup   # remove entries for deleted files
scout optimize  # compact database, reclaim space
scout rebuild   # wipe and regenerate from scratch
```

---

## Performance

Benchmarked on a MacBook Pro M2 against a 10,000-function codebase:

| Operation | Time |
|-----------|------|
| First index (10k functions) | ~8s |
| Incremental index (1 changed file) | <200ms |
| Search query | **<10ms** |
| Search with 50k functions | ~30ms |

---

## Supported languages

| Language | Functions | Methods | Classes / Structs | Call graph |
|----------|:---------:|:-------:|:-----------------:|:----------:|
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
│  .gitignore + .scoutignore + content heuristics │  ← skip generated / minified / binary
└─────────────────────────────────────────────────┘
    │
    ▼
┌────────────────────┐
│  Tree-sitter AST   │  ← parse functions, classes, call edges per language
└────────────────────┘
    │
    ├──────────────────────► SQLite (metadata.db)
    │                         functions, call graph, file hashes
    │
    └──────────────────────► Tantivy (tantivy/)
                              BM25 full-text index


scout "query"
    │
    ├── BM25 search ─────────────────────┐
    │   (always runs)                    │
    │                                    ▼
    ├── Name-match re-rank ─────► Reciprocal Rank Fusion ──► ranked results
    │   (always runs)                    ▲
    │                                    │
    └── Vector search ──────────────────┘
        (runs when model is downloaded)
```

---

## CI / scripting

```bash
# Always plain text, no TUI
scout "database migration" --no-tui --format json | jq '.[0]'

# All auth-related files
scout "authentication" --format json | jq -r '.[].file_path' | sort -u

# Fail CI if more than 50 unused functions accumulate
scout report unused-functions --format json | jq 'length' | xargs -I{} test {} -lt 50
```

---

## Frequently asked questions

**Does Scout send my code anywhere?**
No. Everything runs locally. The index lives in `.scout/` in your project directory. No network requests are made.

**How is this different from `grep` or `ripgrep`?**
`grep` finds text. Scout finds *functions* — it understands code structure. `scout "validate JWT"` surfaces a function called `check_token` whose body handles JWT validation, even if the words "validate JWT" never appear in its source.

**How is this different from GitHub's code search?**
GitHub search requires your code to be on GitHub. Scout works on private repos, local clones, and fully offline.

**How big can the codebase be?**
Tested on repos with 100,000+ functions. Search stays under 30ms. First index of a monorepo that size takes a few minutes; subsequent runs skip unchanged files.

**Can I search multiple repos at once?**
Yes — see [Cross-repo search](#cross-repo-search). Use `scout repos add` or set them up during `scout init`.

**The index is out of date after I pull. What do I do?**
Run `scout index`, start the daemon with `scout daemon start`, or install git hooks with `scout daemon install-hooks`.

**What files does Scout skip?**
Anything in `.gitignore`, plus generated files, `.d.ts`, `.pb.go`, minified JS, binaries, and files over 1 MB. Add a `.scoutignore` for anything else.

**Is Rust required to use Scout?**
Only to build from source. Pre-built binaries for macOS (Intel + Apple Silicon), Linux, and Windows are on the [Releases](https://github.com/ParthPatel00/scout/releases) page.

---

## Documentation

- [Setup guide](docs/setup.md) — detailed installation for all platforms
- [Full command reference](docs/usage.md) — every flag and option
