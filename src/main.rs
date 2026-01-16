#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Releaseビルドの時コンソールを非表示

use spc2midi_tsuu::App;
use iced_aw::ICED_AW_FONT_BYTES;

pub fn main() -> iced::Result {
    iced::daemon(App::new, App::update, App::view)
        .subscription(App::subscription)
        .title(App::title)
        .theme(App::theme)
        .font(ICED_AW_FONT_BYTES)
        .run()
}
