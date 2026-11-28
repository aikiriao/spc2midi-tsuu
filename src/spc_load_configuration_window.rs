use crate::types::*;
use crate::Message;
use iced::widget::{checkbox, column, row, text};
use iced::{alignment, Element, Length};
use iced_aw::number_input;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct SPCLoadConfigurationWindow {
    spc_load_configure: Arc<RwLock<SPCLoadConfigure>>,
}

impl SPC2MIDI2Window for SPCLoadConfigurationWindow {
    fn title(&self) -> String {
        "SPC Load Configuration".to_string()
    }

    fn view(&self) -> Element<'_, Message> {
        let spc_load_configure = self.spc_load_configure.read().unwrap();
        let content = column![
            row![checkbox(spc_load_configure.auto_drum_detection)
                .label("Auto Drum Detection")
                .on_toggle(move |flag| Message::SPCLoadAutoDrumDetectionChanged(flag))]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![checkbox(spc_load_configure.enable_pitch_bend_for_drum)
                .label("Enable Pitch Bend for Drum")
                .on_toggle(move |flag| Message::SPCLoadEnablePitchBendForDrumChanged(flag))]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                checkbox(spc_load_configure.auto_pitch_bend_range_detection)
                    .label("Auto Pitch Bend Width Detection")
                    .on_toggle(
                        move |flag| Message::SPCLoadAutoPitchBendRangeDetectionChanged(flag)
                    ),
                text("Fix Width (semitone)"),
                number_input(
                    &spc_load_configure.fixed_pitch_bend_width,
                    1..=48,
                    move |width| { Message::SPCLoadFixedPitchBendWidthChanged(width) },
                )
                .step(1)
                .on_input_maybe(
                    if spc_load_configure.auto_pitch_bend_range_detection {
                        None
                    } else {
                        Some(move |width| Message::SPCLoadFixedPitchBendWidthChanged(width))
                    }
                ),
            ]
            .spacing(10)
            .padding(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![checkbox(spc_load_configure.preserve_source_parameter)
                .label("Preserve Source Parameters (Configs)")
                .on_toggle(
                    move |flag| Message::SPCLoadPreserveSourceParameterChanged(flag)
                )]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
        ]
        .spacing(10)
        .padding(10)
        .width(Length::Fill)
        .align_x(alignment::Alignment::Center);
        content.into()
    }
}

impl SPCLoadConfigurationWindow {
    pub fn new(spc_load_configure: Arc<RwLock<SPCLoadConfigure>>) -> Self {
        Self {
            spc_load_configure: spc_load_configure,
        }
    }
}
