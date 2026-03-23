//! CLI settings protocol for cosmic-applet-settings hub integration.

use crate::config::Config;

pub fn describe() {
    let config = Config::load();

    let slider_options = serde_json::json!([
        {"value": "brightness", "label": "Backlight Brightness"},
        {"value": "volume", "label": "System Volume"},
        {"value": "media_scrub", "label": "Media Scrub"},
        {"value": "zoom", "label": "Zoom Level"},
        {"value": "scroll_speed", "label": "Scroll Speed"}
    ]);

    let left_slider_value = slider_action_to_cli(&config.left_slider);
    let right_slider_value = slider_action_to_cli(&config.right_slider);

    let schema = serde_json::json!({
        "title": "Clevetura TouchOnKeys Settings",
        "description": "Configure your Clevetura keyboard's touch sensitivity and sliders.",
        "sections": [
            {
                "title": "Touch Sensitivity",
                "items": [
                    {
                        "type": "number",
                        "key": "sensitivity",
                        "label": "Sensitivity Level (1-9)",
                        "value": config.sensitivity,
                        "min": 1,
                        "max": 9
                    }
                ]
            },
            {
                "title": "Touch Sliders",
                "items": [
                    {
                        "type": "select",
                        "key": "left_slider",
                        "label": "Left Slider (F2-F6)",
                        "value": left_slider_value,
                        "options": slider_options
                    },
                    {
                        "type": "select",
                        "key": "right_slider",
                        "label": "Right Slider (F7-F11)",
                        "value": right_slider_value,
                        "options": slider_options
                    }
                ]
            },
            {
                "title": "Profiles",
                "items": [
                    {
                        "type": "toggle",
                        "key": "profiles_enabled",
                        "label": "Enable per-app profiles",
                        "value": config.profiles_enabled
                    }
                ]
            }
        ],
        "actions": [
            {"id": "reset", "label": "Reset to Defaults", "style": "destructive"}
        ]
    });

    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}

pub fn set(key: &str, value: &str) {
    let mut config = Config::load();

    let result: Result<&str, String> = match key {
        "sensitivity" => match serde_json::from_str::<u8>(value) {
            Ok(level) if (1..=9).contains(&level) => {
                config.sensitivity = level;
                Ok("Updated sensitivity")
            }
            Ok(_) => Err("Sensitivity must be 1-9".to_string()),
            Err(e) => Err(format!("Invalid number: {e}")),
        },
        "left_slider" => match cli_to_slider_action(value) {
            Ok(action) => {
                config.left_slider = action;
                Ok("Updated left slider")
            }
            Err(e) => Err(e),
        },
        "right_slider" => match cli_to_slider_action(value) {
            Ok(action) => {
                config.right_slider = action;
                Ok("Updated right slider")
            }
            Err(e) => Err(e),
        },
        "profiles_enabled" => match serde_json::from_str::<bool>(value) {
            Ok(enabled) => {
                config.profiles_enabled = enabled;
                Ok("Updated profiles setting")
            }
            Err(e) => Err(format!("Invalid boolean: {e}")),
        },
        _ => Err(format!("Unknown key: {key}")),
    };

    match result {
        Ok(msg) => match config.save() {
            Ok(()) => print_response(true, msg),
            Err(e) => print_response(false, &format!("Save failed: {e}")),
        },
        Err(e) => print_response(false, &e),
    }
}

pub fn action(id: &str) {
    match id {
        "reset" => {
            let config = Config::default();
            match config.save() {
                Ok(()) => print_response(true, "Reset to defaults"),
                Err(e) => print_response(false, &format!("Reset failed: {e}")),
            }
        }
        _ => print_response(false, &format!("Unknown action: {id}")),
    }
}

fn slider_action_to_cli(action: &crate::config::SliderAction) -> &'static str {
    use crate::config::SliderAction;
    match action {
        SliderAction::Brightness => "brightness",
        SliderAction::Volume => "volume",
        SliderAction::MediaScrub => "media_scrub",
        SliderAction::ZoomLevel => "zoom",
        SliderAction::ScrollSpeed => "scroll_speed",
        SliderAction::Custom(_) => "brightness",
    }
}

fn cli_to_slider_action(value: &str) -> Result<crate::config::SliderAction, String> {
    use crate::config::SliderAction;
    let s: String =
        serde_json::from_str(value).map_err(|e| format!("Invalid string: {e}"))?;
    match s.as_str() {
        "brightness" => Ok(SliderAction::Brightness),
        "volume" => Ok(SliderAction::Volume),
        "media_scrub" => Ok(SliderAction::MediaScrub),
        "zoom" => Ok(SliderAction::ZoomLevel),
        "scroll_speed" => Ok(SliderAction::ScrollSpeed),
        _ => Err(format!("Unknown slider action: {s}")),
    }
}

fn print_response(ok: bool, message: &str) {
    let resp = serde_json::json!({"ok": ok, "message": message});
    println!("{}", resp);
}
