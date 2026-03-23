//! Bluetooth Low Energy communication with Clevetura keyboards.
//!
//! Uses the same protocol layers as USB HID:
//! - Layer 1: Firmware commands (authorize, battery, FW version)
//! - Layer 2: Protobuf app protocol (settings, profiles, gestures)
//!
//! Over BLE, both layers use the same GATT characteristic for read/write,
//! with the same end-byte (0x0A) framing and base64 encoding for protobuf.
//!
//! Service UUID: d0bf1500-c402-424a-80b0-bc7aeced077e
//! Characteristic UUID: d0bf0001-c402-424a-80b0-bc7aeced077e

use base64::Engine;
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use prost::Message;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

use crate::proto;

/// Clevetura BLE service UUID.
const SERVICE_UUID: &str = "d0bf1500-c402-424a-80b0-bc7aeced077e";
/// Clevetura BLE characteristic UUID for data exchange.
const CHAR_UUID: &str = "d0bf0001-c402-424a-80b0-bc7aeced077e";
/// End-of-message byte.
const END_BYTE: u8 = 0x0A;
/// BLE chunk size (from reverse engineering).
const CHUNK_SIZE: usize = 56;
/// Read timeout per notification.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Authorization key (same as USB).
const AUTH_KEY: [u8; 8] = [0x96, 0x25, 0xa6, 0xd9, 0xfb, 0x64, 0xcd, 0xea];

/// Firmware command IDs (Layer 1).
mod cmd {
    pub const AUTHORIZE: u8 = 121;
    pub const GET_FW_VERSION: u8 = 131;
    pub const GET_BAT_LEVEL: u8 = 153;
    pub const GET_SERIAL_NUMBER: u8 = 145;
    pub const GET_PROTOCOL_VERSION: u8 = 130;
}

/// Information about a discovered BLE Clevetura device.
#[derive(Debug, Clone)]
pub struct BleDeviceInfo {
    pub name: String,
    pub address: String,
}

/// BLE connection to a Clevetura keyboard.
pub struct BleConnection {
    peripheral: Peripheral,
    characteristic: Characteristic,
}

/// Get the default BLE adapter.
pub async fn get_adapter() -> Result<Adapter, String> {
    let manager = Manager::new()
        .await
        .map_err(|e| format!("BLE manager init failed: {e}"))?;

    manager
        .adapters()
        .await
        .map_err(|e| format!("Failed to get BLE adapters: {e}"))?
        .into_iter()
        .next()
        .ok_or("No BLE adapters found".to_string())
}

/// Scan for Clevetura keyboards over BLE.
pub async fn scan_devices(scan_duration: Duration) -> Result<Vec<BleDeviceInfo>, String> {
    let adapter = get_adapter().await?;

    let service_uuid =
        Uuid::parse_str(SERVICE_UUID).map_err(|e| format!("Invalid service UUID: {e}"))?;

    adapter
        .start_scan(ScanFilter {
            services: vec![service_uuid],
        })
        .await
        .map_err(|e| format!("BLE scan failed: {e}"))?;

    tokio::time::sleep(scan_duration).await;

    adapter
        .stop_scan()
        .await
        .map_err(|e| format!("Failed to stop scan: {e}"))?;

    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| format!("Failed to get peripherals: {e}"))?;

    let mut devices = Vec::new();
    for p in peripherals {
        if let Ok(Some(props)) = p.properties().await {
            let has_service = props
                .services
                .iter()
                .any(|s| s.to_string() == SERVICE_UUID);
            if has_service {
                devices.push(BleDeviceInfo {
                    name: props.local_name.unwrap_or_else(|| "Clevetura".to_string()),
                    address: props.address.to_string(),
                });
            }
        }
    }

    Ok(devices)
}

impl BleConnection {
    /// Connect to a Clevetura keyboard over BLE by address.
    pub async fn connect_by_address(address: &str) -> Result<Self, String> {
        let adapter = get_adapter().await?;

        let char_uuid =
            Uuid::parse_str(CHAR_UUID).map_err(|e| format!("Invalid UUID: {e}"))?;

        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| format!("Failed to list peripherals: {e}"))?;

        let peripheral = {
            let mut found = None;
            for p in peripherals {
                if let Ok(Some(props)) = p.properties().await {
                    if props.address.to_string() == address {
                        found = Some(p);
                        break;
                    }
                }
            }
            found.ok_or(format!("Device not found: {address}"))?
        };

        if !peripheral
            .is_connected()
            .await
            .map_err(|e| format!("Connection check failed: {e}"))?
        {
            peripheral
                .connect()
                .await
                .map_err(|e| format!("BLE connect failed: {e}"))?;
        }

        peripheral
            .discover_services()
            .await
            .map_err(|e| format!("Service discovery failed: {e}"))?;

