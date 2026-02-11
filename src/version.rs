use crate::errors::FgmError;
use anyhow::Context;
use regex::Regex;
use semver::Version;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct VersionInput {
    pub raw: String,
    pub major: u64,
    pub minor: u64,
    pub patch: Option<u64>,
    pub prerelease: Option<(String, u64)>,
}

impl VersionInput {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let raw = s.trim().to_string();
        let mut t = raw.as_str();
        if let Some(rest) = t.strip_prefix("go") {
            t = rest;
        }
        let re = Regex::new(
            r"^(?P<maj>\d+)\.(?P<min>\d+)(?:\.(?P<patch>\d+))?(?P<suffix>(?:rc|beta)\d+)?$",
        )
        .expect("regex compile");
        let caps = re
            .captures(t)
            .ok_or_else(|| FgmError::InvalidVersion(raw.clone()))?;
        let major: u64 = caps["maj"].parse().context("parse major")?;
        let minor: u64 = caps["min"].parse().context("parse minor")?;
        let patch: Option<u64> = caps.name("patch").map(|m| m.as_str().parse()).transpose()?;

        let prerelease = caps.name("suffix").map(|m| {
            let suf = m.as_str();
            if let Some(n) = suf.strip_prefix("rc") {
                ("rc".to_string(), n.parse::<u64>().unwrap_or(0))
            } else if let Some(n) = suf.strip_prefix("beta") {
                ("beta".to_string(), n.parse::<u64>().unwrap_or(0))
            } else {
                ("pre".to_string(), 0)
            }
        });

