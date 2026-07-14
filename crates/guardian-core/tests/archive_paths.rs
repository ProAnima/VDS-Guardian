use guardian_core::ArchivePath;
use serde::Deserialize;

const FIXTURE: &[u8] = include_bytes!("fixtures/hostile-archive-paths.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivePathFixture {
    path: String,
    accepted: bool,
}

#[test]
fn hostile_archive_path_corpus_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures: Vec<ArchivePathFixture> = serde_json::from_slice(FIXTURE)?;
    for fixture in fixtures {
        assert_eq!(
            ArchivePath::parse(&fixture.path).is_ok(),
            fixture.accepted,
            "archive path corpus mismatch: {:?}",
            fixture.path
        );
    }
    Ok(())
}
