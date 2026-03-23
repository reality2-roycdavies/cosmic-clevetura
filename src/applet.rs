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

const APP_ID: &str = "io.github.reality2_roycdavies.cosmic-clevetura";

enum KeyboardCommand {
    Refresh,
}

#[derive(Debug)]
enum KeyboardEvent {
    StateUpdate(KeyboardState),
}

#[derive(Debug, Clone)]
pub enum Message {
    PollStatus,
    SetSensitivity(u8),
    OpenSettings,
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
}

pub struct CleveturaApplet {
    core: Core,
    popup: Option<Id>,
    state: KeyboardState,
    config: Config,
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
                        KeyboardEvent::StateUpdate(new_state) => {
                            self.state = new_state;
                        }
                    }
                }
                self.config = Config::load();
            }

            Message::SetSensitivity(level) => {
                // Sensitivity is software-side only — save to config
                self.config.sensitivity = level;
                let _ = self.config.save();
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

            let svg_data = keyboard_icon_svg(&color, true, &self.state.mode);
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

            // Battery
            if let Some(batt) = self.state.battery_percent {
                if batt >= 0 {
                    info = info.push(text::body(format!("Battery: {}%", batt)));
                } else {
                    info = info.push(text::body("Battery: Charging"));
                }
            }

            // Firmware
            if let Some(ref fw) = self.state.firmware_version {
                info = info.push(text::caption(format!("Firmware: {fw}")));
            }

            // Serial
            if let Some(ref serial) = self.state.serial_number {
                info = info.push(text::caption(format!("Serial: {serial}")));
            }

            info
        } else {
            column![text::body("Disconnected"),
                text::caption("Connect your Clevetura keyboard via USB or Bluetooth"),
            ]
            .spacing(2)
        };

        // Sensitivity control (software-side)
        let sensitivity_label = format!("Touch Sensitivity: {}/9", self.config.sensitivity);
        let mut sensitivity_row =
            row![text::body(sensitivity_label), Space::new().width(Length::Fill),]
                .spacing(4)
                .align_y(Alignment::Center);

        let can_decrease = self.config.sensitivity > 1;
        let minus_btn: Element<Message> = if can_decrease {
            widget::button::standard("-")
                .on_press(Message::SetSensitivity(self.config.sensitivity - 1))
                .into()
        } else {
            widget::button::standard("-").into()
        };

        let can_increase = self.config.sensitivity < 9;
        let plus_btn: Element<Message> = if can_increase {
            widget::button::standard("+")
                .on_press(Message::SetSensitivity(self.config.sensitivity + 1))
                .into()
        } else {
            widget::button::standard("+").into()
        };

        sensitivity_row = sensitivity_row.push(minus_btn).push(plus_btn);

        // Slider config info
        let left_slider_text = format!("Left slider: {}", self.config.left_slider);
        let right_slider_text = format!("Right slider: {}", self.config.right_slider);
        let slider_section =
            column![text::caption(left_slider_text), text::caption(right_slider_text),].spacing(2);

        // Profiles status
        let profiles_text = if self.config.profiles_enabled {
            format!("Profiles: {} configured", self.config.profiles.len())
        } else {
            "Profiles: Disabled".to_string()
        };

        // Settings button
        let settings_row = row![
            text::caption(profiles_text),
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
            slider_section,
            divider(),
            settings_row,
        ]
        .spacing(8)
        .padding(12)
    }
}

async fn run_background(
    cmd_rx: std::sync::mpsc::Receiver<KeyboardCommand>,
    event_tx: std::sync::mpsc::Sender<KeyboardEvent>,
) {
    loop {
        // Process commands from UI
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                KeyboardCommand::Refresh => {}
            }
        }

        // Poll keyboard state
        let state = keyboard::poll_state();
        let _ = event_tx.send(KeyboardEvent::StateUpdate(state));

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

/// Generate a TouchOnKeys shield SVG icon with theme color.
fn keyboard_icon_svg(color: &str, _connected: bool, mode: &KeyboardMode) -> String {
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
