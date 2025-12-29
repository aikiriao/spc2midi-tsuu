use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, PauseStreamError, PlayStreamError, Stream, StreamConfig};
use fixed_resample::ReadStatus;
use iced::border::Radius;
use iced::keyboard::key::Named;
use iced::widget::canvas::{self, stroke, Cache, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::{
    button, center, center_x, checkbox, column, container, operation, pick_list, row, scrollable,
    slider, space, stack, text, text_input, toggler, tooltip, vertical_slider, Column, Space,
    Stack,
};
use iced::{
    alignment, event, mouse, theme, window, Border, Color, Element, Function, Length, Padding,
    Point, Rectangle, Renderer, Size, Subscription, Task, Theme, Vector,
};
use iced_aw::menu::{self, Item, Menu};
use iced_aw::style::{menu_bar::primary, Status};
use iced_aw::{iced_aw_font, menu_bar, menu_items, ICED_AW_FONT_BYTES};
use iced_aw::{quad, widgets::InnerBounds};
use midir::{MidiOutput, MidiOutputPort};
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
    ReceivedPlayStartRequest,
    ReceivedPlayStopRequest,
    SPCMuteFlagToggled(bool),
    MIDIMuteFlagToggled(bool),
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
    signal: Vec<f32>,
    start_address: usize,
    end_address: usize,
    loop_start_sample: usize,
}

#[derive(Debug)]
struct MainWindow {
    title: String,
    source_infos: Arc<RwLock<BTreeMap<u8, SourceInformation>>>,
    pcm_spc_mute: bool,
    midi_spc_mute: bool,
}

#[derive(Debug)]
struct PreferenceWindow {
    title: String,
}

#[derive(Debug)]
struct SRNWindow {
    title: String,
    window_id: window::Id,
    srn_no: u8,
    source_info: Arc<SourceInformation>,
    enable_loop_play: bool,
    cache: Cache,
}

struct App {
    theme: iced::Theme,
    main_window_id: window::Id,
    windows: BTreeMap<window::Id, Box<dyn SPC2MIDI2Window>>,
    spc_file: Option<Box<SPCFile>>,
    source_infos: Arc<RwLock<BTreeMap<u8, SourceInformation>>>,
    stream_device: Device,
    stream_config: StreamConfig,
    stream: Option<Stream>,
    stream_played_samples: Arc<AtomicUsize>,
    stream_is_playing: Arc<AtomicBool>,
    pcm_spc: Option<Arc<Mutex<Box<spc700::spc::SPC<spc700::sdsp::SDSP>>>>>,
    midi_spc: Option<Arc<Mutex<Box<spc700::spc::SPC<spc700::mididsp::MIDIDSP>>>>>,
    pcm_spc_mute: Arc<AtomicBool>,
    midi_spc_mute: Arc<AtomicBool>,
}

impl Default for App {
    fn default() -> Self {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no output device available");
        let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).unwrap();

        Self {
            theme: iced::Theme::Nord,
            main_window_id: window::Id::unique(),
            windows: BTreeMap::new(),
            spc_file: None,
            source_infos: Arc::new(RwLock::new(BTreeMap::new())),
            stream_config: device.default_output_config().unwrap().into(),
            stream_device: device,
            stream: None,
            stream_played_samples: Arc::new(AtomicUsize::new(0)),
            stream_is_playing: Arc::new(AtomicBool::new(false)),
            pcm_spc: None,
            midi_spc: None,
            pcm_spc_mute: Arc::new(AtomicBool::new(false)),
            midi_spc_mute: Arc::new(AtomicBool::new(false)),
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
                let (id, open) = window::open(window::Settings::default());
                let window =
                    MainWindow::new(SPC2MIDI2_TITLE_STR.to_string(), self.source_infos.clone());
                self.main_window_id = id;
                self.windows.insert(id, Box::new(window));
                return open.map(Message::MainWindowOpened);
            }
            Message::MainWindowOpened(id) => {}
            Message::OpenPreferenceWindow => {
                let (id, open) = window::open(window::Settings::default());
                self.windows.insert(
                    id,
                    Box::new(PreferenceWindow::new("Preference".to_string())),
                );
                return open.map(Message::PreferenceWindowOpened);
            }
            Message::PreferenceWindowOpened(id) => {}
            Message::OpenSRNWindow(srn_no) => {
                let (id, open) = window::open(window::Settings::default());
                let infos = self.source_infos.read().unwrap();
                if let Some(source) = infos.get(&srn_no) {
                    let window =
                        SRNWindow::new(id, format!("SRN 0x{:02X}", srn_no), srn_no, source);
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
                            60 * 2,
                            &spc_file.header.spc_register,
                            &spc_file.ram,
                            &spc_file.dsp_register,
                        );
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
        Subscription::batch(vec![
            window::close_events().map(Message::WindowClosed),
            event::listen().map(Message::EventOccurred),
        ])
    }

