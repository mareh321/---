use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn s(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    pub fn i(value: i64) -> Self {
        Self::Num(value as f64)
    }

    pub fn u(value: usize) -> Self {
        Self::Num(value as f64)
    }

    pub fn f(value: f64) -> Self {
        if value.is_finite() {
            Self::Num(value)
        } else {
            Self::Null
        }
    }

    pub fn arr(items: Vec<Self>) -> Self {
        Self::Array(items)
    }

    pub fn obj(entries: Vec<(&str, Self)>) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        let Self::Object(entries) = self else {
            return None;
        };
        entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Self::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Num(n) = self {
            Some(*n)
        } else {
            None
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        let n = self.as_f64()?;
        if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= usize::MAX as f64 {
            Some(n as usize)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }

    pub fn as_array(&self) -> Option<&[Self]> {
        if let Self::Array(items) = self {
            Some(items)
        } else {
            None
        }
    }

    pub fn to_json_string(&self) -> String {
        let mut out = String::with_capacity(256);
        self.write_to(&mut out);
        out
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Num(n) => {
                if !n.is_finite() {
                    out.push_str("null");
                } else if *n == 0.0 {
                    out.push('0');
                } else if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{n}");
                }
            }
            Self::Str(s) => write_json_str(s, out),
            Self::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_to(out);
                }
                out.push(']');
            }
            Self::Object(entries) => {
                out.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_str(key, out);
                    out.push(':');
                    value.write_to(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 128 {
            return Err("слишком большая глубина JSON".into());
        }
        self.skip_ws();
        let Some(b) = self.peek() else {
            return Err("неожиданный конец JSON".into());
        };
        match b {
            b'{' => self.parse_object(depth + 1),
            b'[' => self.parse_array(depth + 1),
            b'"' => self.parse_string().map(Json::Str),
            b't' => {
                if self.src[self.pos..].starts_with("true") {
                    self.pos += 4;
                    Ok(Json::Bool(true))
                } else {
                    Err("ожидалось true".into())
                }
            }
            b'f' => {
                if self.src[self.pos..].starts_with("false") {
                    self.pos += 5;
                    Ok(Json::Bool(false))
                } else {
                    Err("ожидалось false".into())
                }
            }
            b'n' => {
                if self.src[self.pos..].starts_with("null") {
                    self.pos += 4;
                    Ok(Json::Null)
                } else {
                    Err("ожидалось null".into())
                }
            }
            b'-' | b'0'..=b'9' => self.parse_number(),
            other => Err(format!("неожиданный символ в JSON: '{}'", other as char)),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Json, String> {
        self.pos += 1; // '{'
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err("ожидался строковый ключ объекта JSON".into());
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err("ожидалось ':' в объекте JSON".into());
            }
            self.pos += 1;
            let value = self.parse_value(depth)?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err("ожидалось ',' или '}' в объекте JSON".into()),
            }
        }
        Ok(Json::Object(entries))
    }

    fn parse_array(&mut self, depth: usize) -> Result<Json, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.parse_value(depth)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err("ожидалось ',' или ']' в массиве JSON".into()),
            }
        }
        Ok(Json::Array(items))
    }

    fn parse_hex4(&mut self) -> Result<u16, String> {
        if self.pos + 4 > self.bytes.len() {
            return Err("неполная последовательность \\uXXXX".into());
        }
        let slice = &self.src[self.pos..self.pos + 4];
        self.pos += 4;
        u16::from_str_radix(slice, 16)
            .map_err(|_| format!("некорректная последовательность \\u{slice}"))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening '"'
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(out);
            }
            if b == b'\\' {
                self.pos += 1;
                let Some(&esc) = self.bytes.get(self.pos) else {
                    return Err("незавершённая escape-последовательность в строке JSON".into());
                };
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{08}'),
                    b'f' => out.push('\u{0C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let code = self.parse_hex4()?;
                        if (0xD800..=0xDBFF).contains(&code) {
                            if !self.src[self.pos..].starts_with("\\u") {
                                return Err(
                                    "ожидалась вторая половина суррогатной пары UTF-16".into()
                                );
                            }
                            self.pos += 2;
                            let low = self.parse_hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err("некорректная суррогатная пара UTF-16".into());
                            }
                            let cp = 0x10000
                                + (((u32::from(code) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
                            let ch =
                                char::from_u32(cp).ok_or("некорректная кодовая точка Unicode")?;
                            out.push(ch);
                        } else {
                            let ch = char::from_u32(u32::from(code))
                                .ok_or("некорректная кодовая точка Unicode")?;
                            out.push(ch);
                        }
                    }
                    other => {
                        return Err(format!(
                            "неизвестная escape-последовательность '\\{}'",
                            other as char
                        ));
                    }
                }
                continue;
            }
            if b < 0x20 {
                return Err("недопустимый управляющий символ в строке JSON".into());
            }
            let ch = self.src[self.pos..]
                .chars()
                .next()
                .ok_or("некорректный UTF-8 в JSON")?;
            out.push(ch);
            self.pos += ch.len_utf8();
        }
        Err("незакрытая строка JSON".into())
    }

    fn parse_number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err("некорректное число JSON".into()),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err("после точки в числе JSON требуются цифры".into());
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err("в экспоненте числа JSON требуются цифры".into());
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let raw = &self.src[start..self.pos];
        let num = raw
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("некорректное число JSON: {raw}"))?;
        Ok(Json::Num(num))
    }
}

pub fn parse_json(input: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        src: input,
        pos: 0,
    };
    let value = parser.parse_value(0)?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err("лишние символы после окончания JSON".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_serializes_nested_json_with_unicode_and_escapes() {
        let raw = r#"{"id":42,"ok":true,"arr":[1,-2.5,null,"Привет\n\"°C\"\u0020\uD83D\uDE00"]}"#;
        let val = parse_json(raw).unwrap();
        assert_eq!(val.get("id").and_then(Json::as_usize), Some(42));
        assert_eq!(val.get("ok").and_then(Json::as_bool), Some(true));
        let arr = val.get("arr").and_then(Json::as_array).unwrap();
        assert_eq!(arr[0].as_f64(), Some(1.0));
        assert_eq!(arr[1].as_f64(), Some(-2.5));
        assert_eq!(arr[2], Json::Null);
        assert_eq!(arr[3].as_str(), Some("Привет\n\"°C\" 😀"));

        let roundtrip = parse_json(&val.to_json_string()).unwrap();
        assert_eq!(roundtrip, val);
        assert!(val.to_json_string().contains("\"id\":42"));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_json("").is_err());
        assert!(parse_json("{\"a\":1").is_err());
        assert!(parse_json("123 trailing").is_err());
        assert!(parse_json("\"unterminated").is_err());
    }
}