        let characteristic = peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == char_uuid)
            .ok_or("Clevetura characteristic not found")?;

        // Subscribe to notifications for reading responses
        peripheral
            .subscribe(&characteristic)
            .await
            .map_err(|e| format!("Failed to subscribe: {e}"))?;

        Ok(Self {
            peripheral,
            characteristic,
        })
    }

    /// Send raw bytes and receive raw response (Layer 1 — firmware commands).
    async fn send_raw(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        // Append end-byte and chunk
        let mut payload = data.to_vec();
        payload.push(END_BYTE);

        for chunk in payload.chunks(CHUNK_SIZE) {
            self.peripheral
                .write(&self.characteristic, chunk, WriteType::WithResponse)
                .await
                .map_err(|e| format!("BLE write failed: {e}"))?;
        }

        // Read response via notifications
        let mut stream = self
            .peripheral
            .notifications()
            .await
            .map_err(|e| format!("Failed to get notifications: {e}"))?;

        let mut response_data = Vec::new();

        loop {
            match timeout(READ_TIMEOUT, stream.next()).await {
                Ok(Some(notification)) => {
                    let data = notification.value;
                    if let Some(end_pos) = data.iter().position(|&b| b == END_BYTE) {
                        response_data.extend_from_slice(&data[..end_pos]);
                        break;
                    } else {
                        response_data.extend_from_slice(&data);
                    }
                }
                Ok(None) => break,
                Err(_) => return Err("BLE read timeout".to_string()),
            }
        }

        Ok(response_data)
    }

    /// Send a firmware command (Layer 1).
    async fn send_command(&self, command_id: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        let mut cmd = vec![command_id];
        cmd.extend_from_slice(payload);

        let response = self.send_raw(&cmd).await?;

        // First byte is status
        if response.is_empty() {
            return Err("Empty response".to_string());
        }
        if response[0] == 2 {
            return Err(format!("Device error for command 0x{:02x}", command_id));
        }

        Ok(response[1..].to_vec())
    }

    /// Send a protobuf request (Layer 2) with CRC and receive protobuf response.
    ///
    /// BLE uses the CRC variant:
    /// 1. Protobuf encode → raw bytes
    /// 2. CRC32 of raw bytes → 4 bytes little-endian
    /// 3. Concat: [raw_bytes, crc32_bytes]
    /// 4. Base64 encode
    /// 5. Prepend byte 0x23 (35)
    /// 6. Send via BLE write (chunked with end-byte)
    pub async fn send_proto_request(
        &self,
        request: &proto::Request,
    ) -> Result<proto::Response, String> {
        // 1. Encode protobuf
        let proto_bytes = request.encode_to_vec();

        // 2. CRC32 (little-endian)
        let crc = crc32fast::hash(&proto_bytes);
        let crc_bytes = crc.to_le_bytes();

        // 3. Concat proto + CRC
        let mut with_crc = proto_bytes.clone();
        with_crc.extend_from_slice(&crc_bytes);

        // 4. Base64 encode
        let b64 = base64::engine::general_purpose::STANDARD.encode(&with_crc);

        // 5. Prepend 0x23 and send
        let mut payload = vec![0x23u8];
        payload.extend_from_slice(b64.as_bytes());

        let response_data = self.send_raw(&payload).await?;

        if response_data.is_empty() {
            return Err("Empty protobuf response".to_string());
        }

        // Response parsing:
        // The response may have a leading 0x23 byte, then base64 data with CRC
        let data = if response_data[0] == 0x23 {
            &response_data[1..]
        } else {
            &response_data[..]
        };

        // Strip null bytes
        let clean: Vec<u8> = data.iter().copied().filter(|&b| b > 0 && b < 128).collect();

        let b64_str = std::str::from_utf8(&clean)
            .map_err(|e| format!("Invalid UTF-8: {e}"))?;

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64_str)
            .map_err(|e| format!("Base64 decode failed: {e}"))?;

        // Last 4 bytes are CRC32 — strip them if present (response is at least 5 bytes)
        let proto_data = if decoded.len() > 4 {
            // Verify CRC
            let payload_end = decoded.len() - 4;
            let response_crc = u32::from_le_bytes([
                decoded[payload_end],
                decoded[payload_end + 1],
                decoded[payload_end + 2],
                decoded[payload_end + 3],
            ]);
            let computed_crc = crc32fast::hash(&decoded[..payload_end]);
            if response_crc == computed_crc {
                &decoded[..payload_end]
            } else {
                // CRC mismatch — try decoding the full data (might not have CRC)
                &decoded[..]
            }
        } else {
            &decoded[..]
        };

        proto::Response::decode(proto_data)
            .map_err(|e| format!("Protobuf decode failed: {e}"))
    }

    /// Authorize the connection.
    pub async fn authorize(&self) -> Result<(), String> {
        self.send_command(cmd::AUTHORIZE, &AUTH_KEY).await?;
        Ok(())
    }

    /// Get battery level (Layer 1 firmware command).
    pub async fn get_battery_level(&self) -> Result<i8, String> {
        let payload = self.send_command(cmd::GET_BAT_LEVEL, &[]).await?;
        if payload.is_empty() {
            return Err("Empty battery response".to_string());
        }
        Ok(payload[0] as i8)
    }

    /// Get firmware version.
    pub async fn get_firmware_version(&self) -> Result<String, String> {
        let payload = self.send_command(cmd::GET_FW_VERSION, &[]).await?;
        if payload.len() < 3 {
            return Err("FW version response too short".to_string());
        }
        Ok(format!("{}.{}.{}", payload[0], payload[1], payload[2]))
    }

    /// Get protocol version.
    pub async fn get_protocol_version(&self) -> Result<String, String> {
        let payload = self.send_command(cmd::GET_PROTOCOL_VERSION, &[]).await?;
        if payload.len() < 3 {
            return Err("Protocol version response too short".to_string());
        }
        Ok(format!("{}.{}.{}", payload[0], payload[1], payload[2]))
    }

    /// Get serial number.
    pub async fn get_serial_number(&self) -> Result<String, String> {
        let payload = self.send_command(cmd::GET_SERIAL_NUMBER, &[]).await?;
        if payload.len() >= 4 {
            let serial = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            Ok(format!("{:012X}", serial))
        } else {
            Ok(payload.iter().map(|b| format!("{:02x}", b)).collect())
        }
    }

    /// Get settings via protobuf (Layer 2).
    pub async fn get_settings(&self) -> Result<proto::AppSettings, String> {
        let request = proto::Request {
            r#type: proto::RequestType::GetSettings as i32,
            get_settings: Some(proto::GetSettingsRequest {}),
            ..Default::default()
        };

        let response = self.send_proto_request(&request).await?;

        let get_settings = response
            .get_settings
            .ok_or("No getSettings in response")?;

        get_settings
            .settings
            .ok_or_else(|| "No settings in response".to_string())
    }

    /// Set settings via protobuf (Layer 2).
    pub async fn set_settings(&self, settings: proto::AppSettings) -> Result<(), String> {
        let request = proto::Request {
            r#type: proto::RequestType::SetSettings as i32,
            set_settings: Some(proto::SetSettingsRequest {
                settings: Some(settings),
            }),
            ..Default::default()
        };

        let response = self.send_proto_request(&request).await?;

        if let Some(bad) = &response.bad_request {
            return Err(format!("Bad request error: {}", bad.error));
        }

        Ok(())
    }

    /// Disconnect from the device.
    pub async fn disconnect(&self) -> Result<(), String> {
        self.peripheral
            .disconnect()
            .await
            .map_err(|e| format!("Disconnect failed: {e}"))
    }

    /// Send a heartbeat and get battery info via protobuf (Layer 2).
    pub async fn heartbeat(&self, active_profile: u32) -> Result<Option<proto::HeartBeatBattery>, String> {
        let request = proto::Request {
            r#type: proto::RequestType::Heartbeat as i32,
            heart_beat: Some(proto::HeartBeat { active_profile }),
            ..Default::default()
        };

        let response = self.send_proto_request(&request).await?;
        Ok(response.heart_beat.and_then(|hb| hb.battery))
    }

    /// Set AI sensitivity via protobuf (Layer 2).
    pub async fn set_sensitivity(&self, level: u32) -> Result<(), String> {
        let settings = proto::AppSettings {
            global: Some(proto::GlobalSettings {
                current_ai_level: Some(level),
                ..Default::default()
            }),
            global_profile: None,
            counter: None,
        };
        self.set_settings(settings).await
    }
}

