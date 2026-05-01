use crate::*;
use clap::Parser;
use rimd::SMFWriter;
use std::error;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// SPC(.spc) file
    #[arg(value_name = "FILE")]
    input: PathBuf,

    /// Input JSON file
    #[arg(long, value_name = "FILE")]
    input_json: Option<PathBuf>,

    /// Output SMF (Standard MIDI file)
    #[arg(short, long, value_name = "FILE")]
    output_smf: Option<PathBuf>,

    /// Output JSON file
    #[arg(long, value_name = "FILE")]
    output_json: Option<PathBuf>,
}

#[cfg(windows)]
mod console {
    use windows_sys::Win32::System::Console::{
        AllocConsole, AttachConsole, FreeConsole, ATTACH_PARENT_PROCESS,
    };

    pub fn attach_parent_or_alloc() {
        unsafe {
            // 親が cmd / PowerShell / Windows Terminal ならそこに接続を試みる
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                // 親にコンソールがなければ新規作成
                AllocConsole();
            }
        }
    }

    pub fn detach() {
        unsafe {
            FreeConsole();
        }
    }
}

/// CLUのメイン処理
pub fn cli_main() -> Result<(), Box<dyn error::Error>> {
    // コンソールを作成
    #[cfg(windows)]
    console::attach_parent_or_alloc();

    let args = Args::parse();
    let mut app = App::default();

    // 出力が指定されてない
    if args.output_smf.is_none() && args.output_json.is_none() {
        eprintln!("No output file specified.");
        return Ok(());
    }

    // SPCファイルを開く
    let spc_file = args.input.clone();
    let data = Box::new(std::fs::read(&spc_file)?);
    let _ = app.update(Message::FileOpened(Ok((
        spc_file.into(),
        LoadedFile::SPCFile(*data),
    ))));

    // JSONを開く
    if let Some(json_file) = &args.input_json {
        let json_string = std::fs::read_to_string(&json_file)?;
        let _ = app.update(Message::FileOpened(Ok((
            json_file.into(),
            LoadedFile::JSONFile(json_string),
        ))));
    }

    // MIDIを出力
    if let Some(output_smf) = &args.output_smf {
        let smf = app.create_smf().expect("Failed to generate SMF");
        let writer = SMFWriter::from_smf(smf);
        writer
            .write_to_file(output_smf)
            .expect("Failed to write SMF");
    }

    // JSONを出力
    if let Some(output_json) = &args.output_json {
        let json = app.create_json();
        let file = File::create(output_json)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &json).expect("Faied to write json");
    }

    // コンソールを破棄
    #[cfg(windows)]
    console::detach();

    Ok(())
}
