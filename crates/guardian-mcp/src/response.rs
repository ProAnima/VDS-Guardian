//! Turning domain results into MCP tool responses.

use rmcp::{ErrorData, model::CallToolResult};
use serde::Serialize;

/// Serializes a successful domain result into a structured tool response.
/// Falls back to a structured error rather than `unwrap`/`panic` on the
/// (in practice unreachable, since every DTO here is a plain
/// string/option-of-string struct) chance that serialization itself fails.
pub(crate) fn ok<T: Serialize>(value: &T) -> rmcp::model::CallToolResult {
    match serde_json::to_value(value) {
        Ok(json) => rmcp::model::CallToolResult::structured(json),
        Err(_) => rmcp::model::CallToolResult::structured_error(serde_json::json!({
            "code": "serialization_failed",
            "message": "The tool result could not be serialized.",
        })),
    }
}

/// Serializes a domain failure into a structured, tool-level error — the
/// request was valid and reached the right composition, but the operation
/// itself did not succeed (not found, rejected, cancelled, ...). This is
/// deliberately not a protocol-level `ErrorData`: MCP clients render
/// protocol errors opaquely, but the caller (an operator or an agent acting
/// on their behalf) needs to see *why* a restore or deploy was rejected.
pub(crate) fn err<T: Serialize>(value: &T) -> rmcp::model::CallToolResult {
    match serde_json::to_value(value) {
        Ok(json) => rmcp::model::CallToolResult::structured_error(json),
        Err(_) => rmcp::model::CallToolResult::structured_error(serde_json::json!({
            "code": "serialization_failed",
            "message": "The tool failure could not be serialized.",
        })),
    }
}

/// The one shape every thin tool handler returns: a domain success or a domain failure, both
/// structured. A protocol-level `ErrorData` is never used for a failed operation.
pub(crate) fn respond<T: Serialize, E: Serialize>(
    result: Result<T, E>,
) -> Result<CallToolResult, ErrorData> {
    Ok(match result {
        Ok(value) => ok(&value),
        Err(failure) => err(&failure),
    })
}

/// The structured failure for a selection whose paths or ids do not validate.
pub(crate) fn invalid_selection() -> Result<CallToolResult, ErrorData> {
    Ok(err(&serde_json::json!({
        "code": "invalid_capture_selection",
        "message": "The capture selection is invalid.",
    })))
}
