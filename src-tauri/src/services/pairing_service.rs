use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    errors::app_error::AppResult,
    models::{PairingInfo, PairingQrPayload},
};

const PAIRING_IDENTITY_KEY: &str = "pairing_identity";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingIdentity {
    pub device_id: String,
    pub pairing_secret: String,
    pub desktop_name: String,
    pub endpoint: Option<String>,
    pub port: Option<u16>,
    pub created_at: String,
}

#[derive(Clone)]
pub struct PairingService {
    pool: SqlitePool,
}

impl PairingService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_or_create_identity(&self) -> AppResult<PairingIdentity> {
        if let Some(identity) = self.load_identity().await? {
            return Ok(identity);
        }

        let identity = self.generate_identity(None);
        self.save_identity(&identity).await?;
        Ok(identity)
    }

    pub async fn reset_identity(&self) -> AppResult<PairingIdentity> {
        let existing_endpoint = self
            .load_identity()
            .await?
            .and_then(|identity| identity.endpoint.zip(identity.port));
        let mut identity = self.generate_identity(
            existing_endpoint
                .as_ref()
                .map(|(endpoint, _)| endpoint.clone()),
        );
        if let Some((_, port)) = existing_endpoint {
            identity.port = Some(port);
        }
        self.save_identity(&identity).await?;
        Ok(identity)
    }

    pub async fn set_endpoint(&self, endpoint: String, port: u16) -> AppResult<PairingIdentity> {
        let mut identity = self.get_or_create_identity().await?;
        identity.endpoint = Some(endpoint);
        identity.port = Some(port);
        self.save_identity(&identity).await?;
        Ok(identity)
    }

    pub async fn current_secret(&self) -> AppResult<String> {
        Ok(self.get_or_create_identity().await?.pairing_secret)
    }

    pub async fn info(&self, receiver_running: bool) -> AppResult<PairingInfo> {
        Ok(to_pairing_info(
            self.get_or_create_identity().await?,
            receiver_running,
        )?)
    }

    fn generate_identity(&self, endpoint: Option<String>) -> PairingIdentity {
        PairingIdentity {
            device_id: format!("desktop-{}", Uuid::new_v4().simple()),
            pairing_secret: format!(
                "rcp_{}{}{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
            ),
            desktop_name: default_desktop_name(),
            endpoint,
            port: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    async fn load_identity(&self) -> AppResult<Option<PairingIdentity>> {
        let value =
            sqlx::query_as::<_, (String,)>("SELECT value FROM app_settings WHERE key = ? LIMIT 1")
                .bind(PAIRING_IDENTITY_KEY)
                .fetch_optional(&self.pool)
                .await?;

        match value {
            Some((raw,)) if !raw.trim().is_empty() => {
                Ok(Some(serde_json::from_str::<PairingIdentity>(&raw)?))
            }
            _ => Ok(None),
        }
    }

    async fn save_identity(&self, identity: &PairingIdentity) -> AppResult<()> {
        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind(PAIRING_IDENTITY_KEY)
            .bind(serde_json::to_string(identity)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn default_desktop_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Recall Desktop".into())
}

fn to_pairing_info(identity: PairingIdentity, receiver_running: bool) -> AppResult<PairingInfo> {
    let qr = PairingQrPayload {
        protocol: "recall-local-pairing".into(),
        version: 1,
        device_id: identity.device_id.clone(),
        desktop_name: identity.desktop_name.clone(),
        endpoint: identity.endpoint.clone(),
        secret: identity.pairing_secret.clone(),
    };

    Ok(PairingInfo {
        device_id: identity.device_id,
        pairing_secret: identity.pairing_secret,
        desktop_name: identity.desktop_name,
        endpoint: identity.endpoint,
        port: identity.port,
        created_at: identity.created_at,
        receiver_running,
        pairing_status: if receiver_running {
            "ready".into()
        } else {
            "not_running".into()
        },
        qr_payload: serde_json::to_string(&qr)?,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
    use std::str::FromStr;

    use crate::db::migrations::run_migrations;

    use super::PairingService;

    async fn make_service() -> PairingService {
        let options = SqliteConnectOptions::from_str(":memory:").expect("options");
        let pool = SqlitePool::connect_with(options).await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        PairingService::new(pool)
    }

    #[tokio::test]
    async fn pairing_identity_is_reused_until_reset() {
        let service = make_service().await;
        let first = service.get_or_create_identity().await.expect("first");
        let second = service.get_or_create_identity().await.expect("second");

        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.pairing_secret, second.pairing_secret);

        let reset = service.reset_identity().await.expect("reset");
        assert_ne!(first.device_id, reset.device_id);
        assert_ne!(first.pairing_secret, reset.pairing_secret);
    }
}
