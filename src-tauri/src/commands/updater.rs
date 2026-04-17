use crate::updater::{self, UpdateInfo, UpdaterError};
use roux_core::UpdateChannel;

#[tauri::command]
#[specta::specta]
pub(crate) async fn check_for_update(
    app: tauri::AppHandle,
    channel: UpdateChannel,
) -> Result<Option<UpdateInfo>, UpdaterError> {
    updater::check_for_update(&app, channel).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn install_update(
    app: tauri::AppHandle,
    channel: UpdateChannel,
) -> Result<(), UpdaterError> {
    updater::install_update(&app, channel).await
}
