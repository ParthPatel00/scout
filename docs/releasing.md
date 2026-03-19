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

A release is triggered by pushing a version tag (`v0.1.4`, `v0.2.0`, etc.). The GitHub Actions
workflow builds Scout for all 5 platforms and attaches the binaries to a release page automatically.

**The golden rule: always let CI pass on `main` before pushing a tag.**

---

### Scenario A — clean release (recommended)

Use `cargo-release` to bump, commit, tag, and push in one step.

**One-time setup:**
```bash
cargo install cargo-release
```

**Release commands:**
```bash
cargo release patch   # bug fixes:    0.1.3 → 0.1.4
cargo release minor   # new features: 0.1.3 → 0.2.0
cargo release major   # breaking:     0.1.3 → 1.0.0
```

That single command does everything:
1. Bumps `Cargo.toml` version
2. Runs `cargo test`
3. Updates `Cargo.lock`
4. Commits with message `release 0.1.4`
5. Creates tag `v0.1.4`
6. Pushes commit + tag to `origin main`

GitHub Actions picks up the tag and builds + publishes the release (~5 minutes).

---

### Scenario B — manual release (step-by-step)

If you prefer to do it manually, or `cargo-release` isn't installed:

```bash
# 1. Make and push your changes as normal commits
git add <files>
git commit -m "feat: your change"
git push origin main

# 2. Wait for CI to go green (Actions tab on GitHub)

# 3. Bump the version in Cargo.toml, then update Cargo.lock
#    Edit Cargo.toml: version = "0.1.4"
cargo update --workspace

# 4. Commit both files
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to v0.1.4"
git push origin main

# 5. Wait for CI to go green again

# 6. Tag the passing commit and push the tag
git tag v0.1.4
git push origin v0.1.4
```

The Release workflow fires on step 6 and publishes the binaries.

---

### Scenario C — you tagged the wrong commit

You pushed a tag but then made another fix commit and need to retag.

```bash
# Delete the tag locally and on remote
git tag -d v0.1.4
git push origin :refs/tags/v0.1.4

# Make your fix, push, wait for CI to pass, then retag
git tag v0.1.4
git push origin v0.1.4
```

Or bump to a new patch version instead (cleaner, no force-pushing):
```bash
# Edit Cargo.toml: version = "0.1.5"
cargo update --workspace
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to v0.1.5"
git push origin main
# wait for CI...
git tag v0.1.5
git push origin v0.1.5
```

---

## What CI does when you push a tag

1. `ci.yml` — runs tests on the commit (same as any push)
2. `release.yml` — detects the tag and builds:
   - macOS Apple Silicon (`aarch64-apple-darwin`)
   - macOS Intel (`x86_64-apple-darwin`)
   - Linux x86_64 (`x86_64-unknown-linux-gnu`)
   - Linux ARM64 (`aarch64-unknown-linux-gnu`) — cross-compiled
   - Windows x86_64 (`x86_64-pc-windows-msvc`)
3. Binaries are packaged (`.tar.gz` / `.zip`) with SHA256 checksums
4. A GitHub Release page is created automatically with the binaries attached

Watch it live under the **Actions** tab on GitHub.

---

## Versioning convention (semver)

| Change | Example | When to use |
|--------|---------|-------------|
| Patch  | `0.1.3 → 0.1.4` | Bug fixes, no new features |
| Minor  | `0.1.3 → 0.2.0` | New features, backwards compatible |
| Major  | `0.1.3 → 1.0.0` | Breaking changes to CLI or data format |

---

## If the release workflow fails

**Fix the code, delete the tag, re-tag:**
```bash
git tag -d v0.1.4
git push origin :refs/tags/v0.1.4
# fix the issue, push, wait for CI, then:
git tag v0.1.4
git push origin v0.1.4
```

**Undo the release commit entirely:**
```bash
git tag -d v0.1.4
git push origin :refs/tags/v0.1.4
git reset --hard HEAD~1
git push origin main --force
```
