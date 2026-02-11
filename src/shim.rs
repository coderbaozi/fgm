use crate::{config::Config, version::GoVersion};
use anyhow::Context;
use std::path::PathBuf;

#[cfg(unix)]
fn force_symlink(target: &PathBuf, link: &PathBuf) -> anyhow::Result<()> {
    if link.exists() || std::fs::symlink_metadata(link).is_ok() {
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_dir_all(link);
    }
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("Failed to create symlink: {} -> {}", link.display(), target.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn force_symlink(_target: &PathBuf, _link: &PathBuf) -> anyhow::Result<()> {
    anyhow::bail!("Symlink-based switching is not supported on this platform")
}

pub fn set_current(cfg: &Config, v: &GoVersion) -> anyhow::Result<()> {
    let go_root = v.go_root(&cfg.versions_dir);
    if !go_root.exists() {
        anyhow::bail!(
            "Target version is not installed: {} (missing {})",
            v.tag,
            go_root.display()
        );
    }
    force_symlink(&go_root, &cfg.current_link)?;

    let go = cfg.bin_dir.join("go");
    let gofmt = cfg.bin_dir.join("gofmt");
    force_symlink(&cfg.current_link.join("bin/go"), &go)?;
    force_symlink(&cfg.current_link.join("bin/gofmt"), &gofmt)?;
    Ok(())
}

pub fn clear_current(cfg: &Config) -> anyhow::Result<()> {
    if cfg.current_link.exists() || std::fs::symlink_metadata(&cfg.current_link).is_ok() {
        let _ = std::fs::remove_file(&cfg.current_link);
        let _ = std::fs::remove_dir_all(&cfg.current_link);
    }
    for name in ["go", "gofmt"] {
        let p = cfg.bin_dir.join(name);
        if p.exists() || std::fs::symlink_metadata(&p).is_ok() {
            let _ = std::fs::remove_file(&p);
            let _ = std::fs::remove_dir_all(&p);
        }
    }
    Ok(())
}

pub fn current(cfg: &Config) -> anyhow::Result<Option<GoVersion>> {
    let target = match std::fs::read_link(&cfg.current_link) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let tag = target
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string());
    let Some(tag) = tag else { return Ok(None) };
    Ok(Some(GoVersion::from_tag(&tag)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::GoVersion;
    use std::{fs, path::PathBuf};

    fn temp_cfg() -> (tempfile::TempDir, Config) {
        let td = tempfile::tempdir().unwrap();
        let cfg = Config::new(
            Some(PathBuf::from(td.path())),
            Some("https://example.invalid".to_string()),
            true,
            false,
        )
        .unwrap();
        cfg.ensure_dirs().unwrap();
        (td, cfg)
    }

    #[test]
    fn set_current_fails_if_target_not_installed() {
        let (_td, cfg) = temp_cfg();
        let v = GoVersion::from_tag("go1.22.4").unwrap();
        let err = set_current(&cfg, &v).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Target version is not installed"));
    }

    #[cfg(unix)]
    #[test]
    fn set_current_current_and_clear_current_work() {
        let (_td, cfg) = temp_cfg();
        let v = GoVersion::from_tag("go1.22.4").unwrap();

        // Prepare minimal directory layout: versions/goX/go/bin/go|gofmt
        let go_root = v.go_root(&cfg.versions_dir);
        fs::create_dir_all(go_root.join("bin")).unwrap();
        fs::write(go_root.join("bin/go"), b"fake").unwrap();
        fs::write(go_root.join("bin/gofmt"), b"fake").unwrap();

        set_current(&cfg, &v).unwrap();
        let cur = current(&cfg).unwrap().unwrap();
        assert_eq!(cur.tag, v.tag);

        // go/gofmt under bin should be symlinks (at least should exist).
        assert!(cfg.bin_dir.join("go").exists());
        assert!(cfg.bin_dir.join("gofmt").exists());

        clear_current(&cfg).unwrap();
        assert!(current(&cfg).unwrap().is_none());
        assert!(!cfg.bin_dir.join("go").exists());
        assert!(!cfg.bin_dir.join("gofmt").exists());
    }
}
