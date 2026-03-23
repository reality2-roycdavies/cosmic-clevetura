//! Embeddable settings page for cosmic-clevetura.
//!
//! Provides the settings UI as standalone State/Message/init/update/view
//! functions that can be embedded in cosmic-applet-settings or wrapped
//! in a standalone Application window.

use cosmic::iced::Length;
use cosmic::widget::{self, button, settings, text, text_input};
use cosmic::Element;

use crate::config::{AppProfile, Config, SliderAction};

const SLIDER_ACTION_LABELS: &[&str] = &[
    "Backlight Brightness",
    "System Volume",
    "Media Scrub",
    "Zoom Level",
    "Scroll Speed",
];

fn slider_action_from_index(idx: usize) -> SliderAction {
    match idx {
        0 => SliderAction::Brightness,
        1 => SliderAction::Volume,
        2 => SliderAction::MediaScrub,
        3 => SliderAction::ZoomLevel,
        4 => SliderAction::ScrollSpeed,
        _ => SliderAction::Brightness,
    }
}

fn slider_action_to_index(action: &SliderAction) -> usize {
    match action {
        SliderAction::Brightness => 0,
        SliderAction::Volume => 1,
        SliderAction::MediaScrub => 2,
        SliderAction::ZoomLevel => 3,
        SliderAction::ScrollSpeed => 4,
        SliderAction::Custom(_) => 0,
    }
}

pub struct State {
    pub config: Config,
    pub status_message: String,
    pub left_slider_idx: usize,
    pub right_slider_idx: usize,
    pub new_profile_name: String,
    pub new_profile_app_id: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    SensitivityChanged(u8),
    LeftSliderSelected(usize),
    RightSliderSelected(usize),
    ProfilesEnabledToggled(bool),
    NewProfileNameChanged(String),
    NewProfileAppIdChanged(String),
    AddProfile,
    RemoveProfile(usize),
    Save,
    ResetDefaults,
}