        Ok(Self {
            raw,
            major,
            minor,
            patch,
            prerelease,
        })
    }

    pub fn matches_tag(&self, tag: &str) -> bool {
        if let Ok(v) = GoVersion::from_tag(tag) {
            if v.semver.major != self.major || v.semver.minor != self.minor {
                return false;
            }
            if let Some(p) = self.patch {
                if v.semver.patch != p {
                    return false;
                }
            }
            if let Some((kind, n)) = &self.prerelease {
                let want = format!("{kind}.{n}");
                if v.semver.pre.as_str() != want {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoVersion {
    pub tag: String,
    pub semver: Version,
}

impl GoVersion {
    pub fn from_input(input: &VersionInput) -> anyhow::Result<Self> {
        let patch = input.patch.unwrap_or(0);
        let mut v = Version::new(input.major, input.minor, patch);
        if let Some((kind, n)) = &input.prerelease {
            v.pre = semver::Prerelease::new(&format!("{kind}.{n}"))
                .context("invalid prerelease")?;
        }
        let tag = if input.prerelease.is_some() && input.patch.is_none() {
            // Go official pre-release tags look like: go1.22rc1 / go1.22beta1 (no .0)
            let (kind, n) = input
                .prerelease
                .as_ref()
                .expect("prerelease checked")
                .clone();
            format!("go{}.{}{}{}", input.major, input.minor, kind, n)
        } else {
            format!("go{}.{}.{}", input.major, input.minor, patch)
        };
        Ok(Self {
            tag,
            semver: v,
        })
    }

    pub fn from_tag(tag: &str) -> anyhow::Result<Self> {
        let input = VersionInput::parse(tag)?;
        let mut v = Self::from_input(&input)?;
        v.tag = if tag.starts_with("go") {
            tag.to_string()
        } else {
            format!("go{}", tag)
        };
        Ok(v)
    }

    pub fn install_root(&self, versions_dir: &PathBuf) -> PathBuf {
        versions_dir.join(&self.tag)
    }

    pub fn go_root(&self, versions_dir: &PathBuf) -> PathBuf {
        self.install_root(versions_dir).join("go")
    }
}

pub fn installed_versions(cfg: &crate::config::Config) -> anyhow::Result<Vec<GoVersion>> {
    let mut out = Vec::new();
    for ent in std::fs::read_dir(&cfg.versions_dir)
        .with_context(|| format!("Failed to read directory: {}", cfg.versions_dir.display()))?
    {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("go") {
            continue;
        }
        if let Ok(v) = GoVersion::from_tag(&name) {
            out.push(v);
        }
    }
    out.sort_by(|a, b| a.semver.cmp(&b.semver));
    Ok(out)
}

pub fn resolve_installed(
    cfg: &crate::config::Config,
    req: &VersionInput,
) -> anyhow::Result<Option<GoVersion>> {
    let mut matched = installed_versions(cfg)?
        .into_iter()
        .filter(|v| req.matches_tag(&v.tag))
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| b.semver.cmp(&a.semver));
    Ok(matched.into_iter().next())
}

pub fn uninstall(cfg: &crate::config::Config, tag: &str) -> anyhow::Result<()> {
    let dir = cfg.versions_dir.join(tag);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("Failed to remove directory: {}", dir.display()))?;
    }
    if let Some(cur) = crate::shim::current(cfg)? {
        if cur.tag == tag {
            crate::shim::clear_current(cfg)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::{fs, path::PathBuf};

    #[test]
    fn parse_version_inputs() {
        let a = VersionInput::parse("go1.22.4").unwrap();
        assert_eq!((a.major, a.minor, a.patch), (1, 22, Some(4)));
        let b = VersionInput::parse("1.22").unwrap();
        assert_eq!((b.major, b.minor, b.patch), (1, 22, None));
        let c = VersionInput::parse("go1.21.0").unwrap();
        assert_eq!((c.major, c.minor, c.patch), (1, 21, Some(0)));

        let d = VersionInput::parse("go1.22rc1").unwrap();
        assert_eq!((d.major, d.minor, d.patch), (1, 22, None));
        assert_eq!(d.prerelease.as_ref().map(|x| (x.0.as_str(), x.1)), Some(("rc", 1)));

        let dv = GoVersion::from_input(&d).unwrap();
        assert_eq!(dv.tag, "go1.22rc1");
        assert_eq!(dv.semver.to_string(), "1.22.0-rc.1");
    }

    #[test]
    fn matches_tag_supports_minor_only() {
        let req = VersionInput::parse("1.22").unwrap();
        assert!(req.matches_tag("go1.22.4"));
        assert!(req.matches_tag("go1.22.0"));
        assert!(!req.matches_tag("go1.21.9"));
    }

    #[test]
    fn resolve_installed_picks_highest_patch() {
        let td = tempfile::tempdir().unwrap();
        let cfg = Config::new(
            Some(PathBuf::from(td.path())),
            Some("https://example.invalid".to_string()),
            true,
            false,
        )
        .unwrap();
        cfg.ensure_dirs().unwrap();

        for tag in ["go1.22.3", "go1.22.4", "go1.21.9"] {
            fs::create_dir_all(cfg.versions_dir.join(tag)).unwrap();
        }

        let req = VersionInput::parse("1.22").unwrap();
        let got = resolve_installed(&cfg, &req).unwrap().unwrap();
        assert_eq!(got.tag, "go1.22.4");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_clears_current_when_matches() {
        let td = tempfile::tempdir().unwrap();
        let cfg = Config::new(
            Some(PathBuf::from(td.path())),
            Some("https://example.invalid".to_string()),
            true,
            false,
        )
        .unwrap();
        cfg.ensure_dirs().unwrap();

        let v = GoVersion::from_tag("go1.22.4").unwrap();
        let go_root = v.go_root(&cfg.versions_dir);
        fs::create_dir_all(go_root.join("bin")).unwrap();
        fs::write(go_root.join("bin/go"), b"fake").unwrap();
        fs::write(go_root.join("bin/gofmt"), b"fake").unwrap();
        crate::shim::set_current(&cfg, &v).unwrap();

        uninstall(&cfg, &v.tag).unwrap();
        assert!(!cfg.versions_dir.join(&v.tag).exists());
        assert!(crate::shim::current(&cfg).unwrap().is_none());
    }
}
