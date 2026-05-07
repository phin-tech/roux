use rmcp::{
    handler::server::wrapper::Parameters,
    model::CallToolResult,
    schemars::JsonSchema,
    tool, tool_router,
    transport::stdio,
    ErrorData, ServiceExt,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[cfg(test)]
const MCP_TOOL_NAMES: &[&str] = &[
    "roux_list_sessions",
    "roux_get_session",
    "roux_list_panes",
    "roux_create_session",
    "roux_create_pane",
    "roux_send_text",
    "roux_get_latest_output",
    "roux_focus",
    "roux_read_notes",
    "roux_search_notes",
    "roux_append_notes",
    "roux_notes_vault_root",
];

#[derive(Debug)]
enum McpToolError {
    Disabled,
    InvalidParams(&'static str),
    Socket(String),
    TaskJoin(String),
    SocketResponse(String),
}

impl std::fmt::Display for McpToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpToolError::Disabled => write!(f, "Roux MCP is disabled in Settings"),
            McpToolError::InvalidParams(message) => write!(f, "{message}"),
            McpToolError::Socket(message) => write!(f, "{message}"),
            McpToolError::TaskJoin(message) => write!(f, "{message}"),
            McpToolError::SocketResponse(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for McpToolError {}

impl From<McpToolError> for ErrorData {
    fn from(error: McpToolError) -> Self {
        match error {
            McpToolError::InvalidParams(message) => ErrorData::invalid_params(message, None),
            McpToolError::Disabled => ErrorData::invalid_request(error.to_string(), None),
            McpToolError::Socket(_)
            | McpToolError::TaskJoin(_)
            | McpToolError::SocketResponse(_) => ErrorData::internal_error(error.to_string(), None),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionIdParams {
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionParams {
    pub name: Option<String>,
    pub working_dir: Option<String>,
    pub worktree_branch: Option<String>,
    pub profile: Option<String>,
    pub nono_profile: Option<String>,
    #[serde(default)]
    pub nono_allow_dirs: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaneParams {
    pub session_id: String,
    pub profile: Option<String>,
    pub direction: Option<String>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendTextParams {
    pub session_id: String,
    pub pane_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub enter: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatestOutputParams {
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FocusParams {
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesTargetParams {
    pub scope: String,
    pub session_id: Option<String>,
    pub topic: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesSearchParams {
    pub tags: Vec<String>,
    pub scope: Option<String>,
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesAppendParams {
    pub scope: String,
    pub session_id: Option<String>,
    pub topic: Option<String>,
    pub content: String,
    #[serde(default)]
    pub timestamped: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RouxMcpServer;

#[tool_router(server_handler)]
impl RouxMcpServer {
    #[tool(description = "List active Roux sessions.")]
    async fn roux_list_sessions(&self) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({ "command": "session-list" })).await
    }

    #[tool(description = "Get a single Roux session by sessionId.")]
    async fn roux_get_session(
        &self,
        Parameters(params): Parameters<SessionIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({
            "command": "session-poll",
            "session_id": params.session_id,
        }))
        .await
    }

    #[tool(description = "List panes for a Roux session.")]
    async fn roux_list_panes(
        &self,
        Parameters(params): Parameters<SessionIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({
            "command": "session-panes-list",
            "session_id": params.session_id,
        }))
        .await
    }

    #[tool(
        description = "Create a Roux session. workingDir or an existing session context is required by Roux."
    )]
    async fn roux_create_session(
        &self,
        Parameters(params): Parameters<CreateSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(build_create_session_request(params)).await
    }

    #[tool(description = "Create a safe Roux pane in an existing session.")]
    async fn roux_create_pane(
        &self,
        Parameters(params): Parameters<CreatePaneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(build_create_pane_request(params)).await
    }

    #[tool(description = "Send text to a Roux session or pane. enter defaults to false.")]
    async fn roux_send_text(
        &self,
        Parameters(params): Parameters<SendTextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(build_send_text_request(params)).await
    }

    #[tool(
        description = "Get recent terminal output from a Roux pane or session. Provide paneId or sessionId; maxBytes defaults to 8192 and is capped by Roux."
    )]
    async fn roux_get_latest_output(
        &self,
        Parameters(params): Parameters<LatestOutputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(build_latest_output_request(params)).await
    }

    #[tool(description = "Focus a Roux session or pane.")]
    async fn roux_focus(
        &self,
        Parameters(params): Parameters<FocusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if params.session_id.is_none() && params.pane_id.is_none() {
            return Err(McpToolError::InvalidParams("sessionId or paneId required").into());
        }
        call_socket(json!({
            "command": "focus",
            "session_id": params.session_id,
            "pane_id": params.pane_id,
        }))
        .await
    }

    #[tool(description = "Read a Roux notes scope/topic.")]
    async fn roux_read_notes(
        &self,
        Parameters(params): Parameters<NotesTargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({
            "command": "notes-read",
            "args": notes_target(params.scope, params.session_id, params.topic),
        }))
        .await
    }

    #[tool(description = "Search Roux notes by tag.")]
    async fn roux_search_notes(
        &self,
        Parameters(params): Parameters<NotesSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({
            "command": "notes-search",
            "args": {
                "tags": params.tags,
                "scope": params.scope,
                "exact": params.exact,
            },
        }))
        .await
    }

    #[tool(description = "Append content to a Roux notes scope/topic.")]
    async fn roux_append_notes(
        &self,
        Parameters(params): Parameters<NotesAppendParams>,
    ) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({
            "command": "notes-append",
            "args": {
                "target": notes_target(params.scope, params.session_id, params.topic),
                "content": params.content,
                "timestamped": params.timestamped,
                "tags": params.tags,
            },
        }))
        .await
    }

    #[tool(description = "Get the Roux notes vault root path.")]
    async fn roux_notes_vault_root(&self) -> Result<CallToolResult, ErrorData> {
        call_socket(json!({ "command": "notes-vault-root", "args": {} })).await
    }
}

pub async fn run_stdio_server() -> anyhow::Result<()> {
    let service = RouxMcpServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn call_socket(request: Value) -> Result<CallToolResult, ErrorData> {
    call_socket_typed(request).await.map_err(Into::into)
}

async fn call_socket_typed(request: Value) -> Result<CallToolResult, McpToolError> {
    ensure_mcp_enabled().await?;
    let response =
        tokio::task::spawn_blocking(move || crate::cli_socket::send_socket_command(request))
            .await
            .map_err(|e| McpToolError::TaskJoin(e.to_string()))?
            .map_err(McpToolError::Socket)?;
    response_to_tool_output(response)
}

async fn ensure_mcp_enabled() -> Result<(), McpToolError> {
    let response = tokio::task::spawn_blocking(|| {
        crate::cli_socket::send_socket_command(json!({ "command": "mcp-enabled" }))
    })
    .await
    .map_err(|e| McpToolError::TaskJoin(e.to_string()))?
    .map_err(McpToolError::Socket)?;

    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(McpToolError::SocketResponse(
            response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown Roux socket error")
                .to_string(),
        ));
    }
    let enabled = response
        .get("data")
        .and_then(|data| data.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if enabled {
        Ok(())
    } else {
        Err(McpToolError::Disabled)
    }
}

fn response_to_tool_output(response: Value) -> Result<CallToolResult, McpToolError> {
    if response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let data = response.get("data").cloned().unwrap_or(Value::Null);
        Ok(CallToolResult::structured(json!({ "data": data })))
    } else {
        Err(McpToolError::SocketResponse(
            response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown Roux socket error")
                .to_string(),
        ))
    }
}

fn build_create_session_request(params: CreateSessionParams) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(name) = params.name {
        args.insert("name".into(), Value::String(name));
    }
    if let Some(working_dir) = params.working_dir {
        args.insert("working_dir".into(), Value::String(working_dir));
    }
    if let Some(worktree_branch) = params.worktree_branch {
        args.insert("worktree_branch".into(), Value::String(worktree_branch));
    }
    if let Some(profile) = params.profile {
        args.insert("profile".into(), Value::String(profile));
    }
    if let Some(nono_profile) = params.nono_profile {
        args.insert("nono_profile".into(), Value::String(nono_profile));
    }
    if !params.nono_allow_dirs.is_empty() {
        args.insert(
            "nono_allow_dirs".into(),
            Value::Array(params.nono_allow_dirs.into_iter().map(Value::String).collect()),
        );
    }
    json!({ "command": "session-create", "args": args })
}

fn build_create_pane_request(params: CreatePaneParams) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(profile) = params.profile {
        args.insert("profile".into(), Value::String(profile));
    }
    if let Some(direction) = params.direction {
        args.insert("direction".into(), Value::String(direction));
    }
    if let Some(working_dir) = params.working_dir {
        args.insert("working_dir".into(), Value::String(working_dir));
    }
    json!({
        "command": "session-panes-create",
        "session_id": params.session_id,
        "args": args,
    })
}

fn build_send_text_request(params: SendTextParams) -> Value {
    json!({
        "command": "send",
        "session_id": params.session_id,
        "pane_id": params.pane_id,
        "args": {
            "text": params.text,
            "enter": params.enter,
        },
    })
}

fn build_latest_output_request(params: LatestOutputParams) -> Value {
    let mut request = serde_json::Map::new();
    request.insert("command".into(), Value::String("latest-output".into()));
    if let Some(session_id) = params.session_id {
        request.insert("session_id".into(), Value::String(session_id));
    }
    if let Some(pane_id) = params.pane_id {
        request.insert("pane_id".into(), Value::String(pane_id));
    }

    let mut args = serde_json::Map::new();
    if let Some(max_bytes) = params.max_bytes {
        args.insert("max_bytes".into(), Value::Number(max_bytes.into()));
    }
    request.insert("args".into(), Value::Object(args));
    Value::Object(request)
}

fn notes_target(scope: String, session_id: Option<String>, topic: Option<String>) -> Value {
    json!({
        "scope": scope,
        "sessionId": session_id,
        "topic": topic,
        "overrideSlug": Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_tool_list_excludes_destructive_capabilities() {
        assert!(MCP_TOOL_NAMES.contains(&"roux_send_text"));
        assert!(MCP_TOOL_NAMES.contains(&"roux_get_latest_output"));
        assert!(!MCP_TOOL_NAMES.contains(&"roux_run_command"));
        assert!(!MCP_TOOL_NAMES.contains(&"roux_kill_pty"));
        assert!(!MCP_TOOL_NAMES.contains(&"roux_remove_worktree"));
        assert!(!MCP_TOOL_NAMES.contains(&"roux_delete_session_permanently"));
    }

    #[test]
    fn latest_output_builds_read_only_socket_command() {
        let request = build_latest_output_request(LatestOutputParams {
            session_id: Some("sid".into()),
            pane_id: Some("pane".into()),
            max_bytes: Some(4096),
        });

        assert_eq!(request["command"], "latest-output");
        assert_eq!(request["session_id"], "sid");
        assert_eq!(request["pane_id"], "pane");
        assert_eq!(request["args"]["max_bytes"], 4096);
    }

    #[test]
    fn send_text_builds_existing_socket_command_and_defaults_no_enter() {
        let request = build_send_text_request(SendTextParams {
            session_id: "sid".into(),
            pane_id: Some("pane".into()),
            text: "hello".into(),
            enter: false,
        });

        assert_eq!(request["command"], "send");
        assert_eq!(request["session_id"], "sid");
        assert_eq!(request["pane_id"], "pane");
        assert_eq!(request["args"]["text"], "hello");
        assert_eq!(request["args"]["enter"], false);
    }

    #[test]
    fn create_pane_uses_session_panes_create_socket_command() {
        let request = build_create_pane_request(CreatePaneParams {
            session_id: "sid".into(),
            profile: Some("plain-shell".into()),
            direction: Some("vertical".into()),
            working_dir: Some("/repo".into()),
        });

        assert_eq!(request["command"], "session-panes-create");
        assert_eq!(request["session_id"], "sid");
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["direction"], "vertical");
        assert_eq!(request["args"]["working_dir"], "/repo");
    }

    #[test]
    fn socket_error_becomes_tool_error() {
        let result = response_to_tool_output(json!({
            "ok": false,
            "error": "Roux is not running",
        }));

        match result {
            Ok(_) => panic!("expected tool error"),
            Err(err) => assert_eq!(err.to_string(), "Roux is not running"),
        }
    }

    /// Claude Desktop / Claude Code's MCP client silently drops every tool
    /// when a server's `tools/list` response carries `outputSchema`, `title`,
    /// or `annotations` at the tool level (anthropics/claude-code#25081,
    /// closed as not planned). Keep these unset on every Roux tool so the
    /// connector continues to expose tools.
    #[test]
    fn tools_omit_fields_that_break_claude_desktop() {
        let tools = RouxMcpServer::tool_router().list_all();
        assert!(!tools.is_empty(), "expected at least one tool");
        for tool in tools {
            assert!(
                tool.output_schema.is_none(),
                "tool `{}` must not advertise outputSchema",
                tool.name
            );
            assert!(
                tool.title.is_none(),
                "tool `{}` must not advertise a top-level title",
                tool.name
            );
            assert!(
                tool.annotations.is_none(),
                "tool `{}` must not advertise toolAnnotations",
                tool.name
            );
        }
    }
}
