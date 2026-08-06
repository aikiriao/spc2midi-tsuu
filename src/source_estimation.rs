use crate::types::*;
use num_complex::Complex;
use num_traits::Pow;
use realfft::RealFftPlanner;
use std::f32::consts::PI;

/// SPCの出力サンプリングレート
const SPC_SAMPLING_RATE: f32 = 32000.0;
/// センターピッチ(A4)
const A4_PITCH_HZ: f32 = 440.0;
/// 有効なピッチ候補と認めるスレッショルド
const PITCH_PEAK_THRESHOLD: f32 = 0.9;

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
    let one_shot = source_info.loop_start_sample == nsmpls || source_info.loop_start_sample == 0;

    // 最初の1/8と最後の1/8のパワーの比
    let power_ratio = {
        let div_num_samples = nsmpls / NUM_DIVISIONS;
        let mut first_power = 0.0;
        for i in 0..div_num_samples {
            first_power += signal[i] * signal[i];
        }
        let mut last_power = 0.0;
        for i in (nsmpls - div_num_samples)..nsmpls {
            last_power += signal[i] * signal[i];
        }
        if (first_power > 0.0) && (last_power > 0.0) {
            10.0 * (first_power / last_power).log10()
        } else if first_power > 0.0 {
            120.0
        } else {
            -120.0
        }
    };

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

    // ショートループ（1波形分だけのループ）
    if nsmpls < (SPC_SAMPLING_RATE / 100.0) as usize {
        return false;
    }

    // ワンショット音源 or 波形全体ループ音源
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

// 最適なセンターノートの小数部の推定
// 半音単位で整数付近に偏るようにセンターノートの小数部を定める問題で考える
fn compute_optimal_center_note_fraction(pitch_sequence: &Vec<u16>) -> f32 {
    const F64PI: f64 = std::f64::consts::PI;
    let mut sum_exp = Complex::new(0.0, 0.0);

    if pitch_sequence.len() == 0 {
        return 0.0;
    }

    // expの和を取る
    for pitch in pitch_sequence {
        sum_exp += (Complex::I * 2.0 * F64PI * 12.0 * (*pitch as f64).log2()).exp();
    }

    // 偏角argは[-pi, pi)なので[0, 2pi)に変換
    let mut sum_arg = sum_exp.arg();
    if sum_arg < 0.0 {
        sum_arg += 2.0 * F64PI;
    }

    (-sum_arg / (2.0 * F64PI)).rem_euclid(1.0) as f32
}

