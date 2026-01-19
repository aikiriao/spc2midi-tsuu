use crate::program::*;
use crate::types::*;
use crate::Message;
use crate::SPC_SAMPLING_RATE;
use iced::keyboard::key::Named;
use iced::widget::canvas::{self, stroke, Cache, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::{button, checkbox, column, combo_box, row, text};
use iced::{
    alignment, mouse, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme,
};
use iced_aw::number_input;
use num_traits::pow::Pow;
use std::cmp;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct SRNWindow {
    title: String,
    srn_no: u8,
    source_info: Arc<SourceInformation>,
    source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    midi_preview: Arc<AtomicBool>,
    preview_loop: Arc<AtomicBool>,
    program_box: combo_box::State<Program>,
    cache: Cache,
}

/// 描画モード
pub enum DrawMode {
    WaveForm, // 時間波形
    Spectrum, // 周波数スペクトル
}

impl Default for DrawMode {
    fn default() -> Self {
        Self::WaveForm
    }
}

impl SPC2MIDI2Window for SRNWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let srn_no = self.srn_no;
        let params = self.source_parameter.read().unwrap();
        let param = params.get(&self.srn_no).unwrap();
        let center_note_int = (param.center_note >> 9) as u8;
        let center_note_fraction = (param.center_note & 0x1FF) as f32 / 512.0;
        let parameter_controller = column![
            row![checkbox(param.mute)
                .label("Mute")
                .on_toggle(|flag| Message::SRNMuteFlagToggled(self.srn_no, flag)),]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![combo_box(
                &self.program_box,
                "Program",
                Some(&param.program),
                move |program| Message::ProgramSelected(srn_no, program),
            ),]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                text("Center Note"),
                number_input(&center_note_int, 0..=127, move |note| {
                    Message::CenterNoteIntChanged(srn_no, note)
                })
                .step(1),
                button("↓").on_press(Message::SRNCenterNoteOctaveDownClicked(self.srn_no)),
                button("↑").on_press(Message::SRNCenterNoteOctaveUpClicked(self.srn_no)),
                text("Fraction"),
                number_input(&center_note_fraction, 0.0..=1.0, move |fraction| {
                    Message::CenterNoteFractionChanged(srn_no, fraction)
                },)
                .step(1.0 / 512.0),
                {
                    let note = param.center_note as f32 / 512.0;
                    text(format!("{:8.2}Hz", note_to_frequency(note))).width(90)
                },
                button("Reset").on_press(Message::SRNNoteEstimationClicked(self.srn_no)),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                text("Velocity"),
                number_input(&param.noteon_velocity, 1..=127, move |velocity| {
                    Message::NoteOnVelocityChanged(srn_no, velocity)
                },)
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                text("Pitch Bend"),
                checkbox(param.enable_pitch_bend)
                    .label("On")
                    .on_toggle(move |flag| Message::EnablePitchBendFlagToggled(srn_no, flag)),
                text("Width (semitone)"),
                number_input(&param.pitch_bend_width, 1..=48, move |width| {
                    Message::PitchBendWidthChanged(srn_no, width)
                },)
                .step(1),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                text("Pan"),
                checkbox(param.auto_pan)
                    .label("Auto")
                    .on_toggle(move |flag| Message::AutoPanFlagToggled(srn_no, flag)),
                number_input(
                    &param.fixed_pan,
                    if param.auto_pan {
                        param.fixed_pan..=param.fixed_pan
                    } else {
                        0..=127
                    },
                    move |pan| { Message::FixedPanChanged(srn_no, pan) }
                )
                .step(1),
                text("Volume"),
                checkbox(param.auto_volume)
                    .label("Auto")
                    .on_toggle(move |flag| Message::AutoVolumeFlagToggled(srn_no, flag)),
                number_input(
                    &param.fixed_volume,
                    if param.auto_volume {
                        param.fixed_volume..=param.fixed_volume
                    } else {
                        0..=127
                    },
                    move |volume| { Message::FixedVolumeChanged(srn_no, volume) }
                )
                .step(1),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
            row![
                checkbox(param.envelope_as_expression)
                    .label("Envelope as Expression")
                    .on_toggle(move |flag| Message::EnvelopeAsExpressionFlagToggled(srn_no, flag)),
                checkbox(param.echo_as_effect1)
                    .label("Echo as Effect1")
                    .on_toggle(move |flag| Message::EchoAsEffect1FlagToggled(srn_no, flag)),
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(alignment::Alignment::Center),
        ];
        let preview_controller = row![
            button("Play / Stop").on_press(Message::ReceivedSRNPlayStartRequest(self.srn_no)),
            button("MIDI Preview").on_press(Message::ReceivedMIDIPreviewRequest(self.srn_no)),
            checkbox(self.preview_loop.load(Ordering::Relaxed))
                .label("Loop")
                .on_toggle(|flag| Message::SRNPlayLoopFlagToggled(flag)),
            checkbox(self.midi_preview.load(Ordering::Relaxed))
                .label("MIDI Update Preview")
                .on_toggle(|flag| Message::SRNMIDIPreviewFlagToggled(flag)),
        ];

        column![
            Canvas::new(self)
                .width(Length::Fill)
                .height(Length::FillPortion(2)),
            parameter_controller
                .spacing(10)
                .width(Length::Fill)
                .height(Length::FillPortion(2)),
            preview_controller
                .spacing(10)
                .width(Length::Fill)
                .height(Length::Shrink)
                .align_y(alignment::Alignment::Center),
        ]
        .spacing(10)
        .padding(10)
        .width(Length::Fill)
        .align_x(alignment::Alignment::Center)
        .into()
    }
}

