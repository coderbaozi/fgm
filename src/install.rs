use crate::{
    config::Config,
    errors::FgmError,
    version::{GoVersion, VersionInput},
};
use anyhow::Context;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct InstallOptions {
    pub force: bool,
    pub verify: bool,
}

pub async fn fetch_latest_tag(cfg: &Config) -> anyhow::Result<String> {
    let base = cfg
        .mirror
        .trim_end_matches('/')
        .trim_end_matches("/dl")
        .trim_end_matches('/');
    let url = format!("{base}/VERSION?m=text");
    let client = reqwest::Client::new();
    let body = client
        .get(url)
        .send()
        .await
        .context("Failed to request latest version")?
        .error_for_status()
        .context("Latest version request returned error status")?
        .text()
        .await
        .context("Failed to read latest response")?;
    let tag = body.lines().next().unwrap_or("").trim().to_string();
    if tag.starts_with("go") {
        Ok(tag)
    } else {
        anyhow::bail!("Unexpected latest response format: {tag}")
    }
}

#[derive(Debug, Deserialize)]
struct DlVersionEntry {
    version: String,
    #[serde(default)]
    stable: bool,
}

fn mirror_dl_base(cfg: &Config) -> String {
    cfg.mirror.trim_end_matches('/').to_string()
}

async fn fetch_versions_index(cfg: &Config) -> anyhow::Result<Vec<DlVersionEntry>> {
    // Go official endpoint: /dl/?mode=json&include=all
    // - Compatible with mirror = https://go.dev/dl or other mirrors exposing a /dl entry.
    let dl_base = mirror_dl_base(cfg);
    let url = format!("{dl_base}/?mode=json&include=all");
    let client = reqwest::Client::new();
    let body = client
        .get(url)
        .send()
        .await
        .context("Failed to request versions index")?
        .error_for_status()
        .context("Versions index request returned error status")?
        .text()
        .await
        .context("Failed to read versions index response")?;

    let entries: Vec<DlVersionEntry> = parse_versions_index_json(&body)?;
    Ok(entries)
}

fn parse_versions_index_json(body: &str) -> anyhow::Result<Vec<DlVersionEntry>> {
    let entries: Vec<DlVersionEntry> = serde_json::from_str(body)
        .with_context(|| "Failed to parse versions index JSON")?;
    Ok(entries)
}

fn select_latest_patch_for_minor(
    entries: impl IntoIterator<Item = DlVersionEntry>,
    req: &VersionInput,
) -> Option<GoVersion> {
    let mut best: Option<GoVersion> = None;
    for e in entries {
        if !e.stable {
            continue;
        }
        let Ok(v) = GoVersion::from_tag(&e.version) else {
            continue;
        };
        if v.semver.major != req.major || v.semver.minor != req.minor {
            continue;
        }
        match &best {
            None => best = Some(v),
            Some(cur) => {
                if v.semver > cur.semver {
                    best = Some(v)
                }
            }
        }
    }
    best
}

/// Resolve `1.22` to the latest stable patch for that minor (e.g. `go1.22.4`).
pub async fn resolve_latest_patch_for_minor(
    cfg: &Config,
    req: &VersionInput,
) -> anyhow::Result<GoVersion> {
    if req.patch.is_some() {
        anyhow::bail!("This function expects input without patch")
    }
    if req.prerelease.is_some() {
        anyhow::bail!("This function does not handle rc/beta versions")
    }

    let entries = fetch_versions_index(cfg).await?;
    select_latest_patch_for_minor(entries, req)
        .with_context(|| {
            format!(
                "No available version found: {}.{} (check mirror or network)",
                req.major, req.minor
            )
        })
}

