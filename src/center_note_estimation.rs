use czt::{c32, transform};
use std::f32::consts::PI;

/// SPCの出力サンプリングレート
const SPC_SAMPLING_RATE: f32 = 32000.0;
/// センターピッチ(A4)
const A4_PITCH_HZ: f32 = 440.0;
/// 有効なピッチ候補と認めるスレッショルド
const PITCH_PEAK_THRESHOLD: f32 = 0.8;

macro_rules! chirp(
    ($m:expr) => ({
        c32::from_polar(&1.0, &(-2.0 * PI / $m as f32))
    });
);

fn detect_nonzero_erea(signal: &Vec<f32>) -> (usize, usize) {
    let mut start = 0;
    let mut end = signal.len() - 1;

    while start < signal.len() && signal[start].abs() < 1e-8 {
        start += 1;
    }

    while end > 0 && signal[end].abs() < 1e-8 {
        end -= 1;
    }

    (start, end)
}

/// センターノートの推定
pub fn center_note_estimation(signal: &Vec<f32>) -> f32 {
    // 分析範囲の切り出し
    let (start, end) = detect_nonzero_erea(signal);
    let signal = if start < end {
        signal[start..end].to_vec()
    } else {
        signal.to_vec()
    };

    // Chirp-z transform
    let m = signal.len();
    let spec = transform(signal.as_slice(), m, chirp!(m), c32::new(1.0, 0.0));

    // 対数パワースペクトルに変換
    let logspec: Vec<f32> = spec
        .iter()
        .map(|c| 10.0 * f32::log10(c.re * c.re + c.im * c.im))
        .collect();

    // 最大値
    let (argmax, max) =
        logspec
            .iter()
            .enumerate()
            .fold((usize::MIN, f32::MIN), |(i_a, a), (i_b, &b)| {
                if b > a {
                    (i_b, b)
                } else {
                    (i_a, a)
                }
            });

    // ピークをとるインデックスを探索
    let mut peaks = Vec::new();
    for i in 1..(m - 1) {
        if logspec[i] >= PITCH_PEAK_THRESHOLD * max {
            if logspec[i - 1] < logspec[i] && logspec[i + 1] < logspec[i] {
                peaks.push(i);
            }
        }
    }

    // 最初の候補をピッチとする
    // 候補がなければ単純に最大のインデックス
    let pitch_bin = if peaks.len() > 0 { peaks[0] } else { argmax };

    let peak_hz = (pitch_bin as f32 / m as f32) * SPC_SAMPLING_RATE;
    let estimated_note = 12.0 * f32::log2(peak_hz / A4_PITCH_HZ) + 69.0;

    estimated_note.clamp(0.0, 127.0)
}
