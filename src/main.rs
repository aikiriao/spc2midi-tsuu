mod center_note_estimation;
mod program;

use crate::center_note_estimation::*;
use crate::program::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, PauseStreamError, PlayStreamError, Stream, StreamConfig};
use fixed_resample::ReadStatus;
use iced::border::Radius;
use iced::keyboard::key::Named;
use iced::widget::canvas::{self, stroke, Cache, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::{
    button, center, center_x, checkbox, column, combo_box, container, operation, pick_list, row,
    scrollable, slider, space, stack, text, text_input, toggler, tooltip, vertical_slider, Column,
    Space, Stack,
};
use iced::{
    alignment, event, mouse, theme, time, window, Border, Color, Element, Font, Function, Length,
    Padding, Point, Rectangle, Renderer, Size, Subscription, Task, Theme, Vector,
};
use iced_aw::menu::{self, Item, Menu};
use iced_aw::style::{menu_bar::primary, Status};
use iced_aw::{iced_aw_font, menu_bar, menu_items, number_input, ICED_AW_FONT_BYTES};
use iced_aw::{quad, widgets::InnerBounds};
use midir::{MidiOutput, MidiOutputConnection, MidiOutputPort};
use num_traits::pow::Pow;
use rfd::AsyncFileDialog;
use rimd::{Event as MidiEvent, MidiMessage, SMFFormat, SMFWriter, Track, TrackEvent, SMF};
use samplerate::{convert, ConverterType};
use std::any::Any;
use std::collections::BTreeMap;
use std::ffi::OsStr;
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
const SPC2MIDI2_TITLE_STR: &'static str = "spc2midi-tsuu";
/// SPCの出力サンプリングレート
const SPC_SAMPLING_RATE: u32 = 32000;
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
/// デフォルトのMIDIファイル出力時間(sec)
const DEFAULT_OUTPUT_DURATION_MSEC: u64 = 60 * 1000;

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
    FileOpened(Result<(PathBuf, Vec<u8>), Error>),
    SaveSMF,
    SMFSaved(Result<(), Error>),
    MenuSelected,
    EventOccurred(iced::Event),
    ReceivedSRNPlayStartRequest(u8, bool),
    SRNPlayLoopFlagToggled(window::Id, bool),
    SRNMIDIPreviewFlagToggled(window::Id, bool),
    ReceivedPlayStartRequest,
    ReceivedPlayStopRequest,
    SPCMuteFlagToggled(bool),
    MIDIMuteFlagToggled(bool),
    ProgramSelected(window::Id, Program),
    ReceivedMIDIPreviewRequest(u8),
    CenterNoteIntChanged(window::Id, u8),
    CenterNoteFractionChanged(window::Id, f32),
    NoteOnVelocityChanged(window::Id, u8),
    PitchBendWidthChanged(window::Id, u8),
    EnablePitchBendFlagToggled(window::Id, bool),
    AutoPanFlagToggled(window::Id, bool),
    FixedPanChanged(window::Id, u8),
    AutoVolumeFlagToggled(window::Id, bool),
    FixedVolumeChanged(window::Id, u8),
    EnvelopeAsExpressionFlagToggled(window::Id, bool),
    EchoAsEffect1FlagToggled(window::Id, bool),
    ReceivedSourceParameterUpdate,
    AudioOutputDeviceSelected(window::Id, String),
    MIDIOutputPortSelected(window::Id, String),
    MIDIOutputBpmChanged(window::Id, u8),
    MIDIOutputTicksPerQuarterChanged(window::Id, u16),
    MIDIOutputUpdatePeriodChanged(window::Id, u8),
    MIDIOutputDurationChanged(window::Id, u64),
    Tick,
}

trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

trait SPC2MIDI2Window: AsAny {
    fn title(&self) -> String;
    fn view(&self) -> Element<'_, Message>;
}

/// 音源情報
#[derive(Debug, Clone)]
struct SourceInformation {
    /// デコードした信号
    signal: Vec<f32>,
    /// 開始アドレス
    start_address: usize,
    /// 終端アドレス
    end_address: usize,
    /// ループ開始サンプル
    loop_start_sample: usize,
}

/// 1音源のパラメータ
#[derive(Debug, Clone)]
struct SourceParameter {
    /// プログラム番号
    program: Program,
    /// 基準ノート（8bit整数・8bit小数部）
    center_note: u16,
    /// ノートオンベロシティ
    noteon_velocity: u8,
    /// ピッチベンド幅（半音単位）
    pitchbend_width: u8,
    /// エンベロープをエクスプレッションとして出力するか
    envelope_as_expression: bool,
    /// パンを発音中に更新するか
    auto_pan: bool,
    /// パン値
    fixed_pan: u8,
    /// ボリュームを発音中に更新するか
    auto_volume: bool,
    /// ボリューム値
    fixed_volume: u8,
    /// ピッチベンドを使うか
    enable_pitch_bend: bool,
    /// エコーをエフェクト1デプスとして出力するか
    echo_as_effect1: bool,
    /// MIDIプレビューを行うか
    enable_midi_preview: bool,
}

/// 再生中の状態
#[derive(Debug, Clone)]
struct PlaybackStatus {
    /// ノートオン中か
    noteon: [bool; 8],
    /// 再生しているソース番号
    srn_no: [u8; 8],
    /// 再生ピッチ
    pitch: [u16; 8],
    /// エクスプレッション
    expression: [u8; 8],
}

/// MIDI出力設定
#[derive(Debug, Clone)]
struct MIDIOutputConfigure {
    /// 出力時間(ms)
    output_duration_msec: u64,
    /// MIDI再生パラメータ更新周期
    playback_parameter_update_period: u8,
    /// BPM
    beats_per_minute: u8,
    /// 四分の一音符当たりのティック数
    ticks_per_quarter: u16,
}

