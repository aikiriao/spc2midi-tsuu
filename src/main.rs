#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Releaseビルドの時コンソールを非表示

use iced_aw::ICED_AW_FONT_BYTES;
use spc2midi_tsuu::cli::*;
use spc2midi_tsuu::App;
use std::env;

pub fn main() -> iced::Result {
    if env::args().len() == 1 {
        // 引数がなければGUIを起動
        iced::daemon(App::new, App::update, App::view)
            .subscription(App::subscription)
            .title(App::title)
            .theme(App::theme)
            .font(ICED_AW_FONT_BYTES)
            .run()
    } else {
        // CLIで実行
        let _ = cli_main();

        Ok(())
    }
}
