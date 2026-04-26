use crate::{
    db::repositories::SharedProjectRepository, errors::app_error::AppResult, models::Project,
};

pub struct ProjectService {
    repository: SharedProjectRepository,
}

impl ProjectService {
    pub fn new(repository: SharedProjectRepository) -> Self {
        Self { repository }
    }

    pub async fn list(&self) -> AppResult<Vec<Project>> {
        self.repository.list().await
    }

    pub async fn create(&self, name: &str, description: Option<String>) -> AppResult<Project> {
        self.repository.create(name, description).await
    }

    pub async fn update(
        &self,
        id: &str,
        name: &str,
        description: Option<String>,
    ) -> AppResult<Project> {
        self.repository.update(id, name, description).await
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        self.repository.delete(id).await
    }
}
