use crate::types::*;
use crate::Message;
use crate::SPC2MIDI2_TITLE_STR;
use cpal::traits::{DeviceTrait, HostTrait};
use iced::widget::{column, combo_box, row, text};
use iced::{alignment, Element, Length};
use iced_aw::number_input;
use midir::MidiOutput;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct DeviceSettingWindow {
    audio_out_device_name: Arc<RwLock<Option<String>>>,
    audio_out_devices_box: combo_box::State<String>,
    midi_out_port_name: Arc<RwLock<Option<String>>>,
    midi_ports_box: combo_box::State<String>,
    audio_output_latency_msec: Arc<AtomicUsize>,
}

impl SPC2MIDI2Window for DeviceSettingWindow {
    fn title(&self) -> String {
        "Device Setting".to_string()
    }

    fn view(&self) -> Element<'_, Message> {
        let audio_device_name = self.audio_out_device_name.read().unwrap();
        let midi_port_name = self.midi_out_port_name.read().unwrap();
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
            row![
                text("Audio Output Latency (msec)"),
                number_input(
                    &self.audio_output_latency_msec.load(Ordering::Relaxed),
                    50..=1000,
                    move |msec| Message::AudioLatencyMsecChanged(msec),
                )
                .step(10),
            ]
            .spacing(10)
            .padding(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
        ]
        .spacing(10)
        .padding(10)
        .width(Length::Fill)
        .align_x(alignment::Alignment::Center);
        content.into()
    }
}

impl DeviceSettingWindow {
    pub fn new(
        audio_out_device_name: Arc<RwLock<Option<String>>>,
        midi_out_port_name: Arc<RwLock<Option<String>>>,
        audio_output_latency_msec: Arc<AtomicUsize>,
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
        let port_name_list = if let Ok(midi_out) = MidiOutput::new(SPC2MIDI2_TITLE_STR) {
            midi_out
                .ports()
                .iter()
                .map(|p| midi_out.port_name(p).expect("Failed to get MIDI port name"))
                .collect()
        } else {
            vec![]
        };
        Self {
            audio_out_device_name: audio_out_device_name,
            audio_out_devices_box: combo_box::State::new(device_name_list),
            midi_out_port_name: midi_out_port_name,
            midi_ports_box: combo_box::State::new(port_name_list),
            audio_output_latency_msec: audio_output_latency_msec,
        }
    }
}
