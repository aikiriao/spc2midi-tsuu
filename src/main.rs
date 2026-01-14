#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Releaseビルドの時コンソールを非表示

mod main_window;
mod preference_window;
mod program;
mod source_estimation;
mod srn_window;
mod types;

use crate::main_window::*;
use crate::preference_window::*;
use crate::program::*;
use crate::source_estimation::*;
use crate::srn_window::*;
use crate::types::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, PauseStreamError, PlayStreamError, Stream, StreamConfig};
use fixed_resample::ReadStatus;
use iced::widget::{center, space};
use iced::{event, window, Subscription, Task, Theme};
use iced_aw::ICED_AW_FONT_BYTES;
use midir::{MidiOutput, MidiOutputConnection};
use rfd::AsyncFileDialog;
use rimd::{
    Event as MidiEvent, MetaEvent, MidiMessage, SMFFormat, SMFWriter, Track, TrackEvent, SMF,
};
use samplerate::{convert, ConverterType};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::BufWriter;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use std::{cmp, io};

use spc700::decoder::*;
use spc700::mididsp::*;
use spc700::spc::*;
use spc700::spc_file::*;
use spc700::types::*;

/// タイトル文字列
pub const SPC2MIDI2_TITLE_STR: &'static str = "spc2midi-tsuu";
/// SPCの出力サンプリングレート
pub const SPC_SAMPLING_RATE: u32 = 32000;
/// PCM正規化定数
const PCM_NORMALIZE_CONST: f32 = 1.0 / 32768.0;
/// 64KHz周期のクロックサイクル SPCのクロック(1.024MHz)を64KHzで割って得られる = 1024000 / 64000
const CLOCK_TICK_CYCLE_64KHZ: u32 = 16;
/// 64kHz間隔に相当するナノ秒
const CLOCK_TICK_CYCLE_64KHZ_NANOSEC: u64 = 15625;
/// MIDIメッセージ：ノートオン
const MIDIMSG_NOTE_ON: u8 = 0x90;
/// MIDIメッセージ：ノートオフ
const MIDIMSG_NOTE_OFF: u8 = 0x80;
/// MIDIメッセージ：プログラムチェンジ
const MIDIMSG_PROGRAM_CHANGE: u8 = 0xC0;
/// MIDIメッセージ：チャンネルモードメッセージ
const MIDIMSG_MODE: u8 = 0xB0;
/// MIDIチェンネルモードメッセージ：オールサウンドオフ
const MIDIMSG_MODE_ALL_SOUND_OFF: u8 = 0x78;
/// MIDIをプレビューする際に使用するチャンネル
const MIDI_PREVIEW_CHANNEL: u8 = 0;
/// MIDIをプレビューする時間(msec)
const MIDI_PREVIEW_DURATION_MSEC: u64 = 500;
/// デフォルトの音源の分析時間(sec)
const DEFAULT_ANALYZING_TIME_SEC: u32 = 120;

pub fn main() -> iced::Result {
    iced::daemon(App::new, App::update, App::view)
        .subscription(App::subscription)
        .title(App::title)
        .theme(App::theme)
        .font(ICED_AW_FONT_BYTES)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    OpenMainWindow,
    MainWindowOpened(window::Id),
    OpenPreferenceWindow,
    PreferenceWindowOpened(window::Id),
    OpenSRNWindow(u8),
    SRNWindowOpened(window::Id),
    WindowClosed(window::Id),
    OpenFile,
    FileOpened(Result<(PathBuf, LoadedFile), Error>),
    SaveSMF,
    SMFSaved(Result<(), Error>),
    SaveJSON,
    JSONSaved(Result<(), Error>),
    MenuSelected,
    EventOccurred(iced::Event),
    ReceivedSRNPlayStartRequest(u8),
    SRNPlayLoopFlagToggled(bool),
    ReceivedPlayStartRequest,
    ReceivedPlayStopRequest,
    SPCMuteFlagToggled(bool),
    MIDIMuteFlagToggled(bool),
    SRNMuteFlagToggled(u8, bool),
    ProgramSelected(u8, Program),
    SRNMIDIPreviewFlagToggled(bool),
    ReceivedMIDIPreviewRequest(u8),
    CenterNoteIntChanged(u8, u8),
    CenterNoteFractionChanged(u8, f32),
    NoteOnVelocityChanged(u8, u8),
    PitchBendWidthChanged(u8, u8),
    EnablePitchBendFlagToggled(u8, bool),
    AutoPanFlagToggled(u8, bool),
    FixedPanChanged(u8, u8),
    AutoVolumeFlagToggled(u8, bool),
    FixedVolumeChanged(u8, u8),
    EnvelopeAsExpressionFlagToggled(u8, bool),
    EchoAsEffect1FlagToggled(u8, bool),
    SRNNoteEstimationClicked(u8),
    ReceivedSourceParameterUpdate,
    AudioOutputDeviceSelected(String),
    MIDIOutputPortSelected(String),
    MIDIOutputBpmChanged(f32),
    MIDIOutputTicksPerQuarterChanged(u16),
    MIDIOutputUpdatePeriodChanged(u8),
    MIDIOutputDurationChanged(u64),
    MuteChannel(u8, bool),
    SoloChannel(u8),
    Tick,
}

