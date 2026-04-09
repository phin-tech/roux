use crate::settings::RouxSettings;

pub(crate) fn update_settings(new_settings: RouxSettings) -> anyhow::Result<RouxSettings> {
    let settings = new_settings.normalized();
    crate::logging::set_enabled(settings.enable_logging);
    crate::settings::save_settings(&settings)?;
    Ok(settings)
}