#[derive(Debug)]
struct MainWindow {
    title: String,
    source_infos: Arc<RwLock<BTreeMap<u8, SourceInformation>>>,
    source_params: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    playback_status: Arc<RwLock<PlaybackStatus>>,
    pcm_spc_mute: bool,
    midi_spc_mute: bool,
    playback_time_sec: f32,
}

#[derive(Debug)]
struct PreferenceWindow {
    title: String,
    window_id: window::Id,
    audio_out_device_name: Option<String>,
    audio_out_devices_box: combo_box::State<String>,
    midi_out_port_name: Option<String>,
    midi_ports_box: combo_box::State<String>,
    playback_parameter_update_period: u8,
    beats_per_minute: u8,
    ticks_per_quarter: Option<u16>,
    ticks_per_quarter_box: combo_box::State<u16>,
    output_duration_msec: u64,
}

#[derive(Debug)]
struct SRNWindow {
    title: String,
    window_id: window::Id,
    srn_no: u8,
    source_info: Arc<SourceInformation>,
    enable_loop_play: bool,
    enable_midi_preview: bool,
    cache: Cache,
    program: Option<Program>,
    program_box: combo_box::State<Program>,
    center_note_int: u8,
    center_note_fraction: f32,
    noteon_velocity: u8,
    pitchbend_width: u8,
    envelope_as_expression: bool,
    auto_pan: bool,
    fixed_pan: u8,
    auto_volume: bool,
    fixed_volume: u8,
    enable_pitch_bend: bool,
    echo_as_effect1: bool,
}

