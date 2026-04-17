use chrono::Utc;
use tauri::{AppHandle, Manager, State};

use crate::{
    db::seed::ensure_seed_data,
    errors::app_error::AppResult,
    models::{AppSettings, BackupPayload, ShortcutBinding},
    services::export_service::ExportService,
    state::app_state::AppState,
};

async fn wipe_local_data(state: &AppState) -> AppResult<()> {
    state.memory_repository.clear().await?;
    state.project_repository.clear().await?;
    state.settings_repository.clear().await?;
    state.license_repository.clear().await?;
    Ok(())
}

async fn apply_runtime_settings(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
) -> AppResult<()> {
    state
        .platform
        .startup
        .apply_launch_on_startup(app, settings.launch_on_startup_enabled)
        .await?;

    let license_state = state.license_service.get_state().await?;
    if settings.floating_widget_enabled && license_state.is_activated {
        let saved_pos = settings.widget_position_x.zip(settings.widget_position_y);
        state.platform.window.ensure_widget(app, saved_pos).await?;
    } else if let Some(widget) = app.get_webview_window("widget") {
        widget.hide()?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state.settings_service.get().await
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    settings: AppSettings,
    state: State<'_, AppState>,
) -> AppResult<AppSettings> {
    let saved = state.settings_service.save(&settings).await?;
    apply_runtime_settings(&app, &state, &saved).await?;
    Ok(saved)
}

#[tauri::command]
pub async fn list_shortcuts(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::ShortcutBinding>> {
    state
        .shortcut_service
        .list(&state.platform.shortcuts.bindings())
        .await
}

#[tauri::command]
pub async fn update_shortcuts(
    state: State<'_, AppState>,
    app: AppHandle,
    shortcuts: Vec<ShortcutBinding>,
) -> AppResult<Vec<ShortcutBinding>> {
    let saved = state
        .shortcut_service
        .save(&state.platform.shortcuts.bindings(), &shortcuts)
        .await?;
    crate::apply_shortcut_bindings(&app, &state, &saved).await?;
    Ok(saved)
}

#[tauri::command]
pub async fn export_data(app: AppHandle, state: State<'_, AppState>) -> AppResult<String> {
    let Some(path) = state.platform.file_system.choose_export_path(&app).await? else {
        return Ok("Export canceled.".into());
    };

    let payload = BackupPayload {
        exported_at: Utc::now().to_rfc3339(),
        version: "0.1.0".into(),
        memories: state.memory_service.list().await?,
        projects: state.project_service.list().await?,
        settings: state.settings_service.get().await?,
        license: state.license_service.get_state().await?,
    };

    ExportService::export_to_path(
        &path,
        payload.memories,
        payload.projects,
        payload.settings,
        payload.license,
    )
    .await
}

#[tauri::command]
pub async fn import_data(app: AppHandle, state: State<'_, AppState>) -> AppResult<String> {
    let Some(path) = state.platform.file_system.choose_import_path(&app).await? else {
        return Ok("Import canceled.".into());
    };

    let payload = ExportService::import_from_path(&path).await?;

    wipe_local_data(&state).await?;

    for project in payload.projects {
        sqlx::query(
            "INSERT INTO projects (id, name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(project.id)
        .bind(project.name)
        .bind(project.description)
        .bind(project.created_at)
        .bind(project.updated_at)
        .execute(&state.pool)
        .await?;
    }

    for memory in payload.memories {
        sqlx::query(
            r#"
            INSERT INTO memories (
              id, source_type, title, content, note, project_id, url, domain, resolved_domain, canonical_url, resolved_title, resolved_description, resolved_image, resolved_site_name, preview_text, memory_type, topic_labels, primary_topic, quality_score, bookmark_quality_score, is_duplicate_of, bookmark_folder_path, enrichment_status, enrichment_error, enriched_at, last_enriched_at, external_id, folder_path, source_app, source_window, resurface_at, resurface_dismissed_at, last_opened_at, open_count, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(memory.id)
        .bind(memory.source_type)
        .bind(memory.title)
        .bind(memory.content)
        .bind(memory.note)
        .bind(memory.project_id)
        .bind(memory.url)
        .bind(memory.domain)
        .bind(memory.resolved_domain)
        .bind(memory.canonical_url)
        .bind(memory.resolved_title)
        .bind(memory.resolved_description)
        .bind(memory.resolved_image)
        .bind(memory.resolved_site_name)
        .bind(memory.preview_text)
        .bind(memory.memory_type)
        .bind(memory.topic_labels)
        .bind(memory.primary_topic)
        .bind(memory.quality_score)
        .bind(memory.bookmark_quality_score)
        .bind(memory.is_duplicate_of)
        .bind(memory.bookmark_folder_path)
        .bind(memory.enrichment_status)
        .bind(memory.enrichment_error)
        .bind(memory.enriched_at)
        .bind(memory.last_enriched_at)
        .bind(memory.external_id)
        .bind(memory.folder_path)
        .bind(memory.source_app)
        .bind(memory.source_window)
        .bind(memory.resurface_at)
        .bind(memory.resurface_dismissed_at)
        .bind(memory.last_opened_at)
        .bind(memory.open_count)
        .bind(memory.created_at)
        .bind(memory.updated_at)
        .execute(&state.pool)
        .await?;
    }

    let saved_settings = state.settings_service.save(&payload.settings).await?;
    state.license_repository.save(&payload.license).await?;
    apply_runtime_settings(&app, &state, &saved_settings).await?;

    Ok(format!("Backup imported from {}", path.display()))
}

#[tauri::command]
pub async fn clear_all_data(state: State<'_, AppState>) -> AppResult<()> {
    wipe_local_data(&state).await
}

#[tauri::command]
pub async fn seed_sample_data(state: State<'_, AppState>) -> AppResult<()> {
    ensure_seed_data(&state.pool).await
}
