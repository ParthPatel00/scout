# Scout — Code Search

Fast, offline code search for your codebase. Just run:

```
scout "authentication with stripe"
```

No configuration. No API keys. Indexes your repo locally using AST parsing and BM25 full-text search. Optional local AI embeddings for semantic search.

## Install

```bash
cargo install --path .
```

## Usage

```bash
# Index your repo (run once, then incrementally after changes)
cd /your/project
scout index

# Search — that's it
scout "authentication with stripe"
scout "retry logic for failed payments"
scout "how does caching work"
```

Scout launches an **interactive TUI** when run in a terminal. Navigate with `j`/`k`, press `Enter` to open the result in your editor at the exact line, `q` to quit. When piped, it outputs plain text.

```
services/auth/service.py:25   AuthenticationError   class · python
  class AuthenticationError(Exception): ...

gateway/main.go:149           Authenticate          method · go
  func (m *AuthMiddleware) Authenticate(next http.Handler)

services/auth/service.py:35   AuthService           class · python
  class AuthService:
```

## Filters

Stack on filters when you need them — all optional:

```bash
scout "validate input" --lang python
scout "rate limit" --path-filter gateway
scout "database" --modified-last 7        # files changed in last 7 days
scout "payment" --exclude-tests
scout "session" --limit 20
```

## Output formats

```bash
scout "payment" --format json             # pipe to jq
scout "error" --format csv               # spreadsheet-friendly
scout "auth" --no-tui                    # always plain text
```

## Call graph context

```bash
scout "process_payment" --show-context

# Shows callers and callees of each result:
# Callers:  handle_checkout (api/checkout.py:45)
# Calls:    validate_card (payments/validator.py:88)
```

## Multi-repo search

```bash
scout repos add backend ~/projects/backend
scout repos add frontend ~/projects/frontend

scout "user session" --all-repos
scout "rate limit" --repos backend,frontend
```

## Find similar functions

```bash
scout --find-similar services/auth/service.py:110
# Returns functions structurally similar across all registered repos
```

## Semantic search (AI)

Requires a one-time model download (~350 MB):

```bash
scout index --download-model   # prints download instructions
scout "retry failed requests" --semantic   # concept-level search
scout "authentication" --best              # BM25 + semantic + name-match
```

Works fully offline — no cloud APIs needed.

## Other commands

```bash
scout index --verbose          # see each file as it's parsed
scout report unused-functions  # find dead code
scout rebuild                  # wipe and regenerate index
scout daemon start             # watch for changes in background
scout daemon install-hooks     # auto-update on git commit
```

## Supported languages

Python, Rust, Go, TypeScript, JavaScript, Java, C/C++

---

[Setup guide](docs/setup.md) · [Full command reference](docs/usage.md)
