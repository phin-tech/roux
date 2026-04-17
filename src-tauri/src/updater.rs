use roux_core::UpdateChannel;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;
use thiserror::Error;

const GITHUB_OWNER: &str = "phin-tech";
const GITHUB_REPO: &str = "roux";
const STABLE_MANIFEST_URL: &str =
    "https://github.com/phin-tech/roux/releases/latest/download/latest.json";
const PRERELEASE_MANIFEST_NAME: &str = "latest-prerelease.json";

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Error)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum UpdaterError {
    #[error("network failure while reaching the update server")]
    Network,
    #[error("update signature verification failed")]
    SignatureInvalid,
    #[error("no release available for the selected channel")]
    NotFound,
    #[error("updater internal error: {message}")]
    Internal { message: String },
}

impl UpdaterError {
    fn from_update_error(err: tauri_plugin_updater::Error) -> Self {
        let message = err.to_string();
        if message.to_lowercase().contains("signature") {
            UpdaterError::SignatureInvalid
        } else if is_network_message(&message) {
            UpdaterError::Network
        } else {
            UpdaterError::Internal { message }
        }
    }

    fn from_reqwest(err: reqwest::Error) -> Self {
        if err.is_connect() || err.is_timeout() || err.is_request() {
            UpdaterError::Network
        } else {
            UpdaterError::Internal {
                message: err.to_string(),
            }
        }
    }
}

fn is_network_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    ["network", "connect", "dns", "timeout", "request", "http"]
        .iter()
        .any(|needle| lower.contains(needle))
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum UpdateProgress {
    Started { content_length: Option<u64> },
    Progress { chunk_length: u64 },
    Finished,
}

pub(crate) async fn resolve_endpoint(channel: UpdateChannel) -> Result<String, UpdaterError> {
    match channel {
        UpdateChannel::Stable => Ok(STABLE_MANIFEST_URL.to_string()),
        UpdateChannel::PreRelease => resolve_latest_prerelease_manifest().await,
    }
}

async fn resolve_latest_prerelease_manifest() -> Result<String, UpdaterError> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        prerelease: bool,
        draft: bool,
        created_at: String,
    }

    let api_url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=30",
        GITHUB_OWNER, GITHUB_REPO
    );
    let client = reqwest::Client::builder()
        .user_agent(format!("roux-updater/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(UpdaterError::from_reqwest)?;

    let releases: Vec<Release> = client
        .get(&api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(UpdaterError::from_reqwest)?
        .error_for_status()
        .map_err(UpdaterError::from_reqwest)?
        .json()
        .await
        .map_err(UpdaterError::from_reqwest)?;

    let newest = releases
        .into_iter()
        .filter(|r| r.prerelease && !r.draft)
        .max_by(|a, b| a.created_at.cmp(&b.created_at))
        .ok_or(UpdaterError::NotFound)?;

    Ok(format!(
        "https://github.com/{}/{}/releases/download/{}/{}",
        GITHUB_OWNER, GITHUB_REPO, newest.tag_name, PRERELEASE_MANIFEST_NAME
    ))
}

pub(crate) async fn check_for_update(
    app: &AppHandle,
    channel: UpdateChannel,
) -> Result<Option<UpdateInfo>, UpdaterError> {
    let endpoint = resolve_endpoint(channel).await?;
    let endpoint_url = endpoint
        .parse::<url::Url>()
        .map_err(|e| UpdaterError::Internal {
            message: format!("invalid endpoint url: {e}"),
        })?;

    let update = app
        .updater_builder()
        .endpoints(vec![endpoint_url])
        .map_err(UpdaterError::from_update_error)?
        .build()
        .map_err(UpdaterError::from_update_error)?
        .check()
        .await
        .map_err(UpdaterError::from_update_error)?;

    Ok(update.map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone().unwrap_or_default(),
    }))
}

pub(crate) async fn install_update(
    app: &AppHandle,
    channel: UpdateChannel,
) -> Result<(), UpdaterError> {
    let endpoint = resolve_endpoint(channel).await?;
    let endpoint_url = endpoint
        .parse::<url::Url>()
        .map_err(|e| UpdaterError::Internal {
            message: format!("invalid endpoint url: {e}"),
        })?;

    let update = app
        .updater_builder()
        .endpoints(vec![endpoint_url])
        .map_err(UpdaterError::from_update_error)?
        .build()
        .map_err(UpdaterError::from_update_error)?
        .check()
        .await
        .map_err(UpdaterError::from_update_error)?
        .ok_or(UpdaterError::NotFound)?;

    let app_for_progress = app.clone();
    let app_for_finish = app.clone();
    let mut started = false;
    update
        .download_and_install(
            move |chunk_length, content_length| {
                // The plugin invokes this callback once per HTTP chunk, starting
                // with the first real chunk — there is no separate start signal.
                // Emit a synthetic Started on the first call so the frontend can
                // initialize its progress state with the total size.
                if !started {
                    started = true;
                    let _ = app_for_progress
                        .emit("updater://progress", UpdateProgress::Started { content_length });
                }
                let _ = app_for_progress.emit(
                    "updater://progress",
                    UpdateProgress::Progress {
                        chunk_length: chunk_length as u64,
                    },
                );
            },
            move || {
                let _ = app_for_finish.emit("updater://progress", UpdateProgress::Finished);
            },
        )
        .await
        .map_err(UpdaterError::from_update_error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stable_endpoint_is_static_manifest_url() {
        let url = resolve_endpoint(UpdateChannel::Stable).await.unwrap();
        assert_eq!(url, STABLE_MANIFEST_URL);
    }

    #[test]
    fn network_messages_classify_as_network() {
        assert!(is_network_message("DNS lookup failed"));
        assert!(is_network_message("connect: connection refused"));
        assert!(is_network_message("HTTP 500"));
        assert!(!is_network_message("unexpected end of file"));
    }

}
