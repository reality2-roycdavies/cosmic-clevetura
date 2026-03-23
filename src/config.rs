use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Action that a touch slider can perform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SliderAction {
    Brightness,
    Volume,
    MediaScrub,
    ZoomLevel,
    ScrollSpeed,
    Custom(String),
}

impl SliderAction {
    pub fn label(&self) -> &str {
        match self {
            SliderAction::Brightness => "Backlight Brightness",
            SliderAction::Volume => "System Volume",
            SliderAction::MediaScrub => "Media Scrub",
            SliderAction::ZoomLevel => "Zoom Level",
            SliderAction::ScrollSpeed => "Scroll Speed",
            SliderAction::Custom(_) => "Custom",
        }
    }

    pub fn all_standard() -> &'static [SliderAction] {
        &[
            SliderAction::Brightness,
            SliderAction::Volume,
            SliderAction::MediaScrub,
            SliderAction::ZoomLevel,
            SliderAction::ScrollSpeed,
        ]
    }
}

impl std::fmt::Display for SliderAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Per-application profile settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProfile {
    pub name: String,
    pub app_id: String,
    pub sensitivity: u8,
    pub left_slider: SliderAction,
    pub right_slider: SliderAction,
}

impl Default for AppProfile {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            app_id: String::new(),
            sensitivity: 5,
            left_slider: SliderAction::Brightness,
            right_slider: SliderAction::Volume,
        }
    }
}

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// AI sensitivity level (1-9) for type/touch switching.
    pub sensitivity: u8,
    /// Left touch slider action (F2-F6).
    pub left_slider: SliderAction,
    /// Right touch slider action (F7-F11).
    pub right_slider: SliderAction,
    /// Per-application profiles.
    pub profiles: Vec<AppProfile>,
    /// Whether per-app profile switching is enabled.
    pub profiles_enabled: bool,
    /// Saved BLE device address for auto-connect when USB is not available.
    pub ble_address: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sensitivity: 5,
            left_slider: SliderAction::Brightness,
            right_slider: SliderAction::Volume,
            profiles: Vec::new(),
            profiles_enabled: false,
            ble_address: None,
        }
    }
}

impl Config {
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("cosmic-clevetura").join("config.json"))
    }

    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or("Could not determine config path")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config dir: {e}"))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        std::fs::write(path, content).map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Find the profile for a given app ID, if any.
    pub fn profile_for_app(&self, app_id: &str) -> Option<&AppProfile> {
        self.profiles.iter().find(|p| p.app_id == app_id)
    }
}
