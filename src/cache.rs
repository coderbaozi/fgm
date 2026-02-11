use anyhow::Context;
use std::path::Path;
use walkdir::WalkDir;

pub fn dir_size_bytes(path: &Path) -> anyhow::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut sum = 0u64;
    for ent in WalkDir::new(path) {
        let ent = ent?;
        if ent.file_type().is_file() {
            sum = sum.saturating_add(ent.metadata()?.len());
        }
    }
    Ok(sum)
}

pub fn clean_dir(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove directory: {}", path.display()))?;
    }
    std::fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    Ok(())
}

pub fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let f = n as f64;
    if f >= GB {
        format!("{:.2} GiB", f / GB)
    } else if f >= MB {
        format!("{:.2} MiB", f / MB)
    } else if f >= KB {
        format!("{:.2} KiB", f / KB)
    } else {
        format!("{n} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn dir_size_bytes_missing_dir_returns_zero() {
        let td = tempfile::tempdir().unwrap();
        let missing = td.path().join("missing");
        let sz = dir_size_bytes(&missing).unwrap();
        assert_eq!(sz, 0);
    }

    #[test]
    fn dir_size_bytes_counts_files_only() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("d");
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("a"), vec![0u8; 3]).unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("b"), vec![0u8; 5]).unwrap();

        let sz = dir_size_bytes(&dir).unwrap();
        assert_eq!(sz, 8);
    }

    #[test]
    fn clean_dir_removes_contents_and_recreates() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("cache");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("x"), b"hello").unwrap();

        clean_dir(&dir).unwrap();
        assert!(dir.exists());
        assert!(!dir.join("x").exists());
    }
}