pub async fn install(cfg: &Config, v: &GoVersion, opt: InstallOptions) -> anyhow::Result<()> {
    let suffix = platform_suffix()?;
    let filename = format!("{}.{}.tar.gz", v.tag, suffix);
    let url = format!("{}/{}", cfg.mirror.trim_end_matches('/'), filename);
    let sha_url = format!("{url}.sha256");
    let cache_file = cfg.cache_dir.join(&filename);

    if cache_file.exists() {
        if opt.verify {
            let expected = fetch_sha256(&sha_url).await?;
            let actual = sha256_file_hex(&cache_file).context("Failed to compute cached file SHA256")?;
            if !eq_hex(&expected, &actual) {
                // Cache corrupted; redownloading.
                let _ = std::fs::remove_file(&cache_file);
            }
        }
    }

    if !cache_file.exists() {
        download_to_cache(
            &url,
            &cache_file,
            if opt.verify { Some(&sha_url) } else { None },
            !cfg.quiet,
        )
        .await?;
    }

    let target_dir = cfg.versions_dir.join(&v.tag);
    if target_dir.exists() {
        if opt.force {
            std::fs::remove_dir_all(&target_dir)
                .with_context(|| format!("Failed to remove existing version: {}", target_dir.display()))?;
        } else {
            anyhow::bail!(
                "Version already exists: {} (use --force to overwrite)",
                target_dir.display()
            );
        }
    }

    // Atomic install: unpack into a temp directory, then rename.
    let tempdir = tempfile::Builder::new()
        .prefix("fgm-install-")
        .tempdir_in(&cfg.versions_dir)
        .with_context(|| format!("Failed to create temp directory: {}", cfg.versions_dir.display()))?;
    let temp_path = tempdir.keep();

    let unpack_pb = if cfg.quiet {
        None
    } else {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_message(format!("Unpacking {filename}"));
        Some(pb)
    };

    unpack_go_tgz(&cache_file, &temp_path)
        .with_context(|| format!("Unpack failed: {}", cache_file.display()))?;

    if let Some(pb) = unpack_pb {
        pb.finish_with_message(format!("Unpacked {filename}"));
    }

    let go_root = temp_path.join("go");
    if !go_root.exists() {
        let _ = std::fs::remove_dir_all(&temp_path);
        anyhow::bail!(
            "Unexpected unpack result: missing go/ directory: {}",
            temp_path.display()
        );
    }
    std::fs::rename(&temp_path, &target_dir)
        .with_context(|| {
            format!(
                "Failed to move install directory: {} -> {}",
                temp_path.display(),
                target_dir.display()
            )
        })?;

    // Basic check: go executable exists.
    let go_bin = target_dir.join("go/bin/go");
    if !go_bin.exists() {
        anyhow::bail!("Unexpected install result: missing {}", go_bin.display());
    }

    Ok(())
}

fn platform_suffix() -> anyhow::Result<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => {
            return Err(FgmError::UnsupportedPlatform {
                os: other.to_string(),
                arch: std::env::consts::ARCH.to_string(),
            }
            .into())
        }
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => {
            return Err(FgmError::UnsupportedPlatform {
                os: std::env::consts::OS.to_string(),
                arch: other.to_string(),
            }
            .into())
        }
    };
    Ok(format!("{os}-{arch}"))
}