impl SRNWindow {
    pub fn new(
        title: String,
        srn_no: u8,
        source_info: &SourceInformation,
        source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
        midi_preview: Arc<AtomicBool>,
        preview_loop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            title: title,
            srn_no: srn_no,
            source_info: source_info.clone().into(),
            source_parameter: source_parameter,
            midi_preview: midi_preview,
            preview_loop: preview_loop,
            program_box: combo_box::State::new(Program::ALL.to_vec()),
            cache: Cache::default(),
        }
    }
}

impl canvas::Program<Message> for SRNWindow {
    type State = DrawMode;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        const TIMELABEL_HEIGHT: f32 = 10.0;
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            match state {
                DrawMode::WaveForm => {
                    // 波形描画
                    draw_waveform(
                        frame,
                        &Rectangle::new(
                            Point::new(0.0, 0.0),
                            Size::new(bounds.width, bounds.height),
                        ),
                        &self.source_info.signal,
                        false,
                    );
                    // ループポイント描画
                    draw_loop_point(
                        frame,
                        &Rectangle::new(
                            Point::new(0.0, 0.0),
                            Size::new(bounds.width, bounds.height),
                        ),
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
                    );
                }
                DrawMode::Spectrum => {
                    let log_spec: Vec<_> = self
                        .source_info
                        .power_spectrum
                        .iter()
                        .map(|p| 10.0 * p.log10())
                        .collect();
                    let max = log_spec.iter().max_by(|a, b| a.total_cmp(&b)).unwrap();
                    let min = log_spec.iter().min_by(|a, b| a.total_cmp(&b)).unwrap();
                    if *min < *max {
                        // スペクトラム描画
                        draw_spectrum(
                            frame,
                            &Rectangle::new(
                                Point::new(0.0, 0.0),
                                Size::new(bounds.width, bounds.height),
                            ),
                            &log_spec,
                            (*min, *max),
                        );
                        // スペクトラムピークラベル描画
                        draw_spectrum_peak_label(
                            frame,
                            &Rectangle::new(
                                Point::new(0.0, 0.0),
                                Size::new(bounds.width, bounds.height),
                            ),
                            &log_spec,
                            SPC_SAMPLING_RATE as f32,
                            6,
                        );
                        // ノート番号に相当する周波数を描画
                        let params = self.source_parameter.read().unwrap();
                        let param = params.get(&self.srn_no).unwrap();
                        draw_center_note_hz(
                            frame,
                            &Rectangle::new(
                                Point::new(0.0, 0.0),
                                Size::new(bounds.width, bounds.height),
                            ),
                            &log_spec,
                            SPC_SAMPLING_RATE as f32,
                            note_to_frequency(param.center_note as f32 / 512.0),
                        );
                    }
                }
            }
        });
        vec![geometry]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced_widget::Action<Message>> {
        match event {
            Event::Keyboard(iced::keyboard::Event::KeyReleased {
                key: iced::keyboard::Key::Named(Named::F6),
                ..
            }) => {
                return Some(iced_widget::Action::publish(
                    Message::ReceivedSRNPlayStartRequest(self.srn_no),
                ))
            }
            Event::Keyboard(iced::keyboard::Event::KeyReleased {
                key: iced::keyboard::Key::Named(Named::F7),
                ..
            }) => {
                return Some(iced_widget::Action::publish(
                    Message::ReceivedMIDIPreviewRequest(self.srn_no),
                ))
            }
            _ => {}
        }
        if let Some(_) = cursor.position_in(bounds) {
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    *state = match *state {
                        DrawMode::WaveForm => DrawMode::Spectrum,
                        DrawMode::Spectrum => DrawMode::WaveForm,
                    };
                    self.cache.clear();
                }
                _ => {}
            }
        } else {
            // キャンバス外のイベントの時は画面の再描画を依頼
            self.cache.clear();
        }
        None
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
            style: stroke::Style::Solid(Color::from_rgb8(200, 200, 200)),
            width: 1.5,
            ..Stroke::default()
        },
    );
}

