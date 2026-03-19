use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::repo::registry::{is_indexed, Registry};

// ─── add ──────────────────────────────────────────────────────────────────────

pub struct AddArgs {
    pub name: String,
    pub path: PathBuf,
}

pub fn add(args: AddArgs) -> Result<()> {
    let abs = args
        .path
        .canonicalize()
        .context("path not found")?;

    let mut registry = Registry::load()?;
    registry.add(&args.name, abs.clone())?;
    registry.save()?;

    let indexed = if is_indexed(&abs) { "indexed" } else { "not yet indexed" };
    println!("Registered '{}' → {} ({})", args.name, abs.display(), indexed);
    Ok(())
}

// ─── list ─────────────────────────────────────────────────────────────────────

pub struct ListArgs;

pub fn list(_args: ListArgs) -> Result<()> {
    let registry = Registry::load()?;

    if registry.repos.is_empty() {
        println!("No repos registered. Use `codesearch repos add <path> --name <name>`.");
        return Ok(());
    }

    println!("{:<20} {:<8} {}", "NAME", "STATUS", "PATH");
    println!("{}", "-".repeat(70));
    for r in &registry.repos {
        let status = if is_indexed(&r.path) { "indexed" } else { "missing" };
        println!("{:<20} {:<8} {}", r.name, status, r.path.display());
    }
    Ok(())
}

// ─── remove ───────────────────────────────────────────────────────────────────

pub struct RemoveArgs {
    pub name: String,
}

pub fn remove(args: RemoveArgs) -> Result<()> {
    let mut registry = Registry::load()?;
    if !registry.remove(&args.name) {
        bail!("No registered repo named '{}'.", args.name);
    }
    registry.save()?;
    println!("Removed '{}'.", args.name);
    Ok(())
}
