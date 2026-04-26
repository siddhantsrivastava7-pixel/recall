use tauri::{AppHandle, Emitter, State};

use crate::{errors::app_error::AppResult, models::PairingInfo, state::app_state::AppState};

#[tauri::command]
pub async fn get_pairing_info(state: State<'_, AppState>) -> AppResult<PairingInfo> {
    state
        .pairing_service
        .info(state.receiver_service.is_running())
        .await
}

#[tauri::command]
pub async fn reset_pairing(app: AppHandle, state: State<'_, AppState>) -> AppResult<PairingInfo> {
    state.pairing_service.reset_identity().await?;
    let info = state
        .pairing_service
        .info(state.receiver_service.is_running())
        .await?;
    app.emit("recall://pairing-updated", &info)?;
    Ok(info)
}