/// 時刻ラベル描画
fn draw_timelabel(frame: &mut Frame, bounds: &Rectangle, sampling_rate: f32, num_samples: usize) {
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
                size: iced::Pixels(16.0),
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

/// スペクトラム描画
fn draw_spectrum(frame: &mut Frame, bounds: &Rectangle, spec: &[f32], db_range: (f32, f32)) {
    const HEIGHT_OFFSET: f32 = 10.0;
    let center = bounds.center();
    let center_left = Point::new(center.x - bounds.width / 2.0, center.y);

    let num_points_to_draw = cmp::min(spec.len(), 4 * bounds.width as usize); // 描画する点数（それ以外は間引く）
    let sample_stride = spec.len() as f32 / num_points_to_draw as f32;

    assert!(db_range.0 < db_range.1);

    // x,y座標の計算クロージャ（周波数軸は対数スケール）
    let normalize = |val: f32, min: f32, max: f32| -> f32 { (val - min) / (max - min) };
    let compute_x = move |s: usize| -> f32 {
        center_left.x
            + bounds.width * normalize((s as f32).log10(), 0.0, ((spec.len() - 1) as f32).log10())
        // 横軸が対数軸なので1オリジン = log(1) = 0
    };
    let compute_y = move |p: f32| -> f32 {
        HEIGHT_OFFSET + bounds.height * (1.0 - normalize(p, db_range.0, db_range.1))
    };

    // 背景を塗りつぶす
    frame.fill_rectangle(
        Point::new(bounds.x, bounds.y),
        Size::new(bounds.width, bounds.height),
        Color::from_rgb8(0, 0, 0),
    );

    // 描画パスを生成
    let path = Path::new(|b| {
        b.move_to(Point::new(center_left.x, compute_y(spec[1]))); // 横軸が対数軸なので1オリジン
        for i in 1..num_points_to_draw {
            b.line_to(Point::new(
                compute_x((i as f32 * sample_stride).round() as usize),
                compute_y(spec[(i as f32 * sample_stride).round() as usize]),
            ));
        }
    });
    // スペクトラム描画
    frame.stroke(
        &path,
        Stroke {
            style: stroke::Style::Solid(Color::from_rgb8(0, 196, 0)),
            width: 1.0,
            ..Stroke::default()
        },
    );
}

/// スペクトラムピークラベル描画
fn draw_spectrum_peak_label(
    frame: &mut Frame,
    bounds: &Rectangle,
    spec: &[f32],
    sampling_rate: f32,
    num_peaks: usize,
) {
    let center = bounds.center();
    let center_left = Point::new(center.x - bounds.width / 2.0, center.y);

    let normalize = |val: f32, min: f32, max: f32| -> f32 { (val - min) / (max - min) };
    let compute_x = move |s: usize| -> f32 {
        center_left.x
            + bounds.width * normalize((s as f32).log10(), 0.0, ((spec.len() - 1) as f32).log10())
    };
    let compute_frequency =
        move |s: usize| -> f32 { sampling_rate * (s as f32) / (2.0 * spec.len() as f32) };

    // スペクトルを降順にソートし対応するビンを並べる
    let mut peak_bins = (0..spec.len()).collect::<Vec<_>>();
    peak_bins.sort_unstable_by(|&i, &j| spec[j].total_cmp(&spec[i]));

    // ピークの周波数を描画
    const FONT_SIZE: f32 = 16.0;
    for i in 0..num_peaks {
        frame.fill_text(canvas::Text {
            content: format!("{:.1}", compute_frequency(peak_bins[i])),
            size: iced::Pixels(FONT_SIZE),
            position: Point::new(
                compute_x(peak_bins[i]),
                bounds.height - FONT_SIZE * (num_peaks - i) as f32,
            ),
            color: Color::WHITE,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Bottom,
            font: Font::MONOSPACE,
            ..canvas::Text::default()
        });
    }
}

/// ノート番号に相当する周波数位置の描画
fn draw_center_note_hz(
    frame: &mut Frame,
    bounds: &Rectangle,
    spec: &[f32],
    sampling_rate: f32,
    center_note_hz: f32,
) {
    let center = bounds.center();
    let center_left = Point::new(center.x - bounds.width / 2.0, center.y);

    let normalize = |val: f32, min: f32, max: f32| -> f32 { (val - min) / (max - min) };
    let bin = 2.0 * spec.len() as f32 * center_note_hz / sampling_rate;
    let line_x = center_left.x
        + bounds.width * normalize(bin.log10(), 0.0, ((spec.len() - 1) as f32).log10());

    let path = Path::new(|b| {
        b.move_to(Point::new(line_x, 0.0));
        b.line_to(Point::new(line_x, bounds.height));
    });
    frame.stroke(
        &path,
        Stroke {
            style: stroke::Style::Solid(Color::from_rgb8(200, 200, 200)),
            width: 1.5,
            ..Stroke::default()
        },
    );
}
