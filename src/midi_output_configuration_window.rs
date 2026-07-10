use crate::types::*;
use crate::Message;
use iced::widget::{button, checkbox, column, combo_box, row, text, tooltip};
use iced::{alignment, Element, Length};
use iced_aw::number_input;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct MIDIOutputConfigurationWindow {
    ticks_per_quarter_box: combo_box::State<u16>,
    volume_curve_box: combo_box::State<VolumeCurve>,
    midi_system_box: combo_box::State<MIDISystem>,
    midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>,
}

impl VolumeCurve {
    pub const ALL: [VolumeCurve; 3] = [Self::SquareRoot, Self::Log, Self::Linear];
}

impl std::fmt::Display for VolumeCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SquareRoot => "Square Root",
            Self::Log => "Log",
            Self::Linear => "Linear",
        })
    }
}

impl MIDISystem {
    pub const ALL: [MIDISystem; 5] = [Self::NONE, Self::GMLevel1, Self::GMLevel2, Self::GS, Self::XG];
}

impl std::fmt::Display for MIDISystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NONE => "None",
            Self::GMLevel1 => "GM Level 1",
            Self::GMLevel2 => "GM Level 2",
            Self::GS => "GS",
            Self::XG => "XG",
        })
    }
}

impl SPC2MIDI2Window for MIDIOutputConfigurationWindow {
    fn title(&self) -> String {
        "MIDI Output Configuration".to_string()
    }

    fn view(&self) -> Element<'_, Message> {
        let midi_output_configure = self.midi_output_configure.read().unwrap();
        let content = column![
            row![
                text("Tempo (BPM)"),
                number_input(
                    &midi_output_configure.beats_per_minute,
                    (MIN_BEATS_PER_MINUTE as f32)..=(MAX_BEATS_PER_MINUTE as f32),
                    Message::MIDIOutputBpmChanged,
                )
                .step(BPM_RESOLUTION),
                tooltip(
                    button("▼").on_press(Message::ReceivedBpmHalfButtonClicked),
                    "Half BPM",
                    tooltip::Position::Top,
                ),
                tooltip(
                    button("▲").on_press(Message::ReceivedBpmDoubleButtonClicked),
                    "Double BPM",
                    tooltip::Position::Top,
                ),
                tooltip(
                    button("Re-analyze").on_press(Message::ReceivedBpmAnalyzeRequest),
                    "Analyzing with Channel Mute",
                    tooltip::Position::Top,
                ),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("Ticks Per Quarter (resolution)"),
                combo_box(
                    &self.ticks_per_quarter_box,
                    "Ticks per quarter (resolution)",
                    Some(&midi_output_configure.ticks_per_quarter),
                    move |ticks| { Message::MIDIOutputTicksPerQuarterChanged(ticks) },
                ),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("Volume Curve"),
                combo_box(
                    &self.volume_curve_box,
                    "Volume Curve",
                    Some(&midi_output_configure.volume_curve),
                    move |curve| { Message::MIDIVolumeCurveChanged(curve) },
                ),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("MIDI Control Change Update Period (msec)"),
                number_input(
                    &midi_output_configure.playback_parameter_update_period,
                    0..=255,
                    move |period| { Message::MIDIOutputUpdatePeriodChanged(period) },
                )
                .step(1),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("Song Duration (msec)"),
                number_input(
                    &midi_output_configure.output_duration_msec,
                    1000..=(3600 * 1000),
                    move |duration| { Message::MIDIOutputDurationChanged(duration) },
                )
                .step(100),
                button("Re-analyze SRN").on_press(Message::ReceivedSRNReanalyzeRequest),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("Target MIDI System"),
                combo_box(
                    &self.midi_system_box,
                    "Target MIDI System",
                    Some(&midi_output_configure.midi_system),
                    move |system| { Message::MIDISystemChanged(system) },
                ),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("SPC700 Clock-Up Factor"),
                number_input(
                    &midi_output_configure.spc_clockup_factor,
                    1..=32,
                    move |factor| { Message::MIDIOutputSPC700ClockUpFactorChanged(factor) },
                )
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("Split Drum Notes Into Separate Tracks"),
                checkbox(midi_output_configure.split_drum_into_separate_tracks).on_toggle(
                    move |flag| Message::MIDIOutputSplitDrumIntoSeparateTracksChanged(flag)
                )
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
            row![
                text("Trim Leading Non-Event Period"),
                checkbox(midi_output_configure.trim_leading_nonevents_period).on_toggle(
                    move |flag| Message::MIDIOutputTrimLeadingNonEventsPeriodChanged(flag)
                )
            ]
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

impl MIDIOutputConfigurationWindow {
    pub fn new(midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>) -> Self {
        Self {
            midi_output_configure: midi_output_configure,
            ticks_per_quarter_box: combo_box::State::new(vec![
                24, 30, 48, 60, 96, 120, 192, 240, 384, 480, 960,
            ]),
            volume_curve_box: combo_box::State::new(VolumeCurve::ALL.to_vec()),
            midi_system_box: combo_box::State::new(MIDISystem::ALL.to_vec()),
        }
    }
}
