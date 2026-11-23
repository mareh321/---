use crate::parse::{elements, eval_operand, identifier, kind, named, Xml, ML};
use crate::value::Calc;
use roxmltree::{Document, ParsingOptions};
use std::io::Write;
use std::ops::Range;
use std::path::Path;

const WS: &str = "http://schemas.mathsoft.com/worksheet30";
const MAX_FORMULA_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token<'a> {
    Number(&'a str, bool),
    Id(&'a str),
    Op(&'static str),
    Mark(char),
    Assign,
    End,
}

fn tokenize(mut rest: &str) -> Calc<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    while !rest.is_empty() {
        let first = rest.chars().next().expect("nonempty");
        if first.is_whitespace() {
            rest = &rest[first.len_utf8()..];
            continue;
        }
        let (token, length) = if first.is_ascii_digit()
            || (first == '.' && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
        {
            let bytes = rest.as_bytes();
            let mut i = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            if bytes.get(i) == Some(&b'.') && bytes.get(i + 1) != Some(&b'.') {
                i += 1;
                i += bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
            }
            if matches!(bytes.get(i), Some(b'e' | b'E')) {
                i += 1;
                if matches!(bytes.get(i), Some(b'-' | b'+')) {
                    i += 1;
                }
                let exponent = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
                if exponent == 0 {
                    return Err("после e требуется показатель степени".into());
                }
                i += exponent;
            }
            let imaginary = bytes.get(i) == Some(&b'i')
                && !rest[i + 1..]
                    .chars()
                    .next()
                    .is_some_and(identifier_character);
            let literal = &rest[..i];
            if !literal.parse::<f64>().is_ok_and(f64::is_finite) {
                return Err("неверное число".into());
            }
            (
                Token::Number(literal, imaginary),
                i + usize::from(imaginary),
            )
        } else if first.is_alphabetic() || first == '_' {
            let length = rest
                .char_indices()
                .skip(1)
                .take_while(|&(i, c)| identifier_character(c) && !rest[i..].starts_with(".."))
                .last()
                .map_or(first.len_utf8(), |(i, c)| i + c.len_utf8());
            (Token::Id(&rest[..length]), length)
        } else if let Some((_, token)) = [
            (":=", Token::Assign),
            ("..", Token::Op("range")),
            ("==", Token::Op("equal")),
            ("!=", Token::Op("notEqual")),
            ("<=", Token::Op("lessOrEqual")),
            (">=", Token::Op("greaterOrEqual")),
        ]
        .into_iter()
        .find(|(text, _)| rest.starts_with(text))
        {
            (token, 2)
        } else {
            let token = match first {
                '+' => Token::Op("plus"),
                ':' => Token::Assign,
                '-' | '−' => Token::Op("minus"),
                '*' | '·' | '×' => Token::Op("mult"),
                '/' | '÷' => Token::Op("div"),
                '^' => Token::Op("pow"),
                '<' => Token::Op("lessThan"),
                '>' => Token::Op("greaterThan"),
                '≠' => Token::Op("notEqual"),
                '≤' => Token::Op("lessOrEqual"),
                '≥' => Token::Op("greaterOrEqual"),
                '(' | ')' | '[' | ']' | ',' | ';' | '=' => Token::Mark(first),
                _ => return Err(format!("неожиданный символ «{first}»")),
            };
            (token, first.len_utf8())
        };
        rest = &rest[length..];
        tokens.push(token);
        if tokens.len() > 512 {
            return Err("формула слишком длинная".into());
        }
    }
    tokens.push(Token::End);
    Ok(tokens)
}

fn identifier_character(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

enum Expression {
    Real(String),
    Imaginary(String),
    Id(String),
    Parenthesized(Box<Expression>),
    Operation(&'static str, Vec<Expression>),
    Call(String, Vec<Expression>),
    Index(Box<Expression>, Vec<Expression>),
    Matrix(usize, usize, Vec<Expression>),
    Span(Vec<Expression>),
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    index: usize,
}

fn precedence(op: &str) -> u8 {
    match op {
        "range" => 1,
        "plus" | "minus" => 3,
        "mult" | "div" => 4,
        "pow" => 6,
        _ => 2,
    }
}

impl Parser<'_> {
    fn current(&self) -> Token<'_> {
        self.tokens[self.index]
    }

    fn take(&mut self) -> Token<'_> {
        let index = self.index;
        if self.tokens[index] != Token::End {
            self.index += 1;
        }
        self.tokens[index]
    }

    fn expect(&mut self, token: Token<'_>) -> Calc<()> {
        if self.take() == token {
            Ok(())
        } else {
            Err(format!("ожидалось {}", description(token)))
        }
    }

    fn arguments(&mut self, close: char, depth: usize) -> Calc<Vec<Expression>> {
        if self.current() == Token::Mark(close) {
            self.take();
            return Ok(Vec::new());
        }
        let mut args = vec![self.expr(0, depth + 1)?];
        while self.current() == Token::Mark(',') {
            self.take();
            args.push(self.expr(0, depth + 1)?);
            if args.len() > 32 {
                return Err("слишком много аргументов".into());
            }
        }
        self.expect(Token::Mark(close))?;
        Ok(args)
    }

    fn matrix(&mut self, depth: usize) -> Calc<Expression> {
        let mut rows = 0;
        let mut cols = 0;
        let mut items = Vec::new();
        loop {
            let mut entries = 1;
            items.push(self.expr(0, depth + 1)?);
            while self.current() == Token::Mark(',') {
                self.take();
                items.push(self.expr(0, depth + 1)?);
                entries += 1;
            }
            if rows == 0 {
                cols = entries;
            }
            if cols != entries {
                return Err("в строках матрицы должно быть одинаковое число элементов".into());
            }
            rows += 1;
            if self.current() != Token::Mark(';') {
                break;
            }
            self.take();
        }
        self.expect(Token::Mark(']'))?;
        Ok(Expression::Matrix(rows, cols, items))
    }

    fn expr(&mut self, minimum: u8, depth: usize) -> Calc<Expression> {
        if depth > 48 {
            return Err("формула содержит слишком много вложенных выражений".into());
        }
        let mut left = match self.take() {
            Token::Number(value, false) => Expression::Real(value.into()),
            Token::Number(value, true) => Expression::Imaginary(value.into()),
            Token::Id(name) if !name.ends_with('.') => Expression::Id(name.into()),
            Token::Mark('(') => {
                let inner = self.expr(0, depth + 1)?;
                self.expect(Token::Mark(')'))?;
                Expression::Parenthesized(Box::new(inner))
            }
            Token::Mark('[') => self.matrix(depth)?,
            Token::Op("minus") => {
                let operand = self.expr(5, depth + 1)?;
                let reverse_sign = |number: String| match number.strip_prefix('-') {
                    Some(absolute) => absolute.to_owned(),
                    None => format!("-{number}"),
                };
                match operand {
                    Expression::Real(number) => Expression::Real(reverse_sign(number)),
                    Expression::Imaginary(number) => Expression::Imaginary(reverse_sign(number)),
                    other => Expression::Operation("neg", vec![other]),
                }
            }
            Token::Op("plus") => self.expr(5, depth + 1)?,
            _ => return Err("ожидалось число, имя или выражение в скобках".into()),
        };
        loop {
            match self.current() {
                Token::Mark('(') => {
                    let Expression::Id(name) = left else {
                        return Err("перед скобками требуется имя функции".into());
                    };
                    self.take();
                    left = Expression::Call(name, self.arguments(')', depth)?);
                }
                Token::Mark('[') => {
                    self.take();
                    let args = self.arguments(']', depth)?;
                    if !(1..=2).contains(&args.len()) {
                        return Err("ожидалось один или два индекса".into());
                    }
                    left = Expression::Index(Box::new(left), args);
                }
                Token::Op(op) if precedence(op) >= minimum => {
                    self.take();
                    let right = self.expr(precedence(op) + u8::from(op != "pow"), depth + 1)?;
                    left = if op == "range" {
                        Expression::Span(vec![left, right])
                    } else {
                        Expression::Operation(op, vec![left, right])
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }
}

fn description(token: Token<'_>) -> String {
    match token {
        Token::Mark(mark) => format!("«{mark}»"),
        Token::End => "окончание формулы".into(),
        _ => "продолжение формулы".into(),
    }
}

fn tag(name: &str, body: &str) -> String {
    format!("<ml:{name}>{body}</ml:{name}>")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn id(name: &str) -> String {
    if let Some((name, subscript)) = name.split_once('.') {
        format!(
            "<ml:id subscript=\"{}\" xml:space=\"preserve\">{}</ml:id>",
            escape(subscript),
            escape(name)
        )
    } else {
        format!("<ml:id xml:space=\"preserve\">{}</ml:id>", escape(name))
    }
}

fn args_xml(args: &[Expression]) -> Calc<String> {
    let joined = args
        .iter()
        .map(Expression::xml)
        .collect::<Calc<Vec<_>>>()?
        .join("");
    Ok(if args.len() > 1 {
        tag("sequence", &joined)
    } else {
        joined
    })
}

fn span_xml(args: &[Expression]) -> Calc<String> {
    let body = match args {
        [first, last] => first.xml()? + &last.xml()?,
        [first, next, last] => tag("sequence", &(first.xml()? + &next.xml()?)) + &last.xml()?,
        _ => {
            return Err(
                "range требует первое и последнее значения либо первое, второе и последнее".into(),
            )
        }
    };
    Ok(tag("range", &body))
}

impl Expression {
    fn target(&self) -> Calc<String> {
        match self {
            Self::Id(_) => self.xml(),
            Self::Index(array, _) if matches!(**array, Self::Id(_)) => self.xml(),
            Self::Call(name, args) if args.iter().all(|arg| matches!(arg, Self::Id(_))) => {
                let body = args
                    .iter()
                    .map(Self::xml)
                    .collect::<Calc<Vec<_>>>()?
                    .join("");
                Ok(tag("function", &(id(name) + &tag("boundVars", &body))))
            }
            _ => Err("слева от := ожидается имя, функция или элемент массива".into()),
        }
    }

    fn xml(&self) -> Calc<String> {
        Ok(match self {
            Self::Real(v) => tag("real", v),
            Self::Imaginary(v) => format!("<ml:imag symbol=\"i\">{v}</ml:imag>"),
            Self::Id(name) => id(name),
            Self::Parenthesized(value) => tag("parens", &value.xml()?),
            Self::Operation(op, args) => {
                let body = args
                    .iter()
                    .map(Self::xml)
                    .collect::<Calc<Vec<_>>>()?
                    .join("");
                tag("apply", &format!("<ml:{op}/>{body}"))
            }
            Self::Call(name, args) if name == "range" => span_xml(args)?,
            Self::Call(name, args)
                if matches!(
                    name.as_str(),
                    "sqrt" | "abs" | "conj" | "transpose" | "det" | "factorial"
                ) =>
            {
                let [operand] = args.as_slice() else {
                    return Err(format!("{name} требует один аргумент"));
                };
                let op = match name.as_str() {
                    "abs" => "absval",
                    "conj" => "conjugate",
                    "det" => "determinant",
                    other => other,
                };
                tag("apply", &format!("<ml:{op}/>{}", operand.xml()?))
            }
            Self::Call(name, args) => tag("apply", &(id(name) + &args_xml(args)?)),
            Self::Index(base, args) => tag(
                "apply",
                &(String::from("<ml:indexer/>") + &base.xml()? + &args_xml(args)?),
            ),
            Self::Span(args) => span_xml(args)?,
            Self::Matrix(rows, cols, entries) => {
                let body = entries
                    .iter()
                    .map(Self::xml)
                    .collect::<Calc<Vec<_>>>()?
                    .join("");
                format!("<ml:matrix rows=\"{rows}\" cols=\"{cols}\">{body}</ml:matrix>")
            }
        })
    }
}

#[derive(Default)]
pub struct FormulaSource {
    pub statement: Range<usize>,
    pub input_number: Option<Range<usize>>,
    pub source: Option<String>,
    eval_in_definition: bool,
    unit_override: Option<Range<usize>>,
    namespaces: String,
}

impl FormulaSource {
    pub fn from_statement(statement: Xml<'_, '_>, editable: bool) -> Option<Self> {
        if !matches!(
            kind(statement),
            "define"
                | "eval"
                | "real"
                | "imag"
                | "complex"
                | "apply"
                | "id"
                | "parens"
                | "matrix"
                | "range"
        ) {
            return None;
        }
        let definition_body = named(statement, ML, "define")
            .then(|| elements(statement).nth(1))
            .flatten();
        let expression = definition_body.unwrap_or(statement);
        let input_number = if editable {
            let body = eval_operand(expression);
            if named(body, ML, "real") {
                Some(body.range())
            } else {
                elements(body)
                    .nth(1)
                    .filter(|&n| named(n, ML, "real"))
                    .map(|n| n.range())
            }
        } else {
            None
        };
        let unit_override = if named(expression, ML, "eval") {
            elements(expression)
                .find(|&n| named(n, ML, "unitOverride"))
                .map(|n| n.range())
        } else {
            None
        };
        let namespaces = if unit_override.is_some() {
            expression
                .namespaces()
                .filter_map(|namespace| match namespace.name() {
                    Some("ml" | "xml") => None,
                    Some(name) => Some(format!(" xmlns:{name}=\"{}\"", escape(namespace.uri()))),
                    None => Some(format!(" xmlns=\"{}\"", escape(namespace.uri()))),
                })
                .collect::<String>()
        } else {
            String::new()
        };
        Some(Self {
            statement: statement.range(),
            input_number,
            source: print_formula(statement, 0).ok(),
            eval_in_definition: definition_body.is_some() && named(expression, ML, "eval"),
            unit_override,
            namespaces,
        })
    }

    pub fn replacement(&self, xml: &str, source: &str) -> Calc<String> {
        if source.len() > MAX_FORMULA_BYTES {
            return Err("формула слишком длинная".into());
        }
        let mut parser = Parser {
            tokens: tokenize(source)?,
            index: 0,
        };
        let left = parser.expr(0, 0)?;
        let unit = self
            .unit_override
            .as_ref()
            .map_or("", |range| &xml[range.clone()]);
        let (name, body) = if parser.current() == Token::Assign {
            parser.take();
            let rhs = parser.expr(0, 0)?.xml()?;
            let show_value = parser.current() == Token::Mark('=') || self.eval_in_definition;
            if parser.current() == Token::Mark('=') {
                parser.take();
            }
            let body = if show_value {
                tag("eval", &(rhs + unit))
            } else {
                rhs
            };
            ("define", left.target()? + &body)
        } else {
            if parser.current() == Token::Mark('=') {
                parser.take();
            }
            ("eval", left.xml()? + unit)
        };
        parser.expect(Token::End)?;
        Ok(format!(
            "<ml:{name} xmlns:ml=\"{ML}\"{}>{body}</ml:{name}>",
            self.namespaces
        ))
    }
}

fn fragment(name: &str, content: &str) -> String {
    format!("<ml:{name} xmlns:ml=\"{ML}\">{content}</ml:{name}>")
}

pub fn literal(value: f64) -> String {
    fragment("real", &value.to_string())
}

fn print_formula(node: Xml<'_, '_>, level: usize) -> Calc<String> {
    if level > 48 {
        return Err("слишком большая глубина".into());
    }
    let print = |n| print_formula(n, level + 1);
    let list = |nodes: &[Xml<'_, '_>]| {
        nodes
            .iter()
            .map(|&n| print_formula(n, level + 1))
            .collect::<Calc<Vec<_>>>()
            .map(|v| v.join(", "))
    };
    let items: Vec<_> = elements(node).take(513).collect();
    if items.len() == 513 {
        return Err("слишком много элементов".into());
    }
    let result = match (kind(node), items.as_slice()) {
        ("define", [left, right, ..]) => {
            let nested_eval = named(*right, ML, "eval");
            let suffix = if nested_eval { " =" } else { "" };
            format!(
                "{} := {}{suffix}",
                print(*left)?,
                print(eval_operand(*right))?
            )
        }
        ("eval", [value, ..]) => format!("{} =", print(*value)?),
        ("id", _) => identifier(node)?,
        ("real", _) => node.text().unwrap_or_default().trim().into(),
        ("imag", _) => format!("{}i", node.text().unwrap_or_default().trim()),
        ("complex", [real, imaginary]) => format!("({} + {})", print(*real)?, print(*imaginary)?),
        ("parens", [value]) => format!("({})", print(*value)?),
        ("function", [name, args]) => format!("{}({})", print(*name)?, print(*args)?),
        ("boundVars" | "sequence", args) => list(args)?,
        ("range", args) => format!("range({})", list(args)?),
        ("matrix", entries) => {
            let cols = node
                .attribute("cols")
                .and_then(|c| c.parse::<usize>().ok())
                .filter(|&c| c > 0 && c <= entries.len() && entries.len() % c == 0)
                .ok_or("неверная матрица")?;
            let rows = entries.chunks(cols).map(list).collect::<Calc<Vec<_>>>()?;
            format!("[{}]", rows.join("; "))
        }
        ("apply", [op, args @ ..]) => {
            let tag = kind(*op);
            match (tag, args) {
                ("id", _) => format!("{}({})", print(*op)?, list(args)?),
                ("indexer", [array, indices @ ..]) => {
                    format!("{}[{}]", print(*array)?, list(indices)?)
                }
                ("neg" | "minus", [value]) => format!("-({})", print(*value)?),
                (
                    "sqrt" | "absval" | "conjugate" | "transpose" | "determinant" | "factorial",
                    [value],
                ) => {
                    let name = match tag {
                        "absval" => "abs",
                        "conjugate" => "conj",
                        "determinant" => "det",
                        other => other,
                    };
                    format!("{name}({})", print(*value)?)
                }
                (operation, [a, b]) => {
                    let symbol = match operation {
                        "plus" => "+",
                        "minus" => "-",
                        "mult" | "scale" => "*",
                        "div" => "/",
                        "pow" => "^",
                        "equal" => "==",
                        "notEqual" => "!=",
                        "lessThan" => "<",
                        "greaterThan" => ">",
                        "lessOrEqual" => "<=",
                        "greaterOrEqual" => ">=",
                        _ => return Err("операция без текстовой записи".into()),
                    };
                    format!("({} {symbol} {})", print(*a)?, print(*b)?)
                }
                _ => return Err("операция без текстовой записи".into()),
            }
        }
        _ => return Err("выражение без текстовой записи".into()),
    };
    if result.len() > MAX_FORMULA_BYTES {
        return Err("формула слишком длинная".into());
    }
    Ok(result)
}

pub fn patch(xml: &str, mut changes: Vec<(Range<usize>, String)>) -> Calc<String> {
    if changes.is_empty() {
        return Ok(xml.into());
    }
    changes.sort_by_key(|(range, _)| range.start);
    let mut result = String::with_capacity(xml.len());
    let mut last = 0;
    for (range, replacement) in changes {
        if range.start < last || range.end < range.start || xml.get(range.clone()).is_none() {
            return Err("области замены в XML пересекаются или выходят за границы".into());
        }
        result.push_str(&xml[last..range.start]);
        result.push_str(&replacement);
        last = range.end;
    }
    result.push_str(&xml[last..]);
    Ok(result)
}

pub fn export_xml(xml: &str, changed: bool) -> Calc<String> {
    let doc = Document::parse_with_options(
        xml,
        ParsingOptions {
            nodes_limit: 500_000,
            ..Default::default()
        },
    )
    .map_err(|err| err.to_string())?;
    let root = doc.root_element();
    let mut edits = Vec::new();
    let header_end = xml
        .strip_prefix("<?xml")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .and_then(|rest| rest.find("?>"))
        .map(|end| end + 7);
    let new_encoding = header_end.is_some_and(|end| {
        let header = xml[..end].to_ascii_lowercase();
        header.contains("encoding") && !header.contains("utf-8")
    });
    if changed || new_encoding {
        for node in doc.root().descendants() {
            let cached = changed
                && named(node, ML, "result")
                && node
                    .parent()
                    .is_some_and(|parent| named(parent, ML, "eval"));
            let rendering = changed
                && named(node, WS, "rendering")
                && node
                    .parent()
                    .is_some_and(|parent| elements(parent).any(|n| named(n, WS, "math")));
            let digest = node.parent().is_some_and(|parent| parent.is_root())
                && node
                    .pi()
                    .is_some_and(|pi| pi.target == "validation-md5-digest");
            if cached || rendering || digest {
                edits.push((node.range(), String::new()));
            }
        }
    }
    let declaration = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
    if let Some(end) = header_end.filter(|_| new_encoding) {
        edits.push((0..end, declaration.into()));
    } else if header_end.is_none() && root.range().start == 0 {
        edits.push((0..0, format!("{declaration}\n")));
    }
    patch(xml, edits)
}

pub fn write_copy(path: &Path, xml: &str) -> Calc<()> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                format!(
                    "Файл уже существует. Укажите новое имя для копии: {}",
                    path.display()
                )
            } else {
                format!("{}: {error}", path.display())
            }
        })?;
    if let Err(error) = file.write_all(xml.as_bytes()).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("Не удалось записать копию: {error}"));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub enum ChartSpec {
    Function {
        expression: String,
        from: f64,
        to: f64,
    },
    Vectors {
        x: String,
        y: String,
    },
}

impl ChartSpec {
    pub fn title(&self) -> String {
        match self {
            Self::Function { expression, .. } => expression.clone(),
            Self::Vectors { x, y } => format!("{y} от {x}"),
        }
    }

    fn xml(&self, id: &str) -> String {
        let properties = match self {
            Self::Function {
                expression,
                from,
                to,
            } => format!(
                "mode=\"function\" expression=\"{}\" from=\"{from}\" to=\"{to}\"",
                escape(expression)
            ),
            Self::Vectors { x, y } => {
                format!("mode=\"vectors\" x=\"{}\" y=\"{}\"", escape(x), escape(y))
            }
        };
        format!("<ls:chart xmlns:ls=\"{LIST_NS}\" region=\"{id}\" {properties}/>")
    }
}

const LIST_NS: &str = "urn:list-rs:worksheet";

fn document(xml: &str) -> Calc<Document<'_>> {
    Document::parse_with_options(
        xml,
        ParsingOptions {
            nodes_limit: 500_000,
            ..Default::default()
        },
    )
    .map_err(|err| err.to_string())
}

pub fn charts(xml: &str) -> Calc<std::collections::HashMap<String, ChartSpec>> {
    let doc = document(xml)?;
    let mut result = std::collections::HashMap::new();
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(result);
    };
    for node in elements(point).filter(|&n| named(n, LIST_NS, "chart")) {
        let Some(id) = node.attribute("region") else {
            continue;
        };
        let data = |attr| node.attribute(attr).unwrap_or_default();
        let chart = match data("mode") {
            "function" => {
                let (Ok(from), Ok(to)) = (data("from").parse(), data("to").parse()) else {
                    continue;
                };
                ChartSpec::Function {
                    expression: data("expression").into(),
                    from,
                    to,
                }
            }
            "vectors" => ChartSpec::Vectors {
                x: data("x").into(),
                y: data("y").into(),
            },
            _ => continue,
        };
        result.insert(id.into(), chart);
    }
    Ok(result)
}

