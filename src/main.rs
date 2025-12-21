use iced::border::Radius;
use iced::widget::{
    button, center, center_x, checkbox, column, container, operation, pick_list, row, scrollable,
    slider, space, text, text_input, toggler, tooltip, vertical_slider, Space,
};
use iced::{
    alignment, theme, window, Border, Color, Element, Function, Length, Padding, Size,
    Subscription, Task, Theme, Vector,
};

use iced_aw::menu::{self, Item, Menu};
use iced_aw::style::{menu_bar::primary, Status};
use iced_aw::{iced_aw_font, menu_bar, menu_items, ICED_AW_FONT_BYTES};
use iced_aw::{quad, widgets::InnerBounds};

use std::collections::BTreeMap;

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
    Debug(String),
    OpenWindow,
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    ScaleInputChanged(window::Id, String),
    ScaleChanged(window::Id, String),
    TitleChanged(window::Id, String),
}

#[derive(Debug)]
struct Window {
    title: String,
    scale_input: String,
    current_scale: f32,
    count: usize,
}

struct App {
    title: String,
    theme: iced::Theme,
    close_on_click: bool,
    windows: BTreeMap<window::Id, Window>,
}

impl Default for App {
    fn default() -> Self {
        let theme = iced::Theme::custom(
            "Custom Theme",
            theme::Palette {
                primary: Color::from([0.45, 0.25, 0.57]),
                ..iced::Theme::Light.palette()
            },
        );

        Self {
            title: "Menu Test".to_string(),
            theme,
            close_on_click: true,
            windows: BTreeMap::new(),
        }
    }
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (_, open) = window::open(window::Settings::default());
        (App { ..App::default() }, open.map(Message::WindowOpened))
    }

    fn title(&self, window_id: window::Id) -> String {
        self.windows
            .get(&window_id)
            .map(|window| window.title.clone())
            .unwrap_or_default()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Debug(s) => {
                self.title = s;
            }
            Message::OpenWindow => {
                let (_, open) = window::open(window::Settings::default());
                return open.map(Message::WindowOpened);
            }
            Message::WindowOpened(id) => {
                let window = Window::new(self.windows.len() + 1);
                let focus_input = operation::focus(format!("input-{id}"));

                self.windows.insert(id, window);

                return focus_input;
            }
            Message::WindowClosed(id) => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    return iced::exit();
                }
            }
            Message::ScaleInputChanged(id, scale) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.scale_input = scale;
                }
            }
            Message::ScaleChanged(id, scale) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.current_scale = scale
                        .parse()
                        .unwrap_or(window.current_scale)
                        .clamp(0.5, 5.0);
                }
            }
            Message::TitleChanged(id, title) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.title = title;
                }
            }
        }
        Task::none()
    }

    fn view(&self, window_id: window::Id) -> iced::Element<'_, Message> {
        if let Some(window) = self.windows.get(&window_id) {
            if window.count == 1 {
                let menu_tpl_1 = |items| Menu::new(items).width(180.0).offset(15.0).spacing(5.0);
                let menu_tpl_2 = |items| Menu::new(items).width(180.0).offset(0.0).spacing(5.0);

                #[rustfmt::skip]
            let mb = menu_bar!(
                (debug_button_s("File"), {
                    let sub5 = menu_tpl_2(menu_items!(
                            (debug_button_f("5")),
                    ));

                    let sub4 = menu_tpl_2(menu_items!(
                            (debug_button_f("4")),
                    )).width(200.0);

                    let sub3 = menu_tpl_2(menu_items!(
                            (debug_button_f("3")),
                            (submenu_button("4"), sub4),
                            (submenu_button("5"), sub5),
                    )).width(180.0);

                    let sub2 = menu_tpl_2(menu_items!(
                            (debug_button_f("2")),
                            (submenu_button("More sub menus"), sub3),
                    )).width(160.0);

                    let sub1 = menu_tpl_2(menu_items!(
                            (debug_button_f("1")),
                            (submenu_button("Another sub menu"), sub2),
                    )).width(220.0);

                    menu_tpl_1(menu_items!(
                            (submenu_button("A sub menu"), sub1),
                            (debug_button_f("0")),
                    )).width(140.0)
                }),
                (debug_button_s("Option"), {
                    menu_tpl_1(menu_items!(
                            (debug_button_f("0")),
                    )).width(140.0)
                }),
                )
                    .draw_path(menu::DrawPath::Backdrop)
                    .close_on_item_click_global(self.close_on_click)
                    .close_on_background_click_global(self.close_on_click)
                    .padding(Padding::new(5.0))
                    .style(|theme:&iced::Theme, status: Status | menu::Style{
                        path_border: Border{
                            radius: Radius::new(0.0),
                            ..Default::default()
                        },
                        path: Color::from_rgb(
                                  theme.extended_palette().primary.weak.color.r * 1.2,
                                  theme.extended_palette().primary.weak.color.g * 1.2,
                                  theme.extended_palette().primary.weak.color.b * 1.2,
                              ).into(),
                              ..primary(theme, status)
                    });

                let r = row![mb, space::horizontal().width(Length::Fill),]
                    .align_y(alignment::Alignment::Center);

                let c = column![
                    r,
                    button(text("New Window")).on_press(Message::OpenWindow),
                    space::vertical().height(Length::Fill),
                ];

                c.into()
            } else {
                center(window.view(window_id)).into()
            }
        } else {
            space().into()
        }
    }

    fn theme(&self, _: window::Id) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        window::close_events().map(Message::WindowClosed)
    }
}

fn base_button<'a>(
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

fn debug_button_s(label: &str) -> Element<'_, Message, iced::Theme, iced::Renderer> {
    base_button(
        text(label)
            .height(Length::Shrink)
            .align_y(alignment::Vertical::Center),
        Message::Debug(label.into()),
    )
    .width(Length::Shrink)
    .height(Length::Shrink)
    .into()
}

fn debug_button_f(label: &str) -> Element<'_, Message, iced::Theme, iced::Renderer> {
    base_button(
        text(label)
            .height(Length::Shrink)
            .align_y(alignment::Vertical::Center),
        Message::Debug(label.into()),
    )
    .width(Length::Fill)
    .height(Length::Shrink)
    .into()
}

fn submenu_button(label: &str) -> Element<'_, Message, iced::Theme, iced::Renderer> {
    row![
        base_button(
            text(label)
                .width(Length::Fill)
                .align_y(alignment::Vertical::Center),
            Message::Debug(label.into())
        ),
        iced_aw_font::right_open()
            .width(Length::Shrink)
            .align_y(alignment::Vertical::Center),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

impl Window {
    fn new(count: usize) -> Self {
        Self {
            title: if count == 1 {
                "spc2midi-tsuu".to_string()
            } else {
                format!("Window_{count}")
            },
            scale_input: "1.0".to_string(),
            current_scale: 1.0,
            count: count,
        }
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        let scale_input = column![
            text("Window scale factor:"),
            text_input("Window Scale", &self.scale_input)
                .on_input(Message::ScaleInputChanged.with(id))
                .on_submit(Message::ScaleChanged(id, self.scale_input.to_string()))
        ];

        let title_input = column![
            text("Window title:"),
            text_input("Window Title", &self.title)
                .on_input(Message::TitleChanged.with(id))
                .id(format!("input-{id}"))
        ];

        let new_window_button = button(text("New Window")).on_press(Message::OpenWindow);

        let content = column![scale_input, title_input, new_window_button]
            .spacing(50)
            .width(Length::Fill)
            .align_x(alignment::Alignment::Center)
            .width(200);

        container(scrollable(center_x(content))).padding(10).into()
    }
}
