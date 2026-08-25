use crate::program::*;
use crate::Message;
use iced::Element;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// デフォルトのMIDIファイル出力時間(sec)
pub const DEFAULT_OUTPUT_DURATION_MSEC: u64 = 60 * 1000;
/// デフォルトのMIDI再生パラメータ更新間隔(msec)
pub const DEFAULT_PLAYBACK_PARAMETER_UPDATE_PERIOD_MSEC: u8 = 5;
/// デフォルトの出力MIDIのBPM
pub const DEFAULT_MIDI_BPM: f32 = 120.0;
/// デフォルトの出力MIDIの四分音符内のティック数
pub const DEFAULT_MIDI_RESOLUSIONS: u16 = 480;
/// デフォルトのSPCのクロックアップ倍率
pub const DEFAULT_SPC_CLOCKUP_FACTOR: u32 = 1;
/// 最小のBPM（テンポ）
pub const MIN_BEATS_PER_MINUTE: u32 = 4;
/// 最大のBPM（テンポ）
pub const MAX_BEATS_PER_MINUTE: u32 = 1920;
/// BPMの最小解像度
pub const BPM_RESOLUTION: f32 = 1.0 / 256.0;

/// ボリュームカーブ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolumeCurve {
    /// 平方根
    SquareRoot,
    /// 対数
    Log,
    /// 線形
    Linear,
}

/// 再生MIDISystem
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MIDISystem {
    /// 指定なし
    NONE,
    /// GM Level 1
    GMLevel1,
    /// GM Level 2
    GMLevel2,
    /// GS
    GS,
    /// XG
    XG,
}

/// GMのパートモード
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GMPartMode {
    Normal,
    Drum,
}

/// GSのパートモード
#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GSPartMode {
    Normal = 0x00,
    RhythmMAP1 = 0x01,
    RhythmMAP2 = 0x02,
}

/// XGのパートモード
#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum XGPartMode {
    Normal = 0x00,
    Drum = 0x01,
    DrumSetup1 = 0x02,
    DrumSetup2 = 0x03,
    DrumSetup3 = 0x04,
    DrumSetup4 = 0x05,
}

/// パート種別
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MIDIPartMode {
    /// GM
    GM(GMPartMode),
    /// GS
    GS(GSPartMode),
    /// XG
    XG(XGPartMode),
}

impl MIDIPartMode {
    /// ドラムパートかどうか判定
    pub fn is_drum_part(&self) -> bool {
        match self {
            MIDIPartMode::GM(mode) => *mode == GMPartMode::Drum,
            MIDIPartMode::GS(mode) => *mode != GSPartMode::Normal,
            MIDIPartMode::XG(mode) => *mode != XGPartMode::Normal,
        }
    }
}

impl std::fmt::Display for MIDIPartMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MIDIPartMode::GM(mode) => match mode {
                GMPartMode::Normal => "Normal",
                GMPartMode::Drum => "Drum",
            }
            MIDIPartMode::GS(mode) => match mode {
                GSPartMode::Normal => "Normal",
                GSPartMode::RhythmMAP1 => "DrumMAP1",
                GSPartMode::RhythmMAP2 => "DrumMAP2",
            }
            MIDIPartMode::XG(mode) => match mode {
                XGPartMode::Normal => "Normal",
                XGPartMode::Drum => "Drum",
                XGPartMode::DrumSetup1 => "DrumSetup1",
                XGPartMode::DrumSetup2 => "DrumSetup2",
                XGPartMode::DrumSetup3 => "DrumSetup3",
                XGPartMode::DrumSetup4 => "DrumSetup4",
            }
        })
    }
}

/// 波形を区別するIDの表示種別
#[derive(Debug, Clone, PartialEq)]
pub enum DisplaySourceIDType {
    /// 波形開始アドレス（デフォルト）
    StartAddress,
    /// SRCN
    SRCN,
}

/// ノート番号の表示タイプ
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayNoteType {
    /// ノート番号
    NoteNumber,
    /// ノート名（中央CがC4）
    NoteNameMiddleC4,
}

/// 波形リストの表示モード
#[derive(Debug, Clone, PartialEq)]
pub enum SampleListOrder {
    /// SPCチャンネル
    SPCChannel,
    /// アドレス降順（spc2midi準拠）
    AddressDescending,
    /// アドレス昇順
    AddressAscending,
    /// SRCN
    SRCN,
}

/// メインウィンドウの行の色の使い分け
#[derive(Debug, Clone, PartialEq)]
pub enum SampleListRowColorStyle {
    /// ストライプ
    Stripe,
    /// 単色
    Solid,
}

/// 音源情報
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SourceInformation {
    /// デコードした信号
    pub signal: Vec<f32>,
    /// パワースペクトル
    pub power_spectrum: Vec<f32>,
    /// 開始アドレス
    pub start_address: usize,
    /// 終端アドレス
    pub end_address: usize,
    /// ループ開始サンプル
    pub loop_start_sample: usize,
    /// チャンネルを使っているか？（8チャンネル分）
    pub using_channel: [bool; 8],
    /// この波形に対してキーオンされたときのピッチ列
    pub pitch_sequence: Vec<u16>,
}

