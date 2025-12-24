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
    OpenWindow(WindowType),
    WindowOpened(window::Id),
    WindowClosed(window::Id),
}

#[derive(Debug, Clone)]
enum WindowType {
    Main,
    Config,
    SRN(u8),
}

#[derive(Debug)]
struct Window {
    title: String,
    window_type: WindowType,
}

struct App {
    title: String,
    theme: iced::Theme,
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
            title: "spc2midi-tsuu".to_string(),
            theme: iced::Theme::Nord,
            windows: BTreeMap::new(),
        }
    }
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            App { ..App::default() },
            Task::done(Message::OpenWindow(WindowType::Main)),
        )
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
            Message::OpenWindow(window_type) => {
                let (id, open) = window::open(window::Settings::default());
                let title = match window_type {
                    WindowType::Main => self.title.clone(),
                    WindowType::Config => "Config".to_string(),
                    WindowType::SRN(no) => format!("SRN {}", no),
                };
                let window = Window::new(title, window_type);
                self.windows.insert(id, window);
                return open.map(Message::WindowOpened);
            }
            Message::WindowOpened(id) => {}
            Message::WindowClosed(id) => {
                if let Some(window) = self.windows.get(&id) {
                    return match window.window_type {
                        WindowType::Main => iced::exit(),
                        _ => {
                            self.windows.remove(&id);
                            Task::none()
                        }
                    };
                }
            }
        }
        Task::none()
    }

    fn view(&self, id: window::Id) -> iced::Element<'_, Message> {
        if let Some(window) = self.windows.get(&id) {
            center(window.view()).into()
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
    fn new(title: String, window_type: WindowType) -> Self {
        Self {
            title: title,
            window_type: window_type,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        match self.window_type {
            WindowType::Main => {
                let menu_tpl_1 = |items| Menu::new(items).width(180.0).offset(15.0).spacing(5.0);
                let menu_tpl_2 = |items| Menu::new(items).width(180.0).offset(0.0).spacing(5.0);

                let mb = menu_bar!(
                    (debug_button_s("File"), {
                        let sub1 = menu_tpl_2(menu_items!((debug_button_f("5")),));

                        menu_tpl_1(menu_items!(
                            (submenu_button("A sub menu"), sub1),
                            (debug_button_f("0")),
                        ))
                        .width(140.0)
                    }),
                    (debug_button_s("Option"), {
                        menu_tpl_1(menu_items!(
                            (base_button(
                                text("Config...")
                                    .height(Length::Shrink)
                                    .align_y(alignment::Vertical::Center),
                                Message::OpenWindow(WindowType::Config),
                            )
                            .width(Length::Fill)
                            .height(Length::Shrink)),
                        ))
                        .width(140.0)
                    }),
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

                let r = row![mb, space::horizontal().width(Length::Fill),]
                    .align_y(alignment::Alignment::Center);

                let c = column![
                    r,
                    button(text("New Window")).on_press(Message::OpenWindow(WindowType::SRN(0))),
                    space::vertical().height(Length::Fill),
                ];

                c.into()
            }
            WindowType::Config => {
                let content = column![text("Super awesome config")]
                    .spacing(50)
                    .width(Length::Fill)
                    .align_x(alignment::Alignment::Center)
                    .width(100);
                content.into()
            }
            WindowType::SRN(..) => {
                let content = column![]
                    .spacing(50)
                    .width(Length::Fill)
                    .align_x(alignment::Alignment::Center)
                    .width(100);
                container(scrollable(center_x(content))).padding(10).into()
            }
        }
    }
}
