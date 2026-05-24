//! Tauri commands exposing the bus subscription surface to the frontend.
//!
//! The Mailbox panel uses these to render a Subscriptions tab and let
//! the user create/delete subscriptions interactively. CLI/MCP mutations
//! flow through the socket protocol — both paths share the same
//! `SubscriptionManager` instance on `AppState`, so changes from one
//! show up immediately on the other.

use roux_core::BusSubscription;
use roux_lib::aliases::ProjectFilter;

use crate::state::AppState;

fn project_filter<'a>(project_id: Option<&'a str>, global: bool) -> ProjectFilter<'a> {
    match (project_id, global) {
        (Some(p), _) => ProjectFilter::Exact(Some(p)),
        (None, true) => ProjectFilter::Exact(None),
        (None, false) => ProjectFilter::Any,
    }
}

#[tauri::command]
pub async fn subscriptions_list(
    alias: Option<String>,
    project_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BusSubscription>, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("bus-subscriptions"))
    {
        return client
            .list_subscriptions(alias.clone(), project_id.clone(), global.unwrap_or(false))
            .await
            .map_err(Into::into);
    }
    let filter = project_filter(project_id.as_deref(), global.unwrap_or(false));
    Ok(match alias {
        Some(a) => state.subscription_manager.for_alias(&a, filter),
        None => state.subscription_manager.list(filter),
    })
}

#[tauri::command]
pub async fn subscriptions_create(
    alias: String,
    pattern: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<BusSubscription, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("bus-subscribe"))
    {
        return client
            .create_subscription(alias.clone(), pattern.clone(), project_id.clone())
            .await
            .map_err(Into::into);
    }
    state
        .subscription_manager
        .subscribe(&alias, &pattern, project_id, Some(&app))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn subscriptions_delete(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("bus-unsubscribe"))
    {
        return client.delete_subscription(id.clone()).await.map_err(Into::into);
    }
    state.subscription_manager.unsubscribe(&id, Some(&app)).map_err(|e| e.to_string())
}
