use crate::eval::MAX_CELLS;
use crate::value::{display_number_precision, Calc};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct CsvTable {
    pub headers: Vec<String>,
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<f64>,
}

impl CsvTable {
    pub fn empty(rows: usize, cols: usize) -> Self {
        let rows = rows.clamp(1, 64);
        let cols = cols.clamp(1, 32);
        Self {
            headers: Vec::new(),
            rows,
            cols,
            values: vec![0.0; rows * cols],
        }
    }

    pub fn to_formula(&self, name: &str) -> Calc<String> {
        let name = name.trim();
        if name.is_empty()
            || !name
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return Err("укажите корректное имя переменной для таблицы (например Data)".into());
        }
        if self.rows == 0 || self.cols == 0 || self.rows * self.cols != self.values.len() {
            return Err("таблица пуста или имеет неверные размеры".into());
        }
        let mut out = format!("{name} := [");
        for r in 0..self.rows {
            if r > 0 {
                out.push_str("; ");
            }
            for c in 0..self.cols {
                if c > 0 {
                    out.push_str(", ");
                }
                let v = self.values[r * self.cols + c];
                if !v.is_finite() {
                    return Err("таблица содержит неконечное число".into());
                }
                out.push_str(&display_number_precision(v, 15));
            }
        }
        out.push(']');
        Ok(out)
    }

    pub fn to_csv_string(&self, delimiter: char, digits: usize) -> String {
        let mut out = String::new();
        if self.headers.len() == self.cols {
            out.push_str(&self.headers.join(&delimiter.to_string()));
            out.push('\n');
        }
        for r in 0..self.rows {
            for c in 0..self.cols {
                if c > 0 {
                    out.push(delimiter);
                }
                out.push_str(&display_number_precision(
                    self.values[r * self.cols + c],
                    digits,
                ));
            }
            out.push('\n');
        }
        out
    }
}

fn detect_delimiter(line: &str) -> char {
    let semi = line.matches(';').count();
    let tab = line.matches('\t').count();
    let comma = line.matches(',').count();
    if tab > 0 && tab >= semi && tab >= comma {
        '\t'
    } else if semi > 0 && semi >= comma {
        ';'
    } else {
        ','
    }
}

fn split_fields(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                current.push('"');
                chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if c == delimiter && !in_quotes {
            fields.push(current.trim().to_owned());
            current.clear();
        } else {
            current.push(c);
        }
    }
    fields.push(current.trim().to_owned());
    fields
}

fn parse_cell(raw: &str, allow_decimal_comma: bool) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = if allow_decimal_comma {
        trimmed.replace(',', ".")
    } else {
        trimmed.to_owned()
    };
    normalized.parse::<f64>().ok().filter(|n| n.is_finite())
}

pub fn parse_csv(text: &str) -> Calc<CsvTable> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err("CSV-файл пуст".into());
    }
    let delimiter = detect_delimiter(lines[0]);
    let allow_decimal_comma = delimiter != ',';
    let first_fields = split_fields(lines[0], delimiter);
    let cols = first_fields.len();
    if cols == 0 {
        return Err("не удалось определить столбцы CSV".into());
    }
    let first_is_numeric = first_fields
        .iter()
        .all(|f| parse_cell(f, allow_decimal_comma).is_some());

    let (headers, data_lines) = if first_is_numeric {
        (Vec::new(), &lines[..])
    } else {
        if lines.len() < 2 {
            return Err("CSV содержит только заголовки без числовых строк".into());
        }
        (first_fields, &lines[1..])
    };
    let rows = data_lines.len();
    if rows == 0 || rows.checked_mul(cols).is_none_or(|n| n > MAX_CELLS) {
        return Err("слишком большая или пустая таблица CSV".into());
    }
    let mut values = Vec::with_capacity(rows * cols);
    for (row_idx, line) in data_lines.iter().enumerate() {
        let fields = split_fields(line, delimiter);
        if fields.len() != cols {
            return Err(format!(
                "в строке {} найдено {} столбцов вместо {}",
                row_idx + 1 + usize::from(!headers.is_empty()),
                fields.len(),
                cols
            ));
        }
        for (col_idx, cell) in fields.iter().enumerate() {
            let num = parse_cell(cell, allow_decimal_comma).ok_or_else(|| {
                format!(
                    "нечисловое значение «{cell}» в строке {}, столбец {}",
                    row_idx + 1 + usize::from(!headers.is_empty()),
                    col_idx + 1
                )
            })?;
            values.push(num);
        }
    }
    Ok(CsvTable {
        headers,
        rows,
        cols,
        values,
    })
}

pub fn read_csv_file(path: &Path) -> Calc<CsvTable> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse_csv(&text)
}

pub fn write_csv_file(path: &Path, csv: &str) -> Calc<()> {
    std::fs::write(path, csv.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_csv_with_headers_and_semicolon_decimal_comma() {
        let raw = "x;y\n0,5;1,25\n1,5;3,75\n2,5;6,25\n";
        let table = parse_csv(raw).unwrap();
        assert_eq!(table.headers, vec!["x", "y"]);
        assert_eq!(table.rows, 3);
        assert_eq!(table.cols, 2);
        assert_eq!(table.values, vec![0.5, 1.25, 1.5, 3.75, 2.5, 6.25]);
        let formula = table.to_formula("T").unwrap();
        assert_eq!(formula, "T := [0.5, 1.25; 1.5, 3.75; 2.5, 6.25]");
    }

    #[test]
    fn rejects_jagged_or_non_numeric_csv() {
        assert!(parse_csv("1,2\n3\n").is_err());
        assert!(parse_csv("a,b\n1,text\n").is_err());
    }
}
