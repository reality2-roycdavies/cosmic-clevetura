use cosmic::app::{Core, Task};
use cosmic::iced::widget::svg;
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Rectangle};
use cosmic::iced_runtime::core::window;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::{self, text};
use cosmic::Element;

use crate::config::Config;
use crate::keyboard::{self, KeyboardMode, KeyboardState};
use crate::proto;

const APP_ID: &str = "io.github.reality2_roycdavies.cosmic-clevetura";

enum KeyboardCommand {
    SetSensitivity(u32),
    SetGlobalSettings(proto::GlobalSettings),
}

#[derive(Debug)]
enum KeyboardEvent {
    StateUpdate {
        hw_state: KeyboardState,
        ai_level: Option<u32>,
        firmware_settings: Option<proto::GlobalSettings>,
        battery_from_heartbeat: Option<proto::HeartBeatBattery>,
        connection_type: &'static str,
    },
    SettingsResult(Result<(), String>),
}

#[derive(Debug, Clone)]
pub enum Message {
    PollStatus,
    SetSensitivity(u32),
    OpenSettings,
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
}

pub struct CleveturaApplet {
    core: Core,
    popup: Option<Id>,
    state: KeyboardState,
    config: Config,
    /// AI sensitivity level from firmware (None if not yet read).
    firmware_ai_level: Option<u32>,
    /// Battery info from heartbeat.
    heartbeat_battery: Option<proto::HeartBeatBattery>,
    /// Current connection type for display.
    connection_type: &'static str,
    cmd_tx: std::sync::mpsc::Sender<KeyboardCommand>,
    event_rx: std::sync::mpsc::Receiver<KeyboardEvent>,
}

