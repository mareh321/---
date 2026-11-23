#[cfg(feature = "desktop")]
pub mod editing;
mod eval;
mod parse;
pub mod plot;
#[cfg(feature = "desktop")]
mod presentation;
pub mod sheet;
mod value;

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

pub use value::{display_number, Calc};
pub const DEMO: &str = include_str!("../examples/pressure.xmcd");
const LIMIT: u64 = 32 * 1024 * 1024;

pub fn read_xml(path: &Path) -> Calc<String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let compressed = reader
        .fill_buf()
        .map_err(|error| error.to_string())?
        .starts_with(&[0x1f, 0x8b]);
    if compressed {
        return decode_xml(flate2::read::MultiGzDecoder::new(reader.take(LIMIT + 1)));
    }
    decode_xml(reader)
}

fn decode_xml(reader: impl Read) -> Calc<String> {
    let mut bytes = Vec::new();
    reader
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("размер XMCD превышает 32 МиБ".into());
    }
    if let Some(xml) = bytes
        .strip_prefix(&[0xff, 0xfe])
        .or_else(|| bytes.strip_prefix(&[0xfe, 0xff]))
    {
        let little_endian = bytes[0] == 0xff;
        let words = xml.chunks_exact(2);
        if !words.remainder().is_empty() {
            return Err("неполный символ UTF-16".into());
        }
        let words: Vec<_> = words
            .map(|pair| {
                let pair = [pair[0], pair[1]];
                if little_endian {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        return String::from_utf16(&words).map_err(|error| error.to_string());
    }
    let start = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        3
    } else {
        0
    };
    String::from_utf8(bytes.split_off(start))
        .map_err(|_| "ожидается XMCD в кодировке UTF-8 или UTF-16 с BOM".into())
}

pub fn number(token: &str) -> Calc<f64> {
    token
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("некорректное число {token}"))
}
