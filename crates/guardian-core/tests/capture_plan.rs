use guardian_core::{FilesystemCapturePlan, PlanId, ProfileId, RepositoryId};

#[test]
fn capture_plan_rejects_unsafe_roots() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = FilesystemCapturePlan {
        plan_id: PlanId::parse("plan-001")?,
        version: 1,
        profile_id: ProfileId::parse("profile-001")?,
        repository_id: RepositoryId::parse("repository-001")?,
        roots: vec!["/srv/app".to_owned()],
    };
    assert!(plan.validate().is_ok());
    assert_eq!(plan.canonical_sha256()?.len(), 64);
    plan.roots = vec!["/srv/../etc".to_owned()];
    assert!(plan.validate().is_err());
    Ok(())
}
