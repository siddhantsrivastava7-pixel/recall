use crate::{
    db::repositories::SharedSettingsRepository, errors::app_error::AppResult, models::AppSettings,
};

pub struct SettingsService {
    repository: SharedSettingsRepository,
}

impl SettingsService {
    pub fn new(repository: SharedSettingsRepository) -> Self {
        Self { repository }
    }

    pub async fn get(&self) -> AppResult<AppSettings> {
        self.repository.get().await
    }

    pub async fn save(&self, settings: &AppSettings) -> AppResult<AppSettings> {
        self.repository.save(settings).await
    }
}
