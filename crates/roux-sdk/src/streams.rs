use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WatchEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "update")]
    Update { event: Box<roux_core::WatchUpdateEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum MailboxEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: Box<roux_core::MailboxEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum AliasEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: roux_core::AliasEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SubscriptionEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: roux_core::BusSubscriptionEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WorkItemEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: Box<roux_core::WorkItemEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}
