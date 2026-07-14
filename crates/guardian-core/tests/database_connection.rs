use guardian_core::{
    DatabaseAuthentication, DatabaseConnection, DatabaseConnectionError, DatabaseEngine,
    DatabaseId, DatabaseServerVersionProbeError, DatabaseServerVersionProbePort, DatabaseVersion,
    VerifyDatabaseConnectionError, VerifyDatabaseConnectionUseCase,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn connection_probe_rejects_injection_before_calling_adapter()
-> Result<(), Box<dyn std::error::Error>> {
    let mut connection = connection()?;
    connection.database_name = "app;drop database app".to_owned();
    let probe = Probe::default();
    let result = VerifyDatabaseConnectionUseCase { probe: &probe }.execute(&connection);
    assert_eq!(
        result,
        Err(VerifyDatabaseConnectionError::Connection(
            DatabaseConnectionError::Invalid
        ))
    );
    assert_eq!(probe.calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn ssh_peer_mode_rejects_nonlocal_database_endpoints_before_calling_adapter()
-> Result<(), Box<dyn std::error::Error>> {
    let mut connection = connection()?;
    connection.host = "database.internal".to_owned();
    let probe = Probe::default();
    let result = VerifyDatabaseConnectionUseCase { probe: &probe }.execute(&connection);
    assert_eq!(
        result,
        Err(VerifyDatabaseConnectionError::Connection(
            DatabaseConnectionError::Invalid
        ))
    );
    assert_eq!(probe.calls.load(Ordering::Relaxed), 0);
    Ok(())
}

fn connection() -> Result<DatabaseConnection, Box<dyn std::error::Error>> {
    Ok(DatabaseConnection {
        database_id: DatabaseId::parse("postgres-main")?,
        engine: DatabaseEngine::PostgreSql,
        host: "localhost".to_owned(),
        port: 5432,
        database_name: "app".to_owned(),
        authentication: DatabaseAuthentication::SshPeer,
    })
}

#[derive(Default)]
struct Probe {
    calls: AtomicUsize,
}

impl DatabaseServerVersionProbePort for Probe {
    fn probe_server(
        &self,
        _: &DatabaseConnection,
    ) -> Result<DatabaseVersion, DatabaseServerVersionProbeError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(DatabaseVersion {
            major: 16,
            minor: 4,
            patch: 0,
        })
    }
}
