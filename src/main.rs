mod album;
mod app;
mod config;
mod input;
mod loader;

use std::{path::PathBuf, process::ExitCode};

use anyhow::{Context, Result, anyhow};
use clap::{ArgGroup, Parser};
use eframe::egui;
use kaishin::{KaishinOptions, UpdateOptions};

use crate::{album::Album, app::Viewer, config::Config};

#[derive(Parser)]
#[command(
    version,
    about,
    group(ArgGroup::new("viewer").args(["path", "config", "recurse", "no_recurse"]).multiple(true)),
    group(ArgGroup::new("update").args(["check_update", "self_update"]).conflicts_with("viewer"))
)]
struct Args {
    /// Image, directory, or ZIP/CBZ archive to open.
    path: Option<PathBuf>,
    /// Load this teravars TOML file or discover config files in this directory.
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Scan subdirectories as well.
    #[arg(short, long, conflicts_with = "no_recurse")]
    recurse: bool,
    /// Scan only the top level, even if recursion is enabled in the config.
    #[arg(long)]
    no_recurse: bool,
    /// Check GitHub Releases for a newer version without installing it.
    #[arg(long)]
    check_update: bool,
    /// Update this installation after confirmation.
    #[arg(long)]
    self_update: bool,
    /// Skip confirmation for --self-update.
    #[arg(long, requires = "self_update", conflicts_with_all = ["check_update", "viewer"])]
    yes: bool,
}

fn main() -> Result<ExitCode> {
    let args = Args::parse();
    let update = if args.check_update {
        Some(Update::Check)
    } else if args.self_update {
        Some(Update::Apply { yes: args.yes })
    } else {
        None
    };
    if let Some(update) = update {
        let status = match update.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:#}");
                ExitCode::FAILURE
            }
        };
        #[cfg(windows)]
        pause_in_private_console();
        return Ok(status);
    }

    let mut config = Config::load(args.config.as_deref())?;
    config.recurse = (config.recurse || args.recurse) && !args.no_recurse;
    let album = match args.path {
        Some(path) => Album::open(&path, config.recurse)
            .with_context(|| format!("opening {}", path.display()))?,
        None => Album::open_folders(&config.folders, config.recurse)?,
    };
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .context("decoding the embedded Mihari icon")?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("mihari")
            .with_icon(icon)
            .with_inner_size([960., 680.])
            .with_min_inner_size([320., 240.]),
        ..Default::default()
    };
    eframe::run_native(
        "Mihari",
        options,
        Box::new(move |cc| Ok(Box::new(Viewer::new(cc.egui_ctx.clone(), album, config)?))),
    )
    .map_err(|error| anyhow!("starting Mihari: {error}"))?;
    Ok(ExitCode::SUCCESS)
}

enum Update {
    Check,
    Apply { yes: bool },
}

impl Update {
    fn run(self) -> Result<()> {
        let opts = KaishinOptions::new("yukimemi", "mihari", "mihari", env!("CARGO_PKG_VERSION"));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("creating the update runtime")?
            .block_on(async {
                match self {
                    Self::Check => {
                        let latest = kaishin::check_latest_release(&opts)
                            .await
                            .context("failed to fetch latest release from GitHub")?;
                        let current = &opts.current_version;
                        if kaishin::is_update_available(current, &latest.tag_name)? {
                            println!(
                                "Mihari {} is available (installed: {current}). Run `mihari --self-update` to install it.",
                                latest.tag_name
                            );
                        } else {
                            println!("Mihari {current} is up to date.");
                        }
                        Ok(())
                    }
                    Self::Apply { yes } => {
                        let update = UpdateOptions::new().yes(yes).prefer_github_release(false);
                        kaishin::run_self_update(&opts, update).await
                    }
                }
            })
    }
}

#[cfg(windows)]
fn pause_in_private_console() {
    use std::io::{self, IsTerminal, Write};

    use windows_sys::Win32::System::Console::GetConsoleProcessList;

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return;
    }

    let mut pid = 0;
    // SAFETY: `pid` is writable storage for the one process ID requested.
    if unsafe { GetConsoleProcessList(&raw mut pid, 1) } == 1 {
        eprint!("Press Enter to close...");
        let _ = io::stderr().flush();
        let _ = io::stdin().read_line(&mut String::new());
    }
}

#[cfg(test)]
mod tests {
    use clap::{Parser, error::ErrorKind};

    use super::Args;

    #[test]
    fn yes_requires_self_update_even_when_other_modes_are_present() {
        let Err(error) = Args::try_parse_from(["mihari", "--yes"]) else {
            panic!("--yes must require --self-update");
        };
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);

        for args in [
            &["--check-update"][..],
            &["self-update"],
            &["--config", "config.toml"],
            &["-r"],
            &["--no-recurse"],
        ] {
            let argv = ["mihari", "--yes"].into_iter().chain(args.iter().copied());
            let Err(error) = Args::try_parse_from(argv) else {
                panic!("--yes must conflict with {args:?}");
            };
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{args:?}");
        }

        let args = Args::try_parse_from(["mihari", "--self-update", "--yes"]).unwrap();
        assert!(args.self_update && args.yes);
    }
}
