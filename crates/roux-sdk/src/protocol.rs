use crate::{RouxError, RouxResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandRequest {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub args: Value,
}

impl CommandRequest {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: Value::Null,
        }
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn pane_id(mut self, pane_id: impl Into<String>) -> Self {
        self.pane_id = Some(pane_id.into());
        self
    }

    pub fn auth_token(mut self, auth_token: impl Into<String>) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }

    pub fn args(mut self, args: Value) -> Self {
        self.args = args;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandResponse {
    pub ok: bool,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

impl CommandResponse {
    pub fn into_result(self) -> RouxResult<Value> {
        if self.ok {
            Ok(self.data.unwrap_or(Value::Null))
        } else {
            Err(RouxError::Command(
                self.error.unwrap_or_else(|| "unknown Roux command error".to_string()),
            ))
        }
    }
}
