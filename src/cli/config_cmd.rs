use anyhow::Result;

use crate::config::Config;

pub struct SetArgs {
    pub key: String,
    pub value: String,
}

pub struct GetArgs {
    pub key: String,
}

pub fn set(args: SetArgs) -> Result<()> {
    let mut cfg = Config::load();
    cfg.set(&args.key, &args.value)?;
    cfg.save()?;
    println!("Set {} = {}", args.key, args.value);
    Ok(())
}

pub fn get(args: GetArgs) -> Result<()> {
    let cfg = Config::load();
    println!("{}", cfg.get(&args.key)?);
    Ok(())
}

pub fn list() -> Result<()> {
    Config::load().list();
    Ok(())
}