async fn fetch_sha256(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    // Compatible with go.dev "pseudo-redirect" pages:
    // - 200 + HTML + Location: /dl/#<filename>.sha256
    // Also compatible with mirrors that return plain-text sha256.
    let mut candidates: Vec<String> = vec![url.to_string()];

    if !url.contains("dl.google.com/go/") {
        if let Some(name) = sha_filename_from_url(url) {
            candidates.push(google_dl_sha256_url(&name));
        }
    }

    // Deduplicate (keep order).
    let mut uniq: Vec<String> = Vec::new();
    for u in candidates {
        if !uniq.iter().any(|x| x == &u) {
            uniq.push(u);
        }
    }

    let mut last_err: Option<anyhow::Error> = None;
    for u in uniq {
        let resp = client
            .get(&u)
            .send()
            .await
            .with_context(|| format!("Failed to request SHA256: {u}"))?
            .error_for_status()
            .with_context(|| format!("SHA256 request returned error status: {u}"))?;

        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let txt = resp
            .text()
            .await
            .with_context(|| format!("Failed to read SHA256 response: {u}"))?;

        if let Some(hex) = parse_sha256_text(&txt) {
            return Ok(hex);
        }

        // If it's HTML, try to extract filename from Location or meta refresh and retry.
        if looks_like_html(&txt) {
            if let Some(name) = sha_filename_from_html(&txt, location.as_deref()) {
                let u2 = google_dl_sha256_url(&name);
                let txt2 = client
                    .get(&u2)
                    .send()
                    .await
                    .with_context(|| format!("Failed to request SHA256: {u2}"))?
                    .error_for_status()
                    .with_context(|| format!("SHA256 request returned error status: {u2}"))?
                    .text()
                    .await
                    .with_context(|| format!("Failed to read SHA256 response: {u2}"))?;

                if let Some(hex) = parse_sha256_text(&txt2) {
                    return Ok(hex);
                }
            }
        }

        last_err = Some(anyhow::anyhow!(
            "Unexpected SHA256 response format (url={u}): {}",
            truncate_for_error(&txt)
        ));
    }

    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!("Unexpected SHA256 response format: {url}")
    }))
}

fn parse_sha256_text(txt: &str) -> Option<String> {
    // Go official format: "<hex>  <filename>" or "<hex>" (dl.google.com)
    let token = txt.trim().split_whitespace().next()?;
    if token.len() != 64 {
        return None;
    }
    if !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(token.to_ascii_lowercase())
}

fn looks_like_html(txt: &str) -> bool {
    let t = txt.trim_start();
    t.starts_with("<!DOCTYPE html") || t.starts_with("<html")
}

fn sha_filename_from_url(url: &str) -> Option<String> {
    let name = url.split('/').last()?;
    if name.ends_with(".sha256") {
        Some(name.to_string())
    } else {
        None
    }
}

