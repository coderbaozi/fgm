use crate::{
    cache,
    config::Config,
    install,
    shim,
    version::{GoVersion, VersionInput},
};
use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::Path;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "fgm", version, about = "Fast Go version manager", long_about = None)]
pub struct Cli {
    /// fgm root directory (default: ~/.fgm)
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    /// Base download URL (default: https://go.dev/dl)
    #[arg(long, global = true)]
    pub mirror: Option<String>,

    /// Reduce output
    #[arg(long, global = true, default_value_t = false)]
    pub quiet: bool,

    /// Print more debug information
    #[arg(long, global = true, default_value_t = false)]
    pub verbose: bool,

    #[command(subcommand)]
    pub cmd: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Install a Go version (download from official or mirror)
    Install {
        /// Version: supports go1.22.4 / 1.22.4 / 1.22 / latest
        version: String,

        /// Overwrite if already installed
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Skip SHA256 verification
        #[arg(long, default_value_t = false)]
        no_verify: bool,

        /// Switch to this version after install
        #[arg(long, default_value_t = false)]
        r#use: bool,
    },

    /// Uninstall a Go version
    Uninstall {
        /// Version: supports go1.22.4 / 1.22.4 / 1.22
        version: String,
    },

    /// List installed Go versions
    List,

    /// Switch current Go version
    Use {
        /// Version: supports go1.22.4 / 1.22.4 / 1.22
        version: String,
    },

    /// Print current Go version
    Current,

    /// Fetch and print latest stable release
    Latest,

    /// Cache management
    #[command(subcommand)]
    Cache(CacheCmd),

    /// Diagnose environment and fgm directories
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum CacheCmd {
    /// Print cache directory
    Dir,
    /// Compute cache size
    Size,
    /// Clean cache
    Clean,
}

fn path_contains_dir(dir: &Path) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|p| p == dir)
}

fn print_path_setup_hint(cfg: &Config) {
    if path_contains_dir(&cfg.bin_dir) {
        return;
    }

    // Print copy-pastable commands (do not try to detect shell).
    let p = cfg.bin_dir.display();
    eprintln!("Hint: PATH does not contain: {p}");
    eprintln!("  zsh : echo 'export PATH=\"{p}:$PATH\"' >> ~/.zshrc && source ~/.zshrc");
    eprintln!("  bash: echo 'export PATH=\"{p}:$PATH\"' >> ~/.bashrc && source ~/.bashrc");
    eprintln!("  fish: fish_add_path {p}");
}

