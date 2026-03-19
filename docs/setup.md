# Setup

## Prerequisites

- **Rust 1.75+** — [rustup.rs](https://rustup.rs)

That's it. Scout compiles to a single static binary with no runtime dependencies.

## Install

```bash
git clone https://github.com/your-org/scout
cd scout
cargo install --path .
```

Verify:

```bash
scout --version
```

## First run

```bash
cd /your/project
scout index
scout "what you're looking for"
```

The index lives in `.codesearch/` at your project root. Add it to `.gitignore`:

```
.codesearch/
```

## Keeping the index current

**Manually** — `scout index` re-parses only changed files, so it's fast to run whenever you want:

```bash
scout index
```

**Background daemon** — auto-updates as files change:

```bash
scout daemon start
```

**Git hooks** — auto-update on every commit/merge/checkout:

```bash
scout daemon install-hooks
```

## Multi-repo setup

Register repos you want to search across:

```bash
scout repos add backend ~/projects/backend
scout repos add frontend ~/projects/frontend
scout repos list
```

Each repo needs its own index:

```bash
scout index --path ~/projects/backend
scout index --path ~/projects/frontend
```

Then:

```bash
scout "authentication" --all-repos
```

## Semantic search (optional)

Get download instructions for the UniXcoder model (~350 MB, one-time):

```bash
scout index --download-model
```

Place the model files in `~/.config/scout/models/unixcoder-base/`. After that, semantic search works offline with no API keys.

If the model isn't present, Scout falls back to BM25 automatically — you never lose basic search.

## Troubleshooting

**"No index found"**
```bash
scout index   # run this first
```

**Index feels stale**
```bash
scout rebuild   # wipe and regenerate from scratch
```

**Daemon not picking up changes**
```bash
scout daemon status
scout daemon stop && scout daemon start
```
