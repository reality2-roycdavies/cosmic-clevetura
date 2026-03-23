//! Clevetura keyboard HID protocol — request/response over vendor HID interface.
//!
//! The keyboard uses a request/response protocol over HID output/input reports:
//! - Output report ID: 0x21 (33) — host to device
//! - Input report ID: 0x22 (34) — device to host
//! - End-of-message byte: 0x0A
//! - Chunk size: 48 bytes per frame
//!
//! Commands are sent as: [OUTPUT_REPORT_ID, COMMAND_ID, ...payload]
//! Responses come as: [INPUT_REPORT_ID, STATUS, ...payload]
//!
//! Additionally, feature report 0xDB (72 bytes) contains device configuration
//! that can be read from any HID interface.

use hidapi::{HidApi, HidDevice};

use crate::config::SliderAction;
use crate::hid::{self, DeviceInfo};

/// HID protocol constants (discovered from TouchOnKeys app).
const OUTPUT_REPORT_ID: u8 = 0x21; // 33 — host to device
const INPUT_REPORT_ID: u8 = 0x22; // 34 — device to host
const END_BYTE: u8 = 0x0A;
const READ_TIMEOUT_MS: i32 = 2000;

/// Command IDs for the Clevetura protocol.
mod cmd {
    pub const DEAUTHORIZE: u8 = 120;
    pub const AUTHORIZE: u8 = 121; // Payload: 8-byte auth key
    pub const GET_PROTOCOL_VERSION: u8 = 130; // Returns 3 bytes
    pub const GET_FW_VERSION: u8 = 131; // Returns 3 bytes
    pub const GET_DEVICE_INFO: u8 = 132; // Returns 57 bytes
    pub const GET_SERIAL_NUMBER: u8 = 145; // Returns 4 bytes
    pub const GET_BAT_LEVEL: u8 = 153; // Returns 1 byte (signed)
    pub const GET_FW_DESCRIPTION: u8 = 150; // Returns 15 bytes
    pub const GET_CHIP_UID: u8 = 149; // Returns 8 bytes
}

/// Response status codes.
mod status {
    pub const OK: u8 = 1;
    pub const ERROR: u8 = 2;
}

/// Live state read from the keyboard.
#[derive(Debug, Clone)]
pub struct KeyboardState {
    pub connected: bool,
    pub battery_percent: Option<i8>,
    pub firmware_version: Option<String>,
    pub protocol_version: Option<String>,
    pub serial_number: Option<String>,
    pub mode: KeyboardMode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyboardMode {
    Typing,
    Touch,
    Unknown,
}

impl std::fmt::Display for KeyboardMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyboardMode::Typing => write!(f, "Typing"),
            KeyboardMode::Touch => write!(f, "Touch"),
            KeyboardMode::Unknown => write!(f, "Unknown"),
        }
    }
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            connected: false,
            battery_percent: None,
            firmware_version: None,
            protocol_version: None,
            serial_number: None,
            mode: KeyboardMode::Unknown,
        }
    }
}

/// Handle to an open Clevetura keyboard for reading/writing settings.
pub struct KeyboardConnection {
    device: HidDevice,
    info: DeviceInfo,
}

impl KeyboardConnection {
    /// Open the vendor HID interface (interface 2) of the first connected Clevetura keyboard.
    pub fn open() -> Result<Self, String> {
        Self::open_interface(2)
    }

    /// Open a specific HID interface by number.
    pub fn open_interface(iface: i32) -> Result<Self, String> {
        let devices = hid::enumerate_devices()?;

        let config_dev = devices
            .iter()
            .find(|d| d.interface_number == iface)
            .ok_or(format!(
                "No interface {} found on Clevetura keyboard",
                iface
            ))?;

        let api = HidApi::new().map_err(|e| format!("Failed to initialize HID API: {e}"))?;

        let device = api
            .open_path(
                std::ffi::CStr::from_bytes_with_nul(
                    &[config_dev.path.as_bytes(), &[0]].concat(),
                )
                .map_err(|_| "Invalid device path")?,
            )
            .map_err(|e| format!("Failed to open device: {e}"))?;

        Ok(Self {
            device,
            info: config_dev.clone(),
        })
    }