fn append_child(xml: &str, node: Xml<'_, '_>, child: &str) -> (Range<usize>, String) {
    let range = node.range();
    let old = &xml[range.clone()];
    if let Some(prefix) = old.strip_suffix("/>") {
        let qname = prefix[1..]
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("regions");
        (range, format!("{prefix}>\n{child}\n</{qname}>"))
    } else {
        let close = range.start + old.rfind("</").expect("XML closing tag");
        (close..close, format!("  {child}\n"))
    }
}

pub enum NewRegion<'a> {
    Formula(&'a str),
    Text(&'a str),
    Chart(&'a ChartSpec),
}

pub fn insert_region(
    xml: &str,
    left: f32,
    top: f32,
    region: NewRegion<'_>,
) -> Calc<(String, String)> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let container = elements(root)
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let last_id = elements(container)
        .filter_map(|n| n.attribute("region-id"))
        .filter_map(|id| id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let id = last_id
        .checked_add(1)
        .ok_or("нет свободного номера региона")?
        .to_string();
    if !(left.is_finite() && top.is_finite()) {
        return Err("неверная позиция на листе".into());
    }
    let left = left.clamp(0., 1_000_000.);
    let top = top.clamp(0., 1_000_000.);
    let mut extras = Vec::new();
    let (content, width, height) = match region {
        NewRegion::Formula(text) => {
            let statement = FormulaSource::default().replacement(xml, text)?;
            (format!("<math>{statement}</math>"), 350, 34)
        }
        NewRegion::Text(text) => {
            if text.trim().is_empty() || text.len() > 24_000 {
                return Err("введите текст до 24 000 байт".into());
            }
            let style = root
                .descendants()
                .find(|&node| named(node, WS, "textStyle"))
                .and_then(|node| node.attribute("name"))
                .map(escape)
                .unwrap_or_else(|| "Normal".into());
            let paragraphs = text
                .lines()
                .map(|line| format!("<p style=\"{style}\">{}</p>", escape(line)))
                .collect::<String>();
            (
                format!("<text>{paragraphs}</text>"),
                480,
                (28 + 19 * (text.lines().count().saturating_sub(1))).min(360),
            )
        }
        NewRegion::Chart(chart) => {
            let data = chart.xml(&id);
            if let Some(node) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
                extras.push(append_child(xml, node, &data));
            } else if let Some(first) = elements(root).next() {
                let at = first.range().start;
                extras.push((
                    at..at,
                    format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
                ));
            }
            ("<plot disable-calc=\"true\"/>".into(), 480, 300)
        }
    };
    let align = top + 10.;
    let node = format!("<region xmlns=\"{WS}\" region-id=\"{id}\" left=\"{left}\" top=\"{top}\" align-y=\"{align}\" width=\"{width}\" height=\"{height}\">{content}</region>");
    extras.push(append_child(xml, container, &node));
    Ok((patch(xml, extras)?, id))
}

pub fn replace_chart(xml: &str, id: &str, spec: &ChartSpec) -> Calc<String> {
    let doc = document(xml)?;
    let point = elements(doc.root_element())
        .find(|&n| named(n, WS, "pointReleaseData"))
        .ok_or("отсутствуют параметры графика")?;
    let node = elements(point)
        .find(|&n| named(n, LIST_NS, "chart") && n.attribute("region") == Some(id))
        .ok_or("отсутствуют параметры графика")?;
    patch(xml, vec![(node.range(), spec.xml(id))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_copy_once_and_refuses_to_replace_it() {
        let path =
            std::env::temp_dir().join(format!("list-rs-copy-test-{}.xmcd", std::process::id()));
        write_copy(&path, "<first/>").unwrap();
        let second = write_copy(&path, "<second/>");
        let actual = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(second.is_err());
        assert_eq!(actual, "<first/>");
    }

    #[test]
    fn edited_export_invalidates_math_caches_and_keeps_plot_data() {
        let old = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
            <?validation-md5-digest 1234?>
            <worksheet xmlns="{WS}" xmlns:ml="{ML}"><regions>
                <region><math><ml:eval><ml:real>1</ml:real><ml:result><ml:real>1</ml:real></ml:result></ml:eval></math><rendering item-idref="2"/></region>
                <region><plot item-idref="5"/><rendering item-idref="6"/></region>
            </regions><binaryContent>DATA</binaryContent></worksheet>"#
        );
        let new = export_xml(&old, true).unwrap();
        assert!(new.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(!new.contains("validation-md5"));
        assert!(!new.contains("ml:result"));
        assert!(!new.contains("item-idref=\"2\""));
        assert!(new.contains("item-idref=\"6\""));
        assert!(new.contains("<binaryContent>DATA</binaryContent>"));
        Document::parse(&new).unwrap();
    }
}
