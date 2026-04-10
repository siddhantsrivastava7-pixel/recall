use tauri::{AppHandle, Manager, State};

use crate::{
    errors::app_error::{AppError, AppResult},
    models::LicenseState,
    state::app_state::AppState,
};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseValidationResult {
    pub valid: bool,
    pub expired: bool,
}

#[tauri::command]
pub async fn validate_license_key(license_key: String) -> AppResult<LicenseValidationResult> {
    let key = license_key.trim().to_uppercase();
    if key.is_empty() {
        return Err(AppError::Invalid("License key is required.".into()));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;

    let response = client
        .post("https://sidbuilds.com/api/validate-key")
        .json(&serde_json::json!({ "key": key }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(AppError::Invalid(
            "Unable to validate your key right now.".into(),
        ));
    }

    response
        .json::<LicenseValidationResult>()
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_license_state(state: State<'_, AppState>) -> AppResult<LicenseState> {
    state.license_service.get_state().await
}

#[tauri::command]
pub async fn activate_license(
    app: AppHandle,
    license_key: String,
    state: State<'_, AppState>,
) -> AppResult<LicenseState> {
    let activated = state.license_service.activate(&license_key).await?;
    let settings = state.settings_service.get().await?;
    if settings.floating_widget_enabled {
        let saved_pos = settings.widget_position_x.zip(settings.widget_position_y);
        state.platform.window.ensure_widget(&app, saved_pos).await?;
    }
    if let Some(main_window) = app.get_webview_window("main") {
        main_window.show()?;
        main_window.set_focus()?;
    }
    if let Some(overlay) = app.get_webview_window("search-overlay") {
        overlay.hide()?;
    }
    Ok(activated)
}

#[tauri::command]
pub async fn deactivate_license(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<LicenseState> {
    let deactivated = state.license_service.deactivate().await?;
    if let Some(widget) = app.get_webview_window("widget") {
        widget.hide()?;
    }
    if let Some(overlay) = app.get_webview_window("search-overlay") {
        overlay.hide()?;
    }
    if let Some(quick_save) = app.get_webview_window("quick-save") {
        quick_save.hide()?;
    }
    Ok(deactivated)
}