struct App {
    theme: iced::Theme,
    main_window_id: window::Id,
    windows: BTreeMap<window::Id, Box<dyn SPC2MIDI2Window>>,
    spc_file: Option<Box<SPCFile>>,
    spc_file_path: Option<PathBuf>,
    source_infos: Arc<RwLock<BTreeMap<u8, SourceInformation>>>,
    source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    playback_status: Arc<RwLock<PlaybackStatus>>,
    midi_output_configure: Arc<RwLock<MIDIOutputConfigure>>,
    stream_device: Device,
    stream_config: StreamConfig,
    stream: Option<Stream>,
    stream_played_samples: Arc<AtomicUsize>,
    stream_is_playing: Arc<AtomicBool>,
    midi_out_conn: Option<Arc<Mutex<MidiOutputConnection>>>,
    pcm_spc: Option<Arc<Mutex<Box<spc700::spc::SPC<spc700::sdsp::SDSP>>>>>,
    midi_spc: Option<Arc<Mutex<Box<spc700::spc::SPC<spc700::mididsp::MIDIDSP>>>>>,
    pcm_spc_mute: Arc<AtomicBool>,
    midi_spc_mute: Arc<AtomicBool>,
    midi_preview: Arc<AtomicBool>,
    preview_loop: Arc<AtomicBool>,
    midi_channel_mute: Arc<RwLock<[bool; 8]>>,
    audio_out_device_name: Arc<RwLock<Option<String>>>,
    midi_out_port_name: Arc<RwLock<Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExportInformation {
    /// MIDI出力設定
    midi_output_configure: MIDIOutputConfigure,
    /// 音源パラメータ割当
    source_parameter: BTreeMap<u8, SourceParameter>,
}

/// 読み込んだデータ
#[derive(Clone, Debug)]
enum LoadedFile {
    SPCFile(Vec<u8>),
    JSONFile(String),
}

impl Default for App {
    fn default() -> Self {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no output device available");
        let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).expect("no MIDI output port available");
        let midi_out_ports = midi_out.ports();
        let midi_out_port_name = if midi_out_ports.len() > 0 {
            Some(midi_out.port_name(&midi_out_ports[0]).unwrap())
        } else {
            None
        };
        Self {
            theme: iced::Theme::Dark,
            main_window_id: window::Id::unique(),
            windows: BTreeMap::new(),
            spc_file: None,
            spc_file_path: None,
            source_infos: Arc::new(RwLock::new(BTreeMap::new())),
            source_parameter: Arc::new(RwLock::new(BTreeMap::new())),
            playback_status: Arc::new(RwLock::new(PlaybackStatus::new())),
            midi_output_configure: Arc::new(RwLock::new(MIDIOutputConfigure::new())),
            stream_config: device.default_output_config().unwrap().into(),
            stream_device: device.clone(),
            stream: None,
            stream_played_samples: Arc::new(AtomicUsize::new(0)),
            stream_is_playing: Arc::new(AtomicBool::new(false)),
            midi_out_conn: if midi_out_ports.len() > 0 {
                match midi_out.connect(&midi_out_ports[0], SPC2MIDI2_TITLE_STR) {
                    Ok(conn) => Some(Arc::new(Mutex::new(conn))),
                    Err(_) => None,
                }
            } else {
                None
            },
            pcm_spc: None,
            midi_spc: None,
            pcm_spc_mute: Arc::new(AtomicBool::new(false)),
            midi_spc_mute: Arc::new(AtomicBool::new(false)),
            midi_preview: Arc::new(AtomicBool::new(true)),
            preview_loop: Arc::new(AtomicBool::new(false)),
            midi_channel_mute: Arc::new(RwLock::new([false; 8])),
            audio_out_device_name: Arc::new(RwLock::new(Some(
                device
                    .description()
                    .expect("Failed to get device name")
                    .to_string(),
            ))),
            midi_out_port_name: Arc::new(RwLock::new(midi_out_port_name)),
        }
    }
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            App { ..App::default() },
            Task::done(Message::OpenMainWindow),
        )
    }

    fn title(&self, id: window::Id) -> String {
        self.windows
            .get(&id)
            .map(|window| window.title().clone())
            .unwrap_or_default()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenMainWindow => {
                let (id, open) = window::open(window::Settings {
                    size: iced::Size::new(500.0, 600.0),
                    ..Default::default()
                });
                let window = MainWindow::new(
                    format!("{} {}", SPC2MIDI2_TITLE_STR, env!("CARGO_PKG_VERSION")),
                    self.theme.clone(),
                    self.source_parameter.clone(),
                    self.playback_status.clone(),
                    self.pcm_spc_mute.clone(),
                    self.midi_spc_mute.clone(),
                    self.midi_channel_mute.clone(),
                );
                self.main_window_id = id;
                self.windows.insert(id, Box::new(window));
                return open.map(Message::MainWindowOpened);
            }
            Message::MainWindowOpened(_id) => {}
            Message::OpenPreferenceWindow => {
                let (id, open) = window::open(window::Settings {
                    size: iced::Size::new(500.0, 500.0),
                    ..Default::default()
                });
                self.windows.insert(
                    id,
                    Box::new(PreferenceWindow::new(
                        self.audio_out_device_name.clone(),
                        self.midi_out_port_name.clone(),
                        self.midi_output_configure.clone(),
                    )),
                );
                return open.map(Message::PreferenceWindowOpened);
            }
            Message::PreferenceWindowOpened(_id) => {}
            Message::OpenSRNWindow(srn_no) => {
                let (id, open) = window::open(window::Settings {
                    size: iced::Size::new(800.0, 600.0),
                    ..Default::default()
                });
                let infos = self.source_infos.read().unwrap();
                if let Some(source) = infos.get(&srn_no) {
                    let window = SRNWindow::new(
                        format!("SRN 0x{:02X}", srn_no),
                        srn_no,
                        source,
                        self.source_parameter.clone(),
                        self.midi_preview.clone(),
                        self.preview_loop.clone(),
                    );
                    self.windows.insert(id, Box::new(window));
                    return open.map(Message::SRNWindowOpened);
                }
            }
            Message::SRNWindowOpened(_id) => {}
            Message::WindowClosed(id) => {
                if id == self.main_window_id {
                    return iced::exit();
                }
            }
            Message::OpenFile => {
                // 再生中の場合は止める
                if self.stream_is_playing.load(Ordering::Relaxed) {
                    self.stream_play_stop().expect("Failed to stop play");
                }
                return Task::perform(open_file(), Message::FileOpened);
            }
            Message::FileOpened(result) => match result {
                Ok((path, data)) => {
                    match data {
                        LoadedFile::SPCFile(data) => {
                            if let Some(spc_file) = parse_spc_file(&data) {
                                self.spc_file = Some(Box::new(spc_file.clone()));
                                self.analyze_sources(
                                    if spc_file.header.duration > 0 {
                                        spc_file.header.duration as u32
                                    } else {
                                        DEFAULT_ANALYZING_TIME_SEC
                                    },
                                    &spc_file.header.spc_register,
                                    &spc_file.ram,
                                    &spc_file.dsp_register,
                                );
                                // SPCを生成
                                self.pcm_spc = Some(Arc::new(Mutex::new(Box::new(SPC::new(
                                    &spc_file.header.spc_register,
                                    &spc_file.ram,
                                    &spc_file.dsp_register,
                                )))));
                                self.midi_spc = Some(Arc::new(Mutex::new(Box::new(SPC::new(
                                    &spc_file.header.spc_register,
                                    &spc_file.ram,
                                    &spc_file.dsp_register,
                                )))));
                                // 再生サンプル数をリセット
                                self.stream_played_samples.store(0, Ordering::Relaxed);
                                // ウィンドウタイトルに開いたファイル名を追記
                                if let Some(window) = self.windows.get_mut(&self.main_window_id) {
                                    let main_window: &mut MainWindow =
                                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                                    main_window.title = format!(
                                        "{} - {}",
                                        main_window.base_title,
                                        path.file_name().unwrap().to_str().unwrap()
                                    );
                                }
                                // 出力時間をSPCの情報を元に設定
                                let mut config = self.midi_output_configure.write().unwrap();
                                config.output_duration_msec = if spc_file.header.duration > 0 {
                                    (spc_file.header.duration as u64) * 1000
                                } else {
                                    DEFAULT_OUTPUT_DURATION_MSEC
                                };
                                self.spc_file_path = Some(path);
                            }
                        }
                        LoadedFile::JSONFile(data) => {
                            match serde_json::from_str::<ExportInformation>(&data) {
                                Ok(json) => {
                                    // 読み込みに成功したら内部コンフィグとパラメータを更新
                                    let mut config = self.midi_output_configure.write().unwrap();
                                    let mut params = self.source_parameter.write().unwrap();
                                    *config = json.midi_output_configure;
                                    // 丸ごと上書きすると設定済みのkeyを消してしまうので追記
                                    for (key, value) in json.source_parameter {
                                        params.insert(key, value);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("ERROR: failed to load json file: {:?}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("ERROR: failed to open file: {:?}", e);
                }
            },
            Message::SaveSMF => {
                if let Some(path) = &self.spc_file_path {
                    if let Some(smf) = self.create_smf() {
                        return Task::perform(
                            save_smf(
                                path.file_stem().unwrap().to_str().unwrap().to_owned() + ".mid",
                                smf,
                            ),
                            Message::SMFSaved,
                        );
                    }
                }
            }
            Message::SMFSaved(_result) => {}
            Message::SaveJSON => {
                if let Some(path) = &self.spc_file_path {
                    return Task::perform(
                        save_json(
                            path.file_stem().unwrap().to_str().unwrap().to_owned() + ".json",
                            self.create_json(),
                        ),
                        Message::JSONSaved,
                    );
                }
            }
            Message::JSONSaved(_result) => {}
            Message::MenuSelected => {}
            Message::EventOccurred(event) => match event {
                iced::event::Event::Window(event) => {
                    if let iced::window::Event::FileDropped(path) = event {
                        return Task::perform(load_file(path), Message::FileOpened);
                    }
                }
                _ => {}
            },
            Message::ReceivedSRNPlayStartRequest(srn_no) => {
                if self.stream_is_playing.load(Ordering::Relaxed) {
                    // 再生中の場合は止める
                    self.stream_play_stop().expect("Failed to stop play");
                } else {
                    // 新規再生処理
                    if let Err(_) = self.srn_play_start(srn_no) {
                        eprintln!("[{}] Faild to start playback", SPC2MIDI2_TITLE_STR);
                    }
                }
            }
            Message::SRNPlayLoopFlagToggled(flag) => {
                self.preview_loop.store(flag, Ordering::Relaxed);
            }
            Message::SRNMIDIPreviewFlagToggled(flag) => {
                self.midi_preview.store(flag, Ordering::Relaxed);
            }
            Message::ReceivedPlayStartRequest => {
                if self.stream_is_playing.load(Ordering::Relaxed) {
                    // 再生中の場合は止める
                    self.stream_play_stop().expect("Failed to stop play");
                } else {
                    // 再生開始
                    if let Err(_) = self.play_start() {
                        eprintln!("[{}] Faild to start playback", SPC2MIDI2_TITLE_STR);
                    }
                }
            }
            Message::ReceivedPlayStopRequest => {
                // 再生中の場合は止める
                if self.stream_is_playing.load(Ordering::Relaxed) {
                    self.stream_play_stop().expect("Failed to stop play");
                }
                // DSPをリセット
                if let Some(spc_file) = &self.spc_file {
                    self.pcm_spc = Some(Arc::new(Mutex::new(Box::new(SPC::new(
                        &spc_file.header.spc_register,
                        &spc_file.ram,
                        &spc_file.dsp_register,
                    )))));
                    self.midi_spc = Some(Arc::new(Mutex::new(Box::new(SPC::new(
                        &spc_file.header.spc_register,
                        &spc_file.ram,
                        &spc_file.dsp_register,
                    )))));
                }
                // Stopの場合は再生サンプル数をリセット
                self.stream_played_samples.store(0, Ordering::Relaxed);
            }
            Message::SPCMuteFlagToggled(flag) => {
                // フラグ書き換え
                self.pcm_spc_mute.clone().store(flag, Ordering::Relaxed);
            }
            Message::MIDIMuteFlagToggled(flag) => {
                if let Some(midi_spc_ref) = &self.midi_spc {
                    let midi_spc = midi_spc_ref.clone();
                    let mut spc = midi_spc.lock().unwrap();
                    // 全チャンネルミュートを切り替え
                    spc.dsp.write_register(
                        &[0u8],
                        DSP_ADDRESS_MIDI_MUTE,
                        if flag { 0xFF } else { 0 },
                    );
                    // トグルスイッチの値を書き換え
                    if let Ok(mut ch_mute) = self.midi_channel_mute.write() {
                        for ch in 0..8 {
                            ch_mute[ch as usize] = flag;
                        }
                    }
                    self.midi_spc_mute.clone().store(flag, Ordering::Relaxed);
                }
                // ミュートの時は音を止める
                if flag {
                    self.stop_midi_all_sound();
                }
            }
            Message::SRNMuteFlagToggled(srn_no, flag) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.mute = flag;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::ProgramSelected(srn_no, program) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.program = program.clone();
                }
                let mut tasks = vec![];
                if self.midi_preview.load(Ordering::Relaxed) {
                    tasks.push(Task::perform(async {}, move |_| {
                        Message::ReceivedMIDIPreviewRequest(srn_no)
                    }));
                }
                tasks.push(Task::perform(async {}, move |_| {
                    Message::ReceivedSourceParameterUpdate
                }));
                return Task::batch(tasks);
            }
            Message::CenterNoteIntChanged(srn_no, note) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.center_note = (param.center_note & 0x00FF) | ((note as u16) << 8);
                }
                let mut tasks = vec![];
                if self.midi_preview.load(Ordering::Relaxed) {
                    tasks.push(Task::perform(async {}, move |_| {
                        Message::ReceivedMIDIPreviewRequest(srn_no)
                    }));
                }
                tasks.push(Task::perform(async {}, move |_| {
                    Message::ReceivedSourceParameterUpdate
                }));
                return Task::batch(tasks);
            }
            Message::CenterNoteFractionChanged(srn_no, fraction) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    let clamped_fraction = f32::round(fraction * 256.0).clamp(0.0, 255.0);
                    param.center_note = (param.center_note & 0xFF00) | (clamped_fraction as u16);
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::NoteOnVelocityChanged(srn_no, velocity) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.noteon_velocity = velocity;
                }
                let mut tasks = vec![];
                if self.midi_preview.load(Ordering::Relaxed) {
                    tasks.push(Task::perform(async {}, move |_| {
                        Message::ReceivedMIDIPreviewRequest(srn_no)
                    }));
                }
                tasks.push(Task::perform(async {}, move |_| {
                    Message::ReceivedSourceParameterUpdate
                }));
                return Task::batch(tasks);
            }
            Message::PitchBendWidthChanged(srn_no, width) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.pitch_bend_width = width;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::EnablePitchBendFlagToggled(srn_no, flag) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.enable_pitch_bend = flag;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::AutoPanFlagToggled(srn_no, flag) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.auto_pan = flag;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::FixedPanChanged(srn_no, pan) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.fixed_pan = pan;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::AutoVolumeFlagToggled(srn_no, flag) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.auto_volume = flag;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::FixedVolumeChanged(srn_no, volume) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.fixed_volume = volume;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::EnvelopeAsExpressionFlagToggled(srn_no, flag) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.envelope_as_expression = flag;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::EchoAsEffect1FlagToggled(srn_no, flag) => {
                let mut params = self.source_parameter.write().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    param.echo_as_effect1 = flag;
                    return Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    });
                }
            }
            Message::SRNNoteEstimationClicked(srn_no) => {
                let mut params = self.source_parameter.write().unwrap();
                let infos = self.source_infos.read().unwrap();
                if let Some(param) = params.get_mut(&srn_no) {
                    if let Some(info) = infos.get(&srn_no) {
                        let (_, center_note) = estimate_drum_and_note(&info);
                        param.center_note = f32::round(center_note * 256.0) as u16;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::ReceivedSourceParameterUpdate => {
                self.apply_source_parameter();
            }
            Message::ReceivedMIDIPreviewRequest(srn_no) => {
                self.preview_midi_sound(srn_no);
            }
            Message::AudioOutputDeviceSelected(device_name) => {
                let mut audio_out_device_name = self.audio_out_device_name.write().unwrap();
                *audio_out_device_name = Some(device_name.clone());
                // オーディオ出力デバイスを再構築
                let devices = cpal::default_host()
                    .devices()
                    .expect("Failed to get devices");
                self.stream_device = devices
                    .filter(|d| d.supports_output())
                    .find(|d| d.description().unwrap().to_string() == device_name)
                    .expect("Failed to create output stream device");
                self.stream_config = self
                    .stream_device
                    .clone()
                    .default_output_config()
                    .unwrap()
                    .into();
            }
            Message::MIDIOutputPortSelected(port_name) => {
                let mut midi_out_port_name = self.midi_out_port_name.write().unwrap();
                *midi_out_port_name = Some(port_name.clone());
                // MIDI出力ポートを再接続
                let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).unwrap();
                let ports = midi_out.ports();
                // 選択したポート名を探す
                let mut i = 0;
                while i < ports.len() {
                    if port_name.clone() == midi_out.port_name(&ports[i]).unwrap() {
                        break;
                    }
                    i += 1;
                }
                // ポート出力作成
                self.midi_out_conn = if i < ports.len() {
                    match midi_out.connect(&ports[i], SPC2MIDI2_TITLE_STR) {
                        Ok(conn) => Some(Arc::new(Mutex::new(conn))),
                        Err(_) => None,
                    }
                } else {
                    None
                };
            }
            Message::MIDIOutputBpmChanged(bpm) => {
                let mut config = self.midi_output_configure.write().unwrap();
                config.beats_per_minute = bpm;
            }
            Message::MIDIOutputTicksPerQuarterChanged(ticks) => {
                let mut config = self.midi_output_configure.write().unwrap();
                config.ticks_per_quarter = ticks;
            }
            Message::MIDIOutputUpdatePeriodChanged(period) => {
                let mut config = self.midi_output_configure.write().unwrap();
                config.playback_parameter_update_period = period;
                // 再生にかかわることなのでパラメータ反映
                return Task::perform(async {}, move |_| Message::ReceivedSourceParameterUpdate);
            }
            Message::MIDIOutputDurationChanged(duration) => {
                let mut config = self.midi_output_configure.write().unwrap();
                config.output_duration_msec = duration;
            }
            Message::MuteChannel(ch, flag) => {
                if let Some(midi_spc_ref) = &self.midi_spc {
                    let midi_spc = midi_spc_ref.clone();
                    let mut spc = midi_spc.lock().unwrap();
                    // 指定チャンネルをミュート
                    let mut flags = spc.dsp.read_register(&[0u8], DSP_ADDRESS_MIDI_MUTE);
                    flags = if flag {
                        flags | (1 << ch)
                    } else {
                        flags & !(1 << ch)
                    };
                    spc.dsp.write_register(&[0u8], DSP_ADDRESS_MIDI_MUTE, flags);
                    let mut ch_flag = self.midi_channel_mute.write().unwrap();
                    ch_flag[ch as usize] = flag;
                }
                if flag {
                    // ミュートの場合は音を止める
                    self.stop_midi_channel_sound(ch);
                    // 全チャンネルミュートになった場合はミュートフラグを立てる
                    let ch_flag = self.midi_channel_mute.read().unwrap();
                    let mut all_ch_mute = true;
                    for ch in 0..8 {
                        if !ch_flag[ch] {
                            all_ch_mute = false;
                            break;
                        }
                    }
                    if all_ch_mute {
                        self.midi_spc_mute.clone().store(true, Ordering::Relaxed);
                    }
                } else {
                    // もはや全チャンネルミュートではないのでフラグを落とす
                    self.midi_spc_mute.clone().store(false, Ordering::Relaxed);
                }
            }
            Message::SoloChannel(ch) => {
                if let Some(midi_spc_ref) = &self.midi_spc {
                    let midi_spc = midi_spc_ref.clone();
                    let mut spc = midi_spc.lock().unwrap();
                    // 指定チャンネル以外をミュート
                    spc.dsp
                        .write_register(&[0u8], DSP_ADDRESS_MIDI_MUTE, !(1 << ch));
                    if let Ok(mut ch_mute) = self.midi_channel_mute.write() {
                        for mute_ch in 0..8 {
                            ch_mute[mute_ch as usize] = mute_ch != ch;
                        }
                    }
                }
                // ミュートの場合は音を止める
                for mute_ch in 0..8 {
                    if mute_ch != ch {
                        self.stop_midi_channel_sound(mute_ch);
                    }
                }
                // もはや全チャンネルミュートではないのでフラグを落とす
                self.midi_spc_mute.clone().store(false, Ordering::Relaxed);
            }
            Message::Tick => {
                // 再生情報取得
                if let Some(midi_spc_ref) = &self.midi_spc {
                    let midi_spc = midi_spc_ref.clone();
                    let spc = midi_spc.lock().unwrap();
                    let mut status = self.playback_status.write().unwrap();
                    *status = read_playback_status(&spc.dsp);
                }

                // 再生情報更新
                if let Some(window) = self.windows.get_mut(&self.main_window_id) {
                    let status = self.playback_status.read().unwrap();
                    let main_win: &mut MainWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let played_samples = self.stream_played_samples.load(Ordering::Relaxed);
                    main_win.playback_time_sec =
                        played_samples as f32 / self.stream_config.sample_rate as f32;
                    for ch in 0..8 {
                        main_win.expression_indicator[ch].value = status.envelope[ch] as f32;
                        main_win.pitch_indicator[ch].value = if status.pitch[ch] > 0 {
                            12.0 * (f32::log2(status.pitch[ch] as f32) - 12.0)
                        } else {
                            0.0
                        };
                        main_win.volume_indicator[ch][0].value = status.volume[ch][0] as f32;
                        main_win.volume_indicator[ch][1].value = status.volume[ch][1] as f32;
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self, id: window::Id) -> iced::Element<'_, Message> {
        if let Some(window) = self.windows.get(&id) {
            center(window.view()).into()
        } else {
            space().into()
        }
    }

    fn theme(&self, _: window::Id) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.stream_is_playing.load(Ordering::Relaxed) {
            Subscription::batch(vec![
                iced::time::every(iced::time::Duration::from_millis(10)).map(|_| Message::Tick),
                window::close_events().map(Message::WindowClosed),
                event::listen().map(Message::EventOccurred),
            ])
        } else {
            Subscription::batch(vec![
                window::close_events().map(Message::WindowClosed),
                event::listen().map(Message::EventOccurred),
            ])
        }
    }

    /// 音源ソースの解析
    fn analyze_sources(
        &mut self,
        analyze_duration_sec: u32,
        register: &SPCRegister,
        ram: &[u8],
        dsp_register: &[u8; 128],
    ) {
        let analyze_duration_64khz_ticks = analyze_duration_sec * 64000;

        // 音源情報を作り直す
        let mut infos = self.source_infos.write().unwrap();
        *infos = BTreeMap::new();
        let mut params = self.source_parameter.write().unwrap();
        *params = BTreeMap::new();

        // 一定期間シミュレートし、サンプルソース番号とそれに紐づく開始アドレスを取得
        let mut midispc: Box<spc700::spc::SPC<spc700::mididsp::MIDIDSP>> =
            Box::new(SPC::new(&register, ram, dsp_register));
        let mut spc: Box<spc700::spc::SPC<spc700::sdsp::SDSP>> =
            Box::new(SPC::new(&register, ram, dsp_register));
        let mut cycle_count = 0;
        let mut tick64khz_count = 0;
        let mut start_address_map = BTreeMap::new();
        let mut signal = vec![];
        while tick64khz_count < analyze_duration_64khz_ticks {
            cycle_count += spc.execute_step() as u32;
            let _ = midispc.execute_step();
            // キーオンが打たれていた時のサンプル番号を取得
            // DSPを動かすとキーオンフラグが落ちることがあるので64kHzティック前に調べる
            let keyon = midispc.dsp.read_register(ram, DSP_ADDRESS_KON);
            if keyon != 0 {
                let brr_dir_base_address =
                    (midispc.dsp.read_register(ram, DSP_ADDRESS_DIR) as u16) << 8;
                for ch in 0..8 {
                    if (keyon >> ch) & 1 != 0 {
                        let sample_source = midispc
                            .dsp
                            .read_register(ram, (ch << 4) | DSP_ADDRESS_V0SRCN);
                        let dir_address =
                            (brr_dir_base_address + 4 * (sample_source as u16)) as usize;
                        start_address_map.insert(sample_source, dir_address);
                    }
                }
            }
            if cycle_count >= CLOCK_TICK_CYCLE_64KHZ {
                midispc.clock_tick_64k_hz();
                if let Some(pcm) = spc.clock_tick_64k_hz() {
                    signal.push(PCM_NORMALIZE_CONST * 0.5 * (pcm[0] as f32 + pcm[1] as f32));
                }
                cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
                tick64khz_count += 1;
            }
        }

        // テンポ推定
        let mut config = self.midi_output_configure.write().unwrap();
        // 小数点以下は0.25に丸め込む
        let estimated_bpm = estimate_bpm(&signal);
        config.beats_per_minute = f32::round(estimated_bpm * 4.0) / 4.0;

        // 波形情報の読み込み
        for (srn, dir_address) in start_address_map.iter() {
            let mut decoder = Decoder::new();
            let mut signal = Vec::new();
            decoder.keyon(ram, *dir_address);
            // 原音ピッチで終端までデコード
            loop {
                let pcm = decoder.process(ram, 0x1000) as f32;
                signal.push(pcm * PCM_NORMALIZE_CONST);
                // 最後のブロックはデコードしない（ループを繋ぐため）
                if decoder.end {
                    break;
                }
            }
            // データ追記
            let start_address =
                make_u16_from_u8(&ram[(*dir_address + 0)..(*dir_address + 2)]) as usize;
            let loop_address =
                make_u16_from_u8(&ram[(*dir_address + 2)..(*dir_address + 4)]) as usize;
            let source_info = SourceInformation {
                signal: signal.clone(),
                start_address: start_address,
                end_address: start_address + (signal.len() * 9) / 16,
                loop_start_sample: ((loop_address - start_address) * 16) / 9,
            };
            infos.insert(*srn, source_info.clone());
            // ドラム音とピッチの推定
            let (is_drum, center_note) = estimate_drum_and_note(&source_info);
            params.insert(
                *srn,
                SourceParameter {
                    mute: false,
                    program: if is_drum {
                        Program::AcousticBassDrum
                    } else {
                        Program::AcousticGrand
                    },
                    center_note: f32::round(center_note * 256.0) as u16,
                    noteon_velocity: 100,
                    pitch_bend_width: 12,
                    envelope_as_expression: !is_drum,
                    auto_pan: true,
                    fixed_pan: 64,
                    auto_volume: true,
                    fixed_volume: 100,
                    enable_pitch_bend: !is_drum,
                    echo_as_effect1: true,
                },
            );
        }
    }

    // SMFを作成
    fn create_smf(&self) -> Option<SMF> {
        if let Some(spc_file) = &self.spc_file {
            let config = self.midi_output_configure.read().unwrap();
            let mut smf = SMF {
                format: SMFFormat::Single,
                tracks: vec![Track {
                    copyright: Some("".to_string()),
                    name: Some(String::from_utf8_lossy(&spc_file.header.music_title).to_string()),
                    events: Vec::new(),
                }],
                division: config.ticks_per_quarter as i16,
            };

            // SPCの作成
            let mut spc: spc700::spc::SPC<spc700::mididsp::MIDIDSP> = SPC::new(
                &spc_file.header.spc_register,
                &spc_file.ram,
                &spc_file.dsp_register,
            );

            // パラメータ適用
            let params = self.source_parameter.read().unwrap();
            apply_source_parameter(&mut spc, &config, &params, &spc_file.ram);

            // メタイベントの設定
            let quarter_usec = (60_000_000.0 / config.beats_per_minute) as u32;
            smf.tracks[0].events.push(TrackEvent {
                vtime: 0,
                event: MidiEvent::Meta(MetaEvent::tempo_setting(quarter_usec)),
            });

            // 出力で決めた時間だけ出力
            let ticks_per_minutes =
                (config.beats_per_minute as u64) * (config.ticks_per_quarter as u64);
            let mut total_ticks = 0;
            let mut total_elapsed_time_nanosec = 0;
            let mut cycle_count = 0;
            while total_elapsed_time_nanosec < config.output_duration_msec * 1000_000 {
                // 64kHzタイマーティックするまで処理
                while cycle_count < CLOCK_TICK_CYCLE_64KHZ {
                    cycle_count += spc.execute_step() as u32;
                }
                cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
                // clock_tick_64k_hz実行後に64KHz周期がすぎるので、ここで時間を増加
                total_elapsed_time_nanosec += CLOCK_TICK_CYCLE_64KHZ_NANOSEC;
                // MIDI出力
                if let Some(out) = spc.clock_tick_64k_hz() {
                    // ティック数：経過ティック数（現時刻までの総ティック数とこれまでのティック数の差）
                    let ticks = (total_elapsed_time_nanosec * ticks_per_minutes) / 60_000_000_000 - total_ticks;
                    // メッセージ追記
                    for i in 0..out.num_messages {
                        let msg = out.messages[i];
                        smf.tracks[0].events.push(TrackEvent {
                            vtime: if i == 0 { ticks } else { 0 },
                            event: MidiEvent::Midi(MidiMessage {
                                data: msg.data[..msg.length].to_vec(),
                            }),
                        });
                    }
                    total_ticks += ticks;
                }
            }

            Some(smf)
        } else {
            None
        }
    }

    // JSON生成
    fn create_json(&self) -> serde_json::Value {
        let config = self.midi_output_configure.read().unwrap();
        let params = self.source_parameter.read().unwrap();
        json!(ExportInformation {
            midi_output_configure: config.clone(),
            source_parameter: params.clone(),
        })
    }

    // 再生開始
    fn play_start(&mut self) -> Result<(), PlayStreamError> {
        const NUM_CHANNELS: usize = 2;
        const BUFFER_SIZE: usize = 2048;

        // SPCの参照をクローン
        let (pcm_spc, midi_spc) =
            if let (Some(pcm_spc_ref), Some(midi_spc_ref)) = (&self.pcm_spc, &self.midi_spc) {
                (pcm_spc_ref.clone(), midi_spc_ref.clone())
            } else {
                return Ok(());
            };

        let midi_out_conn = if let Some(midi_out_conn_ref) = &self.midi_out_conn {
            midi_out_conn_ref.clone()
        } else {
            // TODO: エラーにした方がよい
            return Ok(());
        };

        // リサンプラ初期化 32k -> デバイスの出力レート変換となるように
        let (mut prod, mut cons) = fixed_resample::resampling_channel::<f32, NUM_CHANNELS>(
            NonZero::new(NUM_CHANNELS).unwrap(),
            SPC_SAMPLING_RATE,
            self.stream_config.sample_rate,
            Default::default(),
        );

        // SPCのミュートフラグ取得・設定
        let pcm_spc_mute = self.pcm_spc_mute.clone();
        if let Ok(ch_mute) = self.midi_channel_mute.read() {
            let mut flags = 0;
            for ch in 0..8 {
                if ch_mute[ch] {
                    flags |= 1 << ch;
                }
            }
            let mut spc = midi_spc.lock().unwrap();
            spc.dsp.write_register(&[0u8], DSP_ADDRESS_MIDI_MUTE, flags);
        }

        // SPCにパラメータ適用
        self.apply_source_parameter();

        // 再生済みサンプル数
        let played_samples = self.stream_played_samples.clone();

        // 再生ストリーム作成
        let mut cycle_count = 0;
        let mut pcm_buffer = vec![0.0f32; BUFFER_SIZE * NUM_CHANNELS];
        let stream = match self.stream_device.build_output_stream(
            &self.stream_config,
            move |buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut progress = played_samples.load(Ordering::Relaxed);
                // SPCをロックして獲得
                let mut spc = pcm_spc.lock().unwrap();
                let mut midispc = midi_spc.lock().unwrap();
                // MIDI出力のロック
                let mut conn_out = midi_out_conn.lock().unwrap();

                // レート変換比を信じ、バッファが一定量埋まるまで出力させる
                let mut nsamples = prod.available_frames();
                while nsamples > BUFFER_SIZE / 2 {
                    let cycle = spc.execute_step();
                    let _ = midispc.execute_step();
                    cycle_count += cycle as u32;
                    if cycle_count >= CLOCK_TICK_CYCLE_64KHZ {
                        cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
                        // PCM出力
                        if let Some(pcm) = spc.clock_tick_64k_hz() {
                            let fout = if !pcm_spc_mute.load(Ordering::Relaxed) {
                                [
                                    (pcm[0] as f32) * PCM_NORMALIZE_CONST,
                                    (pcm[1] as f32) * PCM_NORMALIZE_CONST,
                                ]
                            } else {
                                [0.0f32, 0.0f32]
                            };
                            prod.push_interleaved(&fout);
                            nsamples = prod.available_frames();
                        }
                        // MIDI出力
                        if let Some(msgs) = midispc.clock_tick_64k_hz() {
                            for i in 0..msgs.num_messages {
                                let msg = msgs.messages[i];
                                conn_out.send(&msg.data[..msg.length]).unwrap();
                            }
                        }
                    }
                }

                // リサンプラー出力の取り出し
                let frames = buffer.len() / NUM_CHANNELS;
                let status = cons.read_interleaved(&mut pcm_buffer[..frames * NUM_CHANNELS]);
                if let ReadStatus::UnderflowOccurred { .. } = status {
                    eprintln!("input stream fell behind: try increasing channel latency");
                }

                buffer.fill(0.0);
                for ch in 0..NUM_CHANNELS {
                    for (out_chunk, in_chunk) in buffer
                        .chunks_exact_mut(NUM_CHANNELS)
                        .zip(pcm_buffer.chunks_exact(NUM_CHANNELS))
                    {
                        out_chunk[ch] = in_chunk[ch];
                    }
                }

                // 再生サンプル数増加
                progress += frames;
                played_samples.store(progress, Ordering::Relaxed);
            },
            |err| eprintln!("[{}] {err}", SPC2MIDI2_TITLE_STR),
            None,
        ) {
            Ok(stream) => stream,
            Err(_) => return Err(PlayStreamError::DeviceNotAvailable),
        };

        // 再生開始
        self.stream_is_playing.store(true, Ordering::Relaxed);
        stream.play()?;
        self.stream = Some(stream);

        Ok(())
    }

    // プレビュー再生開始
    fn srn_play_start(&mut self, srn_no: u8) -> Result<(), PlayStreamError> {
        // 再生対象の音源をコピー
        let infos = self.source_infos.read().unwrap();
        let source = if let Some(srn) = infos.get(&srn_no) {
            srn.clone()
        } else {
            return Ok(());
        };

        let num_channels = self.stream_config.channels as usize;
        let is_playing = self.stream_is_playing.clone();
        let loop_start_sample = f64::round(
            (source.loop_start_sample * self.stream_config.sample_rate as usize) as f64
                / SPC_SAMPLING_RATE as f64,
        ) as usize;

        // 出力先デバイスのレートに合わせてレート変換
        let resampled_pcm = convert(
            SPC_SAMPLING_RATE,
            self.stream_config.sample_rate,
            1,
            ConverterType::SincBestQuality,
            &source.signal,
        )
        .unwrap();
        let resampled_len = resampled_pcm.len();

        // 音源はモノラルなので出力チャンネル数分コピー
        let mut output = vec![0.0f32; resampled_len * num_channels];
        for smpl in 0..resampled_len {
            for ch in 0..num_channels {
                output[ch as usize + num_channels * smpl] = resampled_pcm[smpl];
            }
        }
        // ループ開始位置は出力サンプル数で上限をかける
        let loop_start_progress = cmp::min(num_channels * loop_start_sample, output.len() - 1);

        // ループフラグ
        let preview_loop = self.preview_loop.clone();

        // 再生サンプル数（ワンショットのプレビュー再生なので再生サンプルはselfに保持しない）
        let mut progress = 0;

        // 再生ストリーム作成
        let stream = match self.stream_device.build_output_stream(
            &self.stream_config,
            move |buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // 一旦バッファを無音で埋める
                buffer.fill(0.0);
                // バッファにコピー
                let num_copy_samples = cmp::min(output.len() - progress, buffer.len());
                buffer[..num_copy_samples]
                    .copy_from_slice(&output[progress..(progress + num_copy_samples)]);
                progress += num_copy_samples;
                // 端点に来た時の処理
                if progress >= output.len() {
                    if preview_loop.load(Ordering::Relaxed) {
                        // ループしながらバッファがいっぱいになるまでコピー
                        let mut buffer_pos = num_copy_samples;
                        progress = loop_start_progress;
                        while buffer_pos < buffer.len() {
                            let num_copy_samples =
                                cmp::min(output.len() - progress, buffer.len() - buffer_pos);
                            buffer[buffer_pos..(buffer_pos + num_copy_samples)]
                                .copy_from_slice(&output[progress..(progress + num_copy_samples)]);
                            buffer_pos += num_copy_samples;
                            progress += num_copy_samples;
                            if progress >= output.len() {
                                progress = loop_start_progress;
                            }
                        }
                    } else {
                        // 再生終了
                        is_playing.store(false, Ordering::Relaxed);
                    }
                }
            },
            |err| eprintln!("[{}] {err}", SPC2MIDI2_TITLE_STR),
            None,
        ) {
            Ok(stream) => stream,
            Err(_) => return Err(PlayStreamError::DeviceNotAvailable),
        };

        // 再生開始
        self.stream_is_playing.store(true, Ordering::Relaxed);
        stream.play()?;
        self.stream = Some(stream);

        Ok(())
    }

    // MIDIの全ての音を止める
    fn stop_midi_all_sound(&mut self) {
        if let Some(midi_out_conn_ref) = &self.midi_out_conn {
            let midi_out_conn = midi_out_conn_ref.clone();
            let mut conn_out = midi_out_conn.lock().unwrap();
            for ch in 0..16 {
                conn_out
                    .send(&[MIDIMSG_MODE | ch, MIDIMSG_MODE_ALL_SOUND_OFF, 0])
                    .unwrap();
            }
        }
    }

    // MIDIの特定チャンネルの音を止める
    fn stop_midi_channel_sound(&mut self, ch: u8) {
        if let Some(midi_out_conn_ref) = &self.midi_out_conn {
            let midi_out_conn = midi_out_conn_ref.clone();
            let mut conn_out = midi_out_conn.lock().unwrap();
            // ATENSION! MIDIVoiceは0..7chにある前提
            conn_out
                .send(&[MIDIMSG_MODE | ch, MIDIMSG_MODE_ALL_SOUND_OFF, 0])
                .unwrap();
        }
    }

    // 再生停止
    fn stream_play_stop(&mut self) -> Result<(), PauseStreamError> {
        if let Some(stream) = &self.stream {
            self.stream_is_playing.store(false, Ordering::Relaxed);
            stream.pause()?;
            self.stream = None;
        }
        self.stop_midi_all_sound();
        Ok(())
    }

    // MIDI楽器音をプレビュー
    fn preview_midi_sound(&self, srn_no: u8) {
        // 再生時のパラメータ設定
        let params = self.source_parameter.read().unwrap();
        let param = params.get(&srn_no).unwrap();
        let program = param.program.clone() as u8;
        let velocity = param.noteon_velocity;
        let note = (param.center_note >> 8) as u8;

        // MIDI出力の作成
        let midi_out_conn = if let Some(midi_out_conn_ref) = &self.midi_out_conn {
            midi_out_conn_ref.clone()
        } else {
            // TODO: エラーにした方が良い
            return;
        };
        let mut conn_out = midi_out_conn.lock().unwrap();

        // ノートオン
        if program < 0x80 {
            conn_out
                .send(&[MIDIMSG_PROGRAM_CHANGE | MIDI_PREVIEW_CHANNEL, program])
                .unwrap();
            conn_out
                .send(&[MIDIMSG_NOTE_ON | MIDI_PREVIEW_CHANNEL, note, velocity])
                .unwrap();
        } else {
            // ドラム音色
            conn_out
                .send(&[MIDIMSG_NOTE_ON | 0x9, program - 0x80, velocity])
                .unwrap();
        }

        // プレビュー時間流す
        thread::sleep(Duration::from_millis(MIDI_PREVIEW_DURATION_MSEC));

        // ノートオフ
        if program < 0x80 {
            conn_out
                .send(&[MIDIMSG_NOTE_OFF | MIDI_PREVIEW_CHANNEL, note, 0])
                .unwrap();
        } else {
            // ドラム音色
            conn_out
                .send(&[MIDIMSG_NOTE_OFF | 0x9, program - 0x80, 0])
                .unwrap();
        }
    }

    // 音源パラメータをDSPに適用
    fn apply_source_parameter(&mut self) {
        if let Some(midi_spc_ref) = &self.midi_spc {
            let midi_spc = midi_spc_ref.clone();
            let config = self.midi_output_configure.read().unwrap();
            let params = self.source_parameter.read().unwrap();
            let mut midispc = midi_spc.lock().unwrap();
            apply_source_parameter(
                &mut midispc,
                &config,
                &params,
                &self.spc_file.as_ref().unwrap().ram,
            );
        }
    }
}

