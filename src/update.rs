use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    process::ExitCode,
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use kaishin::{KaishinOptions, UpdateCheckState, UpdateOptions};
use serde::Deserialize;
use tokio::sync::watch;

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

pub(super) struct AutomaticUpdate {
    cancel: watch::Sender<bool>,
    worker: JoinHandle<()>,
}

impl AutomaticUpdate {
    pub(super) fn stop(self) {
        let _ = self.cancel.send(true);
        if self.worker.join().is_err() {
            eprintln!("Mihari automatic update worker panicked");
        }
    }
}

pub(super) fn start_automatic_update() -> Option<AutomaticUpdate> {
    let (cancel, mut cancelled) = watch::channel(false);
    let closing = cancelled.clone();
    match std::thread::Builder::new()
        .name("mihari-update".into())
        .spawn(move || {
            if let Err(error) = runtime().and_then(|runtime| {
                let result = runtime.block_on(async {
                    tokio::select! {
                        biased;
                        _ = cancelled.changed() => Ok(()),
                        result = automatic_update(&closing) => result,
                    }
                });
                // A cancelled DNS lookup can still be running in Tokio's blocking pool.
                runtime.shutdown_background();
                result
            }) {
                eprintln!("Mihari automatic update failed: {error:#}");
            }
        }) {
        Ok(worker) => Some(AutomaticUpdate { cancel, worker }),
        Err(error) => {
            eprintln!("Mihari could not start automatic updates: {error}");
            None
        }
    }
}

