use tauri::{AppHandle, Emitter, State};

use crate::{
    errors::app_error::AppResult,
    models::{Memory, MemoryInput},
    state::app_state::AppState,
};

async fn emit_and_schedule_memory(
    app: &AppHandle,
    state: &AppState,
    memory: &Memory,
) -> AppResult<()> {
    app.emit("recall://memory-saved", memory)?;
    state
        .link_enrichment_service
        .schedule_for_memory(app.clone(), memory.clone())
        .await;
    Ok(())
}

#[tauri::command]
pub async fn list_memories(state: State<'_, AppState>) -> AppResult<Vec<Memory>> {
    state.memory_service.list().await
}

#[tauri::command]
pub async fn get_memory(id: String, state: State<'_, AppState>) -> AppResult<Option<Memory>> {
    state.memory_service.get(&id).await
}

#[tauri::command]
pub async fn create_memory(
    app: AppHandle,
    input: MemoryInput,
    state: State<'_, AppState>,
) -> AppResult<Memory> {
    let memory = state.memory_service.create(input).await?;
    emit_and_schedule_memory(&app, &state, &memory).await?;
    Ok(memory)
}

#[tauri::command]
pub async fn update_memory(
    app: AppHandle,
    id: String,
    input: MemoryInput,
    state: State<'_, AppState>,
) -> AppResult<Memory> {
    let memory = state.memory_service.update(&id, input).await?;
    emit_and_schedule_memory(&app, &state, &memory).await?;
    Ok(memory)
}

#[tauri::command]
pub async fn delete_memory(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.memory_service.delete(&id).await
}

#[tauri::command]
pub async fn duplicate_memory(
    app: AppHandle,
    id: String,
    state: State<'_, AppState>,
) -> AppResult<Memory> {
    let memory = state.memory_service.duplicate(&id).await?;
    emit_and_schedule_memory(&app, &state, &memory).await?;
    Ok(memory)
}
