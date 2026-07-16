//! Startup configuration for the MCP server process. Unlike `guardian-cli`,
//! which parses these paths fresh on every short-lived invocation, this
//! server is long-lived (one process serves many tool calls over its stdio
//! lifetime), so they are parsed once at startup and held for the process's
//! lifetime. Mirrors `guardian-cli`'s explicit-absolute-path-argument style
//! (there is no Tauri `app_config_dir()` equivalent to lean on here).

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub repositories_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub plans_dir: PathBuf,
    pub config_dir: PathBuf,
    pub vault_dir: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError;

impl ServerConfig {
    pub fn parse(arguments: &[OsString]) -> Result<Self, ConfigError> {
        let mut repositories_dir = None;
        let mut profiles_dir = None;
        let mut plans_dir = None;
        let mut config_dir = None;
        let mut vault_dir = None;
        let mut index = 0;
        while index < arguments.len() {
            match arguments[index].to_str() {
                Some("--repositories-dir") => {
                    index += 1;
                    repositories_dir = arguments.get(index).map(PathBuf::from);
                }
                Some("--profiles-dir") => {
                    index += 1;
                    profiles_dir = arguments.get(index).map(PathBuf::from);
                }
                Some("--plans-dir") => {
                    index += 1;
                    plans_dir = arguments.get(index).map(PathBuf::from);
                }
                Some("--config-dir") => {
                    index += 1;
                    config_dir = arguments.get(index).map(PathBuf::from);
                }
                Some("--vault-dir") => {
                    index += 1;
                    vault_dir = arguments.get(index).map(PathBuf::from);
                }
                _ => return Err(ConfigError),
            }
            index += 1;
        }
        let repositories_dir = repositories_dir.ok_or(ConfigError)?;
        let profiles_dir = profiles_dir.ok_or(ConfigError)?;
        let plans_dir = plans_dir.ok_or(ConfigError)?;
        let config_dir = config_dir.ok_or(ConfigError)?;
        if !repositories_dir.is_absolute()
            || !profiles_dir.is_absolute()
            || !plans_dir.is_absolute()
            || !config_dir.is_absolute()
            || vault_dir.as_deref().is_some_and(|dir| !dir.is_absolute())
        {
            return Err(ConfigError);
        }
        Ok(Self {
            repositories_dir,
            profiles_dir,
            plans_dir,
            config_dir,
            vault_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn requires_all_four_absolute_directories() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        assert!(ServerConfig::parse(&args(&[])).is_err());
        assert!(
            ServerConfig::parse(&args(&[
                "--repositories-dir",
                "relative",
                "--profiles-dir",
                &root.join("p").display().to_string(),
                "--plans-dir",
                &root.join("l").display().to_string(),
                "--config-dir",
                &root.join("c").display().to_string(),
            ]))
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn accepts_absolute_directories_with_an_optional_vault_dir()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let vault_dir = root.join("v");
        let config = ServerConfig::parse(&args(&[
            "--repositories-dir",
            &root.join("r").display().to_string(),
            "--profiles-dir",
            &root.join("p").display().to_string(),
            "--plans-dir",
            &root.join("l").display().to_string(),
            "--config-dir",
            &root.join("c").display().to_string(),
            "--vault-dir",
            &vault_dir.display().to_string(),
        ]))
        .map_err(|_| "valid absolute paths should parse")?;
        assert_eq!(config.vault_dir.as_deref(), Some(vault_dir.as_path()));
        Ok(())
    }

    #[test]
    fn a_relative_vault_dir_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        assert!(
            ServerConfig::parse(&args(&[
                "--repositories-dir",
                &root.join("r").display().to_string(),
                "--profiles-dir",
                &root.join("p").display().to_string(),
                "--plans-dir",
                &root.join("l").display().to_string(),
                "--config-dir",
                &root.join("c").display().to_string(),
                "--vault-dir",
                "relative",
            ]))
            .is_err()
        );
        Ok(())
    }
}
