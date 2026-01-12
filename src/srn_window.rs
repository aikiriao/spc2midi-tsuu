use crate::program::*;
use crate::types::*;
use crate::Message;
use crate::SPC_SAMPLING_RATE;
use iced::widget::canvas::{self, stroke, Cache, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::{button, checkbox, column, combo_box, row, text};
use iced::{
    alignment, mouse, window, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme,
};
use iced_aw::number_input;
use num_traits::pow::Pow;
use std::cmp;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct SRNWindow {
    title: String,
    window_id: window::Id,
    srn_no: u8,
    source_info: Arc<SourceInformation>,
    source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    pub enable_loop_play: bool,
    program_box: combo_box::State<Program>,
    cache: Cache,
}

impl SPC2MIDI2Window for SRNWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let srn_no = self.srn_no;
        let params = self.source_parameter.read().unwrap();
        let param = params.get(&self.srn_no).unwrap();
        let center_note_int = (param.center_note >> 8) as u8;
        let center_note_fraction = (param.center_note & 0xFF) as f32 / 256.0;
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
                },)
                .step(1),
                text("Fraction"),
                number_input(&center_note_fraction, 0.0..=1.0, move |fraction| {
                    Message::CenterNoteFractionChanged(srn_no, fraction)
                },)
                .step(1.0 / 256.0),
                button("Reset").on_press(Message::SRNNoteEstimationClicked(
                        self.srn_no,
                )),
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
            button("Play / Stop").on_press(Message::ReceivedSRNPlayStartRequest(
                self.srn_no,
                self.enable_loop_play
            )),
            checkbox(self.enable_loop_play)
                .label("Loop")
                .on_toggle(|flag| Message::SRNPlayLoopFlagToggled(self.window_id, flag)),
            checkbox(param.enable_midi_preview)
                .label("MIDI Preview")
                .on_toggle(|flag| Message::SRNMIDIPreviewFlagToggled(self.srn_no, flag)),
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
        window_id: window::Id,
        title: String,
        srn_no: u8,
        source_info: &SourceInformation,
        source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    ) -> Self {
        Self {
            window_id: window_id,
            title: title,
            srn_no: srn_no,
            source_info: source_info.clone().into(),
            source_parameter: source_parameter,
            enable_loop_play: false,
            program_box: combo_box::State::new(Program::ALL.to_vec()),
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
            );
        });
        vec![geometry]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<iced_widget::Action<Message>> {
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