/// 1音源のパラメータ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceParameter {
    /// ミュート（出力するか否か）
    pub mute: bool,
    /// プログラム番号
    pub program: Program,
    /// 基準ノート（8bit整数・8bit小数部）
    pub center_note: u16,
    /// ノートオンベロシティ
    pub noteon_velocity: u8,
    /// ピッチベンド幅（半音単位）
    pub pitch_bend_width: u8,
    /// エンベロープをエクスプレッションとして出力するか
    pub envelope_as_expression: bool,
    /// パンを発音中に更新するか
    pub auto_pan: bool,
    /// パン値
    pub fixed_pan: u8,
    /// ボリュームを発音中に更新するか
    pub auto_volume: bool,
    /// ボリューム値
    pub fixed_volume: u8,
    /// リバーブセンド値
    pub fixed_reverb_send: u8,
    /// コーラスセンド値
    pub chorus_send: u8,
    /// ピッチベンドを使うか
    pub enable_pitch_bend: bool,
    /// エコーをリバーブセンドとして出力するか
    pub echo_as_reverb_send: bool,
    /// ノートオン後に再生パラメータを更新するか
    pub update_parameter_after_noteon: bool,
    /// ピッチベンドがピッチベンド幅を越えたときにノートオフとオンを打ちなおすか
    pub retrigger_noteon_on_exceed_pitch_bend_width: bool,
    /// 出力チャンネル（SPCの出力チャンネルをインデックス、出力先MIDIチャンネルが値）
    pub channel_routing: [u8; 8],
    /// 出力チャンネルミュート（各SPCの出力チャンネルでのミュートフラグ）
    pub channel_mute: [bool; 8],
    /// 楽器名
    pub instrument_name: String,
}

/// MIDI出力設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MIDIOutputConfigure {
    /// 出力時間(ms)
    pub output_duration_msec: u64,
    /// MIDI再生パラメータ更新周期
    pub playback_parameter_update_period: u8,
    /// BPM
    pub beats_per_minute: f32,
    /// 四分の一音符当たりのティック数
    pub ticks_per_quarter: u16,
    /// SPC700のクロックアップ倍率
    pub spc_clockup_factor: u32,
    /// ボリュームカーブ
    pub volume_curve: VolumeCurve,
    /// ターゲットMIDIシステム
    pub midi_system: MIDISystem,
    /// ドラム音色をサンプル単位でトラックに分割するか
    pub split_drum_into_separate_tracks: bool,
    /// 先頭のイベントがない区間を取り除くか
    pub trim_leading_nonevents_period: bool,
    /// 各MIDIチャンネルのパート種別
    pub part_mode: [MIDIPartMode; 16],
}

/// 再生中の状態
#[derive(Debug, Clone)]
pub struct PlaybackStatus {
    /// ノートオン中か
    pub noteon: [bool; 8],
    /// 再生しているソース番号
    pub srn_no: [u8; 8],
    /// 再生ピッチ
    pub pitch: [u16; 8],
    /// エンベロープ上位8bit
    pub envelope: [u8; 8],
    /// 左右ボリューム
    pub volume: [[i8; 2]; 8],
}

// インジケータ
#[derive(Debug, Clone, Copy)]
pub struct Indicator {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub formatter: fn(f32) -> String,
}

pub trait SPC2MIDI2Window: AsAny {
    fn title(&self) -> String;
    fn view(&self) -> Element<'_, Message>;
}

pub trait AsAny {
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// AsAnyの実装
impl<T> AsAny for T
where
    T: 'static,
{
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl MIDIOutputConfigure {
    pub fn new() -> Self {
        Self {
            output_duration_msec: DEFAULT_OUTPUT_DURATION_MSEC,
            playback_parameter_update_period: DEFAULT_PLAYBACK_PARAMETER_UPDATE_PERIOD_MSEC,
            beats_per_minute: DEFAULT_MIDI_BPM,
            ticks_per_quarter: DEFAULT_MIDI_RESOLUSIONS,
            spc_clockup_factor: DEFAULT_SPC_CLOCKUP_FACTOR,
            volume_curve: VolumeCurve::SquareRoot,
            midi_system: MIDISystem::NONE,
            split_drum_into_separate_tracks: false,
            trim_leading_nonevents_period: false,
            part_mode: [
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Drum), // 10chのみドラム
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
                MIDIPartMode::GM(GMPartMode::Normal),
            ],
        }
    }
}

impl PlaybackStatus {
    pub fn new() -> Self {
        Self {
            noteon: [false; 8],
            srn_no: [0; 8],
            pitch: [0; 8],
            envelope: [0; 8],
            volume: [[0, 0]; 8],
        }
    }
}

/// 小数点を含むノート番号を周波数に変換
pub fn note_to_frequency(note: f32) -> f32 {
    440.0 * 2.0f32.powf((note - 69.0) / 12.0)
}
