use iced::border::Radius;
use iced::widget::{
    button, center, center_x, checkbox, column, container, operation, pick_list, row, scrollable,
    slider, space, text, text_input, toggler, tooltip, vertical_slider, Space,
};
use iced::{
    alignment, event, theme, window, Border, Color, Element, Function, Length, Padding, Size,
    Subscription, Task, Theme, Vector,
};

use iced_aw::menu::{self, Item, Menu};
use iced_aw::style::{menu_bar::primary, Status};
use iced_aw::{iced_aw_font, menu_bar, menu_items, ICED_AW_FONT_BYTES};
use iced_aw::{quad, widgets::InnerBounds};

use rfd::AsyncFileDialog;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;

use spc700::spc_file::*;

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
    OpenWindow(WindowType),
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    OpenFile,
    FileOpened(Result<(PathBuf, Vec<u8>), Error>),
    MenuSelected,
    EventOccurred(iced::Event),
}

#[derive(Debug, Clone)]
enum WindowType {
    Main,
    Preferences,
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
            Message::OpenWindow(window_type) => {
                let (id, open) = window::open(window::Settings::default());
                let title = match window_type {
                    WindowType::Main => self.title.clone(),
                    WindowType::Preferences => "Preferences".to_string(),
                    WindowType::SRN(no) => format!("SRN 0x{:02X}", no),
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
            Message::OpenFile => {
                return Task::perform(open_file(), Message::FileOpened);
            }
            Message::FileOpened(result) => match result {
                Ok((path, data)) => {
                    if let Some(spcfile) = parse_spc_file(&data) {
                        println!(
                            "Info: {} \n\
                            SPC Register PC: {:#X} A: {:#X} X: {:#X} Y: {:#X} PSW: {:#X} SP: {:#X} \n\
                            Music Title: {} \n\
                            Game Title: {} \n\
                            Creator: {} \n\
                            Comment: {} \n\
                            Generate Date: {}/{}/{} \n\
                            Music Duration: {} (sec) \n\
                            Fadeout Time: {} (msec) \n\
                            Composer: {}",
                            std::str::from_utf8(&spcfile.header.info).unwrap(),
                            spcfile.header.spc_register.pc,
                            spcfile.header.spc_register.a,
                            spcfile.header.spc_register.x,
                            spcfile.header.spc_register.y,
                            spcfile.header.spc_register.psw,
                            spcfile.header.spc_register.sp,
                            std::str::from_utf8(&spcfile.header.music_title)
                                .unwrap()
                                .trim_end_matches('\0'),
                            std::str::from_utf8(&spcfile.header.game_title)
                                .unwrap()
                                .trim_end_matches('\0'),
                            std::str::from_utf8(&spcfile.header.creator)
                                .unwrap()
                                .trim_end_matches('\0'),
                            std::str::from_utf8(&spcfile.header.comment)
                                .unwrap()
                                .trim_end_matches('\0'),
                            spcfile.header.generate_date,
                            spcfile.header.generate_month,
                            spcfile.header.generate_year,
                            spcfile.header.duration,
                            spcfile.header.fadeout_time,
                            std::str::from_utf8(&spcfile.header.composer)
                                .unwrap()
                                .trim_end_matches('\0'),
                        );
                    }
                }
                Err(e) => {
                    eprintln!("ERROR: failed to open wav file: {:?}", e);
                }
            },
            Message::MenuSelected => {}
            Message::EventOccurred(event) => match event {
                iced::event::Event::Window(event) => {
                    if let iced::window::Event::FileDropped(path) = event {
                        return Task::perform(load_file(path), Message::FileOpened);
                    }
                }
                _ => {}
            },
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
        Subscription::batch(vec![
            window::close_events().map(Message::WindowClosed),
            event::listen().map(Message::EventOccurred),
        ])
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Error {
    DialogClosed,
    IoError(io::ErrorKind),
}

async fn open_file() -> Result<(PathBuf, Vec<u8>), Error> {
    let picked_file = AsyncFileDialog::new()
        .set_title("Open a file...")
        .add_filter("SPC", &["spc", "SPC"])
        .pick_file()
        .await
        .ok_or(Error::DialogClosed)?;

    load_file(picked_file).await
}

async fn load_file(path: impl Into<PathBuf>) -> Result<(PathBuf, Vec<u8>), Error> {
    let path = path.into();

    if let Some(extension) = path.extension().and_then(OsStr::to_str) {
        match extension.to_lowercase().as_str() {
            "spc" => {
                let data = std::fs::read(&path).unwrap();
                return Ok((path, data.to_vec()));
            }
            _ => {
                return Err(Error::IoError(io::ErrorKind::Unsupported));
            }
        }
    }

    return Err(Error::IoError(io::ErrorKind::Unsupported));
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
                                    Message::OpenWindow(WindowType::Preferences),
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

                let r = row![menu_bar, space::horizontal().width(Length::Fill),]
                    .align_y(alignment::Alignment::Center);

                let c = column![
                    r,
                    button(text("New Window")).on_press(Message::OpenWindow(WindowType::SRN(0))),
                    space::vertical().height(Length::Fill),
                ];

                c.into()
            }
            WindowType::Preferences => {
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
