use crate::settings::RouxSettings;

pub(crate) fn update_settings(new_settings: RouxSettings) -> anyhow::Result<RouxSettings> {
    let settings = new_settings.normalized();
    crate::logging::set_enabled(settings.enable_logging);
    crate::settings::save_settings(&settings)?;
    Ok(settings)
}

pub(crate) fn update_mcp_config_metadata(
    host: String,
    configured_at_ms: u64,
) -> anyhow::Result<RouxSettings> {
    let mut settings = crate::settings::load_settings();
    settings.mcp_last_configured_host = Some(host);
    settings.mcp_last_configured_at_ms = Some(configured_at_ms);
    update_settings(settings)
}
