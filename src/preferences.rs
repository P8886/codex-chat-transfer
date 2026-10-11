use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

fn settings_path() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("无法找到应用设置目录")?
        .join("CodexChatTransfer/preferences.json"))
}

fn read_at(path: &Path) -> Option<Theme> {
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    serde_json::from_value(value["theme"].clone()).ok()
}

pub fn theme() -> Option<Theme> {
    settings_path().ok().and_then(|path| read_at(&path))
}

fn save_at(path: &Path, theme: Theme) -> Result<()> {
    let mut value = if path.exists() {
        crate::model::read_json(path)?
    } else {
        json!({})
    };
    value
        .as_object_mut()
        .context("应用设置文件格式无效")?
        .insert("theme".into(), serde_json::to_value(theme)?);
    crate::importer::atomic_write(path, &serde_json::to_vec_pretty(&value)?)
}

pub fn save_theme(theme: Theme) -> Result<()> {
    save_at(&settings_path()?, theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_survives_restarts_and_preserves_other_preferences() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("preferences.json");
        assert_eq!(read_at(&path), None);
        std::fs::write(&path, br#"{"unrelated":7}"#).unwrap();
        save_at(&path, Theme::Dark).unwrap();
        assert_eq!(read_at(&path), Some(Theme::Dark));
        assert_eq!(crate::model::read_json(&path).unwrap()["unrelated"], 7);
        save_at(&path, Theme::Light).unwrap();
        assert_eq!(read_at(&path), Some(Theme::Light));
        assert!(serde_json::from_value::<Theme>(json!("invalid")).is_err());
    }
}
