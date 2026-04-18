use crate::session_service::SessionHandle;

pub(crate) async fn set_session_project(
    session_handle: &SessionHandle,
    session_id: &str,
    project_id: Option<String>,
) -> anyhow::Result<()> {
    session_handle.set_project(session_id, project_id).await?;
    Ok(())
}
