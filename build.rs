use {
    std::{env, io},
    winresource::WindowsResource,
};

fn main() -> io::Result<()> {
    // アイコンを設定（Windowsのみ）
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            .set_icon("spc2midi-tsuu.ico")
            .compile()?;
    }
    Ok(())
}
