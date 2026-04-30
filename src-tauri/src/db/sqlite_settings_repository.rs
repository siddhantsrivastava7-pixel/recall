use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::{
    db::repositories::SettingsRepository,
    errors::app_error::AppResult,
    models::{AppSettings, BookmarkBrowser},
};

pub struct SqliteSettingsRepository {
    pool: SqlitePool,
}

impl SqliteSettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn normalize_bookmark_sync_browsers(browsers: Vec<BookmarkBrowser>) -> Vec<BookmarkBrowser> {
    #[cfg(target_os = "macos")]
    {
        let browsers = if browsers == BookmarkBrowser::legacy_default_sync_browsers() {
            let mut upgraded = browsers;
            upgraded.push(BookmarkBrowser::Safari);
            upgraded
        } else {
            browsers
        };

        let mut deduped = Vec::new();
        for browser in browsers {
            if !deduped.contains(&browser) {
                deduped.push(browser);
            }
        }

        return if deduped.is_empty() {
            BookmarkBrowser::default_sync_browsers()
        } else {
            deduped
        };
    }

    #[cfg(not(target_os = "macos"))]
    {
    let mut deduped = Vec::new();
    for browser in browsers {
        if !deduped.contains(&browser) {
            deduped.push(browser);
        }
    }

    if deduped.is_empty() {
        BookmarkBrowser::default_sync_browsers()
    } else {
        deduped
    }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get(&self) -> AppResult<AppSettings> {
        let values = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM app_settings")
            .fetch_all(&self.pool)
            .await?;

        let mut settings = AppSettings::default();

        for (key, value) in values {
            match key.as_str() {
                "floating_widget_enabled" => settings.floating_widget_enabled = value == "true",
                "launch_on_startup_enabled" => settings.launch_on_startup_enabled = value == "true",
                "update_auto_check_enabled" => settings.update_auto_check_enabled = value == "true",
                "bookmark_auto_sync_enabled" => {
                    settings.bookmark_auto_sync_enabled = value == "true"
                }
                "bookmark_sync_interval_minutes" => {
                    settings.bookmark_sync_interval_minutes = value.parse::<u32>().unwrap_or(15)
                }
                "bookmark_sync_browsers" => {
                    settings.bookmark_sync_browsers = normalize_bookmark_sync_browsers(
                        serde_json::from_str(&value)
                            .unwrap_or_else(|_| AppSettings::default().bookmark_sync_browsers),
                    )
                }
                "bookmark_last_synced_at" => {
                    settings.bookmark_last_synced_at = if value.trim().is_empty() {
                        None
                    } else {
                        Some(value)
                    }
                }
                "widget_position_x" => {
                    settings.widget_position_x = value.parse::<f64>().ok();
                }
                "widget_position_y" => {
                    settings.widget_position_y = value.parse::<f64>().ok();
                }
                "ai_enabled" => settings.ai_enabled = value == "true",
                "ai_pause_on_battery" => settings.ai_pause_on_battery = value == "true",
                "ai_heavy_only_on_ac" => settings.ai_heavy_only_on_ac = value == "true",
                _ => {}
            }
        }

        Ok(settings)
    }

    async fn save(&self, settings: &AppSettings) -> AppResult<AppSettings> {
        let mut transaction = self.pool.begin().await?;

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("floating_widget_enabled")
            .bind(if settings.floating_widget_enabled {
                "true"
            } else {
                "false"
            })
            .execute(&mut *transaction)
            .await?;

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("launch_on_startup_enabled")
            .bind(if settings.launch_on_startup_enabled {
                "true"
            } else {
                "false"
            })
            .execute(&mut *transaction)
            .await?;

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("update_auto_check_enabled")
            .bind(if settings.update_auto_check_enabled {
                "true"
            } else {
                "false"
            })
            .execute(&mut *transaction)
            .await?;

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("bookmark_auto_sync_enabled")
            .bind(if settings.bookmark_auto_sync_enabled {
                "true"
            } else {
                "false"
            })
            .execute(&mut *transaction)
            .await?;

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("bookmark_sync_interval_minutes")
            .bind(settings.bookmark_sync_interval_minutes.to_string())
            .execute(&mut *transaction)
            .await?;

        let bookmark_sync_browsers =
            normalize_bookmark_sync_browsers(settings.bookmark_sync_browsers.clone());

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("bookmark_sync_browsers")
            .bind(serde_json::to_string(&bookmark_sync_browsers)?)
            .execute(&mut *transaction)
            .await?;

        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("bookmark_last_synced_at")
            .bind(settings.bookmark_last_synced_at.clone().unwrap_or_default())
            .execute(&mut *transaction)
            .await?;

        if let Some(x) = settings.widget_position_x {
            sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
                .bind("widget_position_x")
                .bind(x.to_string())
                .execute(&mut *transaction)
                .await?;
        }

        if let Some(y) = settings.widget_position_y {
            sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
                .bind("widget_position_y")
                .bind(y.to_string())
                .execute(&mut *transaction)
                .await?;
        }

        // v0.2.0: AI subsystem toggles. Always written so a flip from
        // `true` back to `false` actually persists.
        for (key, value) in [
            ("ai_enabled", settings.ai_enabled),
            ("ai_pause_on_battery", settings.ai_pause_on_battery),
            ("ai_heavy_only_on_ac", settings.ai_heavy_only_on_ac),
        ] {
            sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
                .bind(key)
                .bind(if value { "true" } else { "false" })
                .execute(&mut *transaction)
                .await?;
        }

        transaction.commit().await?;

        self.get().await
    }

    async fn clear(&self) -> AppResult<()> {
        sqlx::query("DELETE FROM app_settings")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
