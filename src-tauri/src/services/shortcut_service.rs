use std::collections::{HashMap, HashSet};

use sqlx::SqlitePool;

use crate::{
    errors::app_error::{AppError, AppResult},
    models::ShortcutBinding,
};

const SHORTCUT_KEY_PREFIX: &str = "shortcut_binding:";
const MODIFIER_TOKENS: &[&str] = &["Ctrl", "Alt", "Shift", "Super"];

pub struct ShortcutService {
    pool: SqlitePool,
}

impl ShortcutService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self, defaults: &[ShortcutBinding]) -> AppResult<Vec<ShortcutBinding>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM app_settings WHERE key LIKE ?",
        )
        .bind(format!("{SHORTCUT_KEY_PREFIX}%"))
        .fetch_all(&self.pool)
        .await?;

        let overrides = rows
            .into_iter()
            .filter_map(|(key, value)| {
                key.strip_prefix(SHORTCUT_KEY_PREFIX)
                    .map(|action| (action.to_string(), value))
            })
            .collect::<HashMap<_, _>>();

        Ok(defaults
            .iter()
            .map(|binding| ShortcutBinding {
                accelerator: overrides
                    .get(&binding.action)
                    .map(|value| normalize_accelerator(value))
                    .unwrap_or_else(|| normalize_accelerator(&binding.accelerator)),
                ..binding.clone()
            })
            .collect())
    }

    pub async fn save(
        &self,
        defaults: &[ShortcutBinding],
        requested: &[ShortcutBinding],
    ) -> AppResult<Vec<ShortcutBinding>> {
        let validated = validate_shortcuts(defaults, requested)?;
        let mut transaction = self.pool.begin().await?;

        for binding in &validated {
            sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)")
                .bind(format!("{SHORTCUT_KEY_PREFIX}{}", binding.action))
                .bind(&binding.accelerator)
                .execute(&mut *transaction)
                .await?;
        }

        transaction.commit().await?;

        self.list(defaults).await
    }
}

pub fn normalize_accelerator(value: &str) -> String {
    value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(normalize_accelerator_part)
        .collect::<Vec<_>>()
        .join("+")
}

fn normalize_accelerator_part(part: &str) -> String {
    let lowered = part.trim().to_ascii_lowercase();
    if let Some(letter) = lowered.strip_prefix("key").filter(|value| value.len() == 1) {
        return letter.to_ascii_uppercase();
    }
    if let Some(digit) = lowered.strip_prefix("digit").filter(|value| value.len() == 1) {
        return digit.to_string();
    }

    match lowered.as_str() {
        "ctrl" | "control" => "Ctrl".into(),
        "alt" | "option" => "Alt".into(),
        "shift" => "Shift".into(),
        "meta" | "super" | "cmd" | "command" => "Super".into(),
        "space" | "spacebar" => "Space".into(),
        "enter" | "return" => "Enter".into(),
        "esc" | "escape" => "Esc".into(),
        "up" | "arrowup" => "Up".into(),
        "down" | "arrowdown" => "Down".into(),
        "left" | "arrowleft" => "Left".into(),
        "right" | "arrowright" => "Right".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "tab" => "Tab".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        value if value.len() == 1 => value.to_ascii_uppercase(),
        _ => {
            let mut chars = lowered.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

pub fn validate_shortcuts(
    defaults: &[ShortcutBinding],
    requested: &[ShortcutBinding],
) -> AppResult<Vec<ShortcutBinding>> {
    let default_map = defaults
        .iter()
        .map(|binding| (binding.action.clone(), binding.clone()))
        .collect::<HashMap<_, _>>();

    if requested.len() != defaults.len() {
        return Err(AppError::Invalid(
            "shortcut update payload is incomplete.".into(),
        ));
    }

    let mut seen_actions = HashSet::new();
    let mut seen_accelerators = HashSet::new();
    let mut validated = Vec::with_capacity(requested.len());

    for binding in requested {
        let Some(default_binding) = default_map.get(&binding.action) else {
            return Err(AppError::Invalid(format!(
                "unknown shortcut action `{}`.",
                binding.action
            )));
        };

        if !default_binding.editable {
            return Err(AppError::Invalid(format!(
                "shortcut `{}` is not editable.",
                binding.description
            )));
        }

        if !seen_actions.insert(binding.action.clone()) {
            return Err(AppError::Invalid(format!(
                "shortcut action `{}` was provided more than once.",
                binding.action
            )));
        }

        let accelerator = normalize_accelerator(&binding.accelerator);
        if accelerator.is_empty() {
            return Err(AppError::Invalid(format!(
                "shortcut `{}` cannot be empty.",
                binding.description
            )));
        }

        if !MODIFIER_TOKENS
            .iter()
            .any(|modifier| accelerator.split('+').any(|part| part == *modifier))
        {
            return Err(AppError::Invalid(format!(
                "shortcut `{}` must include at least one modifier key.",
                binding.description
            )));
        }

        let normalized_key_parts = accelerator.split('+').collect::<Vec<_>>();
        if normalized_key_parts.len() < 2 {
            return Err(AppError::Invalid(format!(
                "shortcut `{}` must include a non-modifier key.",
                binding.description
            )));
        }

        if !seen_accelerators.insert(accelerator.clone()) {
            return Err(AppError::Invalid(format!(
                "shortcut `{}` conflicts with another shortcut.",
                accelerator
            )));
        }

        validated.push(ShortcutBinding {
            action: binding.action.clone(),
            accelerator,
            editable: default_binding.editable,
            description: default_binding.description.clone(),
        });
    }

    for default_binding in defaults {
        if !seen_actions.contains(&default_binding.action) {
            return Err(AppError::Invalid(format!(
                "shortcut `{}` is missing from the update payload.",
                default_binding.description
            )));
        }
    }

    Ok(validated)
}

#[cfg(test)]
mod tests {
    use crate::models::ShortcutBinding;

    use super::{normalize_accelerator, validate_shortcuts};

    fn defaults() -> Vec<ShortcutBinding> {
        vec![
            ShortcutBinding {
                action: "open-search".into(),
                accelerator: "Alt+Space".into(),
                editable: true,
                description: "Open search overlay".into(),
            },
            ShortcutBinding {
                action: "open-quick-save".into(),
                accelerator: "Ctrl+Shift+S".into(),
                editable: true,
                description: "Open quick save".into(),
            },
            ShortcutBinding {
                action: "open-main-app".into(),
                accelerator: "Ctrl+Shift+O".into(),
                editable: true,
                description: "Open main app".into(),
            },
        ]
    }

    #[test]
    fn normalizes_shortcut_accelerators() {
        assert_eq!(normalize_accelerator("ctrl + shift + s"), "Ctrl+Shift+S");
        assert_eq!(normalize_accelerator("alt+space"), "Alt+Space");
        assert_eq!(normalize_accelerator("Control+Shift+KeyS"), "Ctrl+Shift+S");
        assert_eq!(normalize_accelerator("Control+Shift+KeyO"), "Ctrl+Shift+O");
    }

    #[test]
    fn rejects_duplicate_accelerators() {
        let mut requested = defaults();
        requested[1].accelerator = "Alt+Space".into();

        let error = validate_shortcuts(&defaults(), &requested).expect_err("duplicate should fail");
        assert!(error.to_string().contains("conflicts"));
    }

    #[test]
    fn rejects_shortcuts_without_modifiers() {
        let mut requested = defaults();
        requested[0].accelerator = "Space".into();

        let error = validate_shortcuts(&defaults(), &requested).expect_err("missing modifier should fail");
        assert!(error.to_string().contains("modifier"));
    }
}
