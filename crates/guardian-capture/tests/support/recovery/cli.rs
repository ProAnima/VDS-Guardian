use serde_json::Value;
use std::{error::Error, ffi::OsString, path::Path, process::Command};

pub(super) fn run_guardian_cli(arguments: Vec<OsString>) -> Result<Value, Box<dyn Error>> {
    let binary = std::env::var_os("GUARDIAN_CLI_BIN")
        .ok_or("GUARDIAN_CLI_BIN must point to the compiled guardian-cli binary")?;
    let output = Command::new(binary).args(arguments).output()?;
    if !output.status.success() {
        return Err(format!(
            "guardian-cli failed with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

pub(super) fn args<T: AsRef<str>>(values: &[T]) -> Vec<OsString> {
    values
        .iter()
        .map(|value| OsString::from(value.as_ref()))
        .collect()
}

pub(super) fn path(value: &Path) -> String {
    value.to_string_lossy().into_owned()
}
