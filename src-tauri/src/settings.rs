use std::fs;
use std::path::Path;

use crate::model::Settings;

fn path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("settings.json")
}

pub fn load(data_dir: &Path) -> Result<Settings, String> {
    let file = path(data_dir);
    if !file.exists() {
        return Ok(Settings::default());
    }
    let text = fs::read_to_string(&file).map_err(|e| format!("Could not read settings: {e}"))?;
    let mut settings: Settings = serde_json::from_str(&text).unwrap_or_default();
    if settings.theme.is_empty() {
        settings.theme = "system".into();
    }
    if settings.density.is_empty() {
        settings.density = "default".into();
    }
    if settings.reading_pane.is_empty() {
        settings.reading_pane = "none".into();
    }
    Ok(settings)
}

pub fn save(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path(data_dir), text).map_err(|e| format!("Could not save settings: {e}"))
}