impl cosmic::Application for CleveturaApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();

        let config = Config::load();
        let _ = config.save();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(run_background(cmd_rx, event_tx));
        });

        let applet = Self {
            core,
            popup: None,
            state: KeyboardState::default(),
            config,
            firmware_ai_level: None,
            heartbeat_battery: None,
            connection_type: "",
            cmd_tx,
            event_rx,
        };

        (applet, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::PollStatus => {
                while let Ok(event) = self.event_rx.try_recv() {
                    match event {
                        KeyboardEvent::StateUpdate {
                            hw_state,
                            ai_level,
                            firmware_settings,
                            battery_from_heartbeat,
                            connection_type,
                        } => {
                            self.state = hw_state;
                            self.connection_type = connection_type;
                            if let Some(level) = ai_level {
                                self.firmware_ai_level = Some(level);
                            }
                            if let Some(ref global) = firmware_settings {
                                self.config.update_from_firmware(global);
                            }
                            if let Some(batt) = battery_from_heartbeat {
                                self.heartbeat_battery = Some(batt);
                            }
                        }
                        KeyboardEvent::SettingsResult(result) => {
                            if let Err(e) = result {
                                eprintln!("Settings update failed: {e}");
                            }
                        }
                    }
                }
                // Reload config from disk (settings page may have changed it).
                let new_config = Config::load();
                // If firmware-relevant settings changed, push to keyboard.
                if self.state.connected && new_config.to_global_settings() != self.config.to_global_settings() {
                    let _ = self.cmd_tx.send(KeyboardCommand::SetGlobalSettings(
                        new_config.to_global_settings(),
                    ));
                }
                self.config = new_config;
            }

            Message::SetSensitivity(level) => {
                self.firmware_ai_level = Some(level);
                let _ = self.cmd_tx.send(KeyboardCommand::SetSensitivity(level));
            }

            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }

            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }

            Message::OpenSettings => {
                std::thread::spawn(|| {
                    let unified = std::process::Command::new("cosmic-applet-settings")
                        .arg(APP_ID)
                        .spawn();
                    if unified.is_err() {
                        let exe = std::env::current_exe()
                            .unwrap_or_else(|_| "cosmic-clevetura".into());
                        if let Err(e) = std::process::Command::new(exe)
                            .arg("--settings-standalone")
                            .spawn()
                        {
                            eprintln!("Failed to launch settings: {e}");
                        }
                    }
                });
            }
        }

        Task::none()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::PollStatus)
    }

    fn view(&self) -> Element<'_, Message> {
        let suggested = self.core.applet.suggested_size(true);
        let icon_size = suggested.0 as f32;

        let icon: Element<Message> = if self.state.connected {
            let theme = cosmic::theme::active();
            let cosmic_theme = theme.cosmic();
            let fg = cosmic_theme.background.on;
            let color = format!(
                "rgb({},{},{})",
                (fg.red * 255.0) as u8,
                (fg.green * 255.0) as u8,
                (fg.blue * 255.0) as u8,
            );

            let svg_data = keyboard_icon_svg(&color, &self.state.mode);
            let handle = svg::Handle::from_memory(svg_data.into_bytes());
            cosmic::iced::widget::svg(handle)
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size))
                .into()
        } else {
            widget::icon::from_name(
                "io.github.reality2_roycdavies.cosmic-clevetura-disconnected-symbolic",
            )
            .symbolic(true)
            .into()
        };

        let have_popup = self.popup;
        let btn = self
            .core
            .applet
            .button_from_element(icon, true)
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<CleveturaApplet>(
                        move |state: &mut CleveturaApplet| {
                            let new_id = Id::unique();
                            state.popup = Some(new_id);

                            let popup_width = 300u32;
                            let popup_height = 380u32;

                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                Some((popup_width, popup_height)),
                                None,
                                None,
                            );
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            popup_settings
                        },
                        Some(Box::new(|state: &CleveturaApplet| {
                            Element::from(
                                state.core.applet.popup_container(state.popup_content()),
                            )
                            .map(cosmic::Action::App)
                        })),
                    ))
                }
            });

        let tooltip: &str = if self.state.connected {
            match self.state.mode {
                KeyboardMode::Typing => "Clevetura (Typing)",
                KeyboardMode::Touch => "Clevetura (Touch)",
                KeyboardMode::Unknown => "Clevetura (Connected)",
            }
        } else {
            "Clevetura (Disconnected)"
        };

        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            tooltip,
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        "".into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl CleveturaApplet {
    /// Current AI sensitivity level (prefer firmware value over config).
    fn current_sensitivity(&self) -> u32 {
        self.firmware_ai_level
            .unwrap_or(self.config.sensitivity as u32)
    }

    /// Battery percentage from the best available source.
    fn battery_percent(&self) -> Option<i32> {
        // Prefer heartbeat battery (more current), fall back to firmware query
        if let Some(ref hb) = self.heartbeat_battery {
            Some(hb.level)
        } else {
            self.state.battery_percent.map(|b| b as i32)
        }
    }

    fn popup_content(&self) -> widget::Column<'_, Message> {
        use cosmic::iced::widget::{column, container, row, Space};
        use cosmic::iced::{Alignment, Color};

        let title_row = row![
            text::body("Clevetura TouchOnKeys"),
            Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // Connection & device info
        let info_section = if self.state.connected {
            let mut info = column![].spacing(2);

            // Connection type
            if !self.connection_type.is_empty() {
                info = info.push(text::body(format!("Connected via {}", self.connection_type)));
            }

            // Battery
            if let Some(batt) = self.battery_percent() {
                let charging = self
                    .heartbeat_battery
                    .as_ref()
                    .map(|hb| hb.charging)
                    .unwrap_or(false);
                if charging {
                    info = info.push(text::body(format!("Battery: {}% (charging)", batt)));
                } else if batt >= 0 {
                    info = info.push(text::body(format!("Battery: {}%", batt)));
                }
            }

            // Firmware
            if let Some(ref fw) = self.state.firmware_version {
                info = info.push(text::caption(format!("Firmware: {fw}")));
            }

            info
        } else {
            column![
                text::body("Disconnected"),
                text::caption("Connect your Clevetura keyboard via USB or Bluetooth"),
            ]
            .spacing(2)
        };

        // Sensitivity control (reads from firmware)
        let ai_level = self.current_sensitivity();
        let sensitivity_label = format!("Touch Sensitivity: {}/9", ai_level);
        let mut sensitivity_row =
            row![text::body(sensitivity_label), Space::new().width(Length::Fill),]
                .spacing(4)
                .align_y(Alignment::Center);

        let can_decrease = ai_level > 1 && self.state.connected;
        let minus_btn: Element<Message> = if can_decrease {
            widget::button::standard("-")
                .on_press(Message::SetSensitivity(ai_level - 1))
                .into()
        } else {
            widget::button::standard("-").into()
        };

        let can_increase = ai_level < 9 && self.state.connected;
        let plus_btn: Element<Message> = if can_increase {
            widget::button::standard("+")
                .on_press(Message::SetSensitivity(ai_level + 1))
                .into()
        } else {
            widget::button::standard("+").into()
        };

        sensitivity_row = sensitivity_row.push(minus_btn).push(plus_btn);

        // Settings button
        let settings_row = row![
            Space::new().width(Length::Fill),
            widget::button::standard("Settings...").on_press(Message::OpenSettings),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let divider = || {
            container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(
                |theme: &cosmic::Theme| {
                    let cosmic = theme.cosmic();
                    container::Style {
                        background: Some(cosmic::iced::Background::Color(Color::from(
                            cosmic.palette.neutral_5,
                        ))),
                        ..Default::default()
                    }
                },
            )
        };

        column![
            title_row,
            divider(),
            info_section,
            divider(),
            sensitivity_row,
            divider(),
            settings_row,
        ]
        .spacing(8)
        .padding(12)
    }
}

/// Connection type tracking for the background thread.
enum ConnectionMode {
    None,
    Usb { authorized: bool },
    Ble { address: String },
}

/// Send settings to the keyboard via whichever transport is active.
async fn send_settings(
    mode: &ConnectionMode,
    ble_conn: &Option<crate::ble::BleConnection>,
    settings: proto::AppSettings,
) -> Result<(), String> {
    match mode {
        ConnectionMode::Usb { authorized: true } => {
            match keyboard::KeyboardConnection::open() {
                Ok(conn) => proto::set_settings(conn.device(), settings),
                Err(e) => Err(e),
            }
        }
        ConnectionMode::Ble { .. } => {
            if let Some(ref conn) = ble_conn {
                conn.set_settings(settings).await
            } else {
                Err("BLE not connected".to_string())
            }
        }
        _ => Err("Not connected".to_string()),
    }
}

async fn run_background(
    cmd_rx: std::sync::mpsc::Receiver<KeyboardCommand>,
    event_tx: std::sync::mpsc::Sender<KeyboardEvent>,
) {
    let mut mode = ConnectionMode::None;
    let mut ble_conn: Option<crate::ble::BleConnection> = None;

    loop {
        // Process commands from UI
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                KeyboardCommand::SetSensitivity(level) => {
                    let settings = proto::AppSettings {
                        global: Some(proto::GlobalSettings {
                            current_ai_level: Some(level),
                            ..Default::default()
                        }),
                        global_profile: None,
                        counter: None,
                    };

                    let result = send_settings(&mode, &ble_conn, settings).await;
                    let _ = event_tx.send(KeyboardEvent::SettingsResult(result));
                }
                KeyboardCommand::SetGlobalSettings(global) => {
                    let settings = proto::AppSettings {
                        global: Some(global),
                        global_profile: None,
                        counter: None,
                    };

                    let result = send_settings(&mode, &ble_conn, settings).await;
                    let _ = event_tx.send(KeyboardEvent::SettingsResult(result));
                }
            }
        }

        // Poll keyboard state — try USB first, then BLE
        let mut ai_level = None;
        let mut battery_hb = None;
        let mut fw_settings = None;

        let hw_state = match keyboard::KeyboardConnection::open() {
            Ok(conn) => {
                // USB connected
                let was_connected = matches!(mode, ConnectionMode::Usb { .. });
                let authorized = matches!(mode, ConnectionMode::Usb { authorized: true });
                if !authorized {
                    if conn.authorize().is_ok() {
                        mode = ConnectionMode::Usb { authorized: true };

                        // On first connection:
                        // 1. Disable AI touch processing → standard HID touchpad
                        // 2. Re-apply current settings to re-initialize the touch
                        //    controller (triggers firmware to emit proper multitouch)
                        if !was_connected {
                            let request = proto::Request {
                                r#type: proto::RequestType::ControlAi as i32,
                                control_ai: Some(proto::ControlAiRequest { mode: 0 }),
                                ..Default::default()
                            };
                            let _ = proto::send_proto_request(conn.device(), &request);

                            // Read current settings and write them back — this
                            // re-initializes the touch controller for full multitouch.
                            if let Ok(current) = proto::get_settings(conn.device()) {
                                let _ = proto::set_settings(conn.device(), current);
                            }
                        }
                    } else {
                        mode = ConnectionMode::Usb { authorized: false };
                    }
                }

                let state = conn.read_state();

                if matches!(mode, ConnectionMode::Usb { authorized: true }) {
                    if let Ok(settings) = proto::get_settings(conn.device()) {
                        ai_level = settings
                            .global
                            .as_ref()
                            .and_then(|g| g.current_ai_level);
                        fw_settings = settings.global.clone();
                    }
                    if let Ok(batt) = proto::heartbeat(conn.device(), 0) {
                        battery_hb = batt;
                    }
                }

                // Drop any BLE connection when USB is available
                if ble_conn.is_some() {
                    if let Some(ref conn) = ble_conn {
                        let _ = conn.disconnect().await;
                    }
                    ble_conn = None;
                }

                state
            }
            Err(_) => {
                // No USB — try BLE only if we have a saved BLE address
                // (BLE scanning is done manually via settings, not automatically)
                if ble_conn.is_none() {
                    let config = crate::config::Config::load();
                    if let Some(ref addr) = config.ble_address {
                        if let Ok(conn) =
                            crate::ble::BleConnection::connect_by_address(addr).await
                        {
                            mode = ConnectionMode::Ble {
                                address: addr.clone(),
                            };
                            ble_conn = Some(conn);
                        }
                    }
                }

                // Try reading from BLE connection
                if let Some(ref conn) = ble_conn {
                    match conn.get_settings().await {
                        Ok(settings) => {
                            ai_level = settings
                                .global
                                .as_ref()
                                .and_then(|g| g.current_ai_level);
                            fw_settings = settings.global.clone();

                            // Try heartbeat for battery
                            if let Ok(batt) = conn.heartbeat(0).await {
                                battery_hb = batt;
                            }

                            KeyboardState {
                                connected: true,
                                battery_percent: None,
                                firmware_version: None,
                                protocol_version: None,
                                serial_number: None,
                                mode: KeyboardMode::Unknown,
                            }
                        }
                        Err(_) => {
                            // BLE connection lost
                            let _ = conn.disconnect().await;
                            ble_conn = None;
                            mode = ConnectionMode::None;
                            KeyboardState::default()
                        }
                    }
                } else {
                    mode = ConnectionMode::None;
                    KeyboardState::default()
                }
            }
        };

        let conn_type = match &mode {
            ConnectionMode::Usb { .. } if hw_state.connected => "USB",
            ConnectionMode::Ble { .. } if hw_state.connected => "Bluetooth",
            _ => "",
        };

        let _ = event_tx.send(KeyboardEvent::StateUpdate {
            hw_state,
            ai_level,
            firmware_settings: fw_settings,
            battery_from_heartbeat: battery_hb,
            connection_type: conn_type,
        });

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

/// Generate a TouchOnKeys shield SVG icon with theme color.
fn keyboard_icon_svg(color: &str, mode: &KeyboardMode) -> String {
    let wave2 = match mode {
        KeyboardMode::Touch => format!(
            r#"<path d="M4 10.5C5.2 9 6.2 8.2 8 8.2C9.8 8.2 10.8 9 12 10.5" fill="none" stroke="{color}" stroke-width="0.8" stroke-linecap="round" opacity="0.6"/>"#
        ),
        _ => String::new(),
    };

    let dot_opacity = match mode {
        KeyboardMode::Unknown => "0.5",
        _ => "1",
    };

    format!(
        r#"<svg width="16" height="16" viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path d="M8 1.5L2.5 4V9C2.5 11.5 4.5 13.5 8 14.5C11.5 13.5 13.5 11.5 13.5 9V4L8 1.5Z" fill="none" stroke="{color}" stroke-width="1.2" stroke-linejoin="round"/>
  <path d="M5 9C5.8 8 6.5 7.5 8 7.5C9.5 7.5 10.2 8 11 9" fill="none" stroke="{color}" stroke-width="1" stroke-linecap="round"/>
  {wave2}
  <circle cx="8" cy="5.5" r="1" fill="{color}" opacity="{dot_opacity}"/>
</svg>"#
    )
}

pub fn run_applet() -> cosmic::iced::Result {
    cosmic::applet::run::<CleveturaApplet>(())
}
