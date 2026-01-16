use crate::types::SourceInformation;
use czt::{c32, transform};
use num_traits::Pow;
use std::f32::consts::PI;

/// SPCの出力サンプリングレート
const SPC_SAMPLING_RATE: f32 = 32000.0;
/// センターピッチ(A4)
const A4_PITCH_HZ: f32 = 440.0;
/// 有効なピッチ候補と認めるスレッショルド
const PITCH_PEAK_THRESHOLD: f32 = 0.8;
/// 有効なビート候補と認めるスレッショルド
const BPM_PEAK_THRESHOLD: f32 = 0.98;

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

// 超簡易ドラム音判定
fn detect_drum(source_info: &SourceInformation) -> bool {
    const NUM_DIVISIONS: usize = 8;

    let signal = &source_info.signal;
    let power_spec = &source_info.power_spectrum;
    let nsmpls = signal.len();
    let nspecs = power_spec.len();

    if nsmpls == 0 || nspecs == 0 {
        return false;
    }

    // ループ位置が端点にあればワンショット音源
    let one_shot =
        (source_info.loop_start_sample == 0) || (source_info.loop_start_sample == nsmpls);

    // 最初の1/8と最後の1/8のパワーの比
    let power_ratio;
    {
        let mut first_power = 0.0;
        let mut last_power = 0.0;
        let div_num_samples = nsmpls / NUM_DIVISIONS;
        for i in 0..div_num_samples {
            first_power += signal[i] * signal[i];
        }
        for i in (nsmpls - div_num_samples)..nsmpls {
            last_power += signal[i] * signal[i];
        }
        power_ratio = if (first_power > 0.0) && (last_power > 0.0) {
            10.0 * (first_power / last_power).log10()
        } else if first_power > 0.0 {
            120.0
        } else {
            -120.0
        };
    }

    let sum_power = power_spec.iter().sum::<f32>();
    let density: Vec<_> = power_spec.iter().map(|p| *p / sum_power).collect();

    // スペクトラム平坦性
    let sum_log = power_spec.iter().map(|&p| p.ln()).sum::<f32>();
    let geo_mean = (sum_log / (nspecs as f32)).exp();
    let mean = sum_power / (nspecs as f32);
    let sfm = 10.0 * (geo_mean / mean).log10();

    // スペクトル重心
    let centroid = density
        .iter()
        .enumerate()
        .map(|(i, p)| (*p * ((i as f32) * SPC_SAMPLING_RATE)) / (2.0 * (nspecs as f32)))
        .sum::<f32>();

    // スペクトル帯域幅
    let deviation: Vec<_> = (0..nspecs)
        .map(|i| (((i as f32) * SPC_SAMPLING_RATE) / (2.0 * (nspecs as f32)) - centroid).abs())
        .collect();
    let bandwidth = density
        .iter()
        .enumerate()
        .map(|(i, p)| *p * deviation[i] * deviation[i])
        .sum::<f32>()
        .sqrt();

    // ドラム音判定

    // ワンショット音源
    if one_shot {
        return true;
    }

    // パワーの減衰が大きい
    if power_ratio >= 24.0 {
        return true;
    }

    // スペクトル平坦性尺度が大きい
    if sfm >= -10.0 {
        return true;
    }

    // スペクトル重心が高くスペクトル帯域幅が広い
    if centroid >= 8000.0 && bandwidth >= 8000.0 {
        return true;
    }

    false
}

/// センターノートの推定
fn center_note_estimation(source_info: &SourceInformation) -> f32 {
    let power_spec = &source_info.power_spectrum;
    // 対数パワースペクトルに変換
    let log_spec: Vec<f32> = power_spec.iter().map(|p| 10.0 * f32::log10(*p)).collect();

    // 最大値
    let (argmax, max) =
        log_spec
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
    for i in 1..(log_spec.len() - 1) {
        if log_spec[i] >= PITCH_PEAK_THRESHOLD * max {
            if log_spec[i - 1] < log_spec[i] && log_spec[i + 1] < log_spec[i] {
                peaks.push(i);
            }
        }
    }

    // 最初の候補をピッチとする
    // 候補がなければ単純に最大のインデックス
    let pitch_bin = if peaks.len() > 0 { peaks[0] } else { argmax };

    let peak_hz = (pitch_bin as f32 / (2.0 * power_spec.len() as f32)) * SPC_SAMPLING_RATE;
    let estimated_note = 12.0 * f32::log2(peak_hz / A4_PITCH_HZ) + 69.0;

    estimated_note.clamp(0.0, 127.0)
}

