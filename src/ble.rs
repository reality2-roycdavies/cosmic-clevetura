//! Bluetooth Low Energy communication with Clevetura keyboards.
//!
//! Uses the same command protocol as USB HID, but over BLE GATT.
//! Service UUID: d0bf1500-c402-424a-80b0-bc7aeced077e
//! Characteristic UUID: d0bf0001-c402-424a-80b0-bc7aeced077e

use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

/// Clevetura BLE service UUID.
const SERVICE_UUID: &str = "d0bf1500-c402-424a-80b0-bc7aeced077e";
/// Clevetura BLE characteristic UUID for data exchange.
const CHAR_UUID: &str = "d0bf0001-c402-424a-80b0-bc7aeced077e";
/// End-of-message byte (same as USB protocol).
const END_BYTE: u8 = 0x0A;
/// BLE chunk size (from reverse engineering).
const CHUNK_SIZE: usize = 56;

/// Authorization key (same as USB).
const AUTH_KEY: [u8; 8] = [0x96, 0x25, 0xa6, 0xd9, 0xfb, 0x64, 0xcd, 0xea];

/// Command IDs (same as USB protocol).
mod cmd {
    pub const AUTHORIZE: u8 = 121;
    pub const GET_FW_VERSION: u8 = 131;
    pub const GET_BAT_LEVEL: u8 = 153;
    pub const GET_SERIAL_NUMBER: u8 = 145;
}

/// BLE connection to a Clevetura keyboard.
pub struct BleConnection {
    peripheral: Peripheral,
    characteristic: Characteristic,
}

/// Information about a discovered BLE Clevetura device.
#[derive(Debug, Clone)]
pub struct BleDeviceInfo {
    pub name: String,
    pub address: String,
}

/// Scan for Clevetura keyboards over BLE.
pub async fn scan_devices(scan_duration: Duration) -> Result<Vec<BleDeviceInfo>, String> {
    let manager = Manager::new()
        .await
        .map_err(|e| format!("BLE manager init failed: {e}"))?;

    let adapters = manager
        .adapters()
        .await
        .map_err(|e| format!("Failed to get BLE adapters: {e}"))?;

    let adapter = adapters
        .into_iter()
        .next()
        .ok_or("No BLE adapters found")?;

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
    /// Connect to a Clevetura keyboard over BLE.
    pub async fn connect(adapter: &Adapter, address: &str) -> Result<Self, String> {
        let service_uuid =
            Uuid::parse_str(SERVICE_UUID).map_err(|e| format!("Invalid UUID: {e}"))?;
        let char_uuid =
            Uuid::parse_str(CHAR_UUID).map_err(|e| format!("Invalid UUID: {e}"))?;

        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| format!("Failed to list peripherals: {e}"))?;

        let peripheral = peripherals
            .into_iter()
            .find(|p| {
                futures::executor::block_on(async {
                    p.properties()
                        .await
                        .ok()
                        .flatten()
                        .map(|props| props.address.to_string() == address)
                        .unwrap_or(false)
                })
            })
            .ok_or(format!("Device not found: {address}"))?;

        peripheral
            .connect()
            .await
            .map_err(|e| format!("BLE connect failed: {e}"))?;

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

    /// Send a command and receive the response.
    async fn send_command(&self, command_id: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        // Build message: [COMMAND_ID, ...payload, END_BYTE]
        let mut message = vec![command_id];
        message.extend_from_slice(payload);
        message.push(END_BYTE);

        // Chunk and send
        for chunk in message.chunks(CHUNK_SIZE) {
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
        let read_timeout = Duration::from_secs(2);

        loop {
            match timeout(read_timeout, stream.next()).await {
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

        // First byte is status
        if response_data.is_empty() {
            return Err("Empty BLE response".to_string());
        }

        if response_data[0] == 2 {
            return Err(format!("BLE device error for command 0x{:02x}", command_id));
        }

        Ok(response_data[1..].to_vec())
    }

    /// Authorize the BLE connection.
    pub async fn authorize(&self) -> Result<(), String> {
        self.send_command(cmd::AUTHORIZE, &AUTH_KEY).await?;
        Ok(())
    }

    /// Get battery level.
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

    /// Disconnect from the device.
    pub async fn disconnect(&self) -> Result<(), String> {
        self.peripheral
            .disconnect()
            .await
            .map_err(|e| format!("Disconnect failed: {e}"))
    }
}