pub fn init() -> State {
    let config = Config::load();
    let left_slider_idx = slider_action_to_index(&config.left_slider);
    let right_slider_idx = slider_action_to_index(&config.right_slider);

    State {
        config,
        status_message: String::new(),
        left_slider_idx,
        right_slider_idx,
        new_profile_name: String::new(),
        new_profile_app_id: String::new(),
    }
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::SensitivityChanged(level) => {
            if (1..=9).contains(&level) {
                state.config.sensitivity = level;
                state.status_message = "Unsaved changes".to_string();
            }
        }
        Message::LeftSliderSelected(idx) => {
            state.left_slider_idx = idx;
            state.config.left_slider = slider_action_from_index(idx);
            state.status_message = "Unsaved changes".to_string();
        }
        Message::RightSliderSelected(idx) => {
            state.right_slider_idx = idx;
            state.config.right_slider = slider_action_from_index(idx);
            state.status_message = "Unsaved changes".to_string();
        }
        Message::ProfilesEnabledToggled(enabled) => {
            state.config.profiles_enabled = enabled;
            state.status_message = "Unsaved changes".to_string();
        }
        Message::NewProfileNameChanged(name) => {
            state.new_profile_name = name;
        }
        Message::NewProfileAppIdChanged(app_id) => {
            state.new_profile_app_id = app_id;
        }
        Message::AddProfile => {
            if !state.new_profile_name.is_empty() && !state.new_profile_app_id.is_empty() {
                state.config.profiles.push(AppProfile {
                    name: state.new_profile_name.clone(),
                    app_id: state.new_profile_app_id.clone(),
                    sensitivity: state.config.sensitivity,
                    left_slider: state.config.left_slider.clone(),
                    right_slider: state.config.right_slider.clone(),
                });
                state.new_profile_name.clear();
                state.new_profile_app_id.clear();
                state.status_message = "Unsaved changes".to_string();
            }
        }
        Message::RemoveProfile(idx) => {
            if idx < state.config.profiles.len() {
                state.config.profiles.remove(idx);
                state.status_message = "Unsaved changes".to_string();
            }
        }
        Message::Save => match state.config.save() {
            Ok(()) => state.status_message = "Settings saved".to_string(),
            Err(e) => state.status_message = format!("Error: {e}"),
        },
        Message::ResetDefaults => {
            state.config = Config::default();
            state.left_slider_idx = slider_action_to_index(&state.config.left_slider);
            state.right_slider_idx = slider_action_to_index(&state.config.right_slider);
            match state.config.save() {
                Ok(()) => state.status_message = "Reset to defaults and saved".to_string(),
                Err(e) => state.status_message = format!("Error: {e}"),
            }
        }
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    let page_title = text::title1("Clevetura TouchOnKeys Settings");

    // Sensitivity section
    let sensitivity_text = format!("Level {} of 9", state.config.sensitivity);
    let mut sensitivity_buttons = cosmic::iced::widget::row![].spacing(4);
    for level in 1u8..=9 {
        let btn: Element<'_, Message> = if level == state.config.sensitivity {
            button::suggested(format!("{level}"))
                .on_press(Message::SensitivityChanged(level))
                .into()
        } else {
            button::standard(format!("{level}"))
                .on_press(Message::SensitivityChanged(level))
                .into()
        };
        sensitivity_buttons = sensitivity_buttons.push(btn);
    }

    let sensitivity_section = settings::section()
        .title("Touch Sensitivity")
        .add(settings::item(
            "Type/touch switching sensitivity",
            text::caption(sensitivity_text),
        ))
        .add(settings::item_row(vec![sensitivity_buttons.into()]));

    // Touch sliders section
    let sliders_section = settings::section()
        .title("Touch Sliders")
        .add(settings::item(
            "Left slider (F2-F6)",
            widget::dropdown(
                SLIDER_ACTION_LABELS,
                Some(state.left_slider_idx),
                Message::LeftSliderSelected,
            )
            .width(Length::Fixed(250.0)),
        ))
        .add(settings::item(
            "Right slider (F7-F11)",
            widget::dropdown(
                SLIDER_ACTION_LABELS,
                Some(state.right_slider_idx),
                Message::RightSliderSelected,
            )
            .width(Length::Fixed(250.0)),
        ));

    // Profiles section
    let profiles_toggle = widget::toggler(state.config.profiles_enabled)
        .on_toggle(Message::ProfilesEnabledToggled);

    let mut profiles_section = settings::section()
        .title("Per-App Profiles")
        .add(settings::item("Enable per-app profiles", profiles_toggle));

    if state.config.profiles_enabled {
        // List existing profiles
        for (idx, profile) in state.config.profiles.iter().enumerate() {
            let label = format!("{} ({})", profile.name, profile.app_id);
            profiles_section = profiles_section.add(settings::item(
                label,
                button::destructive("Remove").on_press(Message::RemoveProfile(idx)),
            ));
        }

        // Add new profile
        profiles_section = profiles_section
            .add(settings::item(
                "Profile name",
                text_input("e.g. Photoshop", &state.new_profile_name)
                    .on_input(Message::NewProfileNameChanged)
                    .width(Length::Fixed(250.0)),
            ))
            .add(settings::item(
                "Application ID",
                text_input("e.g. org.gimp.GIMP", &state.new_profile_app_id)
                    .on_input(Message::NewProfileAppIdChanged)
                    .width(Length::Fixed(250.0)),
            ))
            .add(settings::item_row(vec![button::suggested("Add Profile")
                .on_press(Message::AddProfile)
                .into()]));
    }

    // Actions
    let save_btn = button::suggested("Save").on_press(Message::Save);
    let reset_btn = button::destructive("Reset to Defaults").on_press(Message::ResetDefaults);

    let actions_section = settings::section()
        .title("Actions")
        .add(settings::item_row(vec![save_btn.into(), reset_btn.into()]));

    let mut content_items: Vec<Element<'_, Message>> = vec![
        page_title.into(),
        sensitivity_section.into(),
        sliders_section.into(),
        profiles_section.into(),
        actions_section.into(),
    ];

    if !state.status_message.is_empty() {
        content_items.push(text::body(&state.status_message).into());
    }

    settings::view_column(content_items).into()
}