async fn automatic_update(closing: &watch::Receiver<bool>) -> Result<()> {
    let exe = std::env::current_exe().context("locating the running executable")?;
    if is_cargo_build_output(&exe) {
        return Ok(());
    }
    let Some(state_path) = kaishin::default_state_path(env!("CARGO_PKG_NAME")) else {
        return Ok(());
    };
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(state_path.with_extension("lock"))?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    #[cfg(windows)]
    let _ = fs::remove_file(exe.with_added_extension("mihari-update-backup"));
    if !kaishin::should_auto_check(
        kaishin::load_check_state(&state_path).as_ref(),
        kaishin::default_interval(),
        SystemTime::now(),
    ) {
        return Ok(());
    }

    // Startup must not run external credential helpers.
    let token = ["GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.trim().is_empty());
    let client = reqwest::Client::builder()
        .user_agent(concat!("mihari/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let request = client
        .get("https://api.github.com/repos/yukimemi/mihari/releases/latest")
        .timeout(Duration::from_secs(5));
    let latest: Release = authorize(request, token.as_deref())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let result = install_available(&exe, &latest, &client, token.as_deref(), closing).await;
    if !*closing.borrow() {
        kaishin::save_check_state(
            &state_path,
            &UpdateCheckState {
                last_checked_unix: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)?
                    .as_secs(),
                last_known_latest: Some(latest.tag_name),
                last_known_url: Some(latest.html_url),
            },
        )?;
    }
    result
}

async fn install_available(
    exe: &Path,
    latest: &Release,
    client: &reqwest::Client,
    token: Option<&str>,
    closing: &watch::Receiver<bool>,
) -> Result<()> {
    if !kaishin::is_update_available(env!("CARGO_PKG_VERSION"), &latest.tag_name)? {
        return Ok(());
    }

    let target = self_update::get_target();
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let (target, asset) = [Some(target), compatible_target(target)]
        .into_iter()
        .flatten()
        .find_map(|target| {
            let name = format!("mihari-{target}.{extension}");
            latest
                .assets
                .iter()
                .find(|asset| asset.name == name)
                .map(|asset| (target, asset))
        })
        .context("no compatible Mihari release archive")?;
    let request = client
        .get(&asset.url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .timeout(Duration::from_secs(120));
    let mut download = authorize(request, token).send().await?.error_for_status()?;
    let work = tempfile::tempdir()?;
    let archive = work.path().join(&asset.name);
    {
        let mut file = File::create(&archive)?;
        while let Some(chunk) = download.chunk().await? {
            file.write_all(&chunk)?;
        }
    }
    let extract = self_update::Extract::from_source(&archive);
    let suffix = std::env::consts::EXE_SUFFIX;
    let mut binary = format!("mihari{suffix}");
    if extract.extract_file(work.path(), &binary).is_err() {
        binary = format!("mihari-{target}{suffix}");
        extract.extract_file(work.path(), &binary)?;
    }
    let binary = work.path().join(binary);
    #[cfg(target_os = "linux")]
    verify_linux_binary(&binary).await?;

    // From here through replacement or rollback there is no cancellation point.
    if !*closing.borrow() {
        replace_executable(exe, &binary)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn verify_linux_binary(binary: &Path) -> Result<()> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .context("timed out checking whether the downloaded binary runs")?
        .context("could not run the downloaded binary")?;
    anyhow::ensure!(
        output.status.success(),
        "downloaded binary does not run on this system: {}",
        String::from_utf8_lossy(&output.stderr).trim(),
    );
    Ok(())
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    url: String,
}

fn authorize(request: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(token) => request.bearer_auth(token.trim()),
        None => request,
    }
}

fn compatible_target(target: &str) -> Option<&'static str> {
    match target {
        "x86_64-pc-windows-msvc" => Some("x86_64-pc-windows-gnu"),
        "x86_64-pc-windows-gnu" => Some("x86_64-pc-windows-msvc"),
        "x86_64-unknown-linux-gnu" => Some("x86_64-unknown-linux-musl"),
        "x86_64-unknown-linux-musl"
            if Path::new("/lib/x86_64-linux-gnu/libc.so.6").exists()
                || Path::new("/lib64/ld-linux-x86-64.so.2").exists() =>
        {
            Some("x86_64-unknown-linux-gnu")
        }
        _ => None,
    }
}

fn replace_executable(exe: &Path, new: &Path) -> Result<()> {
    let parent = exe.parent().context("executable has no parent directory")?;
    let staged = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
    fs::copy(new, &staged)?;
    #[cfg(not(windows))]
    staged.persist(exe)?;
    #[cfg(windows)]
    {
        // The mapped image can be renamed but may not be deletable until exit.
        let backup = exe.with_added_extension("mihari-update-backup");
        match fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("removing the previous update backup"),
        }
        fs::rename(exe, &backup).context("backing up the running executable")?;
        if let Err(error) = staged.persist(exe) {
            fs::rename(&backup, exe).with_context(|| {
                format!("install failed ({error}); restoring {}", backup.display())
            })?;
            return Err(error).context("installing the staged executable");
        }
    }
    Ok(())
}

fn is_cargo_profile(directory: &Path) -> bool {
    // Newer Cargo versions retain an artifact lock even with a separate build-dir.
    directory.join(".cargo-artifact-lock").is_file()
        || (directory.join(".fingerprint").is_dir() && directory.join("deps").is_dir())
}

fn is_cargo_build_output(exe: &Path) -> bool {
    exe.ancestors().skip(1).any(|directory| {
        is_cargo_profile(directory)
            || directory.file_name().is_some_and(|name| {
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
    use super::{Path, is_cargo_build_output, replace_executable};

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

    #[test]
    fn automatic_updates_skip_custom_cargo_target_directories() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let legacy = temp.path().join("build/release");
        std::fs::create_dir_all(legacy.join(".fingerprint"))?;
        std::fs::create_dir_all(legacy.join("deps"))?;
        assert!(is_cargo_build_output(&legacy.join("mihari.exe")));

        let split = temp.path().join("build/x86_64-pc-windows-msvc/release");
        std::fs::create_dir_all(&split)?;
        std::fs::File::create(split.join(".cargo-artifact-lock"))?;
        assert!(is_cargo_build_output(&split.join("mihari.exe")));
        assert!(!is_cargo_build_output(
            &temp.path().join("release/mihari.exe")
        ));
        Ok(())
    }

    #[test]
    fn staged_update_replaces_the_executable_at_its_original_path() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let exe = temp.path().join("mihari.exe");
        let staged = temp.path().join("new.exe");
        std::fs::write(&exe, b"old binary")?;
        std::fs::write(&staged, b"new binary")?;
        replace_executable(&exe, &staged)?;
        assert_eq!(std::fs::read(exe)?, b"new binary");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_rejects_a_binary_that_fails_to_start() -> anyhow::Result<()> {
        let runtime = super::runtime()?;
        runtime.block_on(super::verify_linux_binary(Path::new("/bin/true")))?;
        assert!(
            runtime
                .block_on(super::verify_linux_binary(Path::new("/bin/false")))
                .is_err(),
            "an unsuccessful startup must prevent installation"
        );
        Ok(())
    }
}
