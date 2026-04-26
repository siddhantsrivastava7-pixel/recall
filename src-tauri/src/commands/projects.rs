use tauri::State;

use crate::{errors::app_error::AppResult, models::Project, state::app_state::AppState};

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    state.project_service.list().await
}

#[tauri::command]
pub async fn create_project(
    name: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Project> {
    state.project_service.create(&name, description).await
}

#[tauri::command]
pub async fn update_project(
    id: String,
    name: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Project> {
    state.project_service.update(&id, &name, description).await
}

#[tauri::command]
pub async fn delete_project(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.project_service.delete(&id).await
}
