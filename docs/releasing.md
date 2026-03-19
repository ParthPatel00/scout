# How to publish a release

A GitHub Release is the page where users download Scout. The install commands
in the README point directly to it. Publishing one is a 4-step process.

---

## What actually happens

Pushing code to `main` runs tests only — no binaries are published.

A release is triggered by pushing a **version tag** (e.g. `v0.2.0`). When
GitHub sees a tag matching `v*.*.*`, the release workflow kicks off, builds
Scout for all 5 platforms in parallel (~5 minutes), and attaches the download
files to a new release page automatically.

---

## Step-by-step

### 1. Bump the version in `Cargo.toml`

```toml
[package]
name = "scout"
version = "0.2.0"   # ← change this
```

### 2. Commit it

```bash
git add Cargo.toml
git commit -m "release v0.2.0"
```

### 3. Tag it

```bash
git tag v0.2.0
```

The tag must match the pattern `v<major>.<minor>.<patch>` — that's what
triggers the release workflow.

### 4. Push both the commit and the tag

```bash
git push origin main
git push origin v0.2.0
```

---

## What GitHub does next

1. `ci.yml` runs tests on the commit (as normal).
2. `release.yml` detects the new tag and starts building:
   - macOS Apple Silicon (`aarch64-apple-darwin`)
   - macOS Intel (`x86_64-apple-darwin`)
   - Linux x86_64 (`x86_64-unknown-linux-gnu`)
   - Linux ARM64 (`aarch64-unknown-linux-gnu`)
   - Windows x86_64 (`x86_64-pc-windows-msvc`)
3. Each binary is packaged (`.tar.gz` or `.zip`) with a SHA256 checksum.
4. A release page is created at:
   `https://github.com/ParthPatel00/scout/releases/tag/v0.2.0`
5. All download files are attached to that page automatically.

The whole process takes about 5 minutes. You can watch it live under the
**Actions** tab on GitHub.

---

## After the release

The README install commands use `/releases/latest/download/` which always
points to the most recent tag — no need to update them.

```bash
# This always downloads the latest release, whatever version that is
curl -L https://github.com/ParthPatel00/scout/releases/latest/download/scout-aarch64-apple-darwin.tar.gz | tar xz
```

---

## Versioning convention

Follow [Semantic Versioning](https://semver.org):

| Change | Example | When to use |
|--------|---------|-------------|
| Patch  | `0.1.0` → `0.1.1` | Bug fixes, no new features |
| Minor  | `0.1.0` → `0.2.0` | New features, backwards compatible |
| Major  | `0.1.0` → `1.0.0` | Breaking changes |

---

## If something goes wrong

**The workflow failed mid-build** — fix the issue, delete the tag, re-tag, and push again:

```bash
git tag -d v0.2.0                      # delete local tag
git push origin :refs/tags/v0.2.0      # delete remote tag
# fix the issue, then re-tag
git tag v0.2.0
git push origin v0.2.0
```

**You tagged the wrong commit** — same process: delete both tags, re-tag the right commit.
