use crate::config::Config;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FgmError {
    #[error("Unsupported platform: os={os}, arch={arch}")]
    UnsupportedPlatform { os: String, arch: String },

    #[error("Invalid version format: {0}")]
    InvalidVersion(String),

    #[error("SHA256 verification failed: expected={expected}, actual={actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

pub fn print_doctor(cfg: &Config) -> anyhow::Result<()> {
    println!("fgm root: {}", cfg.root_dir.display());
    println!("versions : {}", cfg.versions_dir.display());
    println!("cache    : {}", cfg.cache_dir.display());
    println!("bin      : {}", cfg.bin_dir.display());
    println!("mirror   : {}", cfg.mirror);
    println!("os/arch  : {}/{}", std::env::consts::OS, std::env::consts::ARCH);

    let current = crate::shim::current(cfg)?;
    match current {
        Some(v) => {
            println!("current  : {}", v.tag);
            let go = cfg.bin_dir.join("go");
            println!("shim(go) : {}", go.display());
        }
        None => {
            println!("current  : (not set)");
            println!(
                "hint     : run fgm use <version> and add {} to PATH",
                cfg.bin_dir.display()
            );
        }
    }
    Ok(())
}