/// 音源パラメータをDSPに適用
fn apply_source_parameter(
    spc: &mut spc700::spc::SPC<spc700::mididsp::MIDIDSP>,
    config: &MIDIOutputConfigure,
    source_params: &BTreeMap<u8, SourceParameter>,
    ram: &[u8],
) {
    // 音源に依存するパラメータ
    for (srn_no, param) in source_params.iter() {
        spc.dsp.write_register(ram, DSP_ADDRESS_SRN_TARGET, *srn_no);
        let mut flag = 0;
        if param.mute {
            flag |= 0x80;
        }
        if param.envelope_as_expression {
            flag |= 0x40;
        }
        if param.enable_pitch_bend {
            flag |= 0x20;
        }
        if param.echo_as_effect1 {
            flag |= 0x10;
        }
        if param.auto_volume {
            flag |= 0x08;
        }
        if param.auto_pan {
            flag |= 0x04;
        }
        spc.dsp.write_register(ram, DSP_ADDRESS_SRN_FLAG, flag);
        spc.dsp
            .write_register(ram, DSP_ADDRESS_SRN_PROGRAM, param.program.clone() as u8);
        spc.dsp
            .write_register(ram, DSP_ADDRESS_SRN_NOTEON_VELOCITY, param.noteon_velocity);
        spc.dsp.write_register(
            ram,
            DSP_ADDRESS_SRN_CENTER_NOTE,
            (param.center_note >> 8) as u8,
        );
        spc.dsp.write_register(
            ram,
            DSP_ADDRESS_SRN_CENTER_NOTE_FRACTION,
            (param.center_note & 0xFF) as u8,
        );
        spc.dsp
            .write_register(ram, DSP_ADDRESS_SRN_FIXED_VOLUME, param.fixed_volume);
        spc.dsp
            .write_register(ram, DSP_ADDRESS_SRN_FIXED_PAN, param.fixed_pan);
        spc.dsp.write_register(
            ram,
            DSP_ADDRESS_SRN_PITCHBEND_SENSITIVITY,
            param.pitch_bend_width,
        );
    }
    // 音源に依存しないパラメータ
    spc.dsp.write_register(
        ram,
        DSP_ADDRESS_PLAYBACK_PARAMETER_UPDATE_PERIOD,
        config.playback_parameter_update_period,
    );
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Error {
    DialogClosed,
    IoError(io::ErrorKind),
}

async fn open_file() -> Result<(PathBuf, LoadedFile), Error> {
    let picked_file = AsyncFileDialog::new()
        .set_title("Open a file...")
        .add_filter("SPC or JSON", &["spc", "SPC", "json"])
        .pick_file()
        .await
        .ok_or(Error::DialogClosed)?;

    load_file(picked_file).await
}

async fn load_file(path: impl Into<PathBuf>) -> Result<(PathBuf, LoadedFile), Error> {
    let path = path.into();

    if let Some(extension) = path.extension().and_then(OsStr::to_str) {
        match extension.to_lowercase().as_str() {
            "spc" => {
                let data = std::fs::read(&path).unwrap();
                return Ok((path, LoadedFile::SPCFile(data.to_vec())));
            }
            "json" => {
                let string = std::fs::read_to_string(&path).unwrap();
                return Ok((path, LoadedFile::JSONFile(string)));
            }
            _ => {
                return Err(Error::IoError(io::ErrorKind::Unsupported));
            }
        }
    }

    return Err(Error::IoError(io::ErrorKind::Unsupported));
}

async fn save_smf(default_file_name: String, smf: SMF) -> Result<(), Error> {
    let picked_file = AsyncFileDialog::new()
        .set_file_name(default_file_name)
        .set_title("Save to a MIDI file...")
        .add_filter("SMF", &["mid", "midi", "MID"])
        .save_file()
        .await
        .ok_or(Error::DialogClosed)?;

    let writer = SMFWriter::from_smf(smf);
    match writer.write_to_file(picked_file.path()) {
        Ok(()) => Ok(()),
        _ => Err(Error::DialogClosed),
    }
}

async fn save_json(default_file_name: String, json: serde_json::Value) -> Result<(), Error> {
    let picked_file = AsyncFileDialog::new()
        .set_file_name(default_file_name)
        .set_title("Save to a JSON file...")
        .add_filter("JSON", &["json"])
        .save_file()
        .await
        .ok_or(Error::DialogClosed)?;

    match File::create(picked_file.path()) {
        Ok(file) => {
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &json).expect("Faied to write json");
            Ok(())
        }
        _ => Err(Error::DialogClosed),
    }
}