struct App {
    theme: iced::Theme,
    main_window_id: window::Id,
    windows: BTreeMap<window::Id, Box<dyn SPC2MIDI2Window>>,
    spc_file: Option<Box<SPCFile>>,
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
    audio_out_device_name: Option<String>,
    midi_out_port_name: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no output device available");
        let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).unwrap();
        let midi_out_ports = midi_out.ports();
        let midi_out_port_name = if midi_out_ports.len() > 0 {
            Some(midi_out.port_name(&midi_out_ports[0]).unwrap())
        } else {
            None
        };
        Self {
            theme: iced::Theme::Nord,
            main_window_id: window::Id::unique(),
            windows: BTreeMap::new(),
            spc_file: None,
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
            audio_out_device_name: Some(
                device
                    .description()
                    .expect("Failed to get device name")
                    .to_string(),
            ),
            midi_out_port_name: midi_out_port_name,
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
                    SPC2MIDI2_TITLE_STR.to_string(),
                    self.source_infos.clone(),
                    self.source_parameter.clone(),
                    self.playback_status.clone(),
                );
                self.main_window_id = id;
                self.windows.insert(id, Box::new(window));
                return open.map(Message::MainWindowOpened);
            }
            Message::MainWindowOpened(id) => {}
            Message::OpenPreferenceWindow => {
                let (id, open) = window::open(window::Settings {
                    size: iced::Size::new(500.0, 500.0),
                    ..Default::default()
                });
                self.windows.insert(
                    id,
                    Box::new(PreferenceWindow::new(
                        id,
                        "Preference".to_string(),
                        self.audio_out_device_name.clone(),
                        self.midi_out_port_name.clone(),
                        self.midi_output_configure.clone(),
                    )),
                );
                return open.map(Message::PreferenceWindowOpened);
            }
            Message::PreferenceWindowOpened(id) => {}
            Message::OpenSRNWindow(srn_no) => {
                let (id, open) = window::open(window::Settings {
                    size: iced::Size::new(800.0, 600.0),
                    ..Default::default()
                });
                let infos = self.source_infos.read().unwrap();
                if let Some(source) = infos.get(&srn_no) {
                    let params = self.source_parameter.read().unwrap();
                    let param = params.get(&srn_no).unwrap();
                    let window =
                        SRNWindow::new(id, format!("SRN 0x{:02X}", srn_no), srn_no, source, param);
                    self.windows.insert(id, Box::new(window));
                    return open.map(Message::SRNWindowOpened);
                }
            }
            Message::SRNWindowOpened(id) => {}
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
                                SPC2MIDI2_TITLE_STR,
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
                    }
                }
                Err(e) => {
                    eprintln!("ERROR: failed to open wav file: {:?}", e);
                }
            },
            Message::SaveSMF => {
                if let Some(smf) = self.create_smf() {
                    return Task::perform(save_smf(smf), Message::SMFSaved);
                }
            }
            Message::SMFSaved(result) => {}
            Message::MenuSelected => {}
            Message::EventOccurred(event) => match event {
                iced::event::Event::Window(event) => {
                    if let iced::window::Event::FileDropped(path) = event {
                        return Task::perform(load_file(path), Message::FileOpened);
                    }
                }
                _ => {}
            },
            Message::ReceivedSRNPlayStartRequest(srn_no, loop_flag) => {
                if self.stream_is_playing.load(Ordering::Relaxed) {
                    // 再生中の場合は止める
                    self.stream_play_stop().expect("Failed to stop play");
                } else {
                    // 新規再生処理
                    if let Err(_) = self.srn_play_start(srn_no, loop_flag) {
                        eprintln!("[{}] Faild to start playback", SPC2MIDI2_TITLE_STR);
                    }
                }
            }
            Message::SRNPlayLoopFlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    // ダウンキャストしてSRNWindowを引っ張り出し変更
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    srn_win.enable_loop_play = flag;
                }
            }
            Message::SRNMIDIPreviewFlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.enable_midi_preview = flag;
                        param.enable_midi_preview = flag;
                    }
                }
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
                if let Some(window) = self.windows.get_mut(&self.main_window_id) {
                    // トグルスイッチの値を書き換え
                    let main_window: &mut MainWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    main_window.pcm_spc_mute = flag;
                    // フラグ書き換え
                    self.pcm_spc_mute.clone().store(flag, Ordering::Relaxed);
                }
            }
            Message::MIDIMuteFlagToggled(flag) => {
                if let Some(window) = self.windows.get_mut(&self.main_window_id) {
                    // トグルスイッチの値を書き換え
                    let main_window: &mut MainWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    main_window.midi_spc_mute = flag;
                    // フラグ書き換え
                    self.midi_spc_mute.clone().store(flag, Ordering::Relaxed);
                }
            }
            Message::ProgramSelected(id, program) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        param.program = program.clone();
                    }
                    srn_win.program = Some(program);
                    let mut tasks = vec![];
                    if srn_win.enable_midi_preview {
                        let srn_no = srn_win.srn_no;
                        tasks.push(Task::perform(async {}, move |_| {
                            Message::ReceivedMIDIPreviewRequest(srn_no)
                        }));
                    }
                    tasks.push(Task::perform(async {}, move |_| {
                        Message::ReceivedSourceParameterUpdate
                    }));
                    return Task::batch(tasks);
                }
            }
            Message::CenterNoteIntChanged(id, note) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.center_note_int = note;
                        param.center_note = (param.center_note & 0x00FF) | ((note as u16) << 8);
                        let mut tasks = vec![];
                        if srn_win.enable_midi_preview {
                            let srn_no = srn_win.srn_no;
                            tasks.push(Task::perform(async {}, move |_| {
                                Message::ReceivedMIDIPreviewRequest(srn_no)
                            }));
                        }
                        tasks.push(Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        }));
                        return Task::batch(tasks);
                    }
                }
            }
            Message::CenterNoteFractionChanged(id, fraction) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        let clamped_fraction = f32::round(fraction * 256.0).clamp(0.0, 255.0);
                        srn_win.center_note_fraction = clamped_fraction / 256.0;
                        param.center_note =
                            (param.center_note & 0xFF00) | (clamped_fraction as u16);
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::NoteOnVelocityChanged(id, velocity) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.noteon_velocity = velocity;
                        param.noteon_velocity = srn_win.noteon_velocity;
                        if srn_win.enable_midi_preview {
                            let srn_no = srn_win.srn_no;
                            return Task::perform(async {}, move |_| {
                                Message::ReceivedMIDIPreviewRequest(srn_no)
                            });
                        }
                    }
                }
            }
            Message::PitchBendWidthChanged(id, width) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.pitchbend_width = width;
                        param.pitchbend_width = srn_win.pitchbend_width;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::EnablePitchBendFlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.enable_pitch_bend = flag;
                        param.enable_pitch_bend = flag;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::AutoPanFlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.auto_pan = flag;
                        param.auto_pan = flag;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::FixedPanChanged(id, pan) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.fixed_pan = pan;
                        param.fixed_pan = pan;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::AutoVolumeFlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.auto_volume = flag;
                        param.auto_volume = flag;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::FixedVolumeChanged(id, volume) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.fixed_volume = volume;
                        param.fixed_volume = volume;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::EnvelopeAsExpressionFlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.envelope_as_expression = flag;
                        param.envelope_as_expression = flag;
                        return Task::perform(async {}, move |_| {
                            Message::ReceivedSourceParameterUpdate
                        });
                    }
                }
            }
            Message::EchoAsEffect1FlagToggled(id, flag) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let srn_win: &mut SRNWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut params = self.source_parameter.write().unwrap();
                    if let Some(param) = params.get_mut(&srn_win.srn_no) {
                        srn_win.echo_as_effect1 = flag;
                        param.echo_as_effect1 = flag;
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
            Message::AudioOutputDeviceSelected(id, device_name) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let pref_win: &mut PreferenceWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    pref_win.audio_out_device_name = Some(device_name.clone());
                    self.audio_out_device_name = Some(device_name.clone());
                    // オーディオ出力デバイスを再構築
                    let mut devices = cpal::default_host()
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
            }
            Message::MIDIOutputPortSelected(id, port_name) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let pref_win: &mut PreferenceWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    pref_win.midi_out_port_name = Some(port_name.clone());
                    self.midi_out_port_name = Some(port_name.clone());
                    // MIDI出力ポートを再接続
                    let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).unwrap();
                    let ports = midi_out.ports();
                    // 選択したポート名を探す
                    let mut i = 0;
                    while i < ports.len() {
                        if port_name == midi_out.port_name(&ports[i]).unwrap() {
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
            }
            Message::MIDIOutputBpmChanged(id, bpm) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let pref_win: &mut PreferenceWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut config = self.midi_output_configure.write().unwrap();
                    pref_win.beats_per_minute = bpm;
                    config.beats_per_minute = bpm;
                }
            }
            Message::MIDIOutputTicksPerQuarterChanged(id, ticks) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let pref_win: &mut PreferenceWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut config = self.midi_output_configure.write().unwrap();
                    pref_win.ticks_per_quarter = Some(ticks);
                    config.ticks_per_quarter = ticks;
                }
            }
            Message::MIDIOutputUpdatePeriodChanged(id, period) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let pref_win: &mut PreferenceWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut config = self.midi_output_configure.write().unwrap();
                    pref_win.playback_parameter_update_period = period;
                    config.playback_parameter_update_period = period;
                }
            }
            Message::MIDIOutputDurationChanged(id, duration) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let pref_win: &mut PreferenceWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let mut config = self.midi_output_configure.write().unwrap();
                    pref_win.output_duration_msec = duration;
                    config.output_duration_msec = duration;
                }
            }
            Message::Tick => {
                if let Some(window) = self.windows.get_mut(&self.main_window_id) {
                    let main_win: &mut MainWindow =
                        window.as_mut().as_any_mut().downcast_mut().unwrap();
                    let played_samples = self.stream_played_samples.load(Ordering::Relaxed);
                    main_win.playback_time_sec =
                        played_samples as f32 / self.stream_config.sample_rate as f32;
                }

                if let Some(midi_spc_ref) = &self.midi_spc {
                    let midi_spc = midi_spc_ref.clone();
                    let spc = midi_spc.lock().unwrap();
                    let mut status = self.playback_status.write().unwrap();
                    *status = read_playback_status(&spc.dsp);
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
        let mut spc: spc700::spc::SPC<spc700::mididsp::MIDIDSP> =
            SPC::new(&register, ram, dsp_register);
        let mut cycle_count = 0;
        let mut tick64khz_count = 0;
        let mut start_address_map = BTreeMap::new();
        while tick64khz_count < analyze_duration_64khz_ticks {
            cycle_count += spc.execute_step() as u32;
            if cycle_count >= CLOCK_TICK_CYCLE_64KHZ {
                spc.clock_tick_64k_hz();
                cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
                tick64khz_count += 1;
            }
            // キーオンが打たれていた時のサンプル番号を取得
            let keyon = spc.dsp.read_register(ram, DSP_ADDRESS_KON);
            if keyon != 0 {
                let brr_dir_base_address =
                    (spc.dsp.read_register(ram, DSP_ADDRESS_DIR) as u16) << 8;
                for ch in 0..8 {
                    if (keyon >> ch) & 1 != 0 {
                        let sample_source =
                            spc.dsp.read_register(ram, (ch << 4) | DSP_ADDRESS_V0SRCN);
                        let dir_address =
                            (brr_dir_base_address + 4 * (sample_source as u16)) as usize;
                        start_address_map.insert(sample_source, dir_address);
                    }
                }
            }
        }

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
            infos.insert(
                *srn,
                SourceInformation {
                    signal: signal.clone(),
                    start_address: start_address,
                    end_address: start_address + (signal.len() * 9) / 16,
                    loop_start_sample: ((loop_address - start_address) * 16) / 9,
                },
            );
            // 推定ピッチ
            let center_note = center_note_estimation(&signal);
            params.insert(
                *srn,
                SourceParameter {
                    program: Program::AcousticGrand,
                    center_note: f32::round(center_note * 256.0) as u16,
                    noteon_velocity: 100,
                    pitchbend_width: 12,
                    envelope_as_expression: true,
                    auto_pan: true,
                    fixed_pan: 64,
                    auto_volume: false,
                    fixed_volume: 100,
                    enable_pitch_bend: true,
                    echo_as_effect1: true,
                    enable_midi_preview: true,
                },
            );
        }
    }

    // SMFを作成
    fn create_smf(&self) -> Option<SMF> {
        if let Some(spc_file) = &self.spc_file {
            let config = self.midi_output_configure.read().unwrap();
            let ticks_per_minutes =
                (config.beats_per_minute as f64) * (config.ticks_per_quarter as f64);
            let mut smf = SMF {
                format: SMFFormat::Single,
                tracks: vec![Track {
                    copyright: Some("tmp".to_string()), // TODO: SPCから出す or ユーザが設定した時間出力
                    name: Some("tmp".to_string()), // TODO: SPCから出す or ユーザが設定した時間出力
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
            let configure = self.midi_output_configure.read().unwrap();
            let params = self.source_parameter.read().unwrap();
            apply_source_parameter(&mut spc, &configure, &params, &spc_file.ram);

            let mut cycle_count = 0;
            let mut total_elapsed_time_nanosec = 0;
            let mut previous_event_time = 0.0;

            // 出力で決めた時間だけ出力
            while total_elapsed_time_nanosec < config.output_duration_msec * 1000_000 {
                // 64kHzタイマーティックするまで処理
                while cycle_count < CLOCK_TICK_CYCLE_64KHZ {
                    cycle_count += spc.execute_step() as u32;
                }
                cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
                // MIDI出力
                if let Some(out) = spc.clock_tick_64k_hz() {
                    // 経過時間からティック数を計算
                    let delta_nano_time = total_elapsed_time_nanosec as f64 - previous_event_time;
                    let ticks = (delta_nano_time * ticks_per_minutes) / (60.0 * 1000_000_000.0);
                    // ティック数は切り捨てる（切り上げると経過時間が未来になって経過時間が負になりうる）
                    for i in 0..out.num_messages {
                        let msg = out.messages[i];
                        smf.tracks[0].events.push(TrackEvent {
                            vtime: if i == 0 { f64::floor(ticks) as u64 } else { 0 },
                            event: MidiEvent::Midi(MidiMessage {
                                data: msg.data[..msg.length].to_vec(),
                            }),
                        });
                    }
                    // 実際のtickから経過時間計算
                    previous_event_time +=
                        (ticks.floor() * 60.0 * 1000_000_000.0) / ticks_per_minutes;
                }
                // 時間を進める
                total_elapsed_time_nanosec += CLOCK_TICK_CYCLE_64KHZ_NANOSEC;
            }

            Some(smf)
        } else {
            None
        }
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

        // 各SPCのミュートフラグ取得
        let pcm_spc_mute = self.pcm_spc_mute.clone();
        let midi_spc_mute = self.midi_spc_mute.clone();

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
                            if !midi_spc_mute.load(Ordering::Relaxed) {
                                for i in 0..msgs.num_messages {
                                    let msg = msgs.messages[i];
                                    conn_out.send(&msg.data[..msg.length]).unwrap();
                                }
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

    // 再生開始
    fn srn_play_start(&mut self, srn_no: u8, loop_flag: bool) -> Result<(), PlayStreamError> {
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
                    if loop_flag {
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

    // 再生停止
    fn stream_play_stop(&mut self) -> Result<(), PauseStreamError> {
        if let Some(stream) = &self.stream {
            self.stream_is_playing.store(false, Ordering::Relaxed);
            stream.pause()?;
            self.stream = None;
        }
        // MIDIにオールサウンドオフを送信
        if let Some(midi_out_conn_ref) = &self.midi_out_conn {
            let midi_out_conn = midi_out_conn_ref.clone();
            let mut conn_out = midi_out_conn.lock().unwrap();
            for ch in 0..15 {
                conn_out
                    .send(&[MIDIMSG_MODE | ch, MIDIMSG_MODE_ALL_SOUND_OFF, 0])
                    .unwrap();
            }
        }
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
        if param.envelope_as_expression {
            flag |= 0x80;
        }
        if param.enable_pitch_bend {
            flag |= 0x40;
        }
        if param.echo_as_effect1 {
            flag |= 0x20;
        }
        if param.auto_volume {
            flag |= 0x10;
        }
        if param.auto_pan {
            flag |= 0x08;
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
            param.pitchbend_width,
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

async fn open_file() -> Result<(PathBuf, Vec<u8>), Error> {
    let picked_file = AsyncFileDialog::new()
        .set_title("Open a file...")
        .add_filter("SPC", &["spc", "SPC"])
        .pick_file()
        .await
        .ok_or(Error::DialogClosed)?;

    load_file(picked_file).await
}

async fn load_file(path: impl Into<PathBuf>) -> Result<(PathBuf, Vec<u8>), Error> {
    let path = path.into();

    if let Some(extension) = path.extension().and_then(OsStr::to_str) {
        match extension.to_lowercase().as_str() {
            "spc" => {
                let data = std::fs::read(&path).unwrap();
                return Ok((path, data.to_vec()));
            }
            _ => {
                return Err(Error::IoError(io::ErrorKind::Unsupported));
            }
        }
    }

    return Err(Error::IoError(io::ErrorKind::Unsupported));
}

async fn save_smf(smf: SMF) -> Result<(), Error> {
    let picked_file = AsyncFileDialog::new()
        .set_file_name("output.mid")
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

fn menu_button<'a>(
    content: impl Into<Element<'a, Message>>,
    msg: Message,
) -> button::Button<'a, Message> {
    button(content)
        .padding([4, 8])
        .style(|theme, status| {
            use iced_widget::button::{Status, Style};

            let palette = theme.extended_palette();
            let base = Style {
                text_color: palette.background.base.text,
                border: Border::default().rounded(6.0),
                ..Style::default()
            };
            match status {
                Status::Active => base.with_background(Color::TRANSPARENT),
                Status::Hovered => base.with_background(Color::from_rgb(
                    palette.primary.weak.color.r * 1.2,
                    palette.primary.weak.color.g * 1.2,
                    palette.primary.weak.color.b * 1.2,
                )),
                Status::Disabled => base.with_background(Color::from_rgb(0.5, 0.5, 0.5)),
                Status::Pressed => base.with_background(palette.primary.weak.color),
            }
        })
        .on_press(msg)
}

impl SPC2MIDI2Window for MainWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let menu_tuple = |items| Menu::new(items).width(180.0).offset(15.0).spacing(5.0);

        let menu_bar = menu_bar!(
            (
                menu_button(
                    text("File")
                        .height(Length::Shrink)
                        .align_y(alignment::Vertical::Center),
                    Message::MenuSelected,
                )
                .width(Length::Shrink)
                .height(Length::Shrink),
                {
                    menu_tuple(menu_items!(
                        (menu_button(
                            text("Open file...")
                                .height(Length::Shrink)
                                .align_y(alignment::Vertical::Center),
                            Message::OpenFile,
                        )
                        .width(Length::Fill)
                        .height(Length::Shrink)),
                        (menu_button(
                            text("Save SMF...")
                                .height(Length::Shrink)
                                .align_y(alignment::Vertical::Center),
                            Message::SaveSMF,
                        )
                        .width(Length::Fill)
                        .height(Length::Shrink)),
                    ))
                    .width(140.0)
                }
            ),
            (
                menu_button(
                    text("Option")
                        .height(Length::Shrink)
                        .align_y(alignment::Vertical::Center),
                    Message::MenuSelected,
                )
                .width(Length::Shrink)
                .height(Length::Shrink),
                {
                    menu_tuple(menu_items!(
                        (menu_button(
                            text("Preferences...")
                                .height(Length::Shrink)
                                .align_y(alignment::Vertical::Center),
                            Message::OpenPreferenceWindow,
                        )
                        .width(Length::Fill)
                        .height(Length::Shrink)),
                    ))
                    .width(140.0)
                }
            ),
        )
        .draw_path(menu::DrawPath::Backdrop)
        .close_on_item_click_global(true)
        .close_on_background_click_global(true)
        .padding(Padding::new(5.0))
        .style(|theme: &iced::Theme, status: Status| menu::Style {
            path_border: Border {
                radius: Radius::new(0.0),
                ..Default::default()
            },
            path: Color::from_rgb(
                theme.extended_palette().primary.weak.color.r * 1.2,
                theme.extended_palette().primary.weak.color.g * 1.2,
                theme.extended_palette().primary.weak.color.b * 1.2,
            )
            .into(),
            ..primary(theme, status)
        });

        let infos = self.source_infos.read().unwrap();
        let params = self.source_params.read().unwrap();
        let srn_list: Vec<_> = infos
            .iter()
            .map(|(key, info)| {
                row![
                    text(format!("0x{:02X}", key)),
                    {
                        let param = params.get(&key).unwrap();
                        text(format!("{} {}", param.program, param.center_note >> 8))
                    },
                    button("Configure").on_press(Message::OpenSRNWindow(*key))
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();

        let status = self.playback_status.read().unwrap();
        let status_list: Vec<_> = (0..8)
            .map(|ch| {
                row![
                    text(format!("{}", ch)),
                    text(format!("0x{:02X}", status.srn_no[ch])),
                    text(format!("{}", if status.noteon[ch] { "ON " } else { "OFF" })),
                    text(if status.pitch[ch] > 0 {
                        format!(
                            "{:+7.2}",
                            12.0 * f32::log2((status.pitch[ch] as f32) / (0x1000 as f32))
                        )
                    } else {
                        format!("")
                    }),
                    text(format!("{:3}", status.expression[ch])),
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();

        let preview_control = row![
            button("Play / Pause").on_press(Message::ReceivedPlayStartRequest),
            button("Stop").on_press(Message::ReceivedPlayStopRequest),
            checkbox(self.pcm_spc_mute)
                .label("SPC Mute")
                .on_toggle(|flag| Message::SPCMuteFlagToggled(flag)),
            checkbox(self.midi_spc_mute)
                .label("MIDI Mute")
                .on_toggle(|flag| Message::MIDIMuteFlagToggled(flag)),
            text(format!("{:8.2} sec", self.playback_time_sec)),
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);

        let r = row![menu_bar, space::horizontal().width(Length::Fill),]
            .align_y(alignment::Alignment::Center);

        let c = column![
            r,
            scrollable(
                Column::from_vec(srn_list)
                    .width(Length::Fill)
                    .height(Length::Fill)
            )
            .width(Length::Fill)
            .height(Length::Fill),
            Column::from_vec(status_list).width(Length::Fill),
            preview_control,
        ];

        c.into()
    }
}

impl MainWindow {
    fn new(
        title: String,
        source_info: Arc<RwLock<BTreeMap<u8, SourceInformation>>>,
        source_params: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
        playback_status: Arc<RwLock<PlaybackStatus>>,
    ) -> Self {
        Self {
            title: title,
            source_infos: source_info,
            source_params: source_params,
            playback_status: playback_status,
            pcm_spc_mute: false,
            midi_spc_mute: false,
            playback_time_sec: 0.0f32,
        }
    }
}

impl SPC2MIDI2Window for PreferenceWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let window_id = self.window_id;
        let content = column![
            combo_box(
                &self.audio_out_devices_box,
                "Audio Output port",
                self.audio_out_device_name.as_ref(),
                move |device_name| Message::AudioOutputDeviceSelected(window_id, device_name),
            ),
            combo_box(
                &self.midi_ports_box,
                "MIDI Output port",
                self.midi_out_port_name.as_ref(),
                move |port_name| Message::MIDIOutputPortSelected(window_id, port_name),
            ),
            number_input(&self.beats_per_minute, 32..=240, move |bpm| {
                Message::MIDIOutputBpmChanged(window_id, bpm)
            },)
            .step(1),
            combo_box(
                &self.ticks_per_quarter_box,
                "Ticks per quarter (resolution)",
                self.ticks_per_quarter.as_ref(),
                move |ticks| { Message::MIDIOutputTicksPerQuarterChanged(window_id, ticks) },
            ),
            number_input(
                &self.playback_parameter_update_period,
                1..=255,
                move |period| { Message::MIDIOutputUpdatePeriodChanged(window_id, period) },
            )
            .step(1),
            number_input(
                &self.output_duration_msec,
                1000..=(3600 * 1000),
                move |duration| { Message::MIDIOutputDurationChanged(window_id, duration) },
            )
            .step(1),
        ]
        .spacing(10)
        .padding(10)
        .width(Length::Fill)
        .align_x(alignment::Alignment::Center);
        content.into()
    }
}

impl PreferenceWindow {
    fn new(
        window_id: window::Id,
        title: String,
        audio_out_device_name: Option<String>,
        midi_out_port_name: Option<String>,
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
        let config = midi_output_configure.read().unwrap();
        Self {
            title: title,
            window_id: window_id,
            audio_out_device_name: audio_out_device_name,
            audio_out_devices_box: combo_box::State::new(device_name_list),
            midi_out_port_name: midi_out_port_name,
            midi_ports_box: combo_box::State::new(port_name_list),
            beats_per_minute: config.beats_per_minute,
            playback_parameter_update_period: config.playback_parameter_update_period,
            output_duration_msec: config.output_duration_msec,
            ticks_per_quarter: Some(config.ticks_per_quarter),
            ticks_per_quarter_box: combo_box::State::new(vec![
                24, 30, 48, 60, 96, 120, 192, 240, 384, 480, 960,
            ]),
        }
    }
}

impl SPC2MIDI2Window for SRNWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let window_id = self.window_id;
        let content = column![
            Canvas::new(self).width(Length::Fill).height(200),
            row![
                button("Play / Pause").on_press(Message::ReceivedSRNPlayStartRequest(
                    self.srn_no,
                    self.enable_loop_play
                )),
                checkbox(self.enable_loop_play)
                    .label("Loop")
                    .on_toggle(|flag| Message::SRNPlayLoopFlagToggled(self.window_id, flag)),
                checkbox(self.enable_midi_preview)
                    .label("MIDI Preview")
                    .on_toggle(|flag| Message::SRNMIDIPreviewFlagToggled(self.window_id, flag)),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            combo_box(
                &self.program_box,
                "Program",
                self.program.as_ref(),
                move |program| Message::ProgramSelected(window_id, program),
            ),
            row![
                number_input(&self.center_note_int, 0..=127, move |note| {
                    Message::CenterNoteIntChanged(window_id, note)
                },)
                .step(1),
                number_input(&self.center_note_fraction, 0.0..=1.0, move |fraction| {
                    Message::CenterNoteFractionChanged(window_id, fraction)
                },)
                .step(1.0 / 256.0),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            number_input(&self.noteon_velocity, 1..=127, move |velocity| {
                Message::NoteOnVelocityChanged(window_id, velocity)
            },)
            .step(1),
            row![
                checkbox(self.enable_pitch_bend)
                    .label("Pitch Bend")
                    .on_toggle(|flag| Message::EnablePitchBendFlagToggled(self.window_id, flag)),
                number_input(&self.pitchbend_width, 1..=48, move |width| {
                    Message::PitchBendWidthChanged(window_id, width)
                },)
                .step(1),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                checkbox(self.auto_pan)
                    .label("Auto Pan")
                    .on_toggle(|flag| Message::AutoPanFlagToggled(self.window_id, flag)),
                number_input(
                    &self.fixed_pan,
                    if self.auto_pan {
                        self.fixed_pan..=self.fixed_pan
                    } else {
                        0..=127
                    },
                    move |pan| { Message::FixedVolumeChanged(window_id, pan) }
                )
                .step(1),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                checkbox(self.auto_volume)
                    .label("Auto Volume")
                    .on_toggle(|flag| Message::AutoVolumeFlagToggled(self.window_id, flag)),
                number_input(
                    &self.fixed_volume,
                    if self.auto_volume {
                        self.fixed_volume..=self.fixed_volume
                    } else {
                        0..=127
                    },
                    move |volume| { Message::FixedVolumeChanged(window_id, volume) }
                )
                .step(1),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            checkbox(self.envelope_as_expression)
                .label("Envelope as Expression")
                .on_toggle(|flag| Message::EnvelopeAsExpressionFlagToggled(self.window_id, flag)),
            checkbox(self.echo_as_effect1)
                .label("Echo as Effect1")
                .on_toggle(|flag| Message::EchoAsEffect1FlagToggled(self.window_id, flag)),
        ]
        .spacing(10)
        .padding(10)
        .width(Length::Fill)
        .align_x(alignment::Alignment::Center);
        content.into()
    }
}

impl SRNWindow {
    fn new(
        window_id: window::Id,
        title: String,
        srn_no: u8,
        source_info: &SourceInformation,
        source_parameter: &SourceParameter,
    ) -> Self {
        Self {
            window_id: window_id,
            title: title,
            srn_no: srn_no,
            source_info: source_info.clone().into(),
            enable_loop_play: false,
            enable_midi_preview: source_parameter.enable_midi_preview,
            cache: Cache::default(),
            program: Some(source_parameter.program.clone()),
            program_box: combo_box::State::new(Program::ALL.to_vec()),
            center_note_int: (source_parameter.center_note >> 8) as u8,
            center_note_fraction: ((source_parameter.center_note & 0xFF) as f32) / 256.0,
            noteon_velocity: source_parameter.noteon_velocity,
            pitchbend_width: source_parameter.pitchbend_width,
            envelope_as_expression: source_parameter.envelope_as_expression,
            auto_pan: source_parameter.auto_pan,
            fixed_pan: source_parameter.fixed_pan,
            auto_volume: source_parameter.auto_volume,
            fixed_volume: source_parameter.fixed_volume,
            enable_pitch_bend: source_parameter.enable_pitch_bend,
            echo_as_effect1: source_parameter.echo_as_effect1,
        }
    }
}

impl canvas::Program<Message> for SRNWindow {
    type State = Option<()>;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        const TIMELABEL_HEIGHT: f32 = 10.0;
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            // 波形描画
            draw_waveform(
                frame,
                &Rectangle::new(Point::new(0.0, 0.0), Size::new(bounds.width, bounds.height)),
                &self.source_info.signal,
                false,
            );
            // ループポイント描画
            draw_loop_point(
                frame,
                &Rectangle::new(Point::new(0.0, 0.0), Size::new(bounds.width, bounds.height)),
                self.source_info.signal.len(),
                self.source_info.loop_start_sample,
            );
            // 時刻ラベル描画
            draw_timelabel(
                frame,
                &Rectangle::new(
                    Point::new(0.0, bounds.height - TIMELABEL_HEIGHT),
                    Size::new(bounds.width, TIMELABEL_HEIGHT),
                ),
                SPC_SAMPLING_RATE as f32,
                self.source_info.signal.len(),
                16,
            );
        });
        vec![geometry]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<iced_widget::Action<Message>> {
        match event {
            Event::Keyboard(iced::keyboard::Event::KeyReleased {
                key: iced::keyboard::Key::Named(Named::Space),
                ..
            }) => Some(iced_widget::Action::publish(
                Message::ReceivedSRNPlayStartRequest(self.srn_no, self.enable_loop_play),
            )),
            _ => None,
        }
    }
}

/// AsAnyの実装
impl<T> AsAny for T
where
    T: 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// 波形描画
fn draw_waveform(frame: &mut Frame, bounds: &Rectangle, pcm: &[f32], amplitude_normalize: bool) {
    let center = bounds.center();
    let half_height = bounds.height / 2.0;
    let center_left = Point::new(center.x - bounds.width / 2.0, center.y);

    let num_points_to_draw = cmp::min(pcm.len(), 4 * bounds.width as usize); // 描画する点数（それ以外は間引く）
    let sample_stride = pcm.len() as f32 / num_points_to_draw as f32;
    let x_offset_delta = bounds.width / num_points_to_draw as f32;

    // 拡大が有効な場合描画する波形を拡大するため最大絶対値を計算
    let pcm_normalizer = if amplitude_normalize {
        let max_abs_pcm = pcm
            .iter()
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap()
            .abs();
        half_height / max_abs_pcm
    } else {
        half_height
    };

    // 背景を塗りつぶす
    frame.fill_rectangle(
        Point::new(bounds.x, bounds.y),
        Size::new(bounds.width, bounds.height),
        Color::from_rgb8(0, 0, 0),
    );

    let line_color = Color::from_rgb8(0, 196, 0);
    let samples_per_pixel = pcm.len() as f32 / bounds.width;
    const USE_PATH_THRESHOLD: f32 = 200.0;
    if samples_per_pixel < USE_PATH_THRESHOLD {
        // 波形描画パスを生成
        let path = Path::new(|b| {
            b.move_to(Point::new(
                center_left.x,
                center.y - pcm[0] * pcm_normalizer,
            ));
            for i in 1..num_points_to_draw {
                b.line_to(Point::new(
                    center_left.x + i as f32 * x_offset_delta,
                    center.y - pcm[(i as f32 * sample_stride).round() as usize] * pcm_normalizer,
                ));
            }
        });
        // 波形描画
        frame.stroke(
            &path,
            Stroke {
                style: stroke::Style::Solid(line_color),
                width: 1.0,
                ..Stroke::default()
            },
        );
    } else {
        // ピクセルあたりのサンプル数が多いときは、最小値と最大値をつなぐ矩形のみ描画
        let mut prev_sample = 0;
        for i in 0..num_points_to_draw {
            const MIN_HEIGHT: f32 = 0.5;
            let current_sample = ((i + 1) as f32 * sample_stride).round() as usize;
            let max_val = pcm[prev_sample..current_sample]
                .iter()
                .max_by(|a, b| a.total_cmp(&b))
                .unwrap();
            let min_val = pcm[prev_sample..current_sample]
                .iter()
                .min_by(|a, b| a.total_cmp(&b))
                .unwrap();

            // 最大と最小の差がない（無音など）ときは高さをクリップ
            let mut height = (max_val - min_val) * pcm_normalizer;
            if height < MIN_HEIGHT {
                height = MIN_HEIGHT;
            }

            // 矩形描画
            frame.fill_rectangle(
                Point::new(
                    center_left.x + i as f32 * x_offset_delta,
                    center.y - max_val * pcm_normalizer,
                ),
                Size::new(1.2, height),
                line_color,
            );
            prev_sample = current_sample;
        }
    }
}

/// ループポイント描画
fn draw_loop_point(
    frame: &mut Frame,
    bounds: &Rectangle,
    num_samples: usize,
    loop_start_sample: usize,
) {
    let line_color = Color::from_rgb8(200, 200, 200);
    let path = Path::new(|b| {
        b.move_to(Point::new(
            (bounds.width * loop_start_sample as f32) / num_samples as f32,
            0.0,
        ));
        b.line_to(Point::new(
            (bounds.width * loop_start_sample as f32) / num_samples as f32,
            bounds.height,
        ));
    });
    frame.stroke(
        &path,
        Stroke {
            style: stroke::Style::Solid(line_color),
            width: 1.5,
            ..Stroke::default()
        },
    );
}

/// 時刻ラベル描画
fn draw_timelabel(
    frame: &mut Frame,
    bounds: &Rectangle,
    sampling_rate: f32,
    num_samples: usize,
    num_labels: usize,
) {
    let timelabel_left_x = bounds.center().x - bounds.width / 2.0;
    let timelabel_y = bounds.center().y;
    let duration = (num_samples as f32) * 1000.0 / sampling_rate;
    // ラベル描画間隔
    let tick = 10.0f32.pow((duration / 2.0).log10().floor());
    let period = 1000.0 / sampling_rate;
    let mut next_tick = tick;
    for i in 0..num_samples {
        let time = (i as f32) * period;
        if time >= next_tick {
            frame.fill_text(canvas::Text {
                content: format!("{:.0}", time),
                size: iced::Pixels(14.0),
                position: Point::new(
                    timelabel_left_x + (i as f32) * bounds.width / (num_samples as f32 - 1.0),
                    timelabel_y,
                ),
                color: Color::WHITE,
                align_x: alignment::Horizontal::Center.into(),
                align_y: alignment::Vertical::Bottom,
                font: Font::MONOSPACE,
                ..canvas::Text::default()
            });
            next_tick += tick;
        }
    }
}

impl PlaybackStatus {
    fn new() -> Self {
        Self {
            noteon: [false; 8],
            srn_no: [0; 8],
            pitch: [0; 8],
            expression: [0; 8],
        }
    }
}

// 再生情報の読み取り
fn read_playback_status(midi_dsp: &spc700::mididsp::MIDIDSP) -> PlaybackStatus {
    let dummy_ram = [0u8];
    let mut status = PlaybackStatus::new();

    let noteon_flags = midi_dsp.read_register(&dummy_ram, DSP_ADDRESS_NOTEON);
    for ch in 0..8 {
        status.noteon[ch] = ((noteon_flags >> ch) & 1) != 0;
        status.srn_no[ch] =
            midi_dsp.read_register(&dummy_ram, DSP_ADDRESS_V0SRCN | ((ch as u8) << 4));
        let pitch_high =
            midi_dsp.read_register(&dummy_ram, DSP_ADDRESS_V0PITCHH | ((ch as u8) << 4));
        let pitch_low =
            midi_dsp.read_register(&dummy_ram, DSP_ADDRESS_V0PITCHL | ((ch as u8) << 4));
        status.pitch[ch] = ((pitch_high as u16) << 8) | (pitch_low as u16);
        status.expression[ch] =
            midi_dsp.read_register(&dummy_ram, DSP_ADDRESS_V0ENVX | ((ch as u8) << 4));
    }

    status
}

impl MIDIOutputConfigure {
    const DEFAULT_PLAYBACK_PARAMETER_UPDATE_PERIOD_MSEC: u8 = 10;
    const MIDI_DEFAULT_BPM: u8 = 120;
    const MIDI_DEFAULT_RESOLUSIONS: u16 = 480;

    fn new() -> Self {
        Self {
            output_duration_msec: DEFAULT_OUTPUT_DURATION_MSEC,
            playback_parameter_update_period: Self::DEFAULT_PLAYBACK_PARAMETER_UPDATE_PERIOD_MSEC,
            beats_per_minute: Self::MIDI_DEFAULT_BPM,
            ticks_per_quarter: Self::MIDI_DEFAULT_RESOLUSIONS,
        }
    }
}
