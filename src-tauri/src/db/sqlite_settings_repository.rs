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
                // v0.5.21: parse u32; clamp invalid rows to the
                // default rather than panicking. `0` is a real
                // value that means "never unload."
                "ai_llm_idle_minutes" => {
                    settings.ai_llm_idle_minutes = value.parse::<u32>().unwrap_or(5)
                }
                // v0.5.22: low-battery pause threshold. `0` = disabled.
                // Clamp to 0..=100 since it's a percent.
                "ai_pause_below_battery_pct" => {
                    let parsed = value.parse::<u32>().unwrap_or(20);
                    settings.ai_pause_below_battery_pct = parsed.min(100);
                }
                // v0.5.32: screenshot retention window in days.
                // `0` = never purge. Cap at a sane high bound to
                // prevent overflow in the cutoff calculation.
                "ai_screenshot_retention_days" => {
                    let parsed = value.parse::<u32>().unwrap_or(60);
                    settings.ai_screenshot_retention_days = parsed.min(36500);
                }
                // v0.5.21: stored as "a" / "b" / "c" or empty for
                // None. Anything else is treated as None — defensive
                // against a hand-edited DB.
                "ai_tier_override" => {
                    settings.ai_tier_override = match value.as_str() {
                        "a" => Some(crate::ai::hardware::HardwareTier::A),
                        "b" => Some(crate::ai::hardware::HardwareTier::B),
                        "c" => Some(crate::ai::hardware::HardwareTier::C),
                        _ => None,
                    };
                }
                "ai_v0_5_6_backfill_done" => {
                    settings.ai_v0_5_6_backfill_done = Some(value == "true")
                }
                "ai_v0_5_7_backfill_done" => {
                    settings.ai_v0_5_7_backfill_done = Some(value == "true")
                }
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

        // v0.5.21: idle-reaper threshold (u32 minutes; 0 = never unload).
        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("ai_llm_idle_minutes")
            .bind(settings.ai_llm_idle_minutes.to_string())
            .execute(&mut *transaction)
            .await?;

        // v0.5.22: low-battery pause threshold (u32 percent; 0 = disabled).
        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("ai_pause_below_battery_pct")
            .bind(settings.ai_pause_below_battery_pct.to_string())
            .execute(&mut *transaction)
            .await?;

        // v0.5.32: screenshot retention window in days (u32; 0 = never purge).
        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("ai_screenshot_retention_days")
            .bind(settings.ai_screenshot_retention_days.to_string())
            .execute(&mut *transaction)
            .await?;

        // v0.5.21: hardware tier override. Stored as a single lowercase
        // letter ("a" / "b" / "c") when set, empty string when None.
        // Empty roundtrips to None on read, so toggling back to "auto"
        // doesn't leave a stale row.
        let tier_value = match settings.ai_tier_override {
            Some(crate::ai::hardware::HardwareTier::A) => "a",
            Some(crate::ai::hardware::HardwareTier::B) => "b",
            Some(crate::ai::hardware::HardwareTier::C) => "c",
            None => "",
        };
        sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
            .bind("ai_tier_override")
            .bind(tier_value)
            .execute(&mut *transaction)
            .await?;

        // v0.5.6 / v0.5.7: backfill completion flags. Optional —
        // only written once the corresponding backfill finishes;
        // until then the row is absent and `get()` defaults to
        // None which triggers the backfill.
        if let Some(done) = settings.ai_v0_5_6_backfill_done {
            sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
                .bind("ai_v0_5_6_backfill_done")
                .bind(if done { "true" } else { "false" })
                .execute(&mut *transaction)
                .await?;
        }
        if let Some(done) = settings.ai_v0_5_7_backfill_done {
            sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
                .bind("ai_v0_5_7_backfill_done")
                .bind(if done { "true" } else { "false" })
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