/// ドラム音とノート番号の推定
pub fn estimate_drum_and_note(source_info: &SourceInformation) -> (bool, f32) {
    (
        detect_drum(&source_info),
        center_note_estimation(&source_info),
    )
}

/// 超簡易テンポ推定
pub fn estimate_bpm(signal: &Vec<f32>) -> f32 {
    const TEMPO_ESTIMATION_FRAME_SIZE: usize = 64;
    const INV_FRAME_SIZE: f32 = 1.0 / (TEMPO_ESTIMATION_FRAME_SIZE as f32);
    const MIN_BPM: usize = 30;
    const MAX_BPM: usize = 240;
    const MIN_LAG: usize = ((60.0 * SPC_SAMPLING_RATE)
        / (MAX_BPM as f32 * TEMPO_ESTIMATION_FRAME_SIZE as f32))
        as usize;
    const MAX_LAG: usize = ((60.0 * SPC_SAMPLING_RATE)
        / (MIN_BPM as f32 * TEMPO_ESTIMATION_FRAME_SIZE as f32))
        as usize;

    // フレームに区切り、RMSを計算
    let rms: Vec<_> = signal
        .chunks(TEMPO_ESTIMATION_FRAME_SIZE)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() * INV_FRAME_SIZE).sqrt())
        .collect();

    // RMSの差分 かつ 0でクリップ
    let mut diff_rms: Vec<_> = rms
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i == 0 {
                rms[0]
            } else {
                (rms[i] - rms[i - 1]).max(0.0)
            }
        })
        .collect();

    // 窓かけ
    diff_rms = diff_rms
        .iter()
        .enumerate()
        .map(|(i, r)| *r * f32::sin((PI * (i as f32)) / (diff_rms.len() - 1) as f32).pow(2.0))
        .collect();

    // 自己相関計算
    let m = diff_rms.len();
    let power_spec: Vec<_> = transform(diff_rms.as_slice(), m, chirp!(m), c32::new(1.0, 0.0))
        .iter()
        .map(|c| c.re * c.re + c.im * c.im)
        .collect();
    let auto_corr: Vec<_> = transform(power_spec.as_slice(), m, chirp!(m), c32::new(1.0, 0.0))
        [..(power_spec.len() / 2)]
        .iter()
        .map(|c| c.re)
        .collect();

    // 候補ラグ内でのピーク
    let max = auto_corr[MIN_LAG..=MAX_LAG]
        .iter()
        .fold(0.0 / 0.0, |m, v| v.max(m));

    // ピーク値から候補ラグを列挙
    let mut peak_lags = vec![];
    for i in MIN_LAG..=MAX_LAG {
        if auto_corr[i] >= BPM_PEAK_THRESHOLD * max {
            peak_lags.push(i);
        }
    }

    // 先頭に見つかったピークをビートとする
    (60.0 * SPC_SAMPLING_RATE) / (peak_lags[0] as f32 * TEMPO_ESTIMATION_FRAME_SIZE as f32)
}

/// パワースペクトルの計算
pub fn compute_power_spectrum(signal: &Vec<f32>) -> Vec<f32> {
    // 分析範囲の切り出し（TODO: 要るか？）
    let (start, end) = detect_nonzero_erea(signal);
    let mut signal = if start < end {
        signal[start..end].to_vec()
    } else {
        signal.to_vec()
    };

    // 正規化 + 窓かけ
    let m = signal.len();
    signal = signal
        .iter()
        .enumerate()
        .map(|(i, r)| {
            *r * f32::sin((PI * (i as f32)) / (signal.len() - 1) as f32).pow(2.0) / (m as f32)
        })
        .collect();

    transform(signal.as_slice(), m, chirp!(m), c32::new(1.0, 0.0))[..=(m / 2)]
        .iter()
        .map(|c| c.re * c.re + c.im * c.im)
        .collect::<Vec<_>>()
}
