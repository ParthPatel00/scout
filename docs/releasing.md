# Releasing Scout

## Day-to-day: pushing code (no release)

Just push to `main` as normal. CI runs tests on every push — nothing is published.

```bash
git add <files>
git commit -m "your message"
git push origin main
```

---

## Publishing a release (downloadable binaries)

A release is triggered by pushing a version tag (`v0.1.3`, `v0.2.0`, etc.). The GitHub Actions workflow builds Scout for all 5 platforms and attaches the binaries to a release page automatically.

### Install cargo-release (one-time setup)

```bash
cargo install cargo-release
```

### Release commands

```bash
cargo release patch   # bug fixes:    0.1.2 → 0.1.3
cargo release minor   # new features: 0.1.2 → 0.2.0
cargo release major   # breaking:     0.1.2 → 1.0.0
```

That single command:
1. Bumps the version in `Cargo.toml`
2. Runs `cargo test` to make sure nothing is broken
3. Commits (`release 0.1.3`)
4. Tags (`v0.1.3`)
5. Pushes the commit and tag to `origin main`

GitHub Actions picks up the tag and builds + publishes the release (~5 minutes).

---

## What happens after you push the tag

1. `ci.yml` — runs tests on the commit (as always)
2. `release.yml` — detects the tag and builds:
   - macOS Apple Silicon (`aarch64-apple-darwin`)
   - macOS Intel (`x86_64-apple-darwin`)
   - Linux x86_64 (`x86_64-unknown-linux-gnu`)
   - Linux ARM64 (`aarch64-unknown-linux-gnu`)
   - Windows x86_64 (`x86_64-pc-windows-msvc`)
3. Binaries are packaged (`.tar.gz` / `.zip`) with SHA256 checksums
4. A release page is created at `https://github.com/ParthPatel00/scout/releases/tag/v0.1.3`

Watch it live under the **Actions** tab on GitHub.

---

## Versioning convention (semver)

| Change | Example | When to use |
|--------|---------|-------------|
| Patch  | `0.1.2 → 0.1.3` | Bug fixes, no new features |
| Minor  | `0.1.2 → 0.2.0` | New features, backwards compatible |
| Major  | `0.1.2 → 1.0.0` | Breaking changes |

---

## If something goes wrong

**The workflow failed mid-build** — fix the issue, delete the tag, re-tag, and push:

```bash
git tag -d v0.1.3                      # delete local tag
git push origin :refs/tags/v0.1.3      # delete remote tag
# fix the issue, then re-release:
cargo release patch
```

**You need to undo the release commit** — delete both the tag and reset the commit:

```bash
git tag -d v0.1.3
git push origin :refs/tags/v0.1.3
git reset --hard HEAD~1
git push origin main --force
```
