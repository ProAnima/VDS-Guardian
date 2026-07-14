use guardian_core::{EnrollProfileUseCase, VdsProfile};
use guardian_profile_store::ProfileStore;
use serde::Serialize;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

pub(super) fn run(arguments: &[OsString]) -> ExitCode {
    match parse(arguments).and_then(execute) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn parse(arguments: &[OsString]) -> Result<ProfileCommand, ProfileFailure> {
    let action = match arguments.first().and_then(|value| value.to_str()) {
        Some("enroll") => ProfileAction::Enroll,
        Some("list") => ProfileAction::List,
        _ => return Err(ProfileFailure::usage()),
    };
    let mut profiles_dir = None;
    let mut input = None;
    let mut json = false;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--json") => json = true,
            Some("--profiles-dir") => {
                index += 1;
                profiles_dir = arguments.get(index).map(PathBuf::from);
            }
            Some("--input") if matches!(action, ProfileAction::Enroll) => {
                index += 1;
                input = arguments.get(index).map(PathBuf::from);
            }
            _ => return Err(ProfileFailure::usage()),
        }
        index += 1;
    }
    let profiles_dir = profiles_dir.ok_or_else(ProfileFailure::usage)?;
    if !json || !profiles_dir.is_absolute() {
        return Err(ProfileFailure::usage());
    }
    let input = match action {
        ProfileAction::Enroll => {
            let input = input.ok_or_else(ProfileFailure::usage)?;
            if !input.is_absolute() {
                return Err(ProfileFailure::usage());
            }
            Some(input)
        }
        ProfileAction::List if input.is_none() => None,
        ProfileAction::List => return Err(ProfileFailure::usage()),
    };
    Ok(ProfileCommand {
        action,
        profiles_dir,
        input,
    })
}

fn execute(command: ProfileCommand) -> Result<ProfileOutput, ProfileFailure> {
    let store = ProfileStore::at(command.profiles_dir);
    match command.action {
        ProfileAction::Enroll => {
            let profile =
                read_profile(command.input.as_deref().ok_or_else(ProfileFailure::usage)?)?;
            EnrollProfileUseCase { store: &store }
                .execute(profile.clone())
                .map_err(|_| ProfileFailure::enrollment())?;
            Ok(ProfileOutput::Enrollment(profile))
        }
        ProfileAction::List => store
            .list()
            .map(ProfileOutput::Profiles)
            .map_err(|_| ProfileFailure::storage()),
    }
}

fn read_profile(path: &Path) -> Result<VdsProfile, ProfileFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProfileFailure::input())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProfileFailure::input());
    }
    let bytes = fs::read(path).map_err(|_| ProfileFailure::input())?;
    serde_json::from_slice(&bytes).map_err(|_| ProfileFailure::input())
}

fn write_success(output: &ProfileOutput) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: output,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&ProfileFailure::serialization()),
    }
}

fn write_error(error: &ProfileFailure) -> ExitCode {
    match serde_json::to_string_pretty(&Failure { ok: false, error }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    ExitCode::FAILURE
}

struct ProfileCommand {
    action: ProfileAction,
    profiles_dir: PathBuf,
    input: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum ProfileAction {
    Enroll,
    List,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ProfileOutput {
    Enrollment(VdsProfile),
    Profiles(Vec<VdsProfile>),
}

#[derive(Serialize)]
struct Success<'a, T> {
    ok: bool,
    data: &'a T,
}

#[derive(Serialize)]
struct Failure<'a, T> {
    ok: bool,
    error: &'a T,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl ProfileFailure {
    fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli profile <enroll|list> --profiles-dir <absolute-path> [--input <absolute-profile-json>] --json",
        }
    }

    fn input() -> Self {
        Self {
            code: "invalid_profile_input",
            message: "The profile input is not a valid regular profile document.",
            usage: "Provide an absolute path to a JSON VDS profile with a pinned host key.",
        }
    }

    fn enrollment() -> Self {
        Self {
            code: "profile_enrollment_failed",
            message: "The pinned VDS profile could not be enrolled.",
            usage: "Check the profile values and retry after resolving local storage access.",
        }
    }

    fn storage() -> Self {
        Self {
            code: "profile_storage_unavailable",
            message: "The local profile store could not be read.",
            usage: "Check local storage access and retry.",
        }
    }

    fn serialization() -> Self {
        Self {
            code: "serialization_failed",
            message: "The JSON response could not be serialized.",
            usage: "Retry and export redacted diagnostics if the problem persists.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProfileAction, ProfileCommand, ProfileFailure, execute, parse};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::{ffi::OsString, fs};

    #[test]
    fn profile_commands_require_json_and_absolute_paths() {
        for arguments in [
            vec!["enroll"],
            vec!["list", "--profiles-dir", "relative", "--json"],
            vec![
                "enroll",
                "--profiles-dir",
                "/tmp/profiles",
                "--input",
                "relative",
                "--json",
            ],
        ] {
            let values = arguments
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert_eq!(parse(&values).err(), Some(ProfileFailure::usage()));
        }
    }

    #[test]
    fn enrollment_requires_a_regular_validated_profile_document()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let profiles_dir = root.path().join("profiles");
        let input = root.path().join("profile.json");
        fs::write(&input, profile_json())?;
        execute(ProfileCommand {
            action: ProfileAction::Enroll,
            profiles_dir: profiles_dir.clone(),
            input: Some(input),
        })
        .map_err(|_| std::io::Error::other("profile enrollment failed"))?;
        assert!(profiles_dir.join("profiles.json").is_file());
        Ok(())
    }

    #[test]
    fn enrollment_rejects_unknown_profile_fields() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let input = root.path().join("profile.json");
        fs::write(&input, br#"{\"unknown\":true}"#)?;
        let result = execute(ProfileCommand {
            action: ProfileAction::Enroll,
            profiles_dir: root.path().join("profiles"),
            input: Some(input),
        });
        assert_eq!(result.err(), Some(ProfileFailure::input()));
        Ok(())
    }

    fn profile_json() -> String {
        let mut blob = Vec::new();
        blob.extend_from_slice(&11_u32.to_be_bytes());
        blob.extend_from_slice(b"ssh-ed25519");
        blob.push(1);
        format!(
            r#"{{"profileId":"profile-001","label":"VDS","credentialId":"credential-001","endpoint":{{"host":"vds.example","port":22,"user":"backup","hostPin":{{"algorithm":"ssh-ed25519","publicKeyBase64":"{}"}}}}}}"#,
            STANDARD.encode(blob)
        )
    }
}
