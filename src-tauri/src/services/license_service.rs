use chrono::Utc;

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
        self.repository.get().await
    }

    pub async fn activate(&self, license_key: &str) -> AppResult<LicenseState> {
        self.verifier.verify(license_key)?;
        let now = Utc::now().to_rfc3339();
        self.repository
            .save(&LicenseState {
                id: "license".into(),
                license_key: Some(license_key.trim().to_uppercase()),
                is_activated: true,
                activated_at: Some(now.clone()),
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
                activated_at: None,
                last_checked_at: Some(Utc::now().to_rfc3339()),
            })
            .await
    }
}
