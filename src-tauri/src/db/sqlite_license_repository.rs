use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::{
    db::repositories::LicenseRepository, errors::app_error::AppResult, models::LicenseState,
};

pub struct SqliteLicenseRepository {
    pool: SqlitePool,
}

impl SqliteLicenseRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LicenseRepository for SqliteLicenseRepository {
    async fn get(&self) -> AppResult<LicenseState> {
        let state = sqlx::query_as::<_, LicenseState>(
            "SELECT id, license_key, is_activated, is_trial, activated_at, expires_at, last_checked_at FROM license_state WHERE id = 'license'",
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(state.unwrap_or(LicenseState {
            id: "license".into(),
            license_key: None,
            is_activated: false,
            is_trial: false,
            activated_at: None,
            expires_at: None,
            last_checked_at: None,
        }))
    }

    async fn save(&self, license_state: &LicenseState) -> AppResult<LicenseState> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO license_state (id, license_key, is_activated, is_trial, activated_at, expires_at, last_checked_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&license_state.id)
        .bind(&license_state.license_key)
        .bind(license_state.is_activated)
        .bind(license_state.is_trial)
        .bind(&license_state.activated_at)
        .bind(&license_state.expires_at)
        .bind(&license_state.last_checked_at)
        .execute(&self.pool)
        .await?;

        self.get().await
    }

    async fn clear(&self) -> AppResult<()> {
        sqlx::query("DELETE FROM license_state")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
