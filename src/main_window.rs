use crate::types::*;
use crate::Message;
use iced::border::Radius;
use iced::widget::canvas::{self, Canvas, Event, Frame, Geometry};
use iced::widget::{button, checkbox, column, row, scrollable, space, text, tooltip, Column};
use iced::{
    alignment, mouse, Border, Color, Element, Font, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme,
};
use iced_aw::menu::{self, Menu};
use iced_aw::style::{menu_bar::primary, Status};
use iced_aw::{menu_bar, menu_items};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct MainWindow {
    pub title: String,
    pub base_title: String,
    theme: iced::Theme,
    source_params: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    playback_status: Arc<RwLock<PlaybackStatus>>,
    pcm_spc_mute: Arc<AtomicBool>,
    midi_spc_mute: Arc<AtomicBool>,
    channel_mute_flags: Arc<AtomicU8>,
    pub playback_time_sec: f32,
    pub midi_bit_rate: f32,
    pub pitch_indicator: [Indicator; 8],
    pub expression_indicator: [Indicator; 8],
    pub volume_indicator: [[Indicator; 2]; 8],
}

impl MainWindow {
    pub fn new(
        title: String,
        theme: iced::Theme,
        source_params: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
        playback_status: Arc<RwLock<PlaybackStatus>>,
        pcm_spc_mute: Arc<AtomicBool>,
        midi_spc_mute: Arc<AtomicBool>,
        channel_mute_flags: Arc<AtomicU8>,
    ) -> Self {
        Self {
            title: title.clone(),
            base_title: title.clone(),
            theme: theme,
            source_params: source_params,
            playback_status: playback_status,
            pcm_spc_mute: pcm_spc_mute,
            midi_spc_mute: midi_spc_mute,
            channel_mute_flags: channel_mute_flags,
            playback_time_sec: 0.0f32,
            midi_bit_rate: 0.0f32,
            expression_indicator: [Indicator::new(0.0, 0.0, 127.0, |value| format!("{:<3}", value));
                8],
            pitch_indicator: [Indicator::new(0.0, -48.0, 48.0, |value| format!("{:+4.1}", value));
                8],
            volume_indicator: [[Indicator::new(0.0, -128.0, 127.0, |value| format!("{}", value));
                2]; 8],
        }
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
                        (menu_button(
                            text("Save JSON...")
                                .height(Length::Shrink)
                                .align_y(alignment::Vertical::Center),
                            Message::SaveJSON,
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
                            Message::OpenPreferencesWindow,
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

        let params = self.source_params.read().unwrap();
        // 音源リスト
        let srn_list: Vec<_> = params
            .iter()
            .map(|(key, param)| {
                row![
                    text(format!("0x{:02X}", key))
                        .width(60)
                        .align_x(alignment::Alignment::Center),
                    text(format!("{}", param.program))
                        .color(if param.mute {
                            self.theme.palette().warning
                        } else {
                            self.theme.palette().text
                        })
                        .width(200)
                        .align_x(alignment::Alignment::Start),
                    text(format!("{:6.2}", param.center_note as f32 / 512.0))
                        .width(60)
                        .align_x(alignment::Alignment::End),
                    text(format!("{}", param.noteon_velocity))
                        .width(60)
                        .align_x(alignment::Alignment::End),
                    button("Open")
                        .on_press(Message::OpenSRNWindow(*key))
                        .width(60),
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();
        // 表インデックス
        let srn_index = row![
            text("SRN").width(60).align_x(alignment::Alignment::Center),
            text("Program")
                .width(200)
                .align_x(alignment::Alignment::Start),
            text("C.Note").width(60).align_x(alignment::Alignment::End),
            text("Velocity")
                .width(60)
                .align_x(alignment::Alignment::End),
            text("Config")
                .width(60)
                .align_x(alignment::Alignment::Center),
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);

        let status = self.playback_status.read().unwrap();
        let channel_mute_flags = self.channel_mute_flags.load(Ordering::Relaxed);
        let expression_indicator = self.expression_indicator;
        let pitch_indicator = self.pitch_indicator;
        let volume_indicator = self.volume_indicator;
        let mut status_list: Vec<_> = (0..8)
            .map(|ch| {
                row![
                    text(format!("{}", ch)).width(10),
                    checkbox((channel_mute_flags >> ch) & 1 != 0)
                        .on_toggle(move |flag| Message::MuteChannel(ch as u8, flag))
                        .width(10),
                    button("S")
                        .style(iced::widget::button::success)
                        .on_press(Message::SoloChannel(ch as u8))
                        .width(30),
                    text(format!("{}", if status.noteon[ch] { "♪" } else { "" }))
                        .align_y(alignment::Alignment::Center)
                        .height(Length::Fill)
                        .width(10),
                    text(format!("0x{:02X}", status.srn_no[ch]))
                        .align_y(alignment::Alignment::Center)
                        .height(Length::Fill)
                        .width(30),
                    {
                        if let Some(param) = params.get(&status.srn_no[ch]) {
                            text(format!("{}", param.program)).color(if param.mute {
                                self.theme.palette().warning
                            } else {
                                self.theme.palette().text
                            })
                        } else {
                            text(format!(""))
                        }
                    }
                    .align_y(alignment::Alignment::Center)
                    .size(14.0)
                    .height(Length::Fill)
                    .width(120),
                    Canvas::new(pitch_indicator[ch])
                        .height(Length::Fill)
                        .width(60),
                    Canvas::new(expression_indicator[ch])
                        .height(Length::Fill)
                        .width(50),
                    Canvas::new(volume_indicator[ch][0])
                        .height(Length::Fill)
                        .width(40),
                    Canvas::new(volume_indicator[ch][1])
                        .height(Length::Fill)
                        .width(40),
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();
        let status_index = row![
            text("Mute").width(35).align_x(alignment::Alignment::Start),
            text("Solo").width(50).align_x(alignment::Alignment::Start),
            text("SRN").width(30).align_x(alignment::Alignment::Start),
            text("Program")
                .width(120)
                .align_x(alignment::Alignment::Start),
            text("Pitch").width(60).align_x(alignment::Alignment::Start),
            text("Env.").width(50).align_x(alignment::Alignment::Start),
            text("Lvol").width(40).align_x(alignment::Alignment::Start),
            text("Rvol").width(40).align_x(alignment::Alignment::Start),
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);
        status_list.insert(0, status_index.into());

        let preview_control = row![
            tooltip(
                button("Play/Pause").on_press(Message::ReceivedPlayStartRequest),
                "(F5)",
                tooltip::Position::FollowCursor,
            ),
            tooltip(
                button("Stop").on_press(Message::ReceivedPlayStopRequest),
                "(F4)",
                tooltip::Position::FollowCursor,
            ),
            checkbox(self.pcm_spc_mute.clone().load(Ordering::Relaxed))
                .label("SPC")
                .on_toggle(|flag| Message::SPCMuteFlagToggled(flag)),
            checkbox(self.midi_spc_mute.clone().load(Ordering::Relaxed))
                .label("MIDI")
                .on_toggle(|flag| Message::MIDIMuteFlagToggled(flag)),
            text(format!("{:8.02}sec", self.playback_time_sec))
                .width(90)
                .align_x(alignment::Alignment::End),
            text(format!("{:8.02}kbps", self.midi_bit_rate / 1000.0))
                .color(if self.midi_bit_rate > 31_500.0 {
                    self.theme.palette().warning
                } else {
                    self.theme.palette().text
                })
                .width(90)
                .align_x(alignment::Alignment::End),
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);

        let r = row![menu_bar, space::horizontal().width(Length::Fill),]
            .align_y(alignment::Alignment::Center);

        let c = column![
            r,
            srn_index,
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

impl Indicator {
    fn new(init_value: f32, min_value: f32, max_value: f32, formatter: fn(f32) -> String) -> Self {
        Self {
            value: init_value,
            min: min_value,
            max: max_value,
            formatter: formatter,
        }
    }
}

impl canvas::Program<Message> for Indicator {
    type State = Option<()>;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // インジケータ描画
        let mut frame = Frame::new(renderer, bounds.size());
        draw_indicator(
            theme,
            &mut frame,
            &Rectangle::new(Point::new(0.0, 0.0), Size::new(bounds.width, bounds.height)),
            self.value,
            self.min,
            self.max,
            self.formatter,
        );
        vec![frame.into_geometry()]
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

/// インジケータ描画
fn draw_indicator(
    theme: &Theme,
    frame: &mut Frame,
    bounds: &Rectangle,
    value: f32,
    min: f32,
    max: f32,
    formatter: fn(f32) -> String,
) {
    let center = bounds.center();

    // 背景を塗りつぶす
    frame.fill_rectangle(
        Point::new(bounds.x, bounds.y),
        Size::new(bounds.width, bounds.height),
        theme.palette().background,
    );

    assert!(min < max);
    let ratio = ((value - min) / (max - min)).clamp(0.0, 1.0);
    frame.fill_rectangle(
        Point::new(bounds.x, bounds.y),
        Size::new(ratio * bounds.width, bounds.height),
        theme.palette().success,
    );

    frame.fill_text(canvas::Text {
        content: formatter(value),
        size: iced::Pixels(16.0),
        position: center,
        color: theme.palette().text,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        font: Font::MONOSPACE,
        ..canvas::Text::default()
    });
}
