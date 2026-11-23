use std::{path::Path, process::ExitCode, thread::JoinHandle};

use anyhow::{Context, Result};
use kaishin::{Checker, KaishinOptions, UpdateOptions};

pub(super) enum Update {
    Check,
    Apply { yes: bool },
}

impl Update {
    pub(super) fn run(self) -> ExitCode {
        let status = match self.execute() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:#}");
                ExitCode::FAILURE
            }
        };
        #[cfg(windows)]
        pause_in_private_console();
        status
    }

    fn execute(self) -> Result<()> {
        let opts = options();
        runtime()?.block_on(async {
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
                        if !latest.html_url.is_empty() {
                            println!("Release notes: {}", latest.html_url);
                        }
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

pub(super) fn start_automatic_update() -> Option<JoinHandle<()>> {
    match std::thread::Builder::new()
        .name("mihari-update".into())
        .spawn(|| {
            if let Err(error) = automatic_update() {
                eprintln!("Mihari automatic update failed: {error:#}");
            }
        }) {
        Ok(worker) => Some(worker),
        Err(error) => {
            eprintln!("Mihari could not start automatic updates: {error}");
            None
        }
    }
}

fn automatic_update() -> Result<()> {
    let exe = std::env::current_exe().context("locating the running executable")?;
    if is_cargo_build_output(&exe) || dirs::data_dir().is_none() {
        return Ok(());
    }
    let checker = Checker::new(env!("CARGO_PKG_NAME"), options());
    runtime()?.block_on(checker.auto_update())?;
    Ok(())
}

fn is_cargo_build_output(exe: &Path) -> bool {
    exe.ancestors().skip(1).any(|directory| {
        directory.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case("debug") || name.eq_ignore_ascii_case("release")
        }) && directory.ancestors().skip(1).take(2).any(|parent| {
            parent
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("target"))
        })
    })
}

fn options() -> KaishinOptions {
    KaishinOptions::new("yukimemi", "mihari", "mihari", env!("CARGO_PKG_VERSION"))
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the update runtime")
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
    use super::{Path, is_cargo_build_output};

    #[test]
    fn automatic_updates_skip_cargo_build_outputs() {
        for path in [
            "project/target/debug/mihari",
            "project/target/release/mihari",
            "project/target/aarch64-apple-darwin/release/mihari",
            "project/Target/x86_64-pc-windows-msvc/Release/mihari.exe",
            "project/target/x86_64-pc-windows-msvc/debug/deps/mihari.exe",
        ] {
            assert!(is_cargo_build_output(Path::new(path)), "{path}");
        }

        for path in [
            "home/.cargo/bin/mihari",
            "downloads/mihari-x86_64-pc-windows-msvc/mihari.exe",
            "project/not-target/x86_64-pc-windows-msvc/release/mihari.exe",
        ] {
            assert!(!is_cargo_build_output(Path::new(path)), "{path}");
        }
    }
}
