use guardian_core::{
    DatabaseAuthentication, DatabaseConnection, DatabaseEngine, DatabaseId,
    DatabaseServerVersionProbeError, DatabaseServerVersionProbePort, DatabaseVersion,
};
use guardian_database::{
    DumpToolProbeError, DumpToolVersion, build_capabilities, parse_dump_tool_probe,
    parse_server_version,
};

#[test]
fn parser_accepts_supported_dump_tool_version_lines() -> Result<(), Box<dyn std::error::Error>> {
    let tools = parse_dump_tool_probe(
        b"postgresql\tpg_dump (PostgreSQL) 16.4\nmysql\tmysqldump  Ver 8.0.42-0ubuntu0 for Linux\n",
    )?;
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].engine, DatabaseEngine::PostgreSql);
    assert_eq!(
        tools[0].version,
        DatabaseVersion {
            major: 16,
            minor: 4,
            patch: 0
        }
    );
    assert_eq!(tools[1].engine, DatabaseEngine::MySql);
    Ok(())
}

#[test]
fn parser_rejects_unknown_or_duplicate_tool_lines() {
    assert!(
        parse_dump_tool_probe(b"postgresql\tpg_dump 16.4\npostgresql\tpg_dump 16.4\n").is_err()
    );
    assert!(parse_dump_tool_probe(b"oracle\texpdp 19.0\n").is_err());
    assert!(parse_dump_tool_probe(b"").is_err());
}

#[test]
fn server_version_probe_accepts_one_bounded_version_line() -> Result<(), Box<dyn std::error::Error>>
{
    assert_eq!(
        parse_server_version(b"16.4\n")?,
        DatabaseVersion {
            major: 16,
            minor: 4,
            patch: 0,
        }
    );
    assert_eq!(
        parse_server_version(b"16.4\nnoise"),
        Err(DumpToolProbeError::Rejected)
    );
    Ok(())
}

#[test]
fn capability_composition_rejects_missing_dump_tool_for_server()
-> Result<(), Box<dyn std::error::Error>> {
    let connection = connection()?;
    let tools = vec![DumpToolVersion {
        engine: DatabaseEngine::MySql,
        version: version(8),
    }];
    assert!(build_capabilities(&[connection], &tools, &ServerProbe).is_err());
    Ok(())
}

#[test]
fn capability_composition_pairs_server_and_dump_versions() -> Result<(), Box<dyn std::error::Error>>
{
    let capabilities = build_capabilities(
        &[connection()?],
        &[DumpToolVersion {
            engine: DatabaseEngine::PostgreSql,
            version: version(16),
        }],
        &ServerProbe,
    )?;
    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0].server_version, version(16));
    assert_eq!(capabilities[0].dump_tool_version, version(16));
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

fn version(major: u16) -> DatabaseVersion {
    DatabaseVersion {
        major,
        minor: 0,
        patch: 0,
    }
}

struct ServerProbe;

impl DatabaseServerVersionProbePort for ServerProbe {
    fn probe_server(
        &self,
        _: &DatabaseConnection,
    ) -> Result<DatabaseVersion, DatabaseServerVersionProbeError> {
        Ok(version(16))
    }
}
