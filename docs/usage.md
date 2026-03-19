# Command Reference

## Search (default)

```bash
scout "query"
```

No subcommand needed. Any unrecognized first argument is treated as a search query. `scout search "query"` also works and is an alias.

```
scout "authentication with stripe"
scout "retry logic for failed requests"
scout "how does rate limiting work"
scout "payment webhook handler"
```

**Options:**

| Flag | Description |
|------|-------------|
| `--lang <LANG>` | Filter by language: `python`, `rust`, `go`, `java`, `typescript`, `javascript`, `cpp` |
| `--path-filter <STR>` | Only show results from files whose path contains this string |
| `--modified-last <N>` | Only files indexed in the last N days |
| `--exclude-tests` | Skip test files |
| `--show-context` | Show callers and callees of each result |
| `--limit <N>` | Max results (default: 10) |
| `--path <PATH>` | Repo root to search (default: current directory) |
| `--no-tui` | Plain text output, no TUI |
| `--format json\|csv` | Machine-readable output |
| `--semantic` | Semantic (vector) search — requires model |
| `--best` | Hybrid: BM25 + semantic + name-match |
| `--all-repos` | Search all registered repos |
| `--repos a,b` | Search specific registered repos |
| `--find-similar FILE:LINE` | Find functions similar to this one |

### TUI controls

| Key | Action |
|-----|--------|
| `j` / `↓` | Next result |
| `k` / `↑` | Previous result |
| `Enter` | **Open in editor at exact line**, exit TUI |
| `o` | Open in editor, stay in TUI |
| `d` / `PageDown` | Scroll preview down |
| `u` / `PageUp` | Scroll preview up |
| `q` / `Esc` | Quit |

Scout detects your editor automatically from `$SCOUT_EDITOR`, `$VISUAL`, `$EDITOR`, or by checking your PATH for `nvim`, `vim`, `hx`, `code`, `zed`, etc. You can override it:

```bash
export SCOUT_EDITOR=nvim   # always use Neovim
export SCOUT_EDITOR=code   # always use VS Code
```

### Examples

```bash
# Language filter
scout "validate input" --lang python
scout "error handling" --lang rust

# Path filter
scout "rate limit" --path-filter gateway
scout "model" --path-filter services/payments

# Time filter
scout "new feature" --modified-last 7

# Combine filters
scout "auth" --lang go --path-filter middleware --limit 5

# Call graph
scout "process_payment" --show-context

# JSON output
scout "payment" --format json | jq '.[].name'
scout "auth" --format json > results.json

# CSV output
scout "error" --format csv

# Cross-repo
scout "session" --all-repos
scout "rate limit" --repos backend,shared

# Find similar functions across repos
scout --find-similar services/auth/service.py:110

# Semantic search
scout "retry failed network requests" --semantic
scout "authentication" --best
```

---

## `scout index`

Build or update the index. Only re-parses files that have changed.

```bash
scout index                  # index current directory
scout index ~/projects/app   # index a specific path
scout index --verbose        # show each file as it's parsed
scout index --download-model # print AI model download instructions
```

Output:
```
Indexed 12 files (203 new units, 316 unchanged) in 0.84s
Index totals: 328 files, 5,420 units — /your/project/.codesearch
```

---

## `scout repos`

Manage repos for cross-repo search.

```bash
scout repos add backend ~/projects/backend
scout repos add frontend ~/projects/frontend
scout repos list
scout repos remove frontend
```

`list` output:
```
NAME                 STATUS   PATH
----------------------------------------------------------------------
backend              indexed  /home/user/projects/backend
frontend             indexed  /home/user/projects/frontend
```

Status is `indexed` if the repo has been indexed, `missing` if not.

---

## `scout report`

```bash
scout report unused-functions            # current directory
scout report unused-functions --path .   # explicit path
```

Lists functions and methods with no callers in the index — useful for finding dead code.

Output:
```
260 potentially unused functions/methods:

data-pipeline/src/lib.rs
    111  function  validate
    132  function  is_known_event_type
```

> Functions called from outside the indexed codebase (e.g. external APIs, separate test repos) will appear here.

---

## `scout daemon`

```bash
scout daemon start            # start background watcher
scout daemon stop             # stop it
scout daemon status           # PID, uptime, last update time
scout daemon install-hooks    # add git hooks (post-commit/merge/checkout)
scout daemon update           # manually batch-update without a daemon
```

The daemon watches for file changes and updates the index automatically. `install-hooks` appends to existing git hooks rather than replacing them.

---

## `scout rebuild`

Wipe the index and regenerate from scratch. Use after major refactors or if search results seem wrong.

```bash
scout rebuild
scout rebuild --verbose
```

---

## `scout optimize`

Compact the database and remove orphaned data. Useful after many incremental updates.

```bash
scout optimize
```

---

## `scout cleanup`

Remove index entries for files that have been deleted from disk.

```bash
scout cleanup
```

---

## Scripting

```bash
# All function names matching a query
scout "payment" --format json | jq -r '.[].name'

# File paths, deduplicated
scout "auth" --format json | jq -r '.[].file_path' | sort -u

# Results above a score threshold
scout "validate" --format json | jq '[.[] | select(.score > 150)]'

# Count unused functions
scout report unused-functions --format json 2>/dev/null | jq 'length'
```