    /// 音源ソースの解析
    fn analyze_sources(
        &mut self,
        analyze_duration_sec: u32,
        register: &SPCRegister,
        ram: &[u8],
        dsp_register: &[u8; 128],
    ) {
        let analyze_duration_64khz_tick = analyze_duration_sec * 64000;

        // リストを作り直す
        let mut infos = self.source_infos.write().unwrap();
        *infos = BTreeMap::new();

        // 一定期間シミュレートし、サンプルソース番号とそれに紐づく開始アドレスを取得
        let mut spc: spc700::spc::SPC<spc700::mididsp::MIDIDSP> =
            SPC::new(&register, ram, dsp_register);
        let mut cycle_count = 0;
        let mut tick64khz_count = 0;
        let mut start_address_map = BTreeMap::new();
        while tick64khz_count < analyze_duration_64khz_tick {
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
            let mut last_block_sample = 0;
            loop {
                let pcm = decoder.process(ram, 0x1000) as f32;
                signal.push(pcm * PCM_NORMALIZE_CONST);
                if decoder.end {
                    last_block_sample += 1;
                    if last_block_sample >= 16 {
                        break;
                    }
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
        }
    }

    // SMFを作成
    fn create_smf(&self) -> Option<SMF> {
        if let Some(spc_file) = &self.spc_file {
            const MIDI_BPM: u64 = 120; // TODO: ユーザが設定
            const MIDI_DIVISIONS: u64 = 3200; // TODO: ユーザが設定

            let mut smf = SMF {
                format: SMFFormat::Single,
                tracks: vec![Track {
                    copyright: Some("tmp".to_string()), // TODO: SPCから出す or ユーザが設定した時間出力
                    name: Some("tmp".to_string()), // TODO: SPCから出す or ユーザが設定した時間出力
                    events: Vec::new(),
                }],
                division: MIDI_DIVISIONS as i16,
            };

            // SPCの作成
            let mut spc: spc700::spc::SPC<spc700::mididsp::MIDIDSP> = SPC::new(
                &spc_file.header.spc_register,
                &spc_file.ram,
                &spc_file.dsp_register,
            );

            // TODO: ここで編集済みのパラメータを設定

            let mut cycle_count = 0;
            let mut total_elapsed_time_nanosec = 0;
            let mut previous_event_time = 0.0;

            // 60秒出力する TODO: SPCから読み取った時間 or ユーザが設定した時間出力
            while total_elapsed_time_nanosec < 60 * 1000_000_000 {
                // 64kHzタイマーティックするまで処理
                while cycle_count < CLOCK_TICK_CYCLE_64KHZ {
                    cycle_count += spc.execute_step() as u32;
                }
                cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
                // MIDI出力
                if let Some(out) = spc.clock_tick_64k_hz() {
                    // 経過時間からティック数を計算
                    let delta_nano_time = total_elapsed_time_nanosec as f64 - previous_event_time;
                    let ticks = (delta_nano_time * (MIDI_BPM * MIDI_DIVISIONS) as f64)
                        / (60.0 * 1000_000_000.0);
                    // ティック数は切り捨てる（切り上げると経過時間が未来になって経過時間が負になりうる）
                    for i in 0..out.num_messages {
                        let msg = out.messages[i];
                        smf.tracks[0].events.push(TrackEvent {
                            vtime: if i == 0 { ticks.floor() as u64 } else { 0 },
                            event: MidiEvent::Midi(MidiMessage {
                                data: msg.data[..msg.length].to_vec(),
                            }),
                        });
                    }
                    // 実際のtickから経過時間計算
                    previous_event_time += (ticks.floor() * 60.0 * 1000_000_000.0)
                        / ((MIDI_DIVISIONS * MIDI_BPM) as f64);
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

        // リサンプラ初期化 32k -> デバイスの出力レート変換となるように
        let (mut prod, mut cons) = fixed_resample::resampling_channel::<f32, NUM_CHANNELS>(
            NonZero::new(NUM_CHANNELS).unwrap(),
            SPC_SAMPLING_RATE,
            self.stream_config.sample_rate,
            Default::default(),
        );

        // FIXME: MIDI出力ポートの作成。出力MIDIデバイスを選べるべき
        let midi_out = MidiOutput::new(SPC2MIDI2_TITLE_STR).unwrap();
        let out_ports = midi_out.ports();
        let mut conn_out = midi_out
            .connect(&out_ports[0], SPC2MIDI2_TITLE_STR)
            .unwrap();

        // 各SPCのミュートフラグ取得
        let pcm_spc_mute = self.pcm_spc_mute.clone();
        let midi_spc_mute = self.midi_spc_mute.clone();

        // 再生ストリーム作成
        let mut cycle_count = 0;
        let mut pcm_buffer = vec![0.0f32; BUFFER_SIZE * NUM_CHANNELS];
        let stream = match self.stream_device.build_output_stream(
            &self.stream_config,
            move |buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // SPCをロックして獲得
                let mut spc = pcm_spc.lock().unwrap();
                let mut midispc = midi_spc.lock().unwrap();

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
            },
            |err| eprintln!("[{}] {err}", SPC2MIDI2_TITLE_STR),
            None,
        ) {
            Ok(stream) => stream,
            Err(_) => return Err(PlayStreamError::DeviceNotAvailable),
        };

        // 再生開始
        self.stream_played_samples.store(0, Ordering::Relaxed);
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
        let played_samples = self.stream_played_samples.clone();
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

        // 再生ストリーム作成
        let stream = match self.stream_device.build_output_stream(
            &self.stream_config,
            move |buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut progress = played_samples.load(Ordering::Relaxed);
                // 一旦バッファを無音で埋める
                buffer.fill(0.0);
                // バッファにコピー
                let num_copy_samples = cmp::min(output.len() - progress, buffer.len());
                buffer[..num_copy_samples]
                    .copy_from_slice(&output[progress..progress + num_copy_samples]);
                progress += num_copy_samples;
                // 端点に来た時の処理
                if progress >= output.len() {
                    if loop_flag {
                        // ループしながらバッファがいっぱいになるまでコピー
                        let mut buffer_pos = num_copy_samples;
                        progress = loop_start_sample;
                        while buffer_pos < buffer.len() {
                            let num_copy_samples = cmp::min(output.len() - progress, buffer.len() - buffer_pos);
                            buffer[buffer_pos..(buffer_pos + num_copy_samples)]
                                .copy_from_slice(&output[progress..(progress + num_copy_samples)]);
                            buffer_pos += num_copy_samples;
                            progress += num_copy_samples;
                            if progress >= output.len() {
                                progress = loop_start_sample;
                            }
                        }
                    } else {
                        // 再生終了
                        is_playing.store(false, Ordering::Relaxed);
                    }
                }
                // 再生サンプル増加
                played_samples.store(progress, Ordering::Relaxed);
            },
            |err| eprintln!("[{}] {err}", SPC2MIDI2_TITLE_STR),
            None,
        ) {
            Ok(stream) => stream,
            Err(_) => return Err(PlayStreamError::DeviceNotAvailable),
        };

        // 再生開始
        self.stream_played_samples.store(0, Ordering::Relaxed);
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
        Ok(())
    }
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
        let srn_list: Vec<_> = infos
            .iter()
            .map(|(key, info)| {
                row![
                    text(format!("{} {}", key, info.start_address)),
                    button("Configure").on_press(Message::OpenSRNWindow(*key))
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
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);

        let r = row![menu_bar, space::horizontal().width(Length::Fill),]
            .align_y(alignment::Alignment::Center);

        let c = column![
            r,
            Column::from_vec(srn_list)
                .width(Length::Fill)
                .height(Length::Fill),
            preview_control,
        ];

        c.into()
    }
}

impl MainWindow {
    fn new(title: String, source_info: Arc<RwLock<BTreeMap<u8, SourceInformation>>>) -> Self {
        Self {
            title: title,
            source_infos: source_info,
            pcm_spc_mute: false,
            midi_spc_mute: false,
        }
    }
}

impl SPC2MIDI2Window for PreferenceWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let content = column![text("Super awesome config")]
            .spacing(50)
            .width(Length::Fill)
            .align_x(alignment::Alignment::Center)
            .width(100);
        content.into()
    }
}

impl PreferenceWindow {
    fn new(title: String) -> Self {
        Self { title: title }
    }
}

impl SPC2MIDI2Window for SRNWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
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
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center)
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
    ) -> Self {
        Self {
            window_id: window_id,
            title: title,
            srn_no: srn_no,
            source_info: source_info.clone().into(),
            enable_loop_play: false,
            cache: Cache::default(),
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
