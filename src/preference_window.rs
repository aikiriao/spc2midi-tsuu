use crate::types::*;
use crate::Message;
use crate::SPC2MIDI2_TITLE_STR;
use cpal::traits::{DeviceTrait, HostTrait};
use iced::widget::{button, column, combo_box, row, text};
use iced::{alignment, Element, Length};
use iced_aw::number_input;
use midir::MidiOutput;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct PreferencesWindow {
    audio_out_device_name: Arc<RwLock<Option<String>>>,
    audio_out_devices_box: combo_box::State<String>,
    midi_out_port_name: Arc<RwLock<Option<String>>>,
    midi_ports_box: combo_box::State<String>,
    ticks_per_quarter_box: combo_box::State<u16>,
    spc_clockup_factor_box: combo_box::State<u32>,
    midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>,
}

impl SPC2MIDI2Window for PreferencesWindow {
    fn title(&self) -> String {
        "Preferences".to_string()
    }

    fn view(&self) -> Element<'_, Message> {
        let audio_device_name = self.audio_out_device_name.read().unwrap();
        let midi_port_name = self.midi_out_port_name.read().unwrap();
        let midi_output_configure = self.midi_output_configure.read().unwrap();
        let midi_output_configure_view = column![
            text("MIDI Output Configuration"),
            row![
                text("Tempo (BPM)"),
                number_input(
                    &midi_output_configure.beats_per_minute,
                    30.0..=240.0,
                    move |bpm| { Message::MIDIOutputBpmChanged(bpm) },
                )
                .step(1.0),
                button("Re-estimate Tempo").on_press(Message::ReceivedBpmAnalyzeRequest),
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
                text("SPC700 Clock-Up Factor"),
                combo_box(
                    &self.spc_clockup_factor_box,
                    "SPC700 Clock-Up Factor",
                    Some(&midi_output_configure.spc_clockup_factor),
                    move |factor| { Message::MIDIOutputSPC700ClockUpFactorChanged(factor) },
                ),
            ]
            .spacing(10)
            .padding(10)
            .align_y(alignment::Alignment::Center)
            .width(Length::Fill),
        ];
        let content = column![
            column![
                text("Audio Output Device"),
                combo_box(
                    &self.audio_out_devices_box,
                    "Audio Output Device",
                    audio_device_name.as_ref(),
                    move |device_name| Message::AudioOutputDeviceSelected(device_name),
                ),
            ]
            .spacing(10)
            .padding(10)
            .width(Length::Fill)
            .align_x(alignment::Alignment::Start),
            column![
                text("MIDI Output Port"),
                combo_box(
                    &self.midi_ports_box,
                    "MIDI Output Port",
                    midi_port_name.as_ref(),
                    move |port_name| Message::MIDIOutputPortSelected(port_name),
                )
            ]
            .spacing(10)
            .padding(10)
            .width(Length::Fill)
            .align_x(alignment::Alignment::Start),
            midi_output_configure_view
                .spacing(10)
                .padding(10)
                .width(Length::Fill)
                .align_x(alignment::Alignment::Start),
        ]
        .spacing(10)
        .padding(10)
        .width(Length::Fill)
        .align_x(alignment::Alignment::Center);
        content.into()
    }
}

impl PreferencesWindow {
    pub fn new(
        audio_out_device_name: Arc<RwLock<Option<String>>>,
        midi_out_port_name: Arc<RwLock<Option<String>>>,
        midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>,
    ) -> Self {
        let device_name_list: Vec<String> = cpal::default_host()
            .devices()
            .unwrap()
            .filter(|d| d.supports_output())
            .map(|d| {
                d.description()
                    .expect("Failed to get device name")
                    .to_string()
            })
            .collect();
        let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).unwrap();
        let port_name_list: Vec<String> = midi_out
            .ports()
            .iter()
            .map(|p| midi_out.port_name(p).unwrap())
            .collect();
        Self {
            audio_out_device_name: audio_out_device_name,
            audio_out_devices_box: combo_box::State::new(device_name_list),
            midi_out_port_name: midi_out_port_name,
            midi_ports_box: combo_box::State::new(port_name_list),
            midi_output_configure: midi_output_configure,
            ticks_per_quarter_box: combo_box::State::new(vec![
                24, 30, 48, 60, 96, 120, 192, 240, 384, 480, 960,
            ]),
            spc_clockup_factor_box: combo_box::State::new(vec![1, 2, 4, 8, 16, 32]),
        }
    }
}
