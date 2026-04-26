use chrono::{DateTime, Duration, Utc};

use crate::{
    db::repositories::SharedLicenseRepository,
    errors::app_error::{AppError, AppResult},
    models::LicenseState,
};

pub trait LicenseVerifier: Send + Sync {
    fn verify(&self, license_key: &str) -> AppResult<()>;
}

pub struct LocalLicenseVerifier;

impl LicenseVerifier for LocalLicenseVerifier {
    fn verify(&self, license_key: &str) -> AppResult<()> {
        let normalized = license_key.trim().to_uppercase();
        let valid_trial_shape = normalized.starts_with("RC-TRIAL-") && normalized.len() >= 17;
        if valid_trial_shape {
            return Ok(());
        }

        let valid_shape = normalized.starts_with("RC-") && normalized.len() >= 14;
        let checksum = normalized.bytes().fold(0u32, |acc, byte| acc + byte as u32) % 7 == 0;

        if valid_shape && checksum {
            Ok(())
        } else {
            Err(AppError::Invalid(
                "License key did not pass local validation.".into(),
            ))
        }
    }
}

pub struct LicenseService {
    repository: SharedLicenseRepository,
    verifier: Box<dyn LicenseVerifier>,
}

impl LicenseService {
    pub fn new(repository: SharedLicenseRepository, verifier: Box<dyn LicenseVerifier>) -> Self {
        Self {
            repository,
            verifier,
        }
    }

    pub async fn get_state(&self) -> AppResult<LicenseState> {
        let state = self.repository.get().await?;
        if state.is_activated && state.is_trial && is_expired(&state.expires_at) {
            return self
                .repository
                .save(&LicenseState {
                    is_activated: false,
                    last_checked_at: Some(Utc::now().to_rfc3339()),
                    ..state
                })
                .await;
        }

        Ok(state)
    }

    pub async fn activate(&self, license_key: &str) -> AppResult<LicenseState> {
        let normalized = license_key.trim().to_uppercase();
        self.verifier.verify(&normalized)?;
        let now_dt = Utc::now();
        let now = now_dt.to_rfc3339();
        let is_trial = normalized.starts_with("RC-TRIAL-");
        let expires_at = if is_trial {
            Some((now_dt + Duration::days(7)).to_rfc3339())
        } else {
            None
        };
        self.repository
            .save(&LicenseState {
                id: "license".into(),
                license_key: Some(normalized),
                is_activated: true,
                is_trial,
                activated_at: Some(now.clone()),
                expires_at,
                last_checked_at: Some(now),
            })
            .await
    }

    pub async fn deactivate(&self) -> AppResult<LicenseState> {
        self.repository
            .save(&LicenseState {
                id: "license".into(),
                license_key: None,
                is_activated: false,
                is_trial: false,
                activated_at: None,
                expires_at: None,
                last_checked_at: Some(Utc::now().to_rfc3339()),
            })
            .await
    }
}

fn is_expired(expires_at: &Option<String>) -> bool {
    let Some(expires_at) = expires_at else {
        return false;
    };

    DateTime::parse_from_rfc3339(expires_at)
        .map(|expires_at| expires_at.with_timezone(&Utc) <= Utc::now())
        .unwrap_or(false)
}
