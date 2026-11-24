use list_rs::mcp::McpServer;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "list-mcp [--sheet ПУТЬ.xmcd]\n\n\
    MCP-сервер (Model Context Protocol, JSON-RPC 2.0 по stdio) для среды «Лист» (list-rs).\n\
    Предназначен для подключения к ИИ-агентам (Codex, Gemini CLI, Cursor и др.).\n\n\
    Опции:\n\
      --sheet ПУТЬ    Открыть указанный лист .xmcd/.xmcdz при запуске\n\
      -V, --version   Показать версию\n\
      -h, --help      Показать справку";

fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    const MAX_FRAME: u64 = 32 * 1024 * 1024;
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = (&mut *reader)
            .take(MAX_FRAME + 1)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if n as u64 > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP frame exceeds 32 MiB",
            ));
        }
        if !line.iter().all(u8::is_ascii_whitespace) {
            return String::from_utf8(line).map(Some).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 MCP frame")
            });
        }
    }
}

fn run() -> Result<(), String> {
    let mut sheet_path: Option<PathBuf> = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "-h" || arg == "--help" {
            eprintln!("{HELP}");
            return Ok(());
        }
        if arg == "-V" || arg == "--version" {
            eprintln!("list-mcp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if arg == "--sheet" {
            let p = args.next().ok_or("после --sheet укажите путь к файлу")?;
            sheet_path = Some(PathBuf::from(p));
        } else {
            return Err(format!("неизвестный аргумент: {}", arg.to_string_lossy()));
        }
    }

    let mut server = McpServer::new(sheet_path.as_deref())?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    while let Some(frame) = read_frame(&mut reader).map_err(|e| e.to_string())? {
        if let Some(reply) = server.handle_message(&frame) {
            writeln!(writer, "{reply}").map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("list-mcp: ошибка: {error}");
            ExitCode::FAILURE
        }
    }
}