    /// Send a command and receive the response payload.
    ///
    /// The protocol uses the `Eo` class pattern from the TouchOnKeys app:
    /// - send: write [REPORT_ID, COMMAND_ID, ...payload] padded to outputReportSize (64)
    /// - receive: read until we get a response with INPUT_REPORT_ID as first byte,
    ///   then return the payload after the report ID
    ///
    /// On Linux hidapi with hidraw backend:
    /// - write() takes [report_id, ...data] (report_id is prepended)
    /// - read() returns [report_id, ...data] (report_id is included)
    fn send_command(&self, command_id: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        // Build the output: [OUTPUT_REPORT_ID, COMMAND_ID, ...payload]
        // hidapi write: first byte is report ID, then 63 bytes of data
        // (HID descriptor: Report Count = 63, Report Size = 8)
        let mut report = vec![0u8; 64]; // 1 byte report ID + 63 bytes data
        report[0] = OUTPUT_REPORT_ID;
        report[1] = command_id;
        let copy_len = payload.len().min(61);
        report[2..2 + copy_len].copy_from_slice(&payload[..copy_len]);

        self.device
            .write(&report)
            .map_err(|e| format!("Failed to send command 0x{:02x}: {e}", command_id))?;

        // Read response
        let mut buf = [0u8; 64];
        let len = self
            .device
            .read_timeout(&mut buf, READ_TIMEOUT_MS)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if len == 0 {
            return Err("No response from device (timeout)".to_string());
        }

        // hidraw read on Linux: buf[0] is the report ID
        // We expect INPUT_REPORT_ID (0x22), then status, then payload
        if buf[0] == INPUT_REPORT_ID && len > 1 {
            // buf[1] is the status
            if buf[1] == status::ERROR {
                return Err(format!(
                    "Device returned error for command 0x{:02x}",
                    command_id
                ));
            }
            // Return payload after status
            if len > 2 {
                Ok(buf[2..len].to_vec())
            } else {
                Ok(Vec::new())
            }
        } else {
            // Fallback: report ID might be stripped by hidapi
            // In that case buf[0] is the status
            if buf[0] == status::ERROR {
                return Err(format!(
                    "Device returned error for command 0x{:02x}",
                    command_id
                ));
            }
            if len > 1 {
                Ok(buf[1..len].to_vec())
            } else {
                Ok(Vec::new())
            }
        }
    }

    /// Get the battery level as a signed percentage (-128 to 127).
    /// Negative values may indicate charging or error states.
    pub fn get_battery_level(&self) -> Result<i8, String> {
        let payload = self.send_command(cmd::GET_BAT_LEVEL, &[])?;
        if payload.is_empty() {
            return Err("Empty battery response".to_string());
        }
        Ok(payload[0] as i8)
    }

    /// Get the firmware version as "major.minor.patch".
    pub fn get_firmware_version(&self) -> Result<String, String> {
        let payload = self.send_command(cmd::GET_FW_VERSION, &[])?;
        if payload.len() < 3 {
            return Err(format!(
                "Firmware version response too short ({} bytes)",
                payload.len()
            ));
        }
        Ok(format!("{}.{}.{}", payload[0], payload[1], payload[2]))
    }

    /// Get the protocol version as "major.minor.patch".
    pub fn get_protocol_version(&self) -> Result<String, String> {
        let payload = self.send_command(cmd::GET_PROTOCOL_VERSION, &[])?;
        if payload.len() < 3 {
            return Err(format!(
                "Protocol version response too short ({} bytes)",
                payload.len()
            ));
        }
        Ok(format!("{}.{}.{}", payload[0], payload[1], payload[2]))
    }

    /// Get the serial number as a hex string.
    pub fn get_serial_number(&self) -> Result<String, String> {
        let payload = self.send_command(cmd::GET_SERIAL_NUMBER, &[])?;
        // Serial is 4 bytes, little-endian. Pad to match the device's serial format.
        if payload.len() >= 4 {
            let serial = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            Ok(format!("{:012X}", serial))
        } else {
            Ok(payload
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>())
        }
    }

    /// Get full device info (57 bytes of hardware/firmware details).
    pub fn get_device_info_raw(&self) -> Result<Vec<u8>, String> {
        self.send_command(cmd::GET_DEVICE_INFO, &[])
    }

    /// Authorize the connection with the keyboard.
    /// The keyboard requires an authorization handshake before accepting commands.
    pub fn authorize(&self) -> Result<(), String> {
        let auth_key: [u8; 8] = [0x96, 0x25, 0xa6, 0xd9, 0xfb, 0x64, 0xcd, 0xea];
        let _result = self.send_command(cmd::AUTHORIZE, &auth_key)?;
        Ok(())
    }

    /// Read the current keyboard state by querying multiple endpoints.
    pub fn read_state(&self) -> KeyboardState {
        // Authorize first (ignore errors — may already be authorized)
        let _ = self.authorize();

        let battery = self.get_battery_level().ok();
        let fw_version = self.get_firmware_version().ok();
        let protocol_version = self.get_protocol_version().ok();
        let serial = self.get_serial_number().ok();

        KeyboardState {
            connected: true,
            battery_percent: battery,
            firmware_version: fw_version,
            protocol_version: protocol_version,
            serial_number: serial,
            mode: KeyboardMode::Unknown,
        }
    }

    /// Set the AI sensitivity level (1-9).
    ///
    /// Note: Based on reverse engineering, sensitivity is managed in the
    /// companion app software, not in the keyboard firmware. This is a
    /// placeholder for when/if a firmware command is discovered.
    pub fn set_sensitivity(&self, level: u8) -> Result<(), String> {
        if !(1..=9).contains(&level) {
            return Err(format!("Sensitivity must be 1-9, got {level}"));
        }
        // Sensitivity is managed in software (the app), not in firmware
        Ok(())
    }