/// センターノートの推定
fn center_note_estimation(source_info: &SourceInformation) -> f32 {
    // 対数パワースペクトルのオフセット
    const LOG_POWER_SPECTRUM_OFFSET_DB: f32 = 120.0;

    // ループ長からの周期推定
    let nsmpls = source_info.signal.len();
    let loop_length = if nsmpls > source_info.loop_start_sample {
        nsmpls - source_info.loop_start_sample
    } else {
        0
    };
    if loop_length > 0 {
        // ショートループのサンプル数が小さく、かつ波形全体に対するループが大きければ
        // ループ部分が1周期分の波形になっていると思って推定
        if loop_length < (SPC_SAMPLING_RATE / 100.0) as usize && nsmpls < 5 * loop_length {
            let freq = SPC_SAMPLING_RATE / loop_length as f32;
            let estimated_note = 12.0 * f32::log2(freq / A4_PITCH_HZ) + 69.0;
            return estimated_note.clamp(0.0, 127.0);
        }
    }

    // ループ長が長ければ解析区間をループ区間内に絞り込む
    let analyze_signal = if loop_length > (0.05 * SPC_SAMPLING_RATE) as usize {
        source_info.signal[source_info.loop_start_sample..].to_vec()
    } else {
        source_info.signal.clone()
    };

    // 自己相関の先頭にある大きいピークを探索
    let auto_corr = compute_auto_correlation(&analyze_signal);
    let max_corr = auto_corr.iter().fold(0.0 / 0.0, |m, v| v.max(m));
    let mut auto_corr_peak_hzs = Vec::new();
    for i in 1..(auto_corr.len() - 1) {
        let corr = auto_corr[i];
        if corr > 0.0 && auto_corr[i - 1] < corr && auto_corr[i + 1] < corr {
            if corr >= PITCH_PEAK_THRESHOLD * max_corr {
                auto_corr_peak_hzs.push(SPC_SAMPLING_RATE / (i as f32));
            }
        }
    }

    // パワースペクトルでのピークを探索
    let power_spec = compute_power_spectrum(&analyze_signal);
    // 対数パワースペクトルに変換
    let log_spec: Vec<f32> = power_spec
        .iter()
        .map(|p| 10.0 * f32::log10(*p) + LOG_POWER_SPECTRUM_OFFSET_DB)
        .collect();
    let max_log_spec = log_spec.iter().fold(0.0 / 0.0, |m, v| v.max(m));
    // ピークをとるインデックスを探索
    let mut power_spec_peak_hzs = Vec::new();
    let bin_to_freq_normalizer = SPC_SAMPLING_RATE / (2.0 * power_spec.len() as f32);
    for i in 1..(log_spec.len() - 1) {
        if log_spec[i - 1] < log_spec[i] && log_spec[i + 1] < log_spec[i] {
            if log_spec[i] >= PITCH_PEAK_THRESHOLD * max_log_spec {
                power_spec_peak_hzs.push((i as f32) * bin_to_freq_normalizer);
            }
        }
    }

    // 自己相関のピークに該当するピッチ周波数を優先
    let mut pitch_hz = if auto_corr_peak_hzs.len() > 0 {
        auto_corr_peak_hzs[0]
    } else if power_spec_peak_hzs.len() > 0 {
        power_spec_peak_hzs[0]
    } else {
        1.0
    };

    // ピッチ周波数が高いときはパワースペクトルの推定値に置き換える
    // 高周波では、自己相関の1ラグのずれで大きくピッチ周波数が揺らぐため
    if power_spec_peak_hzs.len() > 0 {
        if pitch_hz > 400.0 {
            pitch_hz = power_spec_peak_hzs[0];
        }
    }

    // ピッチ周波数基準のノート番号
    let pitch_note = 12.0 * f32::log2(pitch_hz / A4_PITCH_HZ) + 69.0;

    // 小数部を半音単位の整数になるように補正
    let optimal_frac = compute_optimal_center_note_fraction(&source_info.pitch_sequence);

    // ピッチ基準のノート番号に最も近い 整数 + optimal_frac を選択
    let estimated_note = optimal_frac + (pitch_note - optimal_frac).round();

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
pub fn estimate_bpm(onset_signal: &[f32], sampling_rate: f32) -> f32 {
    // 推定テンポの範囲
    const MIN_ESTIMATED_BPM: f32 = 30.0;
    const MAX_ESTIMATED_BPM: f32 = 240.0;

    // フレームに区切り平均をとる
    // （この操作は間引きに相当するので間引く前にLPFをかけるとよいが低速なのでやめる）
    let frame_size: usize = (sampling_rate * 0.01).round() as usize;
    let onset_envelope: Vec<_> = onset_signal
        .chunks(frame_size)
        .map(|c| c.iter().sum::<f32>() / frame_size as f32)
        .collect();

    // 平均除去
    let mean = onset_envelope.iter().sum::<f32>() / onset_envelope.len() as f32;
    let onset_envelope: Vec<_> = onset_envelope.into_iter().map(|c| c - mean).collect();

    // 自己相関計算
    let auto_corr = compute_auto_correlation(&onset_envelope);

    // 候補ラグ内でのピーク
    let min_lag = ((60.0 * sampling_rate) / (MAX_ESTIMATED_BPM * frame_size as f32)) as usize;
    let max_lag = ((60.0 * sampling_rate) / (MIN_ESTIMATED_BPM * frame_size as f32)) as usize;
    let max_lag = max_lag.min(auto_corr.len() - 1);
    let max = auto_corr[min_lag..=max_lag]
        .iter()
        .fold(0.0 / 0.0, |m, v| v.max(m));

    // ピークを超えた最初のピークをBPMとする
    for i in min_lag..=max_lag {
        if auto_corr[i] >= max {
            return (60.0 * sampling_rate) / (i as f32 * frame_size as f32);
        }
    }

    unreachable!("Failed to find max peak in tempo estimation!");
}

/// パワースペクトルの計算
pub fn compute_power_spectrum(signal: &Vec<f32>) -> Vec<f32> {
    // 分析範囲の切り出し（TODO: 要るか？）
    let (start, end) = detect_nonzero_erea(signal);
    let signal = if start < end {
        signal[start..end].to_vec()
    } else {
        signal.to_vec()
    };

    let m = signal.len();
    // 窓との重み付き平均
    let window: Vec<_> = (0..m)
        .map(|i| f32::sin((PI * (i as f32)) / (m - 1) as f32).pow(2.0))
        .collect();
    let wmean = signal
        .iter()
        .zip(window.iter())
        .map(|(s, w)| s * w)
        .sum::<f32>()
        / window.iter().sum::<f32>();

    // 直流成分除去しつつ窓かけ
    let signal: Vec<_> = signal
        .iter()
        .zip(window.iter())
        .map(|(s, w)| (s - wmean) * w)
        .collect();

    // ゼロ埋め
    let pad_len = m.next_power_of_two() * 4;
    let mut buffer = vec![0.0f32; pad_len];
    let normalized_factor = 1.0 / pad_len as f32;
    for n in 0..m {
        buffer[n] = signal[n] * normalized_factor;
    }

    // パワースペクトル計算
    let mut fft_planner = RealFftPlanner::<f32>::new();
    let r2c = fft_planner.plan_fft_forward(pad_len);
    let mut complex_spectrum = r2c.make_output_vec();
    r2c.process(&mut buffer, &mut complex_spectrum).unwrap();
    for n in 0..pad_len / 2 {
        let re = complex_spectrum[n].re;
        let im = complex_spectrum[n].im;
        buffer[n] = re * re + im * im;
    }

    buffer[0..pad_len / 2].to_vec()
}

/// 自己相関関数の計算
fn compute_auto_correlation(signal: &Vec<f32>) -> Vec<f32> {
    // 後半ゼロ埋めした信号
    let pad_len = signal.len().next_power_of_two() * 2;
    let mut buffer = vec![0.0f32; pad_len];
    let normalized_factor = 1.0 / pad_len as f32;
    for n in 0..signal.len() {
        buffer[n] = signal[n] * normalized_factor;
    }

    // パワースペクトル計算
    let mut fft_planner = RealFftPlanner::<f32>::new();
    let r2c = fft_planner.plan_fft_forward(pad_len);
    let mut complex_spectrum = r2c.make_output_vec();
    r2c.process(&mut buffer, &mut complex_spectrum).unwrap();
    for n in 0..complex_spectrum.len() {
        let re = complex_spectrum[n].re;
        let im = complex_spectrum[n].im;
        complex_spectrum[n].re = re * re + im * im;
        complex_spectrum[n].im = 0.0;
    }

    // 逆FFTして自己相関を求める
    let r2c = fft_planner.plan_fft_inverse(pad_len);
    r2c.process(&mut complex_spectrum, &mut buffer).unwrap();

    buffer[0..signal.len()].to_vec()
}
