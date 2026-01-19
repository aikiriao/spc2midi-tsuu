use crate::types::*;
use crate::Message;
use iced::border::Radius;
use iced::widget::canvas::{self, Canvas, Event, Frame, Geometry};
use iced::widget::{button, checkbox, column, row, scrollable, space, text, Column};
use iced::{
    alignment, mouse, Border, Color, Element, Font, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme,
};
use iced_aw::menu::{self, Menu};
use iced_aw::style::{menu_bar::primary, Status};
use iced_aw::{menu_bar, menu_items};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
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
    midi_channel_mute: Arc<RwLock<[bool; 8]>>,
    pub playback_time_sec: f32,
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
        midi_channel_mute: Arc<RwLock<[bool; 8]>>,
    ) -> Self {
        Self {
            title: title.clone(),
            base_title: title.clone(),
            theme: theme,
            source_params: source_params,
            playback_status: playback_status,
            pcm_spc_mute: pcm_spc_mute,
            midi_spc_mute: midi_spc_mute,
            midi_channel_mute: midi_channel_mute,
            playback_time_sec: 0.0f32,
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

        let params = self.source_params.read().unwrap();
        let srn_list: Vec<_> = params
            .iter()
            .map(|(key, param)| {
                row![
                    text(format!("0x{:02X}", key)),
                    text(format!(
                        "{} Note:{} Velocity:{}",
                        param.program,
                        param.center_note >> 9,
                        param.noteon_velocity,
                    ))
                    .color(if param.mute {
                        self.theme.palette().warning
                    } else {
                        self.theme.palette().text
                    }),
                    button("Configure").on_press(Message::OpenSRNWindow(*key)),
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();

        let status = self.playback_status.read().unwrap();
        let midi_channel_mute = self.midi_channel_mute.read().unwrap();
        let expression_indicator = self.expression_indicator;
        let pitch_indicator = self.pitch_indicator;
        let volume_indicator = self.volume_indicator;
        let status_list: Vec<_> = (0..8)
            .map(|ch| {
                row![
                    text(format!("{}", ch)).width(10),
                    checkbox(midi_channel_mute[ch])
                        .on_toggle(move |flag| Message::MuteChannel(ch as u8, flag))
                        .width(10),
                    button("S")
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

        let preview_control = row![
            button("Play / Pause").on_press(Message::ReceivedPlayStartRequest),
            button("Stop").on_press(Message::ReceivedPlayStopRequest),
            checkbox(self.pcm_spc_mute.clone().load(Ordering::Relaxed))
                .label("SPC Mute")
                .on_toggle(|flag| Message::SPCMuteFlagToggled(flag)),
            checkbox(self.midi_spc_mute.clone().load(Ordering::Relaxed))
                .label("MIDI Mute")
                .on_toggle(|flag| Message::MIDIMuteFlagToggled(flag)),
            text(format!("{:8.02} sec", self.playback_time_sec)).width(Length::Shrink),
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
        theme.palette().primary,
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