// 再生情報の読み取り
fn read_playback_status(midi_dsp: &spc700::mididsp::MIDIDSP) -> PlaybackStatus {
    let mut status = PlaybackStatus::new();

    let noteon_flags = midi_dsp.read_register(&[0u8], DSP_ADDRESS_NOTEON);
    for ch in 0..8 {
        let ch_nibble = (ch as u8) << 4;
        status.noteon[ch] = ((noteon_flags >> ch) & 1) != 0;
        status.srn_no[ch] = midi_dsp.read_register(&[0u8], DSP_ADDRESS_V0SRCN | ch_nibble);
        let pitch_high = midi_dsp.read_register(&[0u8], DSP_ADDRESS_V0PITCHH | ch_nibble);
        let pitch_low = midi_dsp.read_register(&[0u8], DSP_ADDRESS_V0PITCHL | ch_nibble);
        status.pitch[ch] = ((pitch_high as u16) << 8) | (pitch_low as u16);
        status.envelope[ch] = midi_dsp.read_register(&[0u8], DSP_ADDRESS_V0ENVX | ch_nibble);
        status.volume[ch][0] = midi_dsp.read_register(&[0u8], DSP_ADDRESS_V0VOLL | ch_nibble) as i8;
        status.volume[ch][1] = midi_dsp.read_register(&[0u8], DSP_ADDRESS_V0VOLR | ch_nibble) as i8;
    }

    status
}
