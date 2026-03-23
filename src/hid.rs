//! HID device discovery and communication for Clevetura keyboards.

use hidapi::HidApi;

/// Clevetura USB Vendor ID.
pub const VENDOR_ID: u16 = 0x36f7;

/// Known Clevetura Product IDs.
pub const CLVX_S_PID: u16 = 0x5755;

/// All known product IDs.
pub const KNOWN_PIDS: &[(u16, &str)] = &[(CLVX_S_PID, "CLVX S")];

/// Information about a connected Clevetura device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub product_name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial: String,
    pub interface_number: i32,
    pub path: String,
    pub usage_page: u16,
    pub usage: u16,
}

/// Enumerate all connected Clevetura HID devices.
pub fn enumerate_devices() -> Result<Vec<DeviceInfo>, String> {
    let api = HidApi::new().map_err(|e| format!("Failed to initialize HID API: {e}"))?;

    let mut devices = Vec::new();

    for device in api.device_list() {
        if device.vendor_id() == VENDOR_ID {
            let product_name = KNOWN_PIDS
                .iter()
                .find(|(pid, _)| *pid == device.product_id())
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| {
                    device
                        .product_string()
                        .unwrap_or("Unknown Clevetura")
                        .to_string()
                });

            devices.push(DeviceInfo {
                product_name,
                vendor_id: device.vendor_id(),
                product_id: device.product_id(),
                serial: device
                    .serial_number()
                    .unwrap_or("(unknown)")
                    .to_string(),
                interface_number: device.interface_number(),
                path: device.path().to_string_lossy().to_string(),
                usage_page: device.usage_page(),
                usage: device.usage(),
            });
        }
    }

    Ok(devices)
}

/// Check if any Clevetura keyboard is connected.
pub fn is_connected() -> bool {
    enumerate_devices()
        .map(|devs| !devs.is_empty())
        .unwrap_or(false)
}

/// Print a detection report for all connected Clevetura devices.
pub fn print_detection_report() {
    println!("Clevetura Device Detection Report");
    println!("==================================\n");

    match enumerate_devices() {
        Ok(devices) if devices.is_empty() => {
            println!("No Clevetura devices found.");
            println!();
            println!("Troubleshooting:");
            println!("  1. Ensure the keyboard is connected via USB");
            println!("  2. Check that udev rules are installed:");
            println!("     sudo cp resources/99-clevetura.rules /etc/udev/rules.d/");
            println!("     sudo udevadm control --reload-rules");
            println!("     sudo udevadm trigger");
            println!("  3. Reconnect the keyboard");
        }
        Ok(devices) => {
            let unique: std::collections::HashSet<u16> =
                devices.iter().map(|d| d.product_id).collect();
            println!(
                "Found {} device(s) with {} HID interface(s):\n",
                unique.len(),
                devices.len()
            );

            for dev in &devices {
                println!("  {} (VID: {:04x}, PID: {:04x})", dev.product_name, dev.vendor_id, dev.product_id);
                println!("    Serial:    {}", dev.serial);
                println!("    Interface: {}", dev.interface_number);
                println!("    Path:      {}", dev.path);
                println!(
                    "    Usage:     page=0x{:04x} usage=0x{:04x}",
                    dev.usage_page, dev.usage
                );
                println!();
            }
        }
        Err(e) => {
            eprintln!("Error enumerating HID devices: {e}");
        }
    }
}
