use super::RecoveryFailure;
use std::{ffi::OsString, path::PathBuf};

#[cfg(test)]
mod tests;

pub(super) struct RecoveryCommand {
    pub(super) action: RecoveryAction,
    pub(super) repositories_dir: PathBuf,
    pub(super) repository_id: String,
    pub(super) passphrase_file: Option<PathBuf>,
    pub(super) output: Option<PathBuf>,
    pub(super) input: Option<PathBuf>,
    pub(super) confirmation: Option<String>,
    pub(super) vault_dir: Option<PathBuf>,
    pub(super) signing_config_dir: Option<PathBuf>,
    pub(super) repository_path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
pub(super) enum RecoveryAction {
    Init,
    Status,
    Export,
    Import,
}

pub(super) fn parse(arguments: &[OsString]) -> Result<RecoveryCommand, RecoveryFailure> {
    let action = parse_action(arguments)?;
    let mut values = ParsedValues::default();
    let mut index = 1;
    while index < arguments.len() {
        index = values.parse_option(action, arguments, index)?;
    }
    values.finish(action)
}

fn parse_action(arguments: &[OsString]) -> Result<RecoveryAction, RecoveryFailure> {
    match arguments.first().and_then(|value| value.to_str()) {
        Some("init") => Ok(RecoveryAction::Init),
        Some("status") => Ok(RecoveryAction::Status),
        Some("export") => Ok(RecoveryAction::Export),
        Some("import") => Ok(RecoveryAction::Import),
        _ => Err(RecoveryFailure::usage()),
    }
}

#[derive(Default)]
struct ParsedValues<'a> {
    repositories_dir: Option<PathBuf>,
    repository_id: Option<&'a str>,
    passphrase_file: Option<PathBuf>,
    output: Option<PathBuf>,
    input: Option<PathBuf>,
    repository_path: Option<PathBuf>,
    confirmation: Option<&'a str>,
    vault_dir: Option<PathBuf>,
    signing_config_dir: Option<PathBuf>,
    json: bool,
}

impl<'a> ParsedValues<'a> {
    fn parse_option(
        &mut self,
        action: RecoveryAction,
        arguments: &'a [OsString],
        index: usize,
    ) -> Result<usize, RecoveryFailure> {
        let next = index + 1;
        match arguments[index].to_str() {
            Some("--repositories-dir") => {
                self.repositories_dir = arguments.get(next).map(PathBuf::from)
            }
            Some("--repository-id") => {
                self.repository_id = arguments.get(next).and_then(|value| value.to_str())
            }
            Some("--passphrase-file")
                if matches!(action, RecoveryAction::Export | RecoveryAction::Import) =>
            {
                self.passphrase_file = arguments.get(next).map(PathBuf::from)
            }
            Some("--output") if matches!(action, RecoveryAction::Export) => {
                self.output = arguments.get(next).map(PathBuf::from)
            }
            Some("--input") if matches!(action, RecoveryAction::Import) => {
                self.input = arguments.get(next).map(PathBuf::from)
            }
            Some("--repository-path") if matches!(action, RecoveryAction::Import) => {
                self.repository_path = arguments.get(next).map(PathBuf::from)
            }
            Some("--confirmation")
                if matches!(action, RecoveryAction::Export | RecoveryAction::Import) =>
            {
                self.confirmation = arguments.get(next).and_then(|value| value.to_str())
            }
            Some("--vault-dir") => self.vault_dir = arguments.get(next).map(PathBuf::from),
            Some("--signing-config-dir") if matches!(action, RecoveryAction::Init) => {
                self.signing_config_dir = arguments.get(next).map(PathBuf::from)
            }
            Some("--json") => {
                self.json = true;
                return Ok(next);
            }
            _ => return Err(RecoveryFailure::usage()),
        }
        arguments.get(next).ok_or_else(RecoveryFailure::usage)?;
        Ok(index + 2)
    }

    fn finish(self, action: RecoveryAction) -> Result<RecoveryCommand, RecoveryFailure> {
        let repositories_dir = self
            .repositories_dir
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?;
        self.validate_paths(repositories_dir)?;
        let ActionValues {
            passphrase_file,
            output,
            input,
            confirmation,
        } = self.action_values(action)?;
        let repositories_dir = self.repositories_dir.ok_or_else(RecoveryFailure::usage)?;
        let repository_id = self
            .repository_id
            .map(str::to_owned)
            .ok_or_else(RecoveryFailure::usage)?;
        Ok(RecoveryCommand {
            action,
            repositories_dir,
            repository_id,
            passphrase_file,
            output,
            input,
            confirmation,
            vault_dir: self.vault_dir,
            signing_config_dir: self.signing_config_dir,
            repository_path: self.repository_path,
        })
    }

    fn validate_paths(&self, repositories_dir: &std::path::Path) -> Result<(), RecoveryFailure> {
        let optional_paths = [
            self.vault_dir.as_deref(),
            self.signing_config_dir.as_deref(),
            self.repository_path.as_deref(),
        ];
        if !self.json
            || !repositories_dir.is_absolute()
            || optional_paths
                .into_iter()
                .flatten()
                .any(|path| !path.is_absolute())
        {
            return Err(RecoveryFailure::usage());
        }
        Ok(())
    }

    fn action_values(&self, action: RecoveryAction) -> Result<ActionValues, RecoveryFailure> {
        match action {
            RecoveryAction::Init => {
                self.signing_config_dir
                    .as_ref()
                    .ok_or_else(RecoveryFailure::usage)?;
                Ok(ActionValues::default())
            }
            RecoveryAction::Status => Ok(ActionValues::default()),
            RecoveryAction::Export => {
                let passphrase = required_absolute(self.passphrase_file.as_ref())?;
                let output = required_absolute(self.output.as_ref())?;
                let confirmation = self.confirmation.ok_or_else(RecoveryFailure::usage)?;
                Ok(ActionValues {
                    passphrase_file: Some(passphrase),
                    output: Some(output),
                    confirmation: Some(confirmation.to_owned()),
                    ..ActionValues::default()
                })
            }
            RecoveryAction::Import => {
                let passphrase = required_absolute(self.passphrase_file.as_ref())?;
                let input = required_absolute(self.input.as_ref())?;
                let confirmation = self.confirmation.ok_or_else(RecoveryFailure::usage)?;
                Ok(ActionValues {
                    passphrase_file: Some(passphrase),
                    input: Some(input),
                    confirmation: Some(confirmation.to_owned()),
                    ..ActionValues::default()
                })
            }
        }
    }
}

#[derive(Default)]
struct ActionValues {
    passphrase_file: Option<PathBuf>,
    output: Option<PathBuf>,
    input: Option<PathBuf>,
    confirmation: Option<String>,
}

fn required_absolute(path: Option<&PathBuf>) -> Result<PathBuf, RecoveryFailure> {
    path.filter(|value| value.is_absolute())
        .cloned()
        .ok_or_else(RecoveryFailure::usage)
}
