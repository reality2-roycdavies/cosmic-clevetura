mod applet;
mod ble;
mod config;
mod hid;
mod keyboard;
mod profiles;
mod proto;
mod settings;
mod settings_cli;
mod settings_page;
mod slider_actions;

const APPLET_ID: &str = "io.github.reality2_roycdavies.cosmic-clevetura";

fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--settings" | "-s" => open_settings(),
            "--settings-standalone" => settings::run_settings(),
            "--detect" => {
                hid::print_detection_report();
                Ok(())
            }
            "--info" => {
                keyboard::print_device_info();
                Ok(())
            }
            "--probe" => {
                keyboard::probe_reports();
                Ok(())
            }
            "--watch" => {
                keyboard::watch_reports();
                Ok(())
            }
            "--get-settings" => {
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        proto::print_settings(conn.device());
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--set-sensitivity" => {
                if args.len() < 3 {
                    eprintln!("Usage: cosmic-clevetura --set-sensitivity <1-9>");
                    std::process::exit(1);
                }
                let level: u32 = args[2].parse().unwrap_or(0);
                if !(1..=9).contains(&level) {
                    eprintln!("Sensitivity must be 1-9");
                    std::process::exit(1);
                }
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        // Send only the global settings with changed field
                        // (matching how the TouchOnKeys app does it)
                        let settings = proto::AppSettings {
                            global: Some(proto::GlobalSettings {
                                current_ai_level: Some(level),
                                ..Default::default()
                            }),
                            global_profile: None,
                            counter: None,
                        };
                        match proto::set_settings(conn.device(), settings) {
                            Ok(()) => println!("Sensitivity set to {level}"),
                            Err(e) => eprintln!("Failed to set sensitivity: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--ble-scan" => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    println!("Scanning for Clevetura BLE devices (5 seconds)...\n");
                    match ble::scan_devices(std::time::Duration::from_secs(5)).await {
                        Ok(devices) if devices.is_empty() => {
                            println!("No Clevetura BLE devices found.");
                        }
                        Ok(devices) => {
                            println!("Found {} device(s):\n", devices.len());
                            for dev in &devices {
                                println!("  {} ({})", dev.name, dev.address);
                            }
                        }
                        Err(e) => eprintln!("BLE scan error: {e}"),
                    }
                });
                Ok(())
            }
            "--ble-info" => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let address = if args.len() >= 3 {
                        args[2].clone()
                    } else {
                        println!("Scanning for Clevetura BLE devices (3 seconds)...");
                        match ble::scan_devices(std::time::Duration::from_secs(3)).await {
                            Ok(devices) if !devices.is_empty() => {
                                devices[0].address.clone()
                            }
                            _ => {
                                eprintln!("No Clevetura BLE devices found. Use --ble-info <address>");
                                std::process::exit(1);
                            }
                        }
                    };
                    ble::print_ble_info(&address).await;
                });
                Ok(())
            }
            "--help" | "-h" => {
                print_help(&args[0]);
                Ok(())
            }
            "--version" | "-v" => {
                println!("cosmic-clevetura {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
            "--ai-off" => {
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        let request = proto::Request {
                            r#type: proto::RequestType::ControlAi as i32,
                            control_ai: Some(proto::ControlAiRequest { mode: 0 }),
                            ..Default::default()
                        };
                        match proto::send_proto_request(conn.device(), &request) {
                            Ok(resp) => println!("AI off response: type={}, bad_request={:?}", resp.r#type, resp.bad_request),
                            Err(e) => eprintln!("Failed: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--ai-on" => {
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        let request = proto::Request {
                            r#type: proto::RequestType::ControlAi as i32,
                            control_ai: Some(proto::ControlAiRequest { mode: 1 }),
                            ..Default::default()
                        };
                        match proto::send_proto_request(conn.device(), &request) {
                            Ok(resp) => println!("AI on response: type={}, bad_request={:?}", resp.r#type, resp.bad_request),
                            Err(e) => eprintln!("Failed: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--ai-state" => {
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        let request = proto::Request {
                            r#type: proto::RequestType::GetAiState as i32,
                            get_ai_state: Some(proto::GetAiStateRequest {}),
                            ..Default::default()
                        };
                        match proto::send_proto_request(conn.device(), &request) {
                            Ok(resp) => {
                                println!("AI state response: type={}", resp.r#type);
                                if let Some(ai) = resp.get_ai_state {
                                    println!("  mode: {:?}, active: {:?}", ai.mode, ai.active);
                                }
                                if let Some(bad) = resp.bad_request {
                                    println!("  bad_request: {}", bad.error);
                                }
                            }
                            Err(e) => eprintln!("Failed: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--factory-reset" => {
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        let request = proto::Request {
                            r#type: proto::RequestType::PerformFullReset as i32,
                            ..Default::default()
                        };
                        match proto::send_proto_request(conn.device(), &request) {
                            Ok(resp) => println!("Reset response: type={}", resp.r#type),
                            Err(e) => eprintln!("Failed: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--set-os-mode" => {
                let mode: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(-1);
                if !(0..=2).contains(&mode) {
                    eprintln!("Usage: cosmic-clevetura --set-os-mode <0=win|1=mac|2=linux>");
                    std::process::exit(1);
                }
                match keyboard::KeyboardConnection::open() {
                    Ok(conn) => {
                        conn.authorize().ok();
                        match proto::set_os_mode(conn.device(), mode) {
                            Ok(()) => println!("OS mode set to {}", ["Windows", "macOS", "Linux"][mode as usize]),
                            Err(e) => eprintln!("Failed: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Could not open keyboard: {e}"),
                }
                Ok(())
            }
            "--settings-describe" => {
                settings_cli::describe();
                Ok(())
            }
            "--settings-set" => {
                if args.len() < 4 {
                    eprintln!("Usage: cosmic-clevetura --settings-set <key> <json_value>");
                    std::process::exit(1);
                }
                settings_cli::set(&args[2], &args[3]);
                Ok(())
            }
            "--settings-action" => {
                if args.len() < 3 {
                    eprintln!("Usage: cosmic-clevetura --settings-action <action_id>");
                    std::process::exit(1);
                }
                settings_cli::action(&args[2]);
                Ok(())
            }
            _ => {
                eprintln!("Unknown argument: {}", args[1]);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
    } else {
        applet::run_applet()
    }
}

/// Try to open settings via cosmic-applet-settings hub; fall back to standalone.
fn open_settings() -> cosmic::iced::Result {
    use std::process::Command;
    if Command::new("cosmic-applet-settings")
        .arg(APPLET_ID)
        .spawn()
        .is_ok()
    {
        Ok(())
    } else {
        settings::run_settings()
    }
}

fn print_help(program: &str) {
    println!("Clevetura TouchOnKeys for COSMIC Desktop\n");
    println!("Usage: {} [OPTIONS]\n", program);
    println!("Options:");
    println!("  (none)             Run as COSMIC panel applet");
    println!("  --settings, -s     Open settings (via hub or standalone)");
    println!("  --settings-standalone  Open standalone settings window");
    println!("  --detect           Detect connected Clevetura keyboards");
    println!("  --probe            Probe HID feature reports (development)");
    println!("  --watch            Watch report 0xDB for changes (development)");
    println!("  --get-settings     Read current settings from keyboard firmware");
    println!("  --set-sensitivity <1-9>  Set AI touch sensitivity level");
    println!("  --ble-scan         Scan for Clevetura BLE devices");
    println!("  --ble-info [addr]  Read device info over BLE");
    println!("  --version, -v      Show version information");
    println!("  --help, -h         Show this help message");
    println!();
    println!("Configuration: ~/.config/cosmic-clevetura/config.json");
}
