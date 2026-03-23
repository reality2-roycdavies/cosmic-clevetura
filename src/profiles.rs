//! Per-application profile switching.
//!
//! Monitors the active window and applies the matching profile's settings
//! to the keyboard when the focused application changes.

use crate::config::{AppProfile, Config};
use crate::keyboard::KeyboardConnection;

/// Apply a profile's settings to the keyboard.
pub fn apply_profile(conn: &KeyboardConnection, profile: &AppProfile) {
    let _ = conn.set_sensitivity(profile.sensitivity);
    let _ = conn.set_slider(
        crate::keyboard::Slider::Left,
        &profile.left_slider,
    );
    let _ = conn.set_slider(
        crate::keyboard::Slider::Right,
        &profile.right_slider,
    );
}

/// Apply the default (non-profile) settings from config.
pub fn apply_defaults(conn: &KeyboardConnection, config: &Config) {
    let _ = conn.set_sensitivity(config.sensitivity);
    let _ = conn.set_slider(
        crate::keyboard::Slider::Left,
        &config.left_slider,
    );
    let _ = conn.set_slider(
        crate::keyboard::Slider::Right,
        &config.right_slider,
    );
}

/// Get the currently focused application's app ID.
///
/// On COSMIC/Wayland, the compositor exposes the focused toplevel's app_id
/// via the wlr-foreign-toplevel-management protocol. For now we use a
/// D-Bus approach that works with cosmic-comp.
///
/// TODO: Implement proper Wayland protocol monitoring or D-Bus query.
pub fn get_active_app_id() -> Option<String> {
    // Placeholder: in a real implementation, this would query the compositor
    // for the currently focused toplevel's app_id.
    None
}

/// Check if the active app has changed and apply the appropriate profile.
/// Returns the app_id of the currently active application if found.
pub fn check_and_apply(
    conn: &KeyboardConnection,
    config: &Config,
    last_app_id: &Option<String>,
) -> Option<String> {
    if !config.profiles_enabled {
        return None;
    }

    let current = get_active_app_id();

    if current != *last_app_id {
        if let Some(ref app_id) = current {
            if let Some(profile) = config.profile_for_app(app_id) {
                apply_profile(conn, profile);
            } else {
                apply_defaults(conn, config);
            }
        } else {
            apply_defaults(conn, config);
        }
    }

    current
}
