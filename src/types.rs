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
    /// ピッチベンドを使うか
    pub enable_pitch_bend: bool,
    /// エコーをエフェクト1デプスとして出力するか
    pub echo_as_effect1: bool,
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
