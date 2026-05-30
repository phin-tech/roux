use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentTargetKind {
    Session,
    WorkItem,
}

impl AttachmentTargetKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Session => "session",
            Self::WorkItem => "work_item",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "session" => Some(Self::Session),
            "workItem" | "work_item" | "work-item" => Some(Self::WorkItem),
            _ => None,
        }
    }
}

impl std::fmt::Display for AttachmentTargetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentContentKind {
    Text,
    File,
}

impl AttachmentContentKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Text => "text",
            Self::File => "file",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "text" => Some(Self::Text),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

impl std::fmt::Display for AttachmentContentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub document_id: String,
    pub target_kind: AttachmentTargetKind,
    pub target_id: String,
    pub title: Option<String>,
    pub content_kind: AttachmentContentKind,
    pub mime_type: Option<String>,
    pub source_path: Option<String>,
    pub byte_len: u64,
    pub sha256: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDocument {
    pub attachment: Attachment,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInput {
    pub target_kind: AttachmentTargetKind,
    pub target_id: String,
    pub title: Option<String>,
    pub content_kind: AttachmentContentKind,
    pub content: String,
    pub mime_type: Option<String>,
    pub source_path: Option<String>,
}
