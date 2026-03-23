//! Software-side slider action execution.
//!
//! The Clevetura keyboard's touch sliders are handled in software:
//! the keyboard sends raw touch events and our app maps them to
//! OS-level commands (volume, brightness, etc.).

use crate::config::SliderAction;
use std::process::Command;

/// Execute a slider increment action.
pub fn execute_increment(action: &SliderAction) {
    match action {
        SliderAction::Brightness => {
            // Use brightnessctl if available, fall back to xdotool/dbus
            let _ = Command::new("brightnessctl")
                .args(["set", "5%+"])
                .spawn();
        }
        SliderAction::Volume => {
            // Use pactl (PulseAudio/PipeWire) or wpctl (WirePlumber)
            if Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", "5%+"])
                .spawn()
                .is_err()
            {
                let _ = Command::new("pactl")
                    .args(["set-sink-volume", "@DEFAULT_SINK@", "+5%"])
                    .spawn();
            }
        }
        SliderAction::MediaScrub => {
            // Seek forward in media player via playerctl
            let _ = Command::new("playerctl").args(["position", "5+"]).spawn();
        }
        SliderAction::ZoomLevel => {
            // Simulate Ctrl++ for zoom in
            let _ = Command::new("xdotool")
                .args(["key", "ctrl+plus"])
                .spawn();
        }
        SliderAction::ScrollSpeed => {
            // Simulate scroll up
            let _ = Command::new("xdotool")
                .args(["click", "4"]) // scroll up
                .spawn();
        }
        SliderAction::Custom(cmd) => {
            if !cmd.is_empty() {
                let _ = Command::new("sh").args(["-c", cmd]).spawn();
            }
        }
    }
}

/// Execute a slider decrement action.
pub fn execute_decrement(action: &SliderAction) {
    match action {
        SliderAction::Brightness => {
            let _ = Command::new("brightnessctl")
                .args(["set", "5%-"])
                .spawn();
        }
        SliderAction::Volume => {
            if Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"])
                .spawn()
                .is_err()
            {
                let _ = Command::new("pactl")
                    .args(["set-sink-volume", "@DEFAULT_SINK@", "-5%"])
                    .spawn();
            }
        }
        SliderAction::MediaScrub => {
            let _ = Command::new("playerctl")
                .args(["position", "5-"])
                .spawn();
        }
        SliderAction::ZoomLevel => {
            let _ = Command::new("xdotool")
                .args(["key", "ctrl+minus"])
                .spawn();
        }
        SliderAction::ScrollSpeed => {
            let _ = Command::new("xdotool")
                .args(["click", "5"]) // scroll down
                .spawn();
        }
        SliderAction::Custom(cmd) => {
            if !cmd.is_empty() {
                let _ = Command::new("sh").args(["-c", cmd]).spawn();
            }
        }
    }
}
