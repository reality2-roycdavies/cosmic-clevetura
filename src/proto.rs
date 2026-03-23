//! Protobuf-over-HID protocol for Clevetura keyboard configuration.
//!
//! The keyboard uses a two-layer protocol:
//! - Layer 1 (report IDs 0x21/0x22): Firmware commands (auth, battery, FW update)
//! - Layer 2 (report IDs 0x23/0x24): Protobuf app protocol (settings, profiles, gestures)
//!
//! Layer 2 flow:
//! 1. Build protobuf Request message
//! 2. Encode to protobuf bytes
//! 3. Base64 encode to ASCII
//! 4. Send as chunked HID writes (report ID 0x23, end-byte 0x0A, 63-byte chunks)
//! 5. Read chunked HID response (report ID 0x24, end-byte 0x0A)
//! 6. Base64 decode
//! 7. Decode protobuf Response message

use base64::Engine;
use hidapi::HidDevice;
use prost::Message;

/// HID report IDs for the app protocol layer.
const APP_OUTPUT_REPORT_ID: u8 = 0x23; // 35
const APP_INPUT_REPORT_ID: u8 = 0x24; // 36
const END_BYTE: u8 = 0x0A;
const CHUNK_DATA_SIZE: usize = 63; // 64-byte report minus 1-byte report ID
const READ_TIMEOUT_MS: i32 = 2000;
const MAX_READ_RETRIES: usize = 100;
const MAX_EMPTY_RETRIES: usize = 4;

// ── Protobuf message definitions (reconstructed from TouchOnKeys JS) ──

/// Request_Type enum values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum RequestType {
    GetSettings = 0,
    SetSettings = 1,
    GetDeviceInfo = 2,
    Heartbeat = 3,
    SetProfileSettings = 4,
    GetProfileSettings = 5,
    ControlAi = 6,
    GetAiState = 7,
    SetOsMode = 8,
    GetDefaultSettings = 9,
    PerformFullReset = 10,
    GetUserAiData = 11,
    PerformRestart = 12,
}

/// Top-level Request message.
#[derive(Clone, PartialEq, Message)]
pub struct Request {
    #[prost(int32, tag = "1")]
    pub r#type: i32,
    #[prost(message, optional, tag = "2")]
    pub get_settings: Option<GetSettingsRequest>,
    #[prost(message, optional, tag = "3")]
    pub set_settings: Option<SetSettingsRequest>,
    #[prost(message, optional, tag = "4")]
    pub set_profile_settings: Option<SetProfileSettingsRequest>,
    #[prost(message, optional, tag = "6")]
    pub heart_beat: Option<HeartBeat>,
    #[prost(message, optional, tag = "7")]
    pub get_profile_settings: Option<GetProfileSettingsRequest>,
    #[prost(message, optional, tag = "10")]
    pub set_os_mode: Option<SetOsModeRequest>,
}

/// Top-level Response message.
#[derive(Clone, PartialEq, Message)]
pub struct Response {
    #[prost(int32, tag = "1")]
    pub r#type: i32,
    #[prost(message, optional, tag = "2")]
    pub get_settings: Option<GetSettingsResponse>,
    #[prost(message, optional, tag = "3")]
    pub set_settings: Option<SetSettingsResponse>,
    #[prost(message, optional, tag = "6")]
    pub heart_beat: Option<HeartBeatResponse>,
    #[prost(message, optional, tag = "7")]
    pub bad_request: Option<BadRequestResponse>,
    #[prost(message, optional, tag = "8")]
    pub get_profile_settings: Option<GetProfileSettingsResponse>,
}

// ── Sub-messages ──

#[derive(Clone, PartialEq, Message)]
pub struct GetSettingsRequest {}

