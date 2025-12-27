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

use spc700::decoder::*;
use spc700::mididsp::*;
use spc700::spc::*;
use spc700::spc_file::*;
use spc700::types::*;

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

/// 音源情報
#[derive(Debug, Clone)]
struct SourceInformation {
    signal: Vec<i16>,
    start_address: usize,
    end_address: usize,
    loop_start_sample: usize,
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
    spc_file: Option<SPCFile>,
    source_infos: Option<BTreeMap<u8, SourceInformation>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            title: "spc2midi-tsuu".to_string(),
            theme: iced::Theme::Nord,
            windows: BTreeMap::new(),
            spc_file: None,
            source_infos: None,
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
                        self.spc_file = Some(spcfile.clone());
                        self.source_infos = Some(analyze_sources(
                            60 * 10,
                            &spcfile.header.spc_register,
                            &spcfile.ram,
                            &spcfile.dsp_register,
                        ));
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

                let srn_list = scrollable(column![
                    button(text("New Window"))
                        .width(Length::Fill)
                        .on_press(Message::OpenWindow(WindowType::SRN(0))),
                    button(text("New Window"))
                        .width(Length::Fill)
                        .on_press(Message::OpenWindow(WindowType::SRN(1))),
                ]);

                let r = row![menu_bar, space::horizontal().width(Length::Fill),]
                    .align_y(alignment::Alignment::Center);

                let c = column![r, srn_list, space::vertical().height(Length::Fill),];

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

/// 音源ソースの解析
fn analyze_sources(
    analyze_duration_sec: u32,
    register: &SPCRegister,
    ram: &[u8],
    dsp_register: &[u8; 128],
) -> BTreeMap<u8, SourceInformation> {
    const CLOCK_TICK_CYCLE_64KHZ: u32 = 16;
    let analyze_duration_64khz_tick = analyze_duration_sec * 64000;

    // 一定期間シミュレートし、サンプルソース番号とそれに紐づく開始アドレスを取得
    let mut emu: spc700::spc::SPC<spc700::mididsp::MIDIDSP> =
        SPC::new(&register, ram, dsp_register);
    let mut cycle_count = 0;
    let mut tick64khz_count = 0;
    let mut start_address_map = BTreeMap::new();
    while tick64khz_count < analyze_duration_64khz_tick {
        cycle_count += emu.execute_step() as u32;
        if cycle_count >= CLOCK_TICK_CYCLE_64KHZ {
            emu.clock_tick_64k_hz();
            cycle_count -= CLOCK_TICK_CYCLE_64KHZ;
            tick64khz_count += 1;
        }
        // キーオンが打たれていた時のサンプル番号を取得
        let keyon = emu.dsp.read_register(ram, DSP_ADDRESS_KON);
        if keyon != 0 {
            let brr_dir_base_address = (emu.dsp.read_register(ram, DSP_ADDRESS_DIR) as u16) << 8;
            for ch in 0..8 {
                if (keyon >> ch) & 1 != 0 {
                    let sample_source = emu.dsp.read_register(ram, (ch << 4) | DSP_ADDRESS_V0SRCN);
                    let dir_address = (brr_dir_base_address + 4 * (sample_source as u16)) as usize;
                    start_address_map.insert(sample_source, dir_address);
                }
            }
        }
    }

    // 波形情報の読み込み
    let mut source_map = BTreeMap::new();
    for (srn, dir_address) in start_address_map.iter() {
        let mut decoder = Decoder::new();
        let mut signal = Vec::new();
        decoder.keyon(ram, *dir_address);
        // 原音ピッチで終端までデコード
        loop {
            signal.push(decoder.process(ram, 0x1000));
            if decoder.end {
                break;
            }
        }
        // データ追記
        let start_address = make_u16_from_u8(&ram[(*dir_address + 0)..(*dir_address + 2)]) as usize;
        let loop_address = make_u16_from_u8(&ram[(*dir_address + 2)..(*dir_address + 4)]) as usize;
        source_map.insert(
            *srn,
            SourceInformation {
                signal: signal.clone(),
                start_address: start_address,
                end_address: start_address + (signal.len() * 9) / 16,
                loop_start_sample: ((loop_address - start_address) * 16) / 9,
            },
        );
    }

    source_map
}