fn format_list_line(tag: &str, current_tag: Option<&str>) -> String {
    if current_tag.is_some_and(|cur| cur == tag) {
        format!("{} *", tag)
    } else {
        tag.to_string()
    }
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::new(cli.root, cli.mirror, cli.quiet, cli.verbose)?;
    cfg.ensure_dirs()?;

    match cli.cmd {
        Commands::List => {
            let versions = crate::version::installed_versions(&cfg)?;
            if versions.is_empty() {
                if !cfg.quiet {
                    println!("(empty) No installed versions found in: {}", cfg.versions_dir.display());
                }
                return Ok(());
            }
            let current_tag = shim::current(&cfg)?.map(|v| v.tag);
            for v in versions {
                println!("{}", format_list_line(&v.tag, current_tag.as_deref()));
            }
            Ok(())
        }
        Commands::Current => {
            if let Some(v) = shim::current(&cfg)? {
                println!("{}", v.tag);
            } else {
                anyhow::bail!(
                    "No current version set. Run: fgm use <version> (and ensure PATH contains: {})",
                    cfg.bin_dir.display()
                )
            }
            Ok(())
        }
        Commands::Use { version } => {
            let req = VersionInput::parse(&version)?;
            let selected = crate::version::resolve_installed(&cfg, &req)?
                .with_context(|| format!("No matching installed version found: {version}"))?;
            shim::set_current(&cfg, &selected)?;
            if !cfg.quiet {
                println!("Switched to: {}", selected.tag);
                print_path_setup_hint(&cfg);
            }
            Ok(())
        }
        Commands::Uninstall { version } => {
            let req = VersionInput::parse(&version)?;
            let selected = crate::version::resolve_installed(&cfg, &req)?
                .with_context(|| format!("No matching installed version found: {version}"))?;
            crate::version::uninstall(&cfg, &selected.tag)?;
            if !cfg.quiet {
                println!("Uninstalled: {}", selected.tag);
            }
            Ok(())
        }
        Commands::Latest => {
            let tag = install::fetch_latest_tag(&cfg).await?;
            println!("{}", tag);
            Ok(())
        }
        Commands::Install {
            version,
            force,
            no_verify,
            r#use,
        } => {
            let target = if version.trim().eq_ignore_ascii_case("latest") {
                GoVersion::from_tag(&install::fetch_latest_tag(&cfg).await?)?
            } else {
                let req = VersionInput::parse(&version)?;
                if req.patch.is_none() && req.prerelease.is_none() {
                    install::resolve_latest_patch_for_minor(&cfg, &req).await?
                } else {
                    GoVersion::from_input(&req)?
                }
            };

            install::install(
                &cfg,
                &target,
                install::InstallOptions {
                    force,
                    verify: !no_verify,
                },
            )
            .await
            .with_context(|| format!("Install failed: {}", target.tag))?;

            if r#use {
                shim::set_current(&cfg, &target)?;
            }

            if !cfg.quiet {
                println!("Installed: {}", target.tag);
                if r#use {
                    println!("Switched to: {}", target.tag);
                    print_path_setup_hint(&cfg);
                } else {
                    // When --use is not specified, still print PATH setup hint.
                    print_path_setup_hint(&cfg);
                    println!("Hint: run `fgm use {}` to enable this version", target.tag);
                }
            }
            Ok(())
        }
        Commands::Cache(sub) => match sub {
            CacheCmd::Dir => {
                println!("{}", cfg.cache_dir.display());
                Ok(())
            }
            CacheCmd::Size => {
                let bytes = cache::dir_size_bytes(&cfg.cache_dir)?;
                println!("{}", cache::format_bytes(bytes));
                Ok(())
            }
            CacheCmd::Clean => {
                cache::clean_dir(&cfg.cache_dir)?;
                if !cfg.quiet {
                    println!("Cache cleaned: {}", cfg.cache_dir.display());
                }
                Ok(())
            }
        },
        Commands::Doctor => {
            crate::errors::print_doctor(&cfg)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_parses_global_options_and_list() {
        let cli = Cli::try_parse_from(["fgm", "--quiet", "--root", "/tmp/fgm-test", "list"]) // Does not perform actual IO.
            .unwrap();
        assert!(cli.quiet);
        assert_eq!(cli.root.as_ref().unwrap().to_string_lossy(), "/tmp/fgm-test");
        assert!(matches!(cli.cmd, Commands::List));
    }

    #[test]
    fn clap_parses_cache_subcommands() {
        let a = Cli::try_parse_from(["fgm", "cache", "dir"]).unwrap();
        assert!(matches!(a.cmd, Commands::Cache(CacheCmd::Dir)));

        let b = Cli::try_parse_from(["fgm", "cache", "size"]).unwrap();
        assert!(matches!(b.cmd, Commands::Cache(CacheCmd::Size)));

        let c = Cli::try_parse_from(["fgm", "cache", "clean"]).unwrap();
        assert!(matches!(c.cmd, Commands::Cache(CacheCmd::Clean)));
    }

    #[test]
    fn clap_rejects_unknown_command() {
        let err = Cli::try_parse_from(["fgm", "unknown-cmd"]).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn format_list_line_marks_current_with_star_suffix() {
        assert_eq!(format_list_line("go1.22.4", Some("go1.22.4")), "go1.22.4 *");
        assert_eq!(format_list_line("go1.22.4", Some("go1.22.5")), "go1.22.4");
        assert_eq!(format_list_line("go1.22.4", None), "go1.22.4");
    }
}
