use anyhow::Context;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub root_dir: PathBuf,
    pub versions_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub current_link: PathBuf,

    pub mirror: String,
    pub quiet: bool,
    pub verbose: bool,
}

impl Config {
    pub fn new(
        root: Option<PathBuf>,
        mirror: Option<String>,
        quiet: bool,
        verbose: bool,
    ) -> anyhow::Result<Self> {
        let root_dir = match root {
            Some(p) => p,
            None => {
                let home = dirs::home_dir().context("Failed to locate HOME directory")?;
                home.join(".fgm")
            }
        };

        Ok(Self {
            versions_dir: root_dir.join("versions"),
            cache_dir: root_dir.join("cache"),
            bin_dir: root_dir.join("bin"),
            current_link: root_dir.join("current"),
            root_dir,
            mirror: mirror.unwrap_or_else(|| "https://go.dev/dl".to_string()),
            quiet,
            verbose,
        })
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.versions_dir)
            .with_context(|| format!("Failed to create directory: {}", self.versions_dir.display()))?;
        std::fs::create_dir_all(&self.cache_dir)
            .with_context(|| format!("Failed to create directory: {}", self.cache_dir.display()))?;
        std::fs::create_dir_all(&self.bin_dir)
            .with_context(|| format!("Failed to create directory: {}", self.bin_dir.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn new_uses_given_root_and_default_mirror() {
        let td = tempfile::tempdir().unwrap();
        let root = PathBuf::from(td.path());
        let cfg = Config::new(Some(root.clone()), None, true, false).unwrap();

        assert_eq!(cfg.root_dir, root);
        assert_eq!(cfg.versions_dir, cfg.root_dir.join("versions"));
        assert_eq!(cfg.cache_dir, cfg.root_dir.join("cache"));
        assert_eq!(cfg.bin_dir, cfg.root_dir.join("bin"));
        assert_eq!(cfg.current_link, cfg.root_dir.join("current"));
        assert_eq!(cfg.mirror, "https://go.dev/dl");
        assert!(cfg.quiet);
        assert!(!cfg.verbose);
    }

    #[test]
    fn ensure_dirs_creates_expected_directories() {
        let td = tempfile::tempdir().unwrap();
        let cfg = Config::new(Some(PathBuf::from(td.path())), None, true, false).unwrap();
        cfg.ensure_dirs().unwrap();

        assert!(cfg.versions_dir.exists());
        assert!(cfg.cache_dir.exists());
        assert!(cfg.bin_dir.exists());
    }
}
