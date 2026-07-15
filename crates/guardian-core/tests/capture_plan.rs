use guardian_core::{FilesystemCapturePlan, PlanId, ProfileId, RepositoryId};

#[test]
fn capture_plan_rejects_unsafe_roots() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = FilesystemCapturePlan {
        plan_id: PlanId::parse("plan-001")?,
        version: 1,
        profile_id: ProfileId::parse("profile-001")?,
        repository_id: RepositoryId::parse("repository-001")?,
        roots: vec!["/srv/app".to_owned()],
        database_path: None,
    };
    assert!(plan.validate().is_ok());
    assert_eq!(plan.canonical_sha256()?.len(), 64);
    plan.roots = vec!["/srv/../etc".to_owned()];
    assert!(plan.validate().is_err());
    Ok(())
}

#[test]
fn capture_plan_rejects_an_unsafe_database_path() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = FilesystemCapturePlan {
        plan_id: PlanId::parse("plan-002")?,
        version: 1,
        profile_id: ProfileId::parse("profile-001")?,
        repository_id: RepositoryId::parse("repository-001")?,
        roots: vec!["/srv/app".to_owned()],
        database_path: Some("/srv/app/app.sqlite".to_owned()),
    };
    assert!(plan.validate().is_ok());
    plan.database_path = Some("/".to_owned());
    assert!(plan.validate().is_err());
    plan.database_path = Some("/srv/../etc/passwd".to_owned());
    assert!(plan.validate().is_err());
    Ok(())
}
