pub mod migrations;
pub mod repositories;
pub mod seed;
pub mod sqlite_ask_recall_session_repository;
pub mod sqlite_license_repository;
pub mod sqlite_memory_repository;
pub mod sqlite_project_repository;
pub mod sqlite_settings_repository;
pub mod system_projects;

use std::{path::PathBuf, sync::Arc};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use tauri::{AppHandle, Manager};

use crate::{
    db::{
        migrations::run_migrations,
        repositories::{
            SharedAskRecallSessionRepository, SharedLicenseRepository, SharedMemoryRepository,
            SharedProjectRepository, SharedSettingsRepository,
        },
        sqlite_ask_recall_session_repository::SqliteAskRecallSessionRepository,
        sqlite_license_repository::SqliteLicenseRepository,
        sqlite_memory_repository::SqliteMemoryRepository,
        sqlite_project_repository::SqliteProjectRepository,
        sqlite_settings_repository::SqliteSettingsRepository,
        system_projects::ensure_default_inbox_project,
    },
    errors::app_error::AppResult,
};

pub struct DatabaseContext {
    pub pool: SqlitePool,
    pub path: PathBuf,
    pub memory_repository: SharedMemoryRepository,
    pub project_repository: SharedProjectRepository,
    pub settings_repository: SharedSettingsRepository,
    pub license_repository: SharedLicenseRepository,
    pub ask_recall_session_repository: SharedAskRecallSessionRepository,
}

pub async fn initialize_database(app: &AppHandle) -> AppResult<DatabaseContext> {
    let app_data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data_dir)?;
    let database_path = app_data_dir.join("recall.sqlite");
    if !database_path.exists() {
        std::fs::File::create(&database_path)?;
    }
    let connection_options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connection_options)
        .await?;

    run_migrations(&pool).await?;

    ensure_default_inbox_project(&pool).await?;

    let memory_repository = Arc::new(SqliteMemoryRepository::new(pool.clone()));
    let project_repository = Arc::new(SqliteProjectRepository::new(pool.clone()));
    let settings_repository = Arc::new(SqliteSettingsRepository::new(pool.clone()));
    let license_repository = Arc::new(SqliteLicenseRepository::new(pool.clone()));
    let ask_recall_session_repository =
        Arc::new(SqliteAskRecallSessionRepository::new(pool.clone()));

    Ok(DatabaseContext {
        pool,
        path: database_path,
        memory_repository,
        project_repository,
        settings_repository,
        license_repository,
        ask_recall_session_repository,
    })
}

/// In-memory fallback used when the real DB fails to initialize.
/// This lets app.manage() always succeed so bootstrap_app can surface
/// the error gracefully rather than panicking.
pub async fn initialize_fallback_database() -> AppResult<DatabaseContext> {
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    let options = SqliteConnectOptions::from_str(":memory:")?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    run_migrations(&pool).await?;

    let memory_repository = Arc::new(SqliteMemoryRepository::new(pool.clone()));
    let project_repository = Arc::new(SqliteProjectRepository::new(pool.clone()));
    let settings_repository = Arc::new(SqliteSettingsRepository::new(pool.clone()));
    let license_repository = Arc::new(SqliteLicenseRepository::new(pool.clone()));
    let ask_recall_session_repository =
        Arc::new(SqliteAskRecallSessionRepository::new(pool.clone()));

    Ok(DatabaseContext {
        pool,
        path: std::path::PathBuf::from(":memory:"),
        memory_repository,
        project_repository,
        settings_repository,
        license_repository,
        ask_recall_session_repository,
    })
}
