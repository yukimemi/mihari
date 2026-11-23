use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::input::Action;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    #[default]
    Rtl,
    Ltr,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) folders: Vec<PathBuf>,
    pub(crate) recurse: bool,
    pub(crate) spread: bool,
    pub(crate) direction: Direction,
    pub(crate) cover: bool,
    pub(crate) preload: usize,
    pub(crate) slideshow_seconds: u64,
    pub(crate) slideshow_always_on_top: bool,
    pub(crate) auto_update: bool,
    #[serde(rename = "move")]
    pub(crate) move_keys: BTreeMap<char, PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            recurse: false,
            spread: false,
            direction: Direction::Rtl,
            cover: true,
            preload: 4,
            slideshow_seconds: 5,
            slideshow_always_on_top: true,
            auto_update: true,
            move_keys: BTreeMap::new(),
        }
    }
}

impl Config {
    pub(crate) fn load(explicit: Option<&Path>) -> Result<Self> {
        if let Some(path) = explicit {
            let path = std::path::absolute(path)?;
            if path.is_dir() {
                return Self::load_dir(&path);
            }
            return Self::read(path.parent().unwrap_or(&path), std::slice::from_ref(&path))
                .with_context(|| format!("loading {}", path.display()));
        }
        let Some(dir) = dirs::config_dir().map(|dir| dir.join("mihari")) else {
            return Ok(Self::default());
        };
        if !dir
            .try_exists()
            .with_context(|| format!("checking {}", dir.display()))?
        {
            return Ok(Self::default());
        }
        Self::load_dir(&dir)
    }

    fn load_dir(dir: &Path) -> Result<Self> {
        let files = teravars::discover_config_files(dir)
            .with_context(|| format!("discovering config files in {}", dir.display()))?;
        Self::read(dir, &files).with_context(|| format!("loading config from {}", dir.display()))
    }

    fn read(dir: &Path, files: &[PathBuf]) -> Result<Self> {
        if files.is_empty() {
            return Ok(Self::default());
        }
        let mut engine = teravars::Engine::default();
        let mut table =
            teravars::load_merged(files, &mut engine, &teravars::system_context())?.config;
        table.remove("vars");
        let mut config: Self = toml::Value::Table(table).try_into()?;
        ensure!(config.preload <= 8, "preload must be between 0 and 8");
        ensure!(
            (1..=86_400).contains(&config.slideshow_seconds),
            "slideshow_seconds must be between 1 and 86400"
        );
        for folder in &mut config.folders {
            ensure!(
                !folder.as_os_str().is_empty(),
                "folders contains an empty path"
            );
            if folder.is_relative() {
                *folder = dir.join(&*folder);
            }
        }
        for (&key, folder) in &mut config.move_keys {
            ensure!(
                key.is_ascii_alphanumeric(),
                "move key {key:?} must be one ASCII letter or digit"
            );
            ensure!(
                Action::for_char(key).is_none(),
                "move key {key:?} is reserved"
            );
            ensure!(
                !folder.as_os_str().is_empty(),
                "move key {key:?} has an empty folder"
            );
            if folder.is_relative() {
                *folder = dir.join(&*folder);
            }
        }
        Ok(config)
    }
}
