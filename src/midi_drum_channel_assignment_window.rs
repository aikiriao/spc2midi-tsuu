use crate::types::*;
use crate::Message;
use iced::widget::{column, pick_list, row, text, Column};
use iced::{alignment, Element, Length};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct MIDIDrumChannelAssignmentWindow {
    title: String,
    midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>,
}

impl SPC2MIDI2Window for MIDIDrumChannelAssignmentWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let midi_config = self.midi_output_configure.read().unwrap();

        let mode_choise = match midi_config.midi_system {
            MIDISystem::NONE | MIDISystem::GMLevel1 | MIDISystem::GMLevel2 => [
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Drum),
            ]
            .to_vec(),
            MIDISystem::GS => [
                MIDIPartMode::GS(GSPartMode::Normal),
                MIDIPartMode::GS(GSPartMode::RhythmMAP1),
                MIDIPartMode::GS(GSPartMode::RhythmMAP2),
            ]
            .to_vec(),
            MIDISystem::XG => {
                // Drum, DrumSetup3, DrumSetup4は選択禁止とする
                [
                    MIDIPartMode::XG(XGPartMode::Normal),
                    MIDIPartMode::XG(XGPartMode::DrumSetup1),
                    MIDIPartMode::XG(XGPartMode::DrumSetup2),
                ]
                .to_vec()
            }
        };

        let mut status_list: Vec<_> = (8..16)
            .map(|ch| {
                row![
                    text(format!("{}", ch))
                        .width(Length::FillPortion(1))
                        .align_x(alignment::Alignment::Start),
                    pick_list(
                        mode_choise.clone(),
                        Some(midi_config.part_mode[ch].clone()),
                        move |mode| Message::MIDIPartModeChanged(ch as u8, mode)
                    )
                    .width(Length::FillPortion(3)),
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();

        // インデックス
        let ch_index = row![
            text("Channel")
                .width(Length::FillPortion(1))
                .align_x(alignment::Alignment::Start),
            text("Part Mode")
                .width(Length::FillPortion(3))
                .align_x(alignment::Alignment::Start),
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);

        status_list.insert(0, ch_index.into());

        column![Column::from_vec(status_list).width(Length::Fill),].into()
    }
}

impl MIDIDrumChannelAssignmentWindow {
    pub fn new(title: String, midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>) -> Self {
        Self {
            title: title,
            midi_output_configure: midi_output_configure,
        }
    }
}