fn sha_filename_from_html(body: &str, location: Option<&str>) -> Option<String> {
    // Prefer Location: /dl/#<filename>.sha256
    if let Some(loc) = location {
        if let Some(name) = extract_after_hash(loc) {
            if name.ends_with(".sha256") {
                return Some(name);
            }
        }
    }

    // Compatible with: <meta http-equiv="refresh" content="0; url=/dl/#...sha256">
    let lower = body.to_ascii_lowercase();
    if let Some(i) = lower.find("url=/dl/#") {
        let s = &body[i..];
        if let Some(hash_i) = s.find('#') {
            let after = &s[hash_i + 1..];
            let name = take_filename_prefix(after);
            if name.ends_with(".sha256") {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn extract_after_hash(s: &str) -> Option<String> {
    let idx = s.find('#')?;
    Some(s[idx + 1..].trim().trim_start_matches('/').to_string())
}

fn google_dl_sha256_url(name: &str) -> String {
    format!("https://dl.google.com/go/{}", name.trim_start_matches('/'))
}

fn truncate_for_error(s: &str) -> String {
    const MAX: usize = 240;
    let t = s.trim();
    if t.chars().count() <= MAX {
        return t.to_string();
    }
    let head: String = t.chars().take(MAX).collect();
    format!("{head}...")
}

fn take_filename_prefix(s: &str) -> &str {
    // Keep common filename characters to avoid carrying quotes/angle brackets from HTML.
    let end = s
        .char_indices()
        .find(|(_, c)| {
            !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' ))
        })
        .map(|(i, _)| i)
        .unwrap_or_else(|| s.len());
    s[..end].trim()
}

async fn download_to_cache(
    url: &str,
    dest: &Path,
    sha_url: Option<&str>,
    show_progress: bool,
) -> anyhow::Result<()> {
    let expected = if let Some(sha_url) = sha_url {
        Some(fetch_sha256(sha_url).await?)
    } else {
        None
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Download failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("Download returned error status: {url}"))?;

    let filename = url
        .split('/')
        .last()
        .filter(|s| !s.is_empty())
        .unwrap_or("archive");
    let total = resp.content_length();

    let pb = if !show_progress {
        None
    } else if let Some(total) = total {
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::with_template(
                "{msg} [{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        pb.set_message(format!("Downloading {filename}"));
        Some(pb)
    } else {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_message(format!("Downloading {filename}"));
        Some(pb)
    };

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let tmp = PathBuf::from(format!("{}.part", dest.display()));
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .with_context(|| format!("Failed to create file: {}", tmp.display()))?;

    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read download stream")?;
        hasher.update(&chunk);
        if let Some(pb) = &pb {
            pb.inc(chunk.len() as u64);
        }
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .context("Failed to write download data")?;
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .context("Flush failed")?;

    let actual = hex::encode(hasher.finalize());
    if let Some(expected) = expected {
        if !eq_hex(&expected, &actual) {
            let _ = tokio::fs::remove_file(&tmp).await;
            if let Some(pb) = pb {
                pb.finish_and_clear();
            }
            return Err(FgmError::ChecksumMismatch { expected, actual }.into());
        }
    }

    tokio::fs::rename(&tmp, dest)
        .await
        .with_context(|| format!("Failed to move file: {} -> {}", tmp.display(), dest.display()))?;

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Downloaded {filename}"));
    }
    Ok(())
}

fn sha256_file_hex(path: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf).context("Failed to read file")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn unpack_go_tgz(tgz: &Path, dest_dir: &Path) -> anyhow::Result<()> {
    let f = std::fs::File::open(tgz)
        .with_context(|| format!("Failed to open file: {}", tgz.display()))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    ar.unpack(dest_dir)
        .with_context(|| format!("Failed to unpack tar: {}", dest_dir.display()))?;
    Ok(())
}

fn eq_hex(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        shim,
        version::{self, GoVersion},
    };
    use flate2::{write::GzEncoder, Compression};
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use tar::Header;

    fn write_fake_go_tgz(path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let f = fs::File::create(path)
            .with_context(|| format!("Failed to create file: {}", path.display()))?;
        let enc = GzEncoder::new(f, Compression::default());
        let mut tar = tar::Builder::new(enc);

        // go/bin/go
        add_file(&mut tar, "go/bin/go", b"#!/bin/sh\necho fake-go\n", 0o755)?;
        // go/bin/gofmt
        add_file(
            &mut tar,
            "go/bin/gofmt",
            b"#!/bin/sh\necho fake-gofmt\n",
            0o755,
        )?;

        let enc = tar.into_inner().context("Failed to finish writing tar")?;
        enc.finish().context("Failed to finish writing gzip")?;
        Ok(())
    }

    fn add_file(
        tar: &mut tar::Builder<GzEncoder<fs::File>>,
        rel: &str,
        data: &[u8],
        mode: u32,
    ) -> anyhow::Result<()> {
        let mut h = Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_cksum();
        tar.append_data(&mut h, rel, data)
            .with_context(|| format!("Failed to write tar entry: {rel}"))?;
        Ok(())
    }

    fn temp_cfg() -> anyhow::Result<(tempfile::TempDir, Config)> {
        let td = tempfile::tempdir().context("Failed to create temp directory")?;
        let cfg = Config::new(
            Some(PathBuf::from(td.path())),
            Some("https://example.invalid".to_string()),
            true,
            false,
        )?;
        cfg.ensure_dirs()?;
        Ok((td, cfg))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn offline_install_use_current_uninstall_flow() -> anyhow::Result<()> {
        let (_td, cfg) = temp_cfg()?;

        let v = GoVersion::from_tag("go1.22.4")?;
        let suffix = platform_suffix()?;
        let filename = format!("{}.{}.tar.gz", v.tag, suffix);
        let cache_file = cfg.cache_dir.join(filename);
        write_fake_go_tgz(&cache_file)?;

        install(
            &cfg,
            &v,
            InstallOptions {
                force: false,
                verify: false,
            },
        )
        .await?;

        let go_bin = cfg
            .versions_dir
            .join(&v.tag)
            .join("go/bin/go");
        assert!(go_bin.exists(), "go binary missing: {}", go_bin.display());

        shim::set_current(&cfg, &v)?;
        let cur = shim::current(&cfg)?;
        assert_eq!(cur.as_ref().map(|x| x.tag.as_str()), Some(v.tag.as_str()));

        version::uninstall(&cfg, &v.tag)?;
        assert!(!cfg.versions_dir.join(&v.tag).exists());
        assert!(shim::current(&cfg)?.is_none());

        Ok(())
    }

    #[test]
    fn select_latest_patch_for_minor_picks_highest_stable() {
        let req = VersionInput::parse("1.22").unwrap();
        let json = r#"
        [
          {"version":"go1.22beta1","stable":false},
          {"version":"go1.22.3","stable":true},
          {"version":"go1.22.4","stable":true},
          {"version":"go1.21.9","stable":true},
          {"version":"go1.22.5"}
        ]
        "#;
        let entries = parse_versions_index_json(json).unwrap();
        let got = select_latest_patch_for_minor(entries, &req).unwrap();
        assert_eq!(got.tag, "go1.22.4");
    }

    #[test]
    fn parse_versions_index_json_rejects_invalid_json() {
        let err = parse_versions_index_json("not json").unwrap_err();
        assert!(err.to_string().contains("Failed to parse versions index JSON"));
    }

    #[test]
    fn sha256_file_hex_matches_known_value() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("x");
        std::fs::write(&p, b"abc").unwrap();
        let got = sha256_file_hex(&p).unwrap();
        // SHA256("abc")
        assert_eq!(
            got,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn eq_hex_is_case_insensitive_and_trims() {
        assert!(eq_hex(" AA ", "aa"));
        assert!(eq_hex("aA", "Aa"));
        assert!(!eq_hex("aa", "ab"));
    }

    #[test]
    fn parse_sha256_text_accepts_plain_or_with_filename() {
        let hex = "416c35218edb9d20990b5d8fc87be655d8b39926f15524ea35c66ee70273050d";
        assert_eq!(parse_sha256_text(hex).as_deref(), Some(hex));
        assert_eq!(
            parse_sha256_text(&format!("{hex}  go1.22.12.darwin-arm64.tar.gz")).as_deref(),
            Some(hex)
        );
        assert!(parse_sha256_text("not-a-sha").is_none());
    }

    #[test]
    fn sha_filename_from_html_extracts_from_location_or_meta_refresh() {
        let html = r#"<!DOCTYPE html>
<html>
<head>
<meta name="go-import" content="golang.org/dl git https://go.googlesource.com/dl">
<meta http-equiv="refresh" content="0; url=/dl/#go1.22.12.darwin-arm64.tar.gz.sha256">
</head>
<body></body>
</html>"#;

        assert_eq!(
            sha_filename_from_html(html, Some("/dl/#go1.22.12.darwin-arm64.tar.gz.sha256"))
                .as_deref(),
            Some("go1.22.12.darwin-arm64.tar.gz.sha256")
        );
        assert_eq!(
            sha_filename_from_html(html, None).as_deref(),
            Some("go1.22.12.darwin-arm64.tar.gz.sha256")
        );
    }

    #[test]
    fn platform_suffix_matches_current_target() {
        let got = platform_suffix().unwrap();
        let expected_os = match std::env::consts::OS {
            "macos" => "darwin",
            "linux" => "linux",
            other => other,
        };
        let expected_arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            other => other,
        };

        if expected_os == "darwin" || expected_os == "linux" {
            if expected_arch == "amd64" || expected_arch == "arm64" {
                assert_eq!(got, format!("{expected_os}-{expected_arch}"));
            }
        }
    }
}