    /// Configure a touch slider action.
    ///
    /// Note: Slider actions are handled in the companion app software,
    /// not in the keyboard firmware. The keyboard sends raw touch events
    /// and the app maps them to volume/brightness/etc actions.
    pub fn set_slider(&self, _slider: Slider, _action: &SliderAction) -> Result<(), String> {
        // Slider mapping is managed in software (the app), not in firmware
        Ok(())
    }

    pub fn device_info(&self) -> &DeviceInfo {
        &self.info
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Slider {
    Left,
    Right,
}

/// Try to read the keyboard state without keeping a persistent connection.
/// Returns default (disconnected) state if the device can't be opened.
pub fn poll_state() -> KeyboardState {
    match KeyboardConnection::open() {
        Ok(conn) => conn.read_state(),
        Err(_) => KeyboardState::default(),
    }
}

/// Print a full device info report.
pub fn print_device_info() {
    match KeyboardConnection::open() {
        Ok(conn) => {
            println!("Clevetura Device Info");
            println!("=====================\n");

            match conn.authorize() {
                Ok(()) => println!("Authorization:     OK"),
                Err(e) => println!("Authorization:     (error: {e})"),
            }

            match conn.get_firmware_version() {
                Ok(v) => println!("Firmware version:  {v}"),
                Err(e) => println!("Firmware version:  (error: {e})"),
            }

            match conn.get_protocol_version() {
                Ok(v) => println!("Protocol version:  {v}"),
                Err(e) => println!("Protocol version:  (error: {e})"),
            }

            match conn.get_serial_number() {
                Ok(s) => println!("Serial number:     {s}"),
                Err(e) => println!("Serial number:     (error: {e})"),
            }

            match conn.get_battery_level() {
                Ok(b) => println!("Battery level:     {b}%"),
                Err(e) => println!("Battery level:     (error: {e})"),
            }

            match conn.get_device_info_raw() {
                Ok(data) => {
                    println!("\nRaw device info ({} bytes):", data.len());
                    for (i, b) in data.iter().enumerate() {
                        if i > 0 && i % 16 == 0 {
                            println!();
                        }
                        print!(" {:02x}", b);
                    }
                    println!();
                }
                Err(e) => println!("\nDevice info:       (error: {e})"),
            }
        }
        Err(e) => {
            eprintln!("Could not open keyboard: {e}");
        }
    }
}

/// Probe the keyboard to discover available feature reports.
pub fn probe_reports() {
    println!("Probing Clevetura HID feature reports...\n");

    for iface in 0..=2 {
        println!("=== Interface {} ===", iface);
        match KeyboardConnection::open_interface(iface) {
            Ok(conn) => {
                println!(
                    "Connected to {} (interface {})",
                    conn.info.product_name, conn.info.interface_number
                );
                println!();

                let mut found = 0;
                for report_id in 0u8..=255 {
                    let mut buf = [0u8; 256];
                    buf[0] = report_id;

                    match conn.device.get_feature_report(&mut buf) {
                        Ok(len) => {
                            found += 1;
                            println!("Report 0x{:02x} ({} bytes):", report_id, len);
                            for (i, b) in buf[..len].iter().enumerate() {
                                if i > 0 && i % 16 == 0 {
                                    println!();
                                }
                                print!(" {:02x}", b);
                            }
                            println!();
                            println!();
                        }
                        Err(_) => {}
                    }
                }
                if found == 0 {
                    println!("No feature reports found on this interface.\n");
                }
            }
            Err(e) => {
                println!("Could not open: {e}\n");
            }
        }
    }
}

/// Watch report 0xDB on interface 2 continuously, printing only when bytes change.
pub fn watch_reports() {
    println!("Watching report 0xDB for changes (Ctrl+C to stop)...\n");

    match KeyboardConnection::open_interface(2) {
        Ok(conn) => {
            let mut last = [0u8; 256];
            let mut first = true;

            loop {
                let mut buf = [0u8; 256];
                buf[0] = 0xdb;

                match conn.device.get_feature_report(&mut buf) {
                    Ok(len) => {
                        if first || buf[..len] != last[..len] {
                            if !first {
                                print!("Changed bytes:");
                                for i in 0..len {
                                    if buf[i] != last[i] {
                                        print!(" [{}]: {:02x}->{:02x}", i, last[i], buf[i]);
                                    }
                                }
                                println!();
                            }

                            print!("0xDB:");
                            for (i, b) in buf[..len].iter().enumerate() {
                                if i > 0 && i % 16 == 0 {
                                    print!("\n     ");
                                }
                                print!(" {:02x}", b);
                            }
                            println!();
                            println!();

                            last[..len].copy_from_slice(&buf[..len]);
                            first = false;
                        }
                    }
                    Err(e) => {
                        eprintln!("Read error: {e}");
                        break;
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        Err(e) => {
            eprintln!("Could not open keyboard: {e}");
        }
    }
}
