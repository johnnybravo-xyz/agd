use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Subcommand};
use org_core::{Settings, load_settings, settings::default_config_path};

#[derive(Args)]
pub struct ConfigCommand {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Initialize a default native configuration file
    Init,
    /// Show the effective native configuration
    Show,
    /// Show the native configuration file path
    Path,
}

impl ConfigCommand {
    pub fn execute(&self, config_file: Option<String>) -> Result<()> {
        let path = config_path(config_file.as_deref())?;
        match self.action {
            ConfigAction::Init => init(&path),
            ConfigAction::Show => show(&path),
            ConfigAction::Path => {
                println!("{}", path.display());
                Ok(())
            }
        }
    }
}

fn config_path(config_file: Option<&str>) -> Result<PathBuf> {
    let mut path = config_file
        .map(PathBuf::from)
        .unwrap_or(default_config_path()?);
    if path.extension().is_none() {
        path.set_extension("toml");
    }
    Ok(path)
}

fn init(path: &Path) -> Result<()> {
    if path.exists() {
        println!("Configuration file already exists at: {}", path.display());
        println!("Use 'agd config show' to view the effective configuration");
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(&Settings::default())?)?;
    println!("Default configuration file created at: {}", path.display());
    Ok(())
}

fn show(path: &Path) -> Result<()> {
    let loaded = load_settings(Some(path), None)?;
    println!("{}", toml::to_string_pretty(&loaded.settings)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_writes_the_native_settings_schema() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agendum.toml");
        init(&path).unwrap();
        let parsed: Settings = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(parsed.files.roots, vec!["~/org"]);
        assert!(parsed.capture.contains_key("t"));
    }
}