/// Print device info over BLE using protobuf protocol.
pub async fn print_ble_info(address: &str) {
    println!("Connecting to {} over BLE...\n", address);

    match BleConnection::connect_by_address(address).await {
        Ok(conn) => {
            println!("Clevetura BLE Device Info");
            println!("=========================\n");

            // Over BLE, use protobuf (Layer 2) for everything
            // Layer 1 firmware commands are HID-specific

            // Get settings
            match conn.get_settings().await {
                Ok(settings) => {
                    if let Some(global) = &settings.global {
                        if let Some(level) = global.current_ai_level {
                            println!("AI Sensitivity:    {level}/9");
                        }
                        if let Some(v) = global.tap1f_enable {
                            println!("1-finger tap:      {}", if v { "ON" } else { "OFF" });
                        }
                        if let Some(v) = global.tap2f_enable {
                            println!("2-finger tap:      {}", if v { "ON" } else { "OFF" });
                        }
                        if let Some(v) = global.fn_lock {
                            println!("Fn Lock:           {}", if v { "ON" } else { "OFF" });
                        }
                    }
                }
                Err(e) => println!("Settings:          (error: {e})"),
            }

            // Heartbeat for battery
            match conn.heartbeat(0).await {
                Ok(Some(batt)) => {
                    let charge_str = if batt.charging { " (charging)" } else { "" };
                    println!("Battery:           {}%{}", batt.level, charge_str);
                }
                Ok(None) => println!("Battery:           (no data)"),
                Err(e) => println!("Battery:           (error: {e})"),
            }

            let _ = conn.disconnect().await;
            println!("\nDisconnected.");
        }
        Err(e) => {
            eprintln!("Failed to connect: {e}");
        }
    }
}
