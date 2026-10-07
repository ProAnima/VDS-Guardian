//! The fixed shell commands run on the server. Every caller-supplied value is shell-quoted.

use crate::{SshError, shell_quote};
use guardian_core::{DatabaseAuthentication, DatabaseConnection, DatabaseEngine};

pub(crate) fn docker_inspect_command() -> &'static str {
    "ids=$(docker ps --all --quiet --no-trunc) || exit 1; [ -z \"$ids\" ] || printf '%s\\n' \"$ids\" | xargs -r docker inspect --"
}

pub(crate) fn sqlite_snapshot_command(database_path: &str) -> String {
    let path = shell_quote(database_path);
    format!(
        "[ -f {path} ] || exit 1; tmp=$(mktemp) || exit 1; sqlite3 {path} \".backup '$tmp'\" && zstd -q -c \"$tmp\"; status=$?; rm -f \"$tmp\"; exit $status"
    )
}

pub(crate) fn database_disk_budget_probe_command(database_path: &str) -> String {
    let path = shell_quote(database_path);
    format!(
        "size=$(stat -c%s {path} 2>/dev/null) && [ -n \"$size\" ] && free=$(df -Pk {path} | tail -n 1 | awk '{{print $4}}') && printf '%s %s\\n' \"$size\" \"$free\""
    )
}

pub(crate) fn sqlite3_probe_command() -> &'static str {
    "command -v sqlite3 >/dev/null 2>&1"
}

pub(crate) fn zstd_probe_command() -> &'static str {
    "command -v zstd >/dev/null 2>&1"
}

pub(crate) fn database_tool_probe_command() -> &'static str {
    "if command -v pg_dump >/dev/null 2>&1; then printf 'postgresql\\t'; pg_dump --version || exit 1; fi; if command -v mysqldump >/dev/null 2>&1; then printf 'mysql\\t'; mysqldump --version || exit 1; fi"
}

pub(crate) fn database_server_probe_command(
    connection: &DatabaseConnection,
) -> Result<String, SshError> {
    connection
        .validate()
        .map_err(|_| SshError::InvalidDatabaseConnection)?;
    if !matches!(connection.authentication, DatabaseAuthentication::SshPeer) {
        return Err(SshError::UnsupportedDatabaseAuthentication);
    }
    let host = shell_quote(&connection.host);
    let port = shell_quote(&connection.port.to_string());
    let database = shell_quote(&connection.database_name);
    Ok(match connection.engine {
        DatabaseEngine::PostgreSql => format!(
            "psql --no-password --tuples-only --no-align --host {host} --port {port} --dbname {database} --command 'SHOW server_version'"
        ),
        DatabaseEngine::MySql => format!(
            "mysql --protocol=TCP --skip-password --batch --skip-column-names --host {host} --port {port} --database {database} --execute 'SELECT VERSION()'"
        ),
    })
}
