//! Answers to `fs/read_text_file` and `fs/write_text_file`, kept inside a
//! [`Workspace`]. A [`Client`](crate::Client) calls these from its handlers
//! after its own policy (asking the user before a write, say) has passed.

use std::fs;

use crate::error::RpcError;
use crate::schema::{
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use crate::workspace::Workspace;

/// Reads the file, or the `limit` lines from 1-based `line`, if it is
/// inside the workspace. A missing file is "resource not found".
pub fn read_text_file(
    workspace: &Workspace,
    request: &ReadTextFileRequest,
) -> Result<ReadTextFileResponse, RpcError> {
    let path = workspace.check(&request.path)?;
    let text = fs::read_to_string(&path)?;
    if request.line.is_none() && request.limit.is_none() {
        return Ok(ReadTextFileResponse { content: text });
    }
    let start = match request.line {
        Some(0) => return Err(RpcError::invalid_params("lines are numbered from 1")),
        Some(line) => line as usize - 1,
        None => 0,
    };
    let limit = request.limit.map_or(usize::MAX, |limit| limit as usize);
    let content = text.split_inclusive('\n').skip(start).take(limit).collect();
    Ok(ReadTextFileResponse { content })
}

/// Writes the file, creating it (and its directories) if missing, if it is
/// inside the workspace.
pub fn write_text_file(
    workspace: &Workspace,
    request: &WriteTextFileRequest,
) -> Result<WriteTextFileResponse, RpcError> {
    let path = workspace.check(&request.path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, &request.content)?;
    Ok(WriteTextFileResponse {})
}
