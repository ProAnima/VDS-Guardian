use super::*;

#[test]
fn recovery_commands_require_json_and_absolute_paths() {
    for arguments in [
        vec!["init"],
        vec![
            "init",
            "--repositories-dir",
            "relative",
            "--repository-id",
            "r",
            "--json",
        ],
        vec!["status", "--repositories-dir", "/r"],
        vec![
            "export",
            "--repositories-dir",
            "/r",
            "--repository-id",
            "r",
            "--json",
        ],
        vec![
            "import",
            "--repositories-dir",
            "/r",
            "--repository-id",
            "r",
            "--json",
        ],
    ] {
        let values = arguments
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(parse(&values).err(), Some(RecoveryFailure::usage()));
    }
}

#[test]
fn an_unrecognized_action_is_rejected() {
    let values = ["rotate"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    assert_eq!(parse(&values).err(), Some(RecoveryFailure::usage()));
}
