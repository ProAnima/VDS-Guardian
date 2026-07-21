use guardian_configuration::{
    CapturePlanStore, RepositoryRegistration, RepositoryStore, StoredCapturePlan,
};
use guardian_core::{FilesystemCapturePlan, PlanId, ProfileId, RepositoryId};

#[test]
fn capture_plan_store_rejects_a_tampered_digest_on_read() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let store = CapturePlanStore::at(root.path());
    let stored = StoredCapturePlan::new(plan()?)?;
    store.upsert(stored)?;
    let path = root.path().join("plans.json");
    let tampered = std::fs::read_to_string(&path)?.replace("sha256\":\"", "sha256\":\"00");
    std::fs::write(path, tampered)?;
    assert!(store.list().is_err());
    Ok(())
}

#[test]
fn capture_plan_store_round_trips_an_optional_database_path()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = CapturePlanStore::at(root.path());
    let mut with_database = plan()?;
    with_database.plan_id = PlanId::parse("plan-002")?;
    with_database.database_path = Some("/srv/app/app.sqlite".to_owned());
    store.upsert(StoredCapturePlan::new(with_database.clone())?)?;
    store.upsert(StoredCapturePlan::new(plan()?)?)?;
    let stored = store.list()?;
    let reloaded = stored
        .iter()
        .find(|entry| entry.plan.plan_id == with_database.plan_id)
        .ok_or("plan with database_path missing after round trip")?;
    assert_eq!(reloaded.plan.database_path, with_database.database_path);
    let without_database = stored
        .iter()
        .find(|entry| entry.plan.plan_id != with_database.plan_id)
        .ok_or("plan without database_path missing after round trip")?;
    assert_eq!(without_database.plan.database_path, None);
    Ok(())
}

#[test]
fn repository_store_rejects_a_map_key_that_differs_from_the_registration_id()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let location = tempfile::tempdir()?;
    let path = std::fs::canonicalize(location.path())?;
    let store = RepositoryStore::at(root.path());
    store.upsert(RepositoryRegistration::new(
        RepositoryId::parse("repository-001")?,
        "Recovery".to_owned(),
        path,
    )?)?;
    let registry = root.path().join("repositories.json");
    let tampered =
        std::fs::read_to_string(&registry)?.replace("\"repository-001\":", "\"repository-002\":");
    std::fs::write(registry, tampered)?;
    assert!(store.list().is_err());
    Ok(())
}

#[test]
fn repository_store_removes_only_the_requested_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    let store = RepositoryStore::at(root.path());
    let first_id = RepositoryId::parse("repository-001")?;
    let second_id = RepositoryId::parse("repository-002")?;
    store.upsert(RepositoryRegistration::new(
        first_id.clone(),
        "First".to_owned(),
        std::fs::canonicalize(first.path())?,
    )?)?;
    store.upsert(RepositoryRegistration::new(
        second_id.clone(),
        "Second".to_owned(),
        std::fs::canonicalize(second.path())?,
    )?)?;

    assert_eq!(
        store.remove(&first_id)?.map(|entry| entry.repository_id),
        Some(first_id)
    );
    assert!(
        store
            .get(&RepositoryId::parse("repository-001")?)?
            .is_none()
    );
    assert!(store.get(&second_id)?.is_some());
    assert!(
        store
            .remove(&RepositoryId::parse("repository-missing")?)?
            .is_none()
    );
    Ok(())
}

#[test]
fn repository_store_updates_only_an_existing_registration_path()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let original = tempfile::tempdir()?;
    let replacement = tempfile::tempdir()?;
    let store = RepositoryStore::at(root.path());
    let id = RepositoryId::parse("repository-001")?;
    store.upsert(RepositoryRegistration::new(
        id.clone(),
        "Archive".to_owned(),
        std::fs::canonicalize(original.path())?,
    )?)?;

    let updated = store
        .update_path(&id, std::fs::canonicalize(replacement.path())?)?
        .ok_or("registration disappeared")?;
    assert_eq!(updated.label, "Archive");
    assert_eq!(updated.path, std::fs::canonicalize(replacement.path())?);
    assert!(
        store
            .update_path(
                &RepositoryId::parse("repository-missing")?,
                std::fs::canonicalize(original.path())?,
            )?
            .is_none()
    );
    Ok(())
}

fn plan() -> Result<FilesystemCapturePlan, Box<dyn std::error::Error>> {
    Ok(FilesystemCapturePlan {
        plan_id: PlanId::parse("plan-001")?,
        version: 1,
        profile_id: ProfileId::parse("profile-001")?,
        repository_id: RepositoryId::parse("repository-001")?,
        roots: vec!["/srv/app".to_owned()],
        database_path: None,
    })
}
