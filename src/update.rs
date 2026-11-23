use std::process::ExitCode;

use anyhow::{Context, Result};
use kaishin::{KaishinOptions, UpdateOptions};

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
