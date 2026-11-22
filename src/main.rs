mod album;
mod app;
mod config;
mod input;
mod loader;

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use eframe::egui;

use crate::{album::Album, app::Viewer, config::Config};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Image or directory to open.
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
}

fn main() -> Result<()> {
    let args = Args::parse();
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
    .map_err(|error| anyhow!("starting Mihari: {error}"))
}