#[derive(Clone, PartialEq, Message)]
pub struct GetSettingsResponse {
    #[prost(int32, tag = "1")]
    pub status: i32,
    #[prost(message, optional, tag = "2")]
    pub settings: Option<AppSettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SetSettingsRequest {
    #[prost(message, optional, tag = "1")]
    pub settings: Option<AppSettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SetSettingsResponse {
    #[prost(int32, tag = "1")]
    pub status: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SetProfileSettingsRequest {
    #[prost(message, optional, tag = "1")]
    pub settings: Option<ProfileSettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetProfileSettingsRequest {
    #[prost(uint32, tag = "1")]
    pub profile_id: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetProfileSettingsResponse {
    #[prost(int32, tag = "1")]
    pub status: i32,
    #[prost(message, optional, tag = "2")]
    pub settings: Option<ProfileSettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct HeartBeat {
    #[prost(uint32, tag = "1")]
    pub active_profile: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct HeartBeatResponse {
    #[prost(int32, tag = "1")]
    pub status: i32,
    #[prost(message, optional, tag = "2")]
    pub battery: Option<HeartBeatBattery>,
}

#[derive(Clone, PartialEq, Message)]
pub struct HeartBeatBattery {
    #[prost(int32, tag = "1")]
    pub level: i32,
    #[prost(bool, tag = "2")]
    pub charging: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct BadRequestResponse {
    #[prost(int32, tag = "1")]
    pub error: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SetOsModeRequest {
    #[prost(int32, tag = "1")]
    pub mode: i32,
}

/// AppSettings wraps global + profile settings.
#[derive(Clone, PartialEq, Message)]
pub struct AppSettings {
    #[prost(message, optional, tag = "1")]
    pub global: Option<GlobalSettings>,
    #[prost(message, optional, tag = "2")]
    pub global_profile: Option<ProfileSettings>,
    #[prost(uint32, optional, tag = "3")]
    pub counter: Option<u32>,
}

/// Global keyboard settings.
#[derive(Clone, PartialEq, Message)]
pub struct GlobalSettings {
    /// Single-finger tap enabled.
    #[prost(bool, optional, tag = "2")]
    pub tap1f_enable: Option<bool>,
    /// Two-finger tap enabled.
    #[prost(bool, optional, tag = "3")]
    pub tap2f_enable: Option<bool>,
    /// Hold gesture enabled.
    #[prost(bool, optional, tag = "4")]
    pub hold_enable: Option<bool>,
    /// Swap left/right click buttons.
    #[prost(bool, optional, tag = "5")]
    pub swap_click_buttons: Option<bool>,
    /// AI sensitivity level (1-9).
    #[prost(uint32, optional, tag = "6")]
    pub current_ai_level: Option<u32>,
    /// Newbie mode.
    #[prost(bool, optional, tag = "7")]
    pub newbie_mode_enable: Option<bool>,
    /// Touch activation after lift-off.
    #[prost(bool, optional, tag = "8")]
    pub touch_activation_after_lift_off: Option<bool>,
    /// Fn lock.
    #[prost(bool, optional, tag = "9")]
    pub fn_lock: Option<bool>,
    /// Auto brightness.
    #[prost(bool, optional, tag = "11")]
    pub auto_brightness_enable: Option<bool>,
    /// Dominant hand (0=right, 1=left).
    #[prost(int32, optional, tag = "12")]
    pub dominant_hand: Option<i32>,
    /// Battery saving mode.
    #[prost(bool, optional, tag = "14")]
    pub battery_saving_mode_enable: Option<bool>,
    /// Key suppressor.
    #[prost(bool, optional, tag = "15")]
    pub key_suppressor_enable: Option<bool>,
    /// Hold delay on border.
    #[prost(bool, optional, tag = "16")]
    pub hold_delay_on_border_enable: Option<bool>,
    /// Swap Fn and Ctrl keys.
    #[prost(bool, optional, tag = "17")]
    pub swap_fn_ctrl: Option<bool>,
}

/// Per-profile settings (gestures, sliders, F-keys).
#[derive(Clone, PartialEq, Message)]
pub struct ProfileSettings {
    #[prost(uint32, tag = "1")]
    pub id: u32,
    #[prost(message, optional, tag = "2")]
    pub gestures: Option<GestureSettings>,
    #[prost(message, optional, tag = "3")]
    pub touch_zone: Option<TouchZoneSettings>,
    #[prost(message, optional, tag = "5")]
    pub keyboard: Option<KeyboardSettings>,
}

// Placeholder types for gesture/touch/keyboard settings
// These can be fleshed out later as needed.

#[derive(Clone, PartialEq, Message)]
pub struct GestureSettings {
    #[prost(message, optional, tag = "1")]
    pub three_finger: Option<GestureGroup>,
    #[prost(message, optional, tag = "2")]
    pub four_finger: Option<GestureGroup>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GestureGroup {
    #[prost(message, optional, tag = "1")]
    pub swipe: Option<GestureSwipe>,
    #[prost(message, optional, tag = "2")]
    pub tap: Option<GestureTap>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GestureSwipe {
    #[prost(message, optional, tag = "1")]
    pub up: Option<GestureAction>,
    #[prost(message, optional, tag = "2")]
    pub down: Option<GestureAction>,
    #[prost(message, optional, tag = "3")]
    pub left: Option<GestureAction>,
    #[prost(message, optional, tag = "4")]
    pub right: Option<GestureAction>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GestureTap {
    #[prost(message, optional, tag = "1")]
    pub action: Option<GestureAction>,
}

/// Gesture action — oneof with touchpad passthrough, nothing, shortcut, or as-global.
#[derive(Clone, PartialEq, Message)]
pub struct GestureAction {
    #[prost(message, optional, tag = "1")]
    pub touchpad: Option<GestureTouchpad>,
    #[prost(message, optional, tag = "2")]
    pub nothing: Option<GestureNothing>,
    #[prost(message, optional, tag = "3")]
    pub shortcut: Option<GestureShortcut>,
    #[prost(message, optional, tag = "4")]
    pub as_global: Option<GestureAsGlobal>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GestureTouchpad {}

#[derive(Clone, PartialEq, Message)]
pub struct GestureNothing {}

#[derive(Clone, PartialEq, Message)]
pub struct GestureAsGlobal {}

#[derive(Clone, PartialEq, Message)]
pub struct GestureShortcut {
    #[prost(message, repeated, tag = "1")]
    pub direct: Vec<KeyEntry>,
    #[prost(message, repeated, tag = "2")]
    pub opposite: Vec<KeyEntry>,
    #[prost(uint32, optional, tag = "3")]
    pub sensitivity: Option<u32>,
    #[prost(bool, optional, tag = "4")]
    pub continuous: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct TouchZoneSettings {
    #[prost(message, optional, tag = "2")]
    pub slider: Option<SliderSettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SliderSettings {
    #[prost(message, optional, tag = "3")]
    pub left: Option<SliderConfig>,
    #[prost(message, optional, tag = "4")]
    pub right: Option<SliderConfig>,
}

/// Individual slider configuration.
/// Field numbers from JS: sensitivity=2(uint32), customShortcut=11(msg),
/// custom=12(int32/enum), nothing=13(msg), asGlobal=14(msg)
#[derive(Clone, PartialEq, Message)]
pub struct SliderConfig {
    #[prost(uint32, optional, tag = "2")]
    pub sensitivity: Option<u32>,
    #[prost(message, optional, tag = "11")]
    pub custom_shortcut: Option<SliderShortcut>,
    #[prost(int32, optional, tag = "12")]
    pub custom: Option<i32>,
    #[prost(message, optional, tag = "13")]
    pub nothing: Option<SliderNothing>,
    #[prost(message, optional, tag = "14")]
    pub as_global: Option<SliderAsGlobal>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SliderShortcut {
    #[prost(message, optional, tag = "1")]
    pub increment: Option<KeyCombination>,
    #[prost(message, optional, tag = "2")]
    pub decrement: Option<KeyCombination>,
    #[prost(bool, optional, tag = "3")]
    pub continuous: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SliderNothing {}

#[derive(Clone, PartialEq, Message)]
pub struct SliderAsGlobal {}

#[derive(Clone, PartialEq, Message)]
pub struct KeyCombination {
    #[prost(message, repeated, tag = "1")]
    pub keys: Vec<KeyEntry>,
}

#[derive(Clone, PartialEq, Message)]
pub struct KeyEntry {
    #[prost(uint32, optional, tag = "1")]
    pub code: Option<u32>,
    #[prost(uint32, optional, tag = "2")]
    pub r#type: Option<u32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct KeyboardSettings {
    #[prost(message, optional, tag = "1")]
    pub f_key: Option<FKeySettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FKeySettings {
    #[prost(message, optional, tag = "13")]
    pub f1: Option<FKeyAction>,
    #[prost(message, optional, tag = "14")]
    pub f2: Option<FKeyAction>,
    #[prost(message, optional, tag = "15")]
    pub f3: Option<FKeyAction>,
    #[prost(message, optional, tag = "16")]
    pub f4: Option<FKeyAction>,
    #[prost(message, optional, tag = "17")]
    pub f5: Option<FKeyAction>,
    #[prost(message, optional, tag = "18")]
    pub f6: Option<FKeyAction>,
    #[prost(message, optional, tag = "19")]
    pub f7: Option<FKeyAction>,
    #[prost(message, optional, tag = "20")]
    pub f8: Option<FKeyAction>,
    #[prost(message, optional, tag = "21")]
    pub f9: Option<FKeyAction>,
    #[prost(message, optional, tag = "22")]
    pub f10: Option<FKeyAction>,
    #[prost(message, optional, tag = "23")]
    pub f11: Option<FKeyAction>,
    #[prost(message, optional, tag = "24")]
    pub f12: Option<FKeyAction>,
}

/// F-key action — similar oneof pattern.
#[derive(Clone, PartialEq, Message)]
pub struct FKeyAction {
    #[prost(message, optional, tag = "1")]
    pub nothing: Option<FKeyNothing>,
    #[prost(message, optional, tag = "2")]
    pub custom: Option<FKeyCustom>,
    #[prost(message, optional, tag = "3")]
    pub as_global: Option<FKeyAsGlobal>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FKeyNothing {}

#[derive(Clone, PartialEq, Message)]
pub struct FKeyCustom {
    #[prost(message, repeated, tag = "1")]
    pub keys: Vec<KeyEntry>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FKeyAsGlobal {}

// ── Transport: protobuf over chunked HID ──

/// Send a protobuf Request and receive a Response over the app HID protocol.
pub fn send_proto_request(device: &HidDevice, request: &Request) -> Result<Response, String> {
    // 1. Encode protobuf to bytes
    let proto_bytes = request.encode_to_vec();

    // 2. Base64 encode
    let b64 = base64::engine::general_purpose::STANDARD.encode(&proto_bytes);
    let b64_bytes = b64.as_bytes();

    // 3. Append end-byte and chunk into HID reports
    let mut payload = b64_bytes.to_vec();
    payload.push(END_BYTE);

    for chunk in payload.chunks(CHUNK_DATA_SIZE) {
        let mut report = vec![0u8; 64]; // 1 byte report ID + 63 bytes data
        report[0] = APP_OUTPUT_REPORT_ID;
        let len = chunk.len().min(CHUNK_DATA_SIZE);
        report[1..1 + len].copy_from_slice(&chunk[..len]);

        device
            .write(&report)
            .map_err(|e| format!("HID write failed: {e}"))?;
    }

    // 4. Read chunked response
    let mut response_data = Vec::new();
    let mut retries = MAX_READ_RETRIES;
    let mut empty_count = 0;

    loop {
        if retries == 0 {
            return Err("Max read retries exceeded".to_string());
        }
        retries -= 1;

        let mut buf = [0u8; 64];
        let len = device
            .read_timeout(&mut buf, READ_TIMEOUT_MS)
            .map_err(|e| format!("HID read failed: {e}"))?;

        if len == 0 {
            empty_count += 1;
            if empty_count >= MAX_EMPTY_RETRIES {
                return Err("No response data after retries".to_string());
            }
            continue;
        }
        empty_count = 0;

        // First byte should be APP_INPUT_REPORT_ID
        if buf[0] != APP_INPUT_REPORT_ID {
            continue;
        }

        // Look for end-byte in this chunk
        let data = &buf[1..len];
        if let Some(end_pos) = data.iter().position(|&b| b == END_BYTE) {
            response_data.extend_from_slice(&data[..end_pos]);
            break;
        } else {
            response_data.extend_from_slice(data);
        }
    }

    if response_data.is_empty() {
        return Err("Empty response".to_string());
    }

    // Strip null bytes and non-ASCII from response
    response_data.retain(|&b| b > 0 && b < 128);

    // 5. Base64 decode
    let b64_str =
        std::str::from_utf8(&response_data).map_err(|e| format!("Invalid UTF-8 response: {e}"))?;

    let proto_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_str)
        .map_err(|e| format!("Base64 decode failed: {e}"))?;

    // 6. Decode protobuf Response
    let response =
        Response::decode(proto_bytes.as_slice()).map_err(|e| format!("Protobuf decode failed: {e}"))?;

    Ok(response)
}

// ── Convenience functions ──

/// Get current keyboard settings.
pub fn get_settings(device: &HidDevice) -> Result<AppSettings, String> {
    let request = Request {
        r#type: RequestType::GetSettings as i32,
        get_settings: Some(GetSettingsRequest {}),
        ..Default::default()
    };

    let response = send_proto_request(device, &request)?;

    let get_settings = response
        .get_settings
        .ok_or("No getSettings in response")?;

    get_settings
        .settings
        .ok_or_else(|| "No settings in getSettings response".to_string())
}

/// Set global keyboard settings (sensitivity, taps, etc.).
pub fn set_settings(device: &HidDevice, settings: AppSettings) -> Result<(), String> {
    let request = Request {
        r#type: RequestType::SetSettings as i32,
        set_settings: Some(SetSettingsRequest {
            settings: Some(settings),
        }),
        ..Default::default()
    };

    let response = send_proto_request(device, &request)?;

    if let Some(bad) = &response.bad_request {
        return Err(format!("Bad request error: {}", bad.error));
    }

    if let Some(set_resp) = &response.set_settings {
        if set_resp.status != 0 {
            return Err(format!("SetSettings failed with status {}", set_resp.status));
        }
    }

    Ok(())
}

/// Send a heartbeat with the active profile ID. Returns battery info.
pub fn heartbeat(device: &HidDevice, active_profile: u32) -> Result<Option<HeartBeatBattery>, String> {
    let request = Request {
        r#type: RequestType::Heartbeat as i32,
        heart_beat: Some(HeartBeat { active_profile }),
        ..Default::default()
    };

    let response = send_proto_request(device, &request)?;

    Ok(response.heart_beat.and_then(|hb| hb.battery))
}

/// Set OS mode (0=win, 1=mac, 2=linux).
pub fn set_os_mode(device: &HidDevice, mode: i32) -> Result<(), String> {
    let request = Request {
        r#type: RequestType::SetOsMode as i32,
        set_os_mode: Some(SetOsModeRequest { mode }),
        ..Default::default()
    };

    let _response = send_proto_request(device, &request)?;
    Ok(())
}

/// Print current settings for debugging.
pub fn print_settings(device: &HidDevice) {
    match get_settings(device) {
        Ok(settings) => {
            println!("Keyboard Settings (from firmware)");
            println!("==================================\n");

            if let Some(global) = &settings.global {
                println!("Global Settings:");
                if let Some(level) = global.current_ai_level {
                    println!("  AI Sensitivity:    {}/9", level);
                }
                if let Some(v) = global.tap1f_enable {
                    println!("  1-finger tap:      {}", if v { "ON" } else { "OFF" });
                }
                if let Some(v) = global.tap2f_enable {
                    println!("  2-finger tap:      {}", if v { "ON" } else { "OFF" });
                }
                if let Some(v) = global.hold_enable {
                    println!("  Hold gesture:      {}", if v { "ON" } else { "OFF" });
                }
                if let Some(v) = global.swap_click_buttons {
                    println!("  Swap clicks:       {}", if v { "YES" } else { "NO" });
                }
                if let Some(v) = global.fn_lock {
                    println!("  Fn Lock:           {}", if v { "ON" } else { "OFF" });
                }
                if let Some(v) = global.dominant_hand {
                    println!(
                        "  Dominant hand:     {}",
                        if v == 0 { "Right" } else { "Left" }
                    );
                }
                if let Some(v) = global.swap_fn_ctrl {
                    println!("  Swap Fn/Ctrl:      {}", if v { "YES" } else { "NO" });
                }
                if let Some(v) = global.auto_brightness_enable {
                    println!("  Auto brightness:   {}", if v { "ON" } else { "OFF" });
                }
                if let Some(v) = global.battery_saving_mode_enable {
                    println!("  Battery saving:    {}", if v { "ON" } else { "OFF" });
                }
            }

            if let Some(profile) = &settings.global_profile {
                println!("\nGlobal Profile (id={}):", profile.id);
                if let Some(tz) = &profile.touch_zone {
                    if let Some(slider) = &tz.slider {
                        if let Some(left) = &slider.left {
                            println!(
                                "  Left slider:       sensitivity={}",
                                left.sensitivity.unwrap_or(0)
                            );
                        }
                        if let Some(right) = &slider.right {
                            println!(
                                "  Right slider:      sensitivity={}",
                                right.sensitivity.unwrap_or(0)
                            );
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to get settings: {e}");
        }
    }
}
