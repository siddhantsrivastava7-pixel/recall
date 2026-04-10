use chrono::Utc;
use tokio::fs;

use crate::{
    errors::app_error::{AppError, AppResult},
    models::{BackupPayload, LicenseState, Memory, Project},
};

pub struct ExportService;

impl ExportService {
    pub async fn export_to_path(
        path: &std::path::Path,
        memories: Vec<Memory>,
        projects: Vec<Project>,
        settings: crate::models::AppSettings,
        license: LicenseState,
    ) -> AppResult<String> {
        let payload = BackupPayload {
            exported_at: Utc::now().to_rfc3339(),
            version: "0.1.0".into(),
            memories,
            projects,
            settings,
            license,
        };

        let json = serde_json::to_vec_pretty(&payload)?;
        fs::write(path, json).await?;
        Ok(format!("Backup exported to {}", path.display()))
    }

    pub async fn import_from_path(path: &std::path::Path) -> AppResult<BackupPayload> {
        let bytes = fs::read(path).await?;
        let payload = serde_json::from_slice::<BackupPayload>(&bytes)?;

        if payload.version.is_empty() {
            return Err(AppError::Invalid(
                "Backup payload is missing a version.".into(),
            ));
        }

        Ok(payload)
    }
}
