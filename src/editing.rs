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
    GlobalAssign,
    LocalAssign,
    SymEval,
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
        } else if first.is_alphabetic() || first == '_' || first == '°' {
            let length = rest
                .char_indices()
                .skip(1)
                .take_while(|&(i, c)| identifier_character(c) && !rest[i..].starts_with(".."))
                .last()
                .map_or(first.len_utf8(), |(i, c)| i + c.len_utf8());
            (Token::Id(&rest[..length]), length)
        } else if rest.starts_with("===") {
            (Token::GlobalAssign, 3)
        } else if let Some((_, token)) = [
            (":=", Token::Assign),
            ("<-", Token::LocalAssign),
            ("->", Token::SymEval),
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
                '≡' => Token::GlobalAssign,
                '←' => Token::LocalAssign,
                '→' => Token::SymEval,
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
    c.is_alphanumeric() || c == '_' || c == '.' || c == '°'
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
    LocalAssign(String, Box<Expression>),
    LocalIndexedAssign(String, Vec<Expression>, Box<Expression>),
    Program(Vec<Expression>),
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

    fn stmt(&mut self, depth: usize) -> Calc<Expression> {
        let lhs = self.expr(0, depth)?;
        if self.current() == Token::LocalAssign {
            self.take();
            let rhs = self.stmt(depth + 1)?;
            return match lhs {
                Expression::Id(name) => Ok(Expression::LocalAssign(name, Box::new(rhs))),
                Expression::Index(base, indices) => {
                    let Expression::Id(name) = *base else {
                        return Err(
                            "слева от <- ожидается имя переменной или элемент массива".into()
                        );
                    };
                    Ok(Expression::LocalIndexedAssign(name, indices, Box::new(rhs)))
                }
                _ => {
                    Err("слева от <- ожидается имя локальной переменной или элемент массива".into())
                }
            };
        }
        Ok(lhs)
    }

    fn arguments(&mut self, close: char, depth: usize) -> Calc<Vec<Expression>> {
        if self.current() == Token::Mark(close) {
            self.take();
            return Ok(Vec::new());
        }
        let mut args = vec![self.stmt(depth + 1)?];
        while self.current() == Token::Mark(',') {
            self.take();
            args.push(self.stmt(depth + 1)?);
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
                    if name == "prog" {
                        let mut stmts = Vec::new();
                        if self.current() != Token::Mark(')') {
                            stmts.push(self.stmt(depth + 1)?);
                            while self.current() == Token::Mark(';') {
                                self.take();
                                if self.current() == Token::Mark(')') {
                                    break;
                                }
                                stmts.push(self.stmt(depth + 1)?);
                                if stmts.len() > 64 {
                                    return Err("слишком много строк в программном блоке".into());
                                }
                            }
                        }
                        self.expect(Token::Mark(')'))?;
                        left = Expression::Program(stmts);
                    } else {
                        left = Expression::Call(name, self.arguments(')', depth)?);
                    }
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
            Self::LocalAssign(name, rhs) => tag("localDefine", &(id(name) + &rhs.stmt_xml()?)),
            Self::LocalIndexedAssign(name, indices, rhs) => {
                let target = tag(
                    "apply",
                    &(String::from("<ml:indexer/>") + &id(name) + &args_xml(indices)?),
                );
                tag("localDefine", &(target + &rhs.stmt_xml()?))
            }
            Self::Program(stmts) => {
                let body = stmts
                    .iter()
                    .map(Self::stmt_xml)
                    .collect::<Calc<Vec<_>>>()?
                    .join("");
                tag("program", &body)
            }
        })
    }

    fn stmt_xml(&self) -> Calc<String> {
        match self {
            Self::Call(name, args) if name == "if" && (2..=3).contains(&args.len()) => {
                let cond = args[0].xml()?;
                let then_part = args[1].stmt_xml()?;
                let else_part = if args.len() == 3 {
                    tag("otherwise", &args[2].stmt_xml()?)
                } else {
                    String::new()
                };
                Ok(tag("if", &(cond + &then_part + &else_part)))
            }
            Self::Call(name, args) if name == "for" && args.len() == 3 => {
                let Expression::Id(var) = &args[0] else {
                    return Err("for требует имя переменной-счётчика первым аргументом".into());
                };
                Ok(tag(
                    "for",
                    &(id(var) + &args[1].xml()? + &args[2].stmt_xml()?),
                ))
            }
            Self::Call(name, args) if name == "while" && args.len() == 2 => {
                Ok(tag("while", &(args[0].xml()? + &args[1].stmt_xml()?)))
            }
            Self::Call(name, args) if name == "return" && args.len() == 1 => {
                Ok(tag("return", &args[0].xml()?))
            }
            Self::Call(name, args) if name == "break" && args.is_empty() => {
                Ok("<ml:break/>".into())
            }
            Self::Call(name, args) if name == "continue" && args.is_empty() => {
                Ok("<ml:continue/>".into())
            }
            _ => self.xml(),
        }
    }
}

#[derive(Default)]
pub struct FormulaSource {
    pub statement: Range<usize>,
    pub input_number: Option<Range<usize>>,
    pub source: Option<String>,
    pub native_symbolic_commands: bool,
    eval_in_definition: bool,
    unit_override: Option<Range<usize>>,
    namespaces: String,
}

impl FormulaSource {
    pub fn from_statement(statement: Xml<'_, '_>, editable: bool) -> Option<Self> {
        if !matches!(
            kind(statement),
            "define"
                | "globalDefine"
                | "eval"
                | "symEval"
                | "real"
                | "imag"
                | "complex"
                | "apply"
                | "id"
                | "parens"
                | "matrix"
                | "range"
                | "program"
        ) {
            return None;
        }
        let definition_body = (named(statement, ML, "define")
            || named(statement, ML, "globalDefine"))
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
            native_symbolic_commands: statement.descendants().any(|node| {
                named(node, ML, "command")
                    && node
                        .parent()
                        .is_some_and(|parent| named(parent, ML, "symEval"))
            }),
            eval_in_definition: definition_body.is_some() && named(expression, ML, "eval"),
            unit_override,
            namespaces,
        })
    }

    pub fn replacement(&self, xml: &str, source: &str) -> Calc<String> {
        if self.native_symbolic_commands {
            return Err("правка импортированных символьных команд пока не поддерживается".into());
        }
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
        let (name, body) = if matches!(parser.current(), Token::Assign | Token::GlobalAssign) {
            let is_global = parser.take() == Token::GlobalAssign;
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
            (
                if is_global { "globalDefine" } else { "define" },
                left.target()? + &body,
            )
        } else if parser.current() == Token::SymEval {
            parser.take();
            ("symEval", left.xml()?)
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
        ("globalDefine", [left, right, ..]) => {
            let nested_eval = named(*right, ML, "eval");
            let suffix = if nested_eval { " =" } else { "" };
            format!(
                "{} === {}{suffix}",
                print(*left)?,
                print(eval_operand(*right))?
            )
        }
        ("eval", [value, ..]) => format!("{} =", print(*value)?),
        ("symEval", [value, rest @ ..]) => {
            let commands = rest
                .iter()
                .copied()
                .filter(|&part| named(part, ML, "command"))
                .map(|command| {
                    let field = elements(command)
                        .next()
                        .ok_or("пустая символьная команда")?;
                    print(field)
                })
                .collect::<Calc<Vec<_>>>()?;
            format!("{} -> {}", print(*value)?, commands.join("; "))
                .trim_end()
                .to_owned()
        }
        ("id", _) => identifier(node)?,
        ("real", _) => node.text().unwrap_or_default().trim().into(),
        ("imag", _) => format!("{}i", node.text().unwrap_or_default().trim()),
        ("complex", [real, imaginary]) => format!("({} + {})", print(*real)?, print(*imaginary)?),
        ("parens", [value]) => format!("({})", print(*value)?),
        ("function", [name, args]) => format!("{}({})", print(*name)?, print(*args)?),
        ("boundVars" | "sequence", args) => list(args)?,
        ("range", args) => format!("range({})", list(args)?),
        ("program", stmts) => {
            let parts = stmts
                .iter()
                .map(|&n| print_formula(n, level + 1))
                .collect::<Calc<Vec<_>>>()?;
            format!("prog({})", parts.join("; "))
        }
        ("localDefine", [lhs, rhs, ..]) => format!("{} <- {}", print(*lhs)?, print(*rhs)?),
        ("if", [cond, then_n]) => format!("if({}, {})", print(*cond)?, print(*then_n)?),
        ("if", [cond, then_n, else_n]) => {
            let else_text = if named(*else_n, ML, "otherwise") {
                let inner: Vec<_> = elements(*else_n).collect();
                if let [one] = inner.as_slice() {
                    print(*one)?
                } else {
                    format!(
                        "prog({})",
                        inner
                            .iter()
                            .map(|&n| print_formula(n, level + 1))
                            .collect::<Calc<Vec<_>>>()?
                            .join("; ")
                    )
                }
            } else {
                print(*else_n)?
            };
            format!("if({}, {}, {else_text})", print(*cond)?, print(*then_n)?)
        }
        ("otherwise", body) => format!(
            "prog({})",
            body.iter()
                .map(|&n| print_formula(n, level + 1))
                .collect::<Calc<Vec<_>>>()?
                .join("; ")
        ),
        ("for", [var, iter, body]) => format!(
            "for({}, {}, {})",
            print(*var)?,
            print(*iter)?,
            print(*body)?
        ),
        ("while", [cond, body]) => format!("while({}, {})", print(*cond)?, print(*body)?),
        ("return", [val]) => format!("return({})", print(*val)?),
        ("break", _) => "break()".into(),
        ("continue", _) => "continue()".into(),
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
                && node.parent().is_some_and(|parent| {
                    (named(node, ML, "result") && named(parent, ML, "eval"))
                        || (named(node, ML, "symResult") && named(parent, ML, "symEval"))
                });
            let rendering = changed
                && named(node, WS, "rendering")
                && node
                    .parent()
                    .is_some_and(|parent| elements(parent).any(|n| named(n, WS, "math")));
            let digest = node.parent().is_some_and(|parent| parent.is_root())
                && node
                    .pi()
                    .is_some_and(|pi| pi.target == "validation-md5-digest");
            if (cached || rendering || digest)
                && !node
                    .ancestors()
                    .skip(1)
                    .any(|parent| named(parent, ML, "symResult") || named(parent, ML, "result"))
            {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AxisNumber {
    #[default]
    General,
    Decimal,
    Scientific,
    Engineering,
    Fraction,
}

impl AxisNumber {
    pub const ALL: [(Self, &'static str); 5] = [
        (Self::General, "Общий"),
        (Self::Decimal, "Десятичный"),
        (Self::Scientific, "Научный"),
        (Self::Engineering, "Инженерный"),
        (Self::Fraction, "Дробь"),
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartLine {
    #[default]
    Solid,
    Dashed,
    Points,
}

impl ChartLine {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Solid, "Сплошная"),
        (Self::Dashed, "Штриховая"),
        (Self::Points, "Точки"),
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartMarker {
    #[default]
    Auto,
    None,
    Circle,
    Square,
    Diamond,
    Triangle,
    Cross,
}

impl ChartMarker {
    pub const ALL: [(Self, &'static str); 7] = [
        (Self::Auto, "Авто"),
        (Self::None, "Без символа"),
        (Self::Circle, "Круг"),
        (Self::Square, "Квадрат"),
        (Self::Diamond, "Ромб"),
        (Self::Triangle, "Треугольник"),
        (Self::Cross, "Крест"),
    ];

    fn xml_name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Diamond => "diamond",
            Self::Triangle => "triangle",
            Self::Cross => "cross",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AxisPlacement {
    Edges,
    #[default]
    Center,
    Hidden,
}

impl AxisPlacement {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Edges, "По краям"),
        (Self::Center, "По центру"),
        (Self::Hidden, "Не отображать"),
    ];

    /// Positions of the Y and X axis lines as fractions from the left and bottom.
    pub fn positions(self, bounds: [[f64; 2]; 2], logarithmic: [bool; 2]) -> [Option<f64>; 2] {
        match self {
            Self::Edges => [Some(0.); 2],
            Self::Hidden => [None; 2],
            Self::Center => std::array::from_fn(|i| {
                let [min, max] = bounds[i];
                (!logarithmic[i] && min < 0. && max > 0.).then(|| -min / (max - min))
            }),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChartTraceStyle {
    pub label: String,
    pub color: Option<[u8; 3]>,
    pub line: ChartLine,
    /// Zero uses the renderer's default width.
    pub width: u8,
    pub marker: ChartMarker,
    /// Diameter in pixels. Zero uses the renderer's default size.
    pub marker_size: u8,
    /// One marker per this many valid points. Zero uses automatic spacing.
    pub marker_every: u16,
}

impl ChartTraceStyle {
    pub fn line_width(&self, default: f32) -> f32 {
        if self.width == 0 {
            default
        } else {
            f32::from(self.width.min(8))
        }
    }

    pub fn marker_shape(&self, global_markers: bool) -> Option<ChartMarker> {
        match self.marker {
            ChartMarker::Auto => {
                (global_markers || self.line == ChartLine::Points).then_some(ChartMarker::Circle)
            }
            ChartMarker::None => None,
            shape => Some(shape),
        }
    }

    pub fn marker_diameter(&self, default: f32) -> f32 {
        if self.marker_size == 0 {
            default
        } else {
            f32::from(self.marker_size.clamp(3, 16))
        }
    }

    pub fn marker_stride(&self, auto_stride: usize) -> usize {
        match self.marker_every {
            0 if self.line == ChartLine::Points => 1,
            0 => auto_stride.max(1),
            every => usize::from(every.min(1000)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LegendPlacement {
    #[default]
    Above,
    UpperLeft,
    UpperRight,
    LowerLeft,
    LowerRight,
    Below,
}

impl LegendPlacement {
    pub const ALL: [(Self, &'static str); 6] = [
        (Self::Above, "Над графиком"),
        (Self::UpperLeft, "Сверху слева"),
        (Self::UpperRight, "Сверху справа"),
        (Self::LowerLeft, "Снизу слева"),
        (Self::LowerRight, "Снизу справа"),
        (Self::Below, "Под графиком"),
    ];

    pub fn is_corner(self) -> bool {
        !matches!(self, Self::Above | Self::Below)
    }

    pub(crate) fn xml_name(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::UpperLeft => "upper-left",
            Self::UpperRight => "upper-right",
            Self::LowerLeft => "lower-left",
            Self::LowerRight => "lower-right",
            Self::Below => "below",
        }
    }

    pub fn corner_box(self, plot: [f64; 4], series_count: usize) -> Option<LegendBox> {
        if !self.is_corner() || series_count == 0 {
            return None;
        }
        let [left, top, right, bottom] = plot;
        let width = (right - left - 12.).min(155.);
        let height = (bottom - top - 12.).min(16. * series_count as f64 + 8.);
        if width < 24. || height < 8. + 12. * series_count as f64 {
            return None;
        }
        let x = if matches!(self, Self::UpperRight | Self::LowerRight) {
            right - width - 6.
        } else {
            left + 6.
        };
        let y = if matches!(self, Self::LowerLeft | Self::LowerRight) {
            bottom - height - 6.
        } else {
            top + 6.
        };
        Some(LegendBox {
            x,
            y,
            width,
            height,
        })
    }
}

pub struct LegendBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LegendBox {
    pub fn row_y(&self, series: usize, count: usize) -> f64 {
        self.y + 4. + series as f64 * (self.height - 8.) / count as f64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartAppearance {
    pub grid: [bool; 2],
    pub numbers: [bool; 2],
    pub divisions: [u8; 2],
    pub axes: AxisPlacement,
    pub number: AxisNumber,
    pub digits: u8,
    pub trailing_zeros: bool,
    pub title: String,
    pub title_below: bool,
    pub legend_placement: LegendPlacement,
    pub traces: [ChartTraceStyle; 4],
}

impl Default for ChartAppearance {
    fn default() -> Self {
        Self {
            grid: [true; 2],
            numbers: [true; 2],
            divisions: [4; 2],
            axes: AxisPlacement::Center,
            number: AxisNumber::General,
            digits: 3,
            trailing_zeros: false,
            title: String::new(),
            title_below: false,
            legend_placement: LegendPlacement::Above,
            traces: Default::default(),
        }
    }
}

impl ChartAppearance {
    pub fn tick(&self, n: f64) -> String {
        let digits = usize::from(self.digits.min(12));
        let fixed = |value: f64, digits: usize| {
            let s = format!("{value:.digits$}");
            if self.trailing_zeros || digits == 0 {
                s
            } else {
                s.trim_end_matches('0').trim_end_matches('.').into()
            }
        };
        let scientific = || {
            let text = format!("{n:.digits$e}");
            if self.trailing_zeros || digits == 0 {
                return text;
            }
            if let Some((mantissa, exponent)) = text.split_once('e') {
                return format!(
                    "{}e{exponent}",
                    mantissa.trim_end_matches('0').trim_end_matches('.')
                );
            }
            text
        };
        match self.number {
            AxisNumber::General if n.abs() >= 10_000. || (n != 0. && n.abs() < 0.001) => {
                format!("{n:.1e}")
            }
            AxisNumber::General | AxisNumber::Decimal => fixed(n, digits),
            AxisNumber::Scientific => scientific(),
            AxisNumber::Engineering if n != 0. && n.is_normal() => {
                let power = (n.abs().log10() / 3.).floor() as i32 * 3;
                format!("{}e{power}", fixed(n / 10_f64.powi(power), digits))
            }
            AxisNumber::Engineering => scientific(),
            AxisNumber::Fraction if n.abs() < 1e12 => {
                let best = (1..=100)
                    .map(|den| {
                        let num = (n * f64::from(den)).round();
                        (num, den, (n - num / f64::from(den)).abs())
                    })
                    .find(|&(_, _, error)| error <= 10_f64.powi(-(digits as i32 + 1)));
                match best {
                    Some((num, 1, _)) => format!("{num:.0}"),
                    Some((num, den, _)) => format!("{num:.0}/{den}"),
                    None => fixed(n, digits),
                }
            }
            AxisNumber::Fraction => fixed(n, digits),
        }
    }

    fn xml_attrs(&self, out: &mut String) {
        let default = Self::default();
        if self == &default {
            return;
        }
        for (i, axis) in ["x", "y"].into_iter().enumerate() {
            if !self.grid[i] {
                out.push_str(&format!(" grid-{axis}=\"false\""));
            }
            if !self.numbers[i] {
                out.push_str(&format!(" numbers-{axis}=\"false\""));
            }
            if self.divisions[i] != default.divisions[i] {
                out.push_str(&format!(" divisions-{axis}=\"{}\"", self.divisions[i]));
            }
        }
        match self.axes {
            AxisPlacement::Edges => out.push_str(" axes=\"edges\""),
            AxisPlacement::Hidden => out.push_str(" axes=\"hidden\""),
            AxisPlacement::Center => {}
        }
        if self.number != default.number {
            let index = AxisNumber::ALL
                .iter()
                .position(|&(kind, _)| kind == self.number)
                .unwrap_or(0);
            out.push_str(&format!(" axis-number=\"{index}\""));
        }
        if self.digits != default.digits {
            out.push_str(&format!(" axis-digits=\"{}\"", self.digits));
        }
        if self.trailing_zeros {
            out.push_str(" axis-zeros=\"true\"");
        }
        if !self.title.trim().is_empty() {
            out.push_str(&format!(" title=\"{}\"", escape(self.title.trim())));
        }
        if self.title_below {
            out.push_str(" title-below=\"true\"");
        }
        if self.legend_placement != LegendPlacement::Above {
            out.push_str(&format!(
                " legend-placement=\"{}\"",
                self.legend_placement.xml_name()
            ));
        }
        for (i, trace) in self.traces.iter().enumerate() {
            if !trace.label.trim().is_empty() {
                out.push_str(&format!(
                    " trace-{i}-label=\"{}\"",
                    escape(trace.label.trim())
                ));
            }
            if let Some([r, g, b]) = trace.color {
                out.push_str(&format!(" trace-{i}-color=\"{r:02x}{g:02x}{b:02x}\""));
            }
            if trace.width != 0 {
                out.push_str(&format!(" trace-{i}-width=\"{}\"", trace.width.min(8)));
            }
            if trace.marker != ChartMarker::Auto {
                out.push_str(&format!(
                    " trace-{i}-marker=\"{}\"",
                    trace.marker.xml_name()
                ));
            }
            if trace.marker_size != 0 {
                out.push_str(&format!(
                    " trace-{i}-marker-size=\"{}\"",
                    trace.marker_size.clamp(3, 16)
                ));
            }
            if trace.marker_every != 0 {
                out.push_str(&format!(
                    " trace-{i}-marker-every=\"{}\"",
                    trace.marker_every.min(1000)
                ));
            }
            let line = match trace.line {
                ChartLine::Solid => continue,
                ChartLine::Dashed => "dashed",
                ChartLine::Points => "points",
            };
            out.push_str(&format!(" trace-{i}-line=\"{line}\""));
        }
    }

    fn from_node(node: Xml<'_, '_>) -> Self {
        let parse = |name: &str| {
            node.attribute(name)
                .and_then(|text| text.parse::<u8>().ok())
        };
        let mut appearance = Self {
            axes: match node.attribute("axes") {
                Some("edges") => AxisPlacement::Edges,
                Some("hidden") => AxisPlacement::Hidden,
                _ => AxisPlacement::Center,
            },
            number: parse("axis-number")
                .and_then(|i| AxisNumber::ALL.get(usize::from(i)))
                .map(|&(kind, _)| kind)
                .unwrap_or_default(),
            digits: parse("axis-digits").unwrap_or(3).min(12),
            trailing_zeros: node.attribute("axis-zeros") == Some("true"),
            title: node.attribute("title").unwrap_or_default().into(),
            title_below: node.attribute("title-below") == Some("true"),
            legend_placement: node
                .attribute("legend-placement")
                .and_then(|name| {
                    LegendPlacement::ALL
                        .iter()
                        .find(|(p, _)| p.xml_name() == name)
                })
                .map(|(placement, _)| *placement)
                .unwrap_or_default(),
            ..Self::default()
        };
        for (i, axis) in ["x", "y"].into_iter().enumerate() {
            appearance.grid[i] = node.attribute(format!("grid-{axis}").as_str()) != Some("false");
            appearance.numbers[i] =
                node.attribute(format!("numbers-{axis}").as_str()) != Some("false");
            appearance.divisions[i] = parse(&format!("divisions-{axis}"))
                .unwrap_or(4)
                .clamp(1, 20);
        }
        for (i, trace) in appearance.traces.iter_mut().enumerate() {
            let attr = |key: &str| node.attribute(format!("trace-{i}-{key}").as_str());
            trace.label = attr("label").unwrap_or_default().into();
            trace.color = attr("color")
                .filter(|s| s.len() == 6)
                .and_then(|s| u32::from_str_radix(s, 16).ok())
                .map(|rgb| {
                    let [_, r, g, b] = rgb.to_be_bytes();
                    [r, g, b]
                });
            trace.width = parse(&format!("trace-{i}-width")).unwrap_or(0).min(8);
            trace.marker = attr("marker")
                .and_then(|name| {
                    ChartMarker::ALL
                        .iter()
                        .find(|(shape, _)| shape.xml_name() == name)
                })
                .map(|(shape, _)| *shape)
                .unwrap_or_default();
            trace.marker_size = parse(&format!("trace-{i}-marker-size"))
                .filter(|&size| size != 0)
                .map_or(0, |size| size.clamp(3, 16));
            trace.marker_every = attr("marker-every")
                .and_then(|text| text.parse::<u16>().ok())
                .unwrap_or(0)
                .min(1000);
            trace.line = match attr("line") {
                Some("dashed") => ChartLine::Dashed,
                Some("points") => ChartLine::Points,
                _ => ChartLine::Solid,
            };
        }
        appearance
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartOptions {
    pub log_x: bool,
    pub log_y: bool,
    pub equal_scale: bool,
    pub markers: bool,
    pub show_legend: bool,
    pub x_min: Option<f64>,
    pub x_max: Option<f64>,
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    pub x_label: String,
    pub y_label: String,
    pub appearance: ChartAppearance,
}

impl Default for ChartOptions {
    fn default() -> Self {
        Self {
            log_x: false,
            log_y: false,
            equal_scale: false,
            markers: false,
            show_legend: true,
            x_min: None,
            x_max: None,
            y_min: None,
            y_max: None,
            x_label: String::new(),
            y_label: String::new(),
            appearance: ChartAppearance::default(),
        }
    }
}

impl ChartOptions {
    fn xml_attrs(&self) -> String {
        let mut out = String::new();
        if self.log_x {
            out.push_str(" log-x=\"true\"");
        }
        if self.log_y {
            out.push_str(" log-y=\"true\"");
        }
        if self.equal_scale {
            out.push_str(" equal-scale=\"true\"");
        }
        if self.markers {
            out.push_str(" markers=\"true\"");
        }
        if !self.show_legend {
            out.push_str(" legend=\"false\"");
        }
        if let Some(v) = self.x_min {
            out.push_str(&format!(" x-min=\"{v}\""));
        }
        if let Some(v) = self.x_max {
            out.push_str(&format!(" x-max=\"{v}\""));
        }
        if let Some(v) = self.y_min {
            out.push_str(&format!(" y-min=\"{v}\""));
        }
        if let Some(v) = self.y_max {
            out.push_str(&format!(" y-max=\"{v}\""));
        }
        if !self.x_label.trim().is_empty() {
            out.push_str(&format!(" x-label=\"{}\"", escape(self.x_label.trim())));
        }
        if !self.y_label.trim().is_empty() {
            out.push_str(&format!(" y-label=\"{}\"", escape(self.y_label.trim())));
        }
        self.appearance.xml_attrs(&mut out);
        out
    }

    fn from_node(node: Xml<'_, '_>) -> Self {
        let opt_f64 = |attr| {
            node.attribute(attr)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        Self {
            log_x: node.attribute("log-x") == Some("true"),
            log_y: node.attribute("log-y") == Some("true"),
            equal_scale: node.attribute("equal-scale") == Some("true"),
            markers: node.attribute("markers") == Some("true"),
            show_legend: node.attribute("legend") != Some("false"),
            x_min: opt_f64("x-min"),
            x_max: opt_f64("x-max"),
            y_min: opt_f64("y-min"),
            y_max: opt_f64("y-max"),
            x_label: node.attribute("x-label").unwrap_or_default().into(),
            y_label: node.attribute("y-label").unwrap_or_default().into(),
            appearance: ChartAppearance::from_node(node),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
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
    Parametric {
        x_expr: String,
        y_expr: String,
        from: f64,
        to: f64,
    },
    Polar {
        expression: String,
        from: f64,
        to: f64,
    },
}

impl ChartSpec {
    pub fn title(&self) -> String {
        match self {
            Self::Function { expression, .. } => expression.clone(),
            Self::Vectors { x, y } => format!("{y} от {x}"),
            Self::Parametric { x_expr, y_expr, .. } => format!("({x_expr}; {y_expr})"),
            Self::Polar { expression, .. } => format!("r(θ) = {expression}"),
        }
    }

    pub fn is_polar(&self) -> bool {
        matches!(self, Self::Polar { .. })
    }

    fn xml_with_options(&self, id: &str, options: &ChartOptions) -> String {
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
            Self::Parametric {
                x_expr,
                y_expr,
                from,
                to,
            } => format!(
                "mode=\"parametric\" x-expr=\"{}\" y-expr=\"{}\" from=\"{from}\" to=\"{to}\"",
                escape(x_expr),
                escape(y_expr)
            ),
            Self::Polar {
                expression,
                from,
                to,
            } => format!(
                "mode=\"polar\" expression=\"{}\" from=\"{from}\" to=\"{to}\"",
                escape(expression)
            ),
        };
        let extra = options.xml_attrs();
        format!("<ls:chart xmlns:ls=\"{LIST_NS}\" region=\"{id}\" {properties}{extra}/>")
    }

    fn xml(&self, id: &str) -> String {
        self.xml_with_options(id, &ChartOptions::default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plot3dMode {
    Surface,
    Contour,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Plot3dSource {
    Function {
        expression: String,
        x_from: f64,
        x_to: f64,
        y_from: f64,
        y_to: f64,
    },
    Matrix {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plot3dSpec {
    pub mode: Plot3dMode,
    pub source: Plot3dSource,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub grid: usize,
}

impl Plot3dSpec {
    pub fn default_surface() -> Self {
        Self {
            mode: Plot3dMode::Surface,
            source: Plot3dSource::Function {
                expression: "sin(x) * cos(y)".into(),
                x_from: -3.14,
                x_to: 3.14,
                y_from: -3.14,
                y_to: 3.14,
            },
            yaw_deg: 38.0,
            pitch_deg: 30.0,
            grid: 18,
        }
    }

    pub fn default_contour() -> Self {
        Self {
            mode: Plot3dMode::Contour,
            source: Plot3dSource::Function {
                expression: "x^2 - y^2".into(),
                x_from: -3.0,
                x_to: 3.0,
                y_from: -3.0,
                y_to: 3.0,
            },
            yaw_deg: 38.0,
            pitch_deg: 30.0,
            grid: 22,
        }
    }

    pub fn title(&self) -> String {
        let prefix = match self.mode {
            Plot3dMode::Surface => "3D: ",
            Plot3dMode::Contour => "Контуры: ",
        };
        match &self.source {
            Plot3dSource::Function { expression, .. } => format!("{prefix}{expression}"),
            Plot3dSource::Matrix { name } => format!("{prefix}{name}"),
        }
    }

    fn xml(&self, id: &str) -> String {
        let mode = match self.mode {
            Plot3dMode::Surface => "surface",
            Plot3dMode::Contour => "contour",
        };
        let yaw = self.yaw_deg.clamp(-360.0, 360.0);
        let pitch = self.pitch_deg.clamp(-85.0, 85.0);
        let grid = self.grid.clamp(6, 40);
        let body = match &self.source {
            Plot3dSource::Function {
                expression,
                x_from,
                x_to,
                y_from,
                y_to,
            } => format!(
                "source=\"function\" expression=\"{}\" x-from=\"{x_from}\" x-to=\"{x_to}\" y-from=\"{y_from}\" y-to=\"{y_to}\"",
                escape(expression)
            ),
            Plot3dSource::Matrix { name } => {
                format!("source=\"matrix\" matrix=\"{}\"", escape(name))
            }
        };
        format!(
            "<ls:plot3d xmlns:ls=\"{LIST_NS}\" region=\"{id}\" mode=\"{mode}\" {body} yaw=\"{yaw}\" pitch=\"{pitch}\" grid=\"{grid}\"/>"
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AreaSpec {
    pub id: String,
    pub title: String,
    pub top: f32,
    pub bottom: f32,
    pub collapsed: bool,
}

impl AreaSpec {
    fn xml(&self) -> String {
        format!(
            "<ls:area xmlns:ls=\"{LIST_NS}\" id=\"{}\" title=\"{}\" top=\"{}\" bottom=\"{}\" collapsed=\"{}\"/>",
            escape(&self.id),
            escape(self.title.trim()),
            self.top.clamp(0., 1_000_000.),
            self.bottom.clamp(0., 1_000_000.),
            if self.collapsed { "true" } else { "false" }
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageSpec {
    pub name: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub data_base64: String,
}

impl ImageSpec {
    fn xml(&self, id: &str) -> String {
        format!(
            "<ls:image xmlns:ls=\"{LIST_NS}\" region=\"{id}\" name=\"{}\" mime=\"{}\" width=\"{}\" height=\"{}\">{}</ls:image>",
            escape(&self.name),
            escape(&self.mime),
            self.width,
            self.height,
            self.data_base64
        )
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

pub fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;
    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn decode_mathcad_plot_item(raw: &[u8], gzipped: bool) -> Option<ChartSpec> {
    let decoded = if gzipped {
        use std::io::Read;
        let mut bytes = Vec::new();
        flate2::read::MultiGzDecoder::new(raw)
            .take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        bytes
    } else {
        raw.to_vec()
    };
    let tree_pos = decoded.windows(5).position(|w| w == b"\x04tree")?;
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut pos = tree_pos;
    while pos + 10 <= decoded.len() {
        let Some(rel) = decoded[pos..]
            .windows(4)
            .position(|w| w == [0x02, 0x0f, 0x00, 0x00])
        else {
            break;
        };
        let idx = pos + rel;
        if idx + 10 > decoded.len() {
            break;
        }
        let b8 = decoded[idx + 8] as usize;
        let b9 = decoded[idx + 9];
        if b9 == 2 && (2..=65).contains(&b8) {
            let nchars = b8 - 1;
            let start = idx + 10;
            let end = start + nchars * 2;
            if end <= decoded.len() {
                let words: Vec<u16> = decoded[start..end]
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                if let Ok(name) = String::from_utf16(&words) {
                    let trimmed = name.trim();
                    if !trimmed.is_empty()
                        && trimmed
                            .chars()
                            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
                    {
                        found.push((idx, trimmed.to_owned()));
                    }
                }
            }
        }
        pos = idx + 4;
    }
    if found.len() < 2 {
        return None;
    }
    // Split Y-axis traces (first group) from X-axis traces (second group) by the largest byte gap.
    let split = (1..found.len())
        .max_by_key(|&i| found[i].0.saturating_sub(found[i - 1].0))
        .unwrap_or(found.len() / 2);
    let y_names: Vec<&str> = found[..split].iter().map(|(_, s)| s.as_str()).collect();
    let x_names: Vec<&str> = found[split..].iter().map(|(_, s)| s.as_str()).collect();
    if y_names.is_empty() || x_names.is_empty() {
        return None;
    }
    Some(ChartSpec::Vectors {
        x: x_names.join("; "),
        y: y_names.join("; "),
    })
}

pub fn charts(xml: &str) -> Calc<std::collections::HashMap<String, ChartSpec>> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let mut result = std::collections::HashMap::new();
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
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
                "parametric" => {
                    let (Ok(from), Ok(to)) = (data("from").parse(), data("to").parse()) else {
                        continue;
                    };
                    ChartSpec::Parametric {
                        x_expr: data("x-expr").into(),
                        y_expr: data("y-expr").into(),
                        from,
                        to,
                    }
                }
                "polar" => {
                    let (Ok(from), Ok(to)) = (data("from").parse(), data("to").parse()) else {
                        continue;
                    };
                    ChartSpec::Polar {
                        expression: data("expression").into(),
                        from,
                        to,
                    }
                }
                _ => continue,
            };
            result.insert(id.into(), chart);
        }
    }
    if let (Some(regions), Some(binary)) = (
        elements(root).find(|&n| named(n, WS, "regions")),
        elements(root).find(|&n| named(n, WS, "binaryContent")),
    ) {
        let mut items = std::collections::HashMap::new();
        for item in elements(binary).filter(|&n| named(n, WS, "item")) {
            if let (Some(id), Some(text)) = (item.attribute("item-id"), item.text()) {
                let gzipped = item.attribute("content-encoding") == Some("gzip");
                items.insert(id, (text, gzipped));
            }
        }
        for region in elements(regions).filter(|&n| named(n, WS, "region")) {
            let Some(region_id) = region.attribute("region-id") else {
                continue;
            };
            if result.contains_key(region_id) {
                continue;
            }
            let Some(plot) = elements(region).find(|&n| matches!(kind(n), "plot" | "xyPlot"))
            else {
                continue;
            };
            let Some(item_ref) = plot.attribute("item-idref") else {
                continue;
            };
            let Some(&(encoded, gzipped)) = items.get(item_ref) else {
                continue;
            };
            if let Some(raw) = decode_base64(encoded) {
                if let Some(spec) = decode_mathcad_plot_item(&raw, gzipped) {
                    result.insert(region_id.into(), spec);
                }
            }
        }
    }
    Ok(result)
}

pub fn chart_defaults(xml: &str) -> Calc<Option<ChartOptions>> {
    let doc = document(xml)?;
    let defaults = elements(doc.root_element())
        .find(|&n| named(n, WS, "pointReleaseData"))
        .and_then(|point| elements(point).find(|&n| named(n, LIST_NS, "chartDefaults")))
        .map(ChartOptions::from_node);
    Ok(defaults)
}

pub fn set_chart_defaults(xml: &str, options: &ChartOptions) -> Calc<String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let data = format!(
        "<ls:chartDefaults xmlns:ls=\"{LIST_NS}\"{}/>",
        options.xml_attrs()
    );
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        let update = elements(point)
            .find(|&n| named(n, LIST_NS, "chartDefaults"))
            .map_or_else(
                || append_child(xml, point, &data),
                |node| (node.range(), data.clone()),
            );
        return patch(xml, vec![update]);
    }
    let at = elements(root)
        .next()
        .ok_or("пустой лист XMCD")?
        .range()
        .start;
    patch(
        xml,
        vec![(
            at..at,
            format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
        )],
    )
}

pub fn chart_options(xml: &str) -> Calc<std::collections::HashMap<String, ChartOptions>> {
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
        result.insert(id.into(), ChartOptions::from_node(node));
    }
    Ok(result)
}

pub fn plots3d(xml: &str) -> Calc<std::collections::HashMap<String, Plot3dSpec>> {
    let doc = document(xml)?;
    let mut result = std::collections::HashMap::new();
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(result);
    };
    for node in elements(point).filter(|&n| named(n, LIST_NS, "plot3d")) {
        let Some(id) = node.attribute("region") else {
            continue;
        };
        let mode = match node.attribute("mode") {
            Some("contour") => Plot3dMode::Contour,
            _ => Plot3dMode::Surface,
        };
        let yaw_deg = node
            .attribute("yaw")
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(38.0)
            .clamp(-360.0, 360.0);
        let pitch_deg = node
            .attribute("pitch")
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(30.0)
            .clamp(-85.0, 85.0);
        let grid = node
            .attribute("grid")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(18)
            .clamp(6, 40);
        let source = match node.attribute("source") {
            Some("matrix") => Plot3dSource::Matrix {
                name: node.attribute("matrix").unwrap_or_default().into(),
            },
            _ => {
                let parse_f = |k: &str, def: f64| {
                    node.attribute(k)
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(def)
                };
                Plot3dSource::Function {
                    expression: node.attribute("expression").unwrap_or_default().into(),
                    x_from: parse_f("x-from", -3.0),
                    x_to: parse_f("x-to", 3.0),
                    y_from: parse_f("y-from", -3.0),
                    y_to: parse_f("y-to", 3.0),
                }
            }
        };
        result.insert(
            id.into(),
            Plot3dSpec {
                mode,
                source,
                yaw_deg,
                pitch_deg,
                grid,
            },
        );
    }
    Ok(result)
}

pub fn replace_plot3d(xml: &str, id: &str, spec: &Plot3dSpec) -> Calc<String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let data = spec.xml(id);
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        if let Some(node) = elements(point)
            .find(|&n| named(n, LIST_NS, "plot3d") && n.attribute("region") == Some(id))
        {
            return patch(xml, vec![(node.range(), data)]);
        }
        return patch(xml, vec![append_child(xml, point, &data)]);
    }
    let first = elements(root).next().ok_or("пустой лист XMCD")?;
    let at = first.range().start;
    patch(
        xml,
        vec![(
            at..at,
            format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
        )],
    )
}

pub fn areas(xml: &str) -> Calc<Vec<AreaSpec>> {
    let doc = document(xml)?;
    let mut result = Vec::new();
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(result);
    };
    for node in elements(point).filter(|&n| named(n, LIST_NS, "area")) {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let top = parse_coord(node, "top").unwrap_or(0.);
        let bottom = parse_coord(node, "bottom").unwrap_or(top + 120.);
        result.push(AreaSpec {
            id: id.into(),
            title: node.attribute("title").unwrap_or("Область").into(),
            top,
            bottom: bottom.max(top + 20.),
            collapsed: node.attribute("collapsed") == Some("true"),
        });
    }
    result.sort_by(|a, b| a.top.total_cmp(&b.top));
    Ok(result)
}

pub fn insert_area(xml: &str, title: &str, top: f32, bottom: f32) -> Calc<(String, String)> {
    let title = title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err("укажите название области (до 200 символов)".into());
    }
    if !(top.is_finite() && bottom.is_finite()) || bottom <= top + 10. {
        return Err("нижняя граница области должна быть ниже верхней".into());
    }
    let existing = areas(xml)?;
    let next_num = existing
        .iter()
        .filter_map(|a| a.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let id = next_num.to_string();
    let spec = AreaSpec {
        id: id.clone(),
        title: title.into(),
        top,
        bottom,
        collapsed: false,
    };
    let doc = document(xml)?;
    let root = doc.root_element();
    let data = spec.xml();
    let updated = if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        patch(xml, vec![append_child(xml, point, &data)])?
    } else {
        let first = elements(root).next().ok_or("пустой лист XMCD")?;
        let at = first.range().start;
        patch(
            xml,
            vec![(
                at..at,
                format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
            )],
        )?
    };
    Ok((updated, id))
}

pub fn toggle_area(xml: &str, area_id: &str) -> Calc<String> {
    let doc = document(xml)?;
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Err("область не найдена".into());
    };
    let node = elements(point)
        .find(|&n| named(n, LIST_NS, "area") && n.attribute("id") == Some(area_id))
        .ok_or("область не найдена")?;
    let spec = AreaSpec {
        id: area_id.into(),
        title: node.attribute("title").unwrap_or("Область").into(),
        top: parse_coord(node, "top").unwrap_or(0.),
        bottom: parse_coord(node, "bottom").unwrap_or(120.),
        collapsed: node.attribute("collapsed") != Some("true"),
    };
    patch(xml, vec![(node.range(), spec.xml())])
}

pub fn delete_area(xml: &str, area_id: &str) -> Calc<String> {
    let doc = document(xml)?;
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(xml.into());
    };
    if let Some(node) =
        elements(point).find(|&n| named(n, LIST_NS, "area") && n.attribute("id") == Some(area_id))
    {
        return patch(xml, vec![(node.range(), String::new())]);
    }
    Ok(xml.into())
}

pub fn images(xml: &str) -> Calc<std::collections::HashMap<String, ImageSpec>> {
    let doc = document(xml)?;
    let mut result = std::collections::HashMap::new();
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(result);
    };
    for node in elements(point).filter(|&n| named(n, LIST_NS, "image")) {
        let Some(id) = node.attribute("region") else {
            continue;
        };
        let width = node
            .attribute("width")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(280);
        let height = node
            .attribute("height")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(180);
        result.insert(
            id.into(),
            ImageSpec {
                name: node.attribute("name").unwrap_or("Изображение").into(),
                mime: node.attribute("mime").unwrap_or("image/png").into(),
                width,
                height,
                data_base64: node.text().unwrap_or_default().trim().into(),
            },
        );
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStyle {
    Normal,
    Heading1,
    Heading2,
}

impl TextStyle {
    pub fn from_heading_level(level: u8) -> Self {
        match level {
            1 => Self::Heading1,
            2 => Self::Heading2,
            _ => Self::Normal,
        }
    }

    pub fn xmcd_name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Heading1 => "Heading 1",
            Self::Heading2 => "Heading 2",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFont {
    Arial,
    Times,
    Courier,
}

impl TextFont {
    pub const OPTIONS: [Self; 3] = [Self::Arial, Self::Times, Self::Courier];

    pub fn name(self) -> &'static str {
        match self {
            Self::Arial => "Arial",
            Self::Times => "Times New Roman",
            Self::Courier => "Courier New",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

impl TextAlignment {
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFormat {
    pub font: TextFont,
    pub size: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub alignment: TextAlignment,
}

impl TextFormat {
    pub fn for_heading(heading: u8) -> Self {
        Self {
            font: TextFont::Arial,
            size: match heading {
                1 => 18,
                2 => 14,
                _ => 10,
            },
            bold: heading != 0,
            italic: false,
            underline: false,
            alignment: TextAlignment::Left,
        }
    }
}

const FORMAT_STYLE_SEPARATOR: &str = " (List ";

pub(crate) fn text_heading(style: &str) -> u8 {
    let base = style.split(FORMAT_STYLE_SEPARATOR).next().unwrap_or(style);
    let base = base.to_lowercase();
    if base == "heading 1" || base.contains("title") || base == "заголовок 1" {
        1
    } else if base.starts_with("heading") || base.starts_with("заголовок") {
        2
    } else {
        0
    }
}

fn text_styles<'a, 'input>(root: Xml<'a, 'input>) -> Option<Xml<'a, 'input>> {
    ["settings", "presentation", "textRendering", "textStyles"]
        .into_iter()
        .try_fold(root, |parent, tag| {
            elements(parent).find(|&node| named(node, WS, tag))
        })
}

pub(crate) fn paragraph_text_format(p: Xml<'_, '_>) -> Option<TextFormat> {
    let styles = text_styles(p.document().root_element())?;
    let style = p.attribute("style").unwrap_or("Normal");
    let mut format = TextFormat::for_heading(text_heading(style));
    let definition = elements(styles)
        .find(|&node| named(node, WS, "textStyle") && node.attribute("name") == Some(style));
    if definition.is_some_and(|node| node.attribute("base-style").is_some()) {
        return None;
    }
    let inline =
        definition.and_then(|def| elements(def).find(|&node| named(node, WS, "inlineAttr")));
    if let Some(inline) = inline {
        if inline.attribute("line-through") == Some("true")
            || !matches!(
                inline.attribute("vertical-align"),
                None | Some("baseline" | "inherit")
            )
        {
            return None;
        }
        if let Some(name) = inline
            .attribute("font-family")
            .filter(|name| *name != "inherit")
        {
            format.font = TextFont::OPTIONS
                .into_iter()
                .find(|font| font.name().eq_ignore_ascii_case(name))?;
        }
        if let Some(size) = inline
            .attribute("font-size")
            .filter(|size| *size != "inherit")
        {
            format.size = size
                .parse::<u16>()
                .ok()
                .filter(|size| (6..=72).contains(size))?;
        }
        if let Some(weight) = inline
            .attribute("font-weight")
            .filter(|weight| *weight != "inherit")
        {
            format.bold = weight == "bold";
        }
        format.italic = inline.attribute("font-style") == Some("italic");
        format.underline = inline.attribute("underline") == Some("true");
    }
    let inherited_alignment = definition
        .and_then(|def| elements(def).find(|&node| named(node, WS, "blockAttr")))
        .and_then(|block| block.attribute("text-align"));
    format.alignment = match p
        .attribute("text-align")
        .filter(|align| *align != "inherit")
        .or(inherited_alignment)
    {
        None | Some("left" | "inherit") => TextAlignment::Left,
        Some("center") => TextAlignment::Center,
        Some("right") => TextAlignment::Right,
        _ => return None,
    };
    Some(format)
}

pub(crate) fn region_text_format(text: Xml<'_, '_>) -> Option<TextFormat> {
    let mut paragraphs = elements(text);
    let first = paragraphs.next()?;
    let style = first.attribute("style").unwrap_or("Normal");
    let read = |p| {
        if !named(p, WS, "p")
            || elements(p).next().is_some()
            || p.attribute("style").unwrap_or("Normal") != style
        {
            return None;
        }
        paragraph_text_format(p)
    };
    let initial = read(first)?;
    paragraphs
        .all(|p| read(p) == Some(initial))
        .then_some(initial)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageSettings {
    pub header: String,
    pub footer: String,
    pub show_page_numbers: bool,
    pub page_height: f32,
}

impl Default for PageSettings {
    fn default() -> Self {
        Self {
            header: String::new(),
            footer: String::new(),
            show_page_numbers: true,
            page_height: 840.0,
        }
    }
}

pub enum NewRegion<'a> {
    Formula(&'a str),
    Text(&'a str),
    StyledText(&'a str, TextStyle),
    Chart(&'a ChartSpec),
    ChartConfigured(&'a ChartSpec, &'a ChartOptions),
    Reference(&'a str),
    PageBreak,
    Plot3d(&'a Plot3dSpec),
    Image(&'a ImageSpec),
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
        NewRegion::ChartConfigured(chart, options) => {
            let data = chart.xml_with_options(&id, options);
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
        NewRegion::Reference(href) => {
            let href = href.trim();
            if href.is_empty() || href.len() > 2048 {
                return Err("укажите путь к подключаемому файлу XMCD".into());
            }
            (format!("<reference href=\"{}\"/>", escape(href)), 440, 26)
        }
        NewRegion::StyledText(text, style) => {
            if text.trim().is_empty() || text.len() > 24_000 {
                return Err("введите текст до 24 000 байт".into());
            }
            let style_name = style.xmcd_name();
            let paragraphs = text
                .lines()
                .map(|line| format!("<p style=\"{style_name}\">{}</p>", escape(line)))
                .collect::<String>();
            let line_h = match style {
                TextStyle::Heading1 => 30,
                TextStyle::Heading2 => 24,
                TextStyle::Normal => 19,
            };
            (
                format!("<text>{paragraphs}</text>"),
                480,
                (30 + line_h * (text.lines().count().saturating_sub(1))).min(420),
            )
        }
        NewRegion::PageBreak => ("<pageBreak/>".into(), 560, 18),
        NewRegion::Plot3d(spec) => {
            let data = spec.xml(&id);
            if let Some(node) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
                extras.push(append_child(xml, node, &data));
            } else if let Some(first) = elements(root).next() {
                let at = first.range().start;
                extras.push((
                    at..at,
                    format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
                ));
            }
            ("<plot disable-calc=\"true\"/>".into(), 480, 340)
        }
        NewRegion::Image(img) => {
            if img.data_base64.len() > 6 * 1024 * 1024 {
                return Err("изображение слишком большое (лимит ~4 МБ)".into());
            }
            let data = img.xml(&id);
            if let Some(node) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
                extras.push(append_child(xml, node, &data));
            } else if let Some(first) = elements(root).next() {
                let at = first.range().start;
                extras.push((
                    at..at,
                    format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
                ));
            }
            let w = (img.width as usize).clamp(80, 520);
            let h = (img.height as usize).clamp(60, 420);
            ("<image/>".into(), w, h)
        }
    };
    let align = top + 10.;
    let node = format!("<region xmlns=\"{WS}\" region-id=\"{id}\" left=\"{left}\" top=\"{top}\" align-y=\"{align}\" width=\"{width}\" height=\"{height}\">{content}</region>");
    extras.push(append_child(xml, container, &node));
    Ok((patch(xml, extras)?, id))
}

pub fn replace_chart(xml: &str, id: &str, spec: &ChartSpec) -> Calc<String> {
    replace_chart_with_options(xml, id, spec, &ChartOptions::default())
}

pub fn replace_chart_with_options(
    xml: &str,
    id: &str,
    spec: &ChartSpec,
    options: &ChartOptions,
) -> Calc<String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let data = spec.xml_with_options(id, options);
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        if let Some(node) = elements(point)
            .find(|&n| named(n, LIST_NS, "chart") && n.attribute("region") == Some(id))
        {
            return patch(xml, vec![(node.range(), data)]);
        }
        return patch(xml, vec![append_child(xml, point, &data)]);
    }
    let first = elements(root).next().ok_or("пустой лист XMCD")?;
    let at = first.range().start;
    patch(
        xml,
        vec![(
            at..at,
            format!("<pointReleaseData xmlns=\"{WS}\">{data}</pointReleaseData>\n  "),
        )],
    )
}

fn find_open_tag_end(xml_fragment: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, &b) in xml_fragment.as_bytes().iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == b'>' {
            return Some(i + 1);
        }
    }
    None
}

fn set_xml_attr(open_tag: &str, attr: &str, value: &str) -> String {
    let bytes = open_tag.as_bytes();
    let mut i = 0;
    // Skip "<tagName"
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/'
    {
        i += 1;
    }
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' || bytes[i] == b'/' {
            break;
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name = &open_tag[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
            break;
        }
        let q = bytes[i];
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != q {
            i += 1;
        }
        let val_end = i;
        if i < bytes.len() {
            i += 1;
        }
        if name == attr {
            let mut out = String::with_capacity(open_tag.len() + value.len());
            out.push_str(&open_tag[..val_start]);
            out.push_str(&escape(value));
            out.push_str(&open_tag[val_end..]);
            return out;
        }
    }
    let insert_at = if open_tag.ends_with("/>") {
        open_tag.len() - 2
    } else if open_tag.ends_with('>') {
        open_tag.len() - 1
    } else {
        open_tag.len()
    };
    let mut out = String::with_capacity(open_tag.len() + attr.len() + value.len() + 4);
    out.push_str(&open_tag[..insert_at]);
    out.push(' ');
    out.push_str(attr);
    out.push_str("=\"");
    out.push_str(&escape(value));
    out.push('"');
    out.push_str(&open_tag[insert_at..]);
    out
}

fn rewrite_region_open_tag(
    region_xml: &str,
    new_id: Option<&str>,
    old_left: f32,
    old_top: f32,
    old_align_x: Option<f32>,
    old_align_y: Option<f32>,
    new_left: f32,
    new_top: f32,
) -> Calc<String> {
    let end = find_open_tag_end(region_xml).ok_or("некорректный тег region")?;
    let mut tag = region_xml[..end].to_owned();
    if let Some(id) = new_id {
        tag = set_xml_attr(&tag, "region-id", id);
    }
    let left_s = format!("{new_left}");
    let top_s = format!("{new_top}");
    let align_y = old_align_y.map_or(new_top + 10., |ay| new_top + (ay - old_top));
    let align_y_s = format!("{align_y}");
    tag = set_xml_attr(&tag, "left", &left_s);
    tag = set_xml_attr(&tag, "top", &top_s);
    tag = set_xml_attr(&tag, "align-y", &align_y_s);
    if let Some(ax) = old_align_x {
        let align_x = new_left + (ax - old_left);
        tag = set_xml_attr(&tag, "align-x", &format!("{align_x}"));
    }
    Ok(tag + &region_xml[end..])
}

fn parse_coord(node: Xml<'_, '_>, key: &str) -> Option<f32> {
    node.attribute(key)
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|n| n.is_finite())
}

pub fn move_regions(xml: &str, moves: &[(String, f32, f32)]) -> Calc<String> {
    if moves.is_empty() {
        return Ok(xml.into());
    }
    let mut targets = std::collections::HashMap::with_capacity(moves.len());
    for (id, left, top) in moves {
        if !(left.is_finite() && top.is_finite()) {
            return Err("неверная позиция на листе".into());
        }
        targets.insert(
            id.as_str(),
            (left.clamp(0., 1_000_000.), top.clamp(0., 1_000_000.)),
        );
    }
    let doc = document(xml)?;
    let container = elements(doc.root_element())
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let mut edits = Vec::new();
    for node in elements(container).filter(|&n| named(n, WS, "region")) {
        let Some(id) = node.attribute("region-id") else {
            continue;
        };
        let Some(&(new_left, new_top)) = targets.get(id) else {
            continue;
        };
        let range = node.range();
        let region_xml = &xml[range.clone()];
        let open_end = find_open_tag_end(region_xml).ok_or("некорректный тег region")?;
        let old_left = parse_coord(node, "left").unwrap_or(0.);
        let old_top = parse_coord(node, "top").unwrap_or(0.);
        let old_align_x = parse_coord(node, "align-x");
        let old_align_y = parse_coord(node, "align-y");
        let updated = rewrite_region_open_tag(
            &region_xml[..open_end],
            None,
            old_left,
            old_top,
            old_align_x,
            old_align_y,
            new_left,
            new_top,
        )?;
        edits.push((range.start..range.start + open_end, updated));
    }
    patch(xml, edits)
}

pub fn grow_formula_region(xml: &str, id: &str, width: f32, height: f32) -> Calc<String> {
    if !width.is_finite() || !height.is_finite() {
        return Err("неверный размер формулы".into());
    }
    let doc = document(xml)?;
    let region = elements(doc.root_element())
        .find(|&node| named(node, WS, "regions"))
        .and_then(|container| {
            elements(container)
                .find(|&node| named(node, WS, "region") && node.attribute("region-id") == Some(id))
        })
        .ok_or("не найдена формула на листе")?;
    let width = width.ceil().max(parse_coord(region, "width").unwrap_or(0.));
    let height = height
        .ceil()
        .max(parse_coord(region, "height").unwrap_or(0.));
    let range = region.range();
    let end = range.start + find_open_tag_end(&xml[range.clone()]).ok_or("неверный тег региона")?;
    let opening = set_xml_attr(&xml[range.start..end], "width", &width.to_string());
    let opening = set_xml_attr(&opening, "height", &height.to_string());
    patch(xml, vec![(range.start..end, opening)])
}

pub fn delete_regions(xml: &str, ids: &[String]) -> Calc<String> {
    if ids.is_empty() {
        return Ok(xml.into());
    }
    let target_ids: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let doc = document(xml)?;
    let root = doc.root_element();
    let container = elements(root)
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let mut edits = Vec::new();
    for node in elements(container).filter(|&n| named(n, WS, "region")) {
        if node
            .attribute("region-id")
            .is_some_and(|id| target_ids.contains(id))
        {
            edits.push((node.range(), String::new()));
        }
    }
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        for node in elements(point).filter(|&n| {
            named(n, LIST_NS, "chart") || named(n, LIST_NS, "plot3d") || named(n, LIST_NS, "image")
        }) {
            if node
                .attribute("region")
                .is_some_and(|id| target_ids.contains(id))
            {
                edits.push((node.range(), String::new()));
            }
        }
    }
    patch(xml, edits)
}

#[derive(Clone, Debug)]
pub struct ClipboardRegion {
    pub xml: String,
    pub rel_left: f32,
    pub rel_top: f32,
    pub old_left: f32,
    pub old_top: f32,
    pub old_align_x: Option<f32>,
    pub old_align_y: Option<f32>,
    pub chart: Option<(ChartSpec, ChartOptions)>,
    pub plot3d: Option<Plot3dSpec>,
    pub image: Option<ImageSpec>,
    text_style: Option<(String, String)>,
}

pub fn copy_regions(xml: &str, ids: &[String]) -> Calc<Vec<ClipboardRegion>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let target_ids: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let doc = document(xml)?;
    let container = elements(doc.root_element())
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let chart_map = charts(xml)?;
    let options_map = chart_options(xml)?;
    let plot3d_map = plots3d(xml)?;
    let image_map = images(xml)?;
    let styles = text_styles(doc.root_element());
    let mut raw = Vec::new();
    for node in elements(container).filter(|&n| named(n, WS, "region")) {
        let Some(id) = node.attribute("region-id") else {
            continue;
        };
        if !target_ids.contains(id) {
            continue;
        }
        let left = parse_coord(node, "left").unwrap_or(0.);
        let top = parse_coord(node, "top").unwrap_or(0.);
        let text_style = styles.and_then(|styles| {
            let text = elements(node).find(|&part| named(part, WS, "text"))?;
            let name = elements(text).next()?.attribute("style")?;
            if !name.contains(FORMAT_STYLE_SEPARATOR) {
                return None;
            }
            let definition = elements(styles).find(|&part| {
                named(part, WS, "textStyle") && part.attribute("name") == Some(name)
            })?;
            Some((name.to_owned(), xml[definition.range()].to_owned()))
        });
        raw.push(ClipboardRegion {
            xml: xml[node.range()].to_owned(),
            rel_left: left,
            rel_top: top,
            old_left: left,
            old_top: top,
            old_align_x: parse_coord(node, "align-x"),
            old_align_y: parse_coord(node, "align-y"),
            chart: chart_map
                .get(id)
                .cloned()
                .map(|s| (s, options_map.get(id).cloned().unwrap_or_default())),
            plot3d: plot3d_map.get(id).cloned(),
            image: image_map.get(id).cloned(),
            text_style,
        });
    }
    if raw.is_empty() {
        return Ok(raw);
    }
    let min_left = raw.iter().map(|r| r.old_left).fold(f32::INFINITY, f32::min);
    let min_top = raw.iter().map(|r| r.old_top).fold(f32::INFINITY, f32::min);
    for item in &mut raw {
        item.rel_left -= min_left;
        item.rel_top -= min_top;
    }
    Ok(raw)
}

pub fn paste_regions(
    xml: &str,
    items: &[ClipboardRegion],
    base_left: f32,
    base_top: f32,
) -> Calc<(String, Vec<String>)> {
    if items.is_empty() {
        return Ok((xml.into(), Vec::new()));
    }
    if !(base_left.is_finite() && base_top.is_finite()) {
        return Err("неверная позиция на листе".into());
    }
    let doc = document(xml)?;
    let root = doc.root_element();
    let container = elements(root)
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let mut next_id = elements(container)
        .filter_map(|n| n.attribute("region-id"))
        .filter_map(|id| id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let mut new_ids = Vec::with_capacity(items.len());
    let mut region_chunks = Vec::with_capacity(items.len());
    let mut chart_chunks = Vec::new();
    for item in items {
        next_id = next_id
            .checked_add(1)
            .ok_or("нет свободного номера региона")?;
        let id = next_id.to_string();
        let new_left = (base_left + item.rel_left).clamp(0., 1_000_000.);
        let new_top = (base_top + item.rel_top).clamp(0., 1_000_000.);
        let rewritten = rewrite_region_open_tag(
            &item.xml,
            Some(&id),
            item.old_left,
            item.old_top,
            item.old_align_x,
            item.old_align_y,
            new_left,
            new_top,
        )?;
        region_chunks.push(rewritten);
        if let Some((spec, options)) = &item.chart {
            chart_chunks.push(spec.xml_with_options(&id, options));
        }
        if let Some(spec3d) = &item.plot3d {
            chart_chunks.push(spec3d.xml(&id));
        }
        if let Some(img) = &item.image {
            chart_chunks.push(img.xml(&id));
        }
        new_ids.push(id);
    }
    let mut edits = Vec::new();
    if items.iter().any(|item| item.text_style.is_some()) {
        let styles = text_styles(root).ok_or("на целевом листе нет таблицы текстовых стилей")?;
        let mut names: std::collections::HashSet<_> = elements(styles)
            .filter_map(|style| style.attribute("name"))
            .collect();
        let definitions: Vec<_> = items
            .iter()
            .filter_map(|item| item.text_style.as_ref())
            .filter(|(name, _)| names.insert(name.as_str()))
            .map(|(_, markup)| markup.as_str())
            .collect();
        if !definitions.is_empty() {
            edits.push(append_child(xml, styles, &definitions.join("\n")));
        }
    }
    if !chart_chunks.is_empty() {
        let joined_charts = chart_chunks.join("\n  ");
        if let Some(node) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
            edits.push(append_child(xml, node, &joined_charts));
        } else if let Some(first) = elements(root).next() {
            let at = first.range().start;
            edits.push((
                at..at,
                format!("<pointReleaseData xmlns=\"{WS}\">{joined_charts}</pointReleaseData>\n  "),
            ));
        }
    }
    edits.push(append_child(xml, container, &region_chunks.join("\n  ")));
    Ok((patch(xml, edits)?, new_ids))
}

pub fn read_sheet_precision(xml: &str) -> Calc<Option<usize>> {
    let doc = document(xml)?;
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(None);
    };
    let precision = elements(point)
        .find(|&n| named(n, LIST_NS, "format"))
        .and_then(|n| n.attribute("precision"))
        .and_then(|s| s.parse::<usize>().ok())
        .map(|p| p.clamp(1, 15));
    Ok(precision)
}

pub fn set_sheet_precision(xml: &str, precision: Option<usize>) -> Calc<String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let tag_xml = precision.map(|p| {
        let p = p.clamp(1, 15);
        format!("<ls:format xmlns:ls=\"{LIST_NS}\" precision=\"{p}\"/>")
    });
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        if let Some(existing) = elements(point).find(|&n| named(n, LIST_NS, "format")) {
            return patch(xml, vec![(existing.range(), tag_xml.unwrap_or_default())]);
        }
        if let Some(tag) = tag_xml {
            return patch(xml, vec![append_child(xml, point, &tag)]);
        }
        return Ok(xml.into());
    }
    let Some(tag) = tag_xml else {
        return Ok(xml.into());
    };
    let first = elements(root).next().ok_or("пустой лист XMCD")?;
    let at = first.range().start;
    patch(
        xml,
        vec![(
            at..at,
            format!("<pointReleaseData xmlns=\"{WS}\">{tag}</pointReleaseData>\n  "),
        )],
    )
}

pub fn set_display_unit(xml: &str, region_id: &str, unit_expr: Option<&str>) -> Calc<String> {
    let doc = document(xml)?;
    let container = elements(doc.root_element())
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let region = elements(container)
        .find(|&n| named(n, WS, "region") && n.attribute("region-id") == Some(region_id))
        .ok_or("не найден регион на листе")?;
    let math = elements(region)
        .find(|&n| named(n, WS, "math"))
        .ok_or("единицу вывода можно задать только для формулы")?;
    let mut statement = math;
    loop {
        statement = elements(statement)
            .find(|child| child.tag_name().namespace() == Some(ML))
            .ok_or("регион не содержит формулы math30")?;
        if !named(statement, ML, "provenance") {
            break;
        }
    }
    let unit_tag = match unit_expr.map(str::trim).filter(|s| !s.is_empty()) {
        Some(expr_text) => {
            if expr_text.len() > 256 {
                return Err("выражение единицы слишком длинное".into());
            }
            let mut parser = Parser {
                tokens: tokenize(expr_text)?,
                index: 0,
            };
            let parsed = parser.expr(0, 0)?;
            parser.expect(Token::End)?;
            Some(format!(
                "<ml:unitOverride xmlns:ml=\"{ML}\">{}</ml:unitOverride>",
                parsed.xml()?
            ))
        }
        None => None,
    };
    let target_eval = if named(statement, ML, "define") || named(statement, ML, "globalDefine") {
        let rhs = elements(statement)
            .nth(1)
            .ok_or("некорректное определение")?;
        if named(rhs, ML, "eval") {
            Some(rhs)
        } else {
            // Wrap RHS in <ml:eval> if setting a unit; nothing to do if clearing
            let Some(u_tag) = unit_tag else {
                return Ok(xml.into());
            };
            let rhs_range = rhs.range();
            let rhs_xml = &xml[rhs_range.clone()];
            let wrapped = format!("<ml:eval xmlns:ml=\"{ML}\">{rhs_xml}{u_tag}</ml:eval>");
            return patch(xml, vec![(rhs_range, wrapped)]);
        }
    } else if named(statement, ML, "eval") {
        Some(statement)
    } else {
        let Some(u_tag) = unit_tag else {
            return Ok(xml.into());
        };
        let range = statement.range();
        let inner = &xml[range.clone()];
        let wrapped = format!("<ml:eval xmlns:ml=\"{ML}\">{inner}{u_tag}</ml:eval>");
        return patch(xml, vec![(range, wrapped)]);
    };
    let eval_node = target_eval.expect("eval node");
    if let Some(existing) = elements(eval_node).find(|&n| named(n, ML, "unitOverride")) {
        return patch(xml, vec![(existing.range(), unit_tag.unwrap_or_default())]);
    }
    let Some(u_tag) = unit_tag else {
        return Ok(xml.into());
    };
    patch(xml, vec![append_child(xml, eval_node, &u_tag)])
}

pub fn replace_text_region(
    xml: &str,
    region_id: &str,
    text: &str,
    style: TextStyle,
) -> Calc<String> {
    if text.trim().is_empty() || text.len() > 24_000 {
        return Err("введите текст до 24 000 байт".into());
    }
    let doc = document(xml)?;
    let container = elements(doc.root_element())
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let region = elements(container)
        .find(|&n| named(n, WS, "region") && n.attribute("region-id") == Some(region_id))
        .ok_or("не найден текстовый регион на листе")?;
    let text_node = elements(region)
        .find(|&n| named(n, WS, "text"))
        .ok_or("выбранный регион не является текстовым")?;
    let previous = elements(text_node)
        .next()
        .and_then(|p| p.attribute("style"));
    let retained_format = previous
        .filter(|name| TextStyle::from_heading_level(text_heading(name)) == style)
        .and_then(|name| region_text_format(text_node).map(|format| (name, format)));
    let (style_name, paragraph_attrs) = match retained_format {
        Some((name, format)) => (name, format!(" text-align=\"{}\"", format.alignment.name())),
        None => (style.xmcd_name(), String::new()),
    };
    let paragraphs = text
        .lines()
        .map(|line| {
            format!(
                "<p style=\"{}\"{paragraph_attrs}>{}</p>",
                escape(style_name),
                escape(line)
            )
        })
        .collect::<String>();
    let line_h = match style {
        TextStyle::Heading1 => 30,
        TextStyle::Heading2 => 24,
        TextStyle::Normal => 19,
    };
    let new_height = if let Some((_, format)) = retained_format {
        formatted_text_height(text.lines(), region.attribute("width"), format.size)
    } else {
        (30 + line_h * (text.lines().count().saturating_sub(1))).min(420)
    };
    let region_range = region.range();
    let region_xml = &xml[region_range.clone()];
    let open_end = find_open_tag_end(region_xml).ok_or("некорректный тег region")?;
    let updated_open = set_xml_attr(&region_xml[..open_end], "height", &new_height.to_string());
    let mut text_range = text_node.range();
    let mut new_text = format!("<text>{paragraphs}</text>");
    if let Some((start, end)) = find_open_tag_end(&xml[text_range.clone()])
        .zip(xml[text_range.clone()].rfind("</"))
        .filter(|(start, end)| start <= end)
    {
        text_range = text_range.start + start..text_range.start + end;
        new_text = paragraphs;
    }
    patch(
        xml,
        vec![
            (
                region_range.start..region_range.start + open_end,
                updated_open,
            ),
            (text_range, new_text),
        ],
    )
}

fn formatted_text_height<'a>(
    lines: impl Iterator<Item = &'a str>,
    width: Option<&str>,
    size: u16,
) -> usize {
    let width = width
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.)
        .unwrap_or(220.);
    let chars = (width / (f32::from(size) * 0.72)).max(1.) as usize;
    let lines: usize = lines
        .map(|line| line.chars().count().max(1).div_ceil(chars))
        .sum();
    (8 + lines * (usize::from(size) * 3 / 2 + 2)).clamp(24, 100_000)
}

pub fn format_text_region(xml: &str, region_id: &str, format: TextFormat) -> Calc<String> {
    if !(6..=72).contains(&format.size) {
        return Err("размер шрифта должен быть от 6 до 72 пунктов".into());
    }
    let doc = document(xml)?;
    let root = doc.root_element();
    let container = elements(root)
        .find(|&node| named(node, WS, "regions"))
        .ok_or("лист без блока regions")?;
    let region = elements(container)
        .find(|&node| named(node, WS, "region") && node.attribute("region-id") == Some(region_id))
        .ok_or("не найден текстовый регион")?;
    let text = elements(region)
        .find(|&node| named(node, WS, "text"))
        .ok_or("выбранный регион не является текстовым")?;
    let previous = region_text_format(text)
        .ok_or("панель формата доступна для однородного текста без вложенной разметки")?;
    if previous == format {
        return Ok(xml.to_owned());
    }
    let styles = text_styles(root).ok_or("на листе нет таблицы текстовых стилей")?;
    let current_style = elements(text)
        .next()
        .and_then(|p| p.attribute("style"))
        .unwrap_or("Normal");
    let base = current_style
        .split(FORMAT_STYLE_SEPARATOR)
        .next()
        .unwrap_or(current_style);
    let weight = if format.bold { "bold" } else { "normal" };
    let slant = if format.italic { "italic" } else { "normal" };
    let font = format.font.name();
    let size = format.size.to_string();
    let underline = if format.underline { "true" } else { "false" };
    let style_name =
        format!("{base}{FORMAT_STYLE_SEPARATOR}{font} {size} {weight} {slant} {underline})");
    let change_font = TextFormat {
        alignment: previous.alignment,
        ..format
    } != previous;
    let mut changes = Vec::new();
    if change_font
        && !elements(styles).any(|style| {
            named(style, WS, "textStyle") && style.attribute("name") == Some(&style_name)
        })
    {
        let block = elements(styles)
            .find(|&style| {
                named(style, WS, "textStyle") && style.attribute("name") == Some(current_style)
            })
            .and_then(|style| elements(style).find(|&part| named(part, WS, "blockAttr")));
        let mut block_markup = String::from("<blockAttr");
        if let Some(block) = block {
            for attr in block.attributes().filter(|attr| attr.namespace().is_none()) {
                block_markup.push_str(&format!(" {}=\"{}\"", attr.name(), escape(attr.value())));
            }
        }
        block_markup.push_str("/>");
        let definition = format!(
            "<textStyle xmlns=\"{WS}\" name=\"{}\">{block_markup}<inlineAttr font-family=\"{font}\" font-charset=\"0\" font-size=\"{size}\" font-weight=\"{weight}\" font-style=\"{slant}\" underline=\"{underline}\" line-through=\"false\" vertical-align=\"baseline\"/></textStyle>",
            escape(&style_name),
        );
        changes.push(append_child(xml, styles, &definition));
    }
    for paragraph in elements(text) {
        let range = paragraph.range();
        let end = range.start
            + find_open_tag_end(&xml[range.clone()]).ok_or("не найдено начало абзаца")?;
        let opening = &xml[range.start..end];
        let opening = if change_font {
            set_xml_attr(opening, "style", &style_name)
        } else {
            opening.to_owned()
        };
        let opening = set_xml_attr(&opening, "text-align", format.alignment.name());
        changes.push((range.start..end, opening));
    }
    if format.size > previous.size {
        let current = region
            .attribute("height")
            .and_then(|height| height.parse::<f32>().ok())
            .unwrap_or(0.);
        let wanted = formatted_text_height(
            elements(text).map(|p| p.text().unwrap_or_default()),
            region.attribute("width"),
            format.size,
        );
        if wanted as f32 > current {
            let range = region.range();
            let end = range.start
                + find_open_tag_end(&xml[range.clone()]).ok_or("не найдено начало региона")?;
            changes.push((
                range.start..end,
                set_xml_attr(&xml[range.start..end], "height", &wanted.to_string()),
            ));
        }
    }
    patch(xml, changes)
}

pub fn replace_rich_text_region(
    xml: &str,
    region_id: &str,
    rich: &crate::rich_text::RichText,
    style: TextStyle,
) -> Calc<String> {
    let plain = replace_text_region(xml, region_id, &rich.body, style)?;
    let neutral = TextFormat {
        alignment: rich.alignment,
        ..TextFormat::for_heading(0)
    };
    let plain = format_text_region(&plain, region_id, neutral)?;
    let doc = document(&plain)?;
    let region = elements(doc.root_element())
        .find(|&node| named(node, WS, "regions"))
        .and_then(|regions| {
            elements(regions).find(|&node| {
                named(node, WS, "region") && node.attribute("region-id") == Some(region_id)
            })
        })
        .ok_or("не найден текстовый регион")?;
    let text = elements(region)
        .find(|&node| named(node, WS, "text"))
        .ok_or("не найден текст")?;
    let paragraph = elements(text).next().ok_or("не найден абзац")?;
    let style = paragraph.attribute("style").unwrap_or("Normal");
    let mut parts = vec![String::new()];
    let mut largest = neutral.size;
    for (run, format) in rich.runs() {
        largest = largest.max(format.size);
        for (index, line) in run.split('\n').enumerate() {
            if index > 0 {
                parts.push(String::new());
            }
            if line.is_empty() {
                continue;
            }
            let mut body = escape(line);
            for (tag, enabled) in [
                ("u", format.underline),
                ("i", format.italic),
                ("b", format.bold),
            ] {
                if enabled {
                    body = format!("<{tag}>{body}</{tag}>");
                }
            }
            parts
                .last_mut()
                .expect("paragraph exists")
                .push_str(&format!(
                    "<f family=\"{}\" charset=\"0\" size=\"{}\">{body}</f>",
                    format.font.name(),
                    format.size,
                ));
        }
    }
    let paragraphs: String = parts
        .into_iter()
        .map(|body| {
            format!(
                "<p style=\"{}\" text-align=\"{}\">{body}</p>",
                escape(style),
                rich.alignment.name(),
            )
        })
        .collect();
    let range = text.range();
    let markup = &plain[range.clone()];
    let start = range.start + find_open_tag_end(markup).ok_or("не найдено начало текста")?;
    let end = range.start + markup.rfind("</").ok_or("не найден конец текста")?;
    let region_range = region.range();
    let region_end = region_range.start
        + find_open_tag_end(&plain[region_range.clone()]).ok_or("не найдено начало региона")?;
    let height = formatted_text_height(rich.body.split('\n'), region.attribute("width"), largest);
    let region_opening = set_xml_attr(
        &plain[region_range.start..region_end],
        "height",
        &height.to_string(),
    );
    patch(
        &plain,
        vec![
            (region_range.start..region_end, region_opening),
            (start..end, paragraphs),
        ],
    )
}

pub fn read_page_settings(xml: &str) -> Calc<PageSettings> {
    let doc = document(xml)?;
    let Some(point) = elements(doc.root_element()).find(|&n| named(n, WS, "pointReleaseData"))
    else {
        return Ok(PageSettings::default());
    };
    let Some(node) = elements(point).find(|&n| named(n, LIST_NS, "page")) else {
        return Ok(PageSettings::default());
    };
    let page_height = node
        .attribute("page-height")
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(300.0, 2400.0))
        .unwrap_or(840.0);
    Ok(PageSettings {
        header: node.attribute("header").unwrap_or_default().into(),
        footer: node.attribute("footer").unwrap_or_default().into(),
        show_page_numbers: node.attribute("page-numbers") != Some("false"),
        page_height,
    })
}

pub fn set_page_settings(xml: &str, settings: &PageSettings) -> Calc<String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let page_height = settings.page_height.clamp(300.0, 2400.0);
    let tag_xml = format!(
        "<ls:page xmlns:ls=\"{LIST_NS}\" header=\"{}\" footer=\"{}\" page-numbers=\"{}\" page-height=\"{page_height}\"/>",
        escape(settings.header.trim()),
        escape(settings.footer.trim()),
        if settings.show_page_numbers { "true" } else { "false" }
    );
    if let Some(point) = elements(root).find(|&n| named(n, WS, "pointReleaseData")) {
        if let Some(existing) = elements(point).find(|&n| named(n, LIST_NS, "page")) {
            return patch(xml, vec![(existing.range(), tag_xml)]);
        }
        return patch(xml, vec![append_child(xml, point, &tag_xml)]);
    }
    let first = elements(root).next().ok_or("пустой лист XMCD")?;
    let at = first.range().start;
    patch(
        xml,
        vec![(
            at..at,
            format!("<pointReleaseData xmlns=\"{WS}\">{tag_xml}</pointReleaseData>\n  "),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_marker_settings_roundtrip_in_chart_and_worksheet_defaults() {
        let blank = include_str!("../examples/blank.xmcd");
        let spec = ChartSpec::Function {
            expression: "sin(x); cos(x)".into(),
            from: 0.,
            to: 5.,
        };
        let (xml, id) = insert_region(blank, 12., 20., NewRegion::Chart(&spec)).unwrap();
        let mut options = ChartOptions::default();
        assert_eq!(chart_options(&xml).unwrap()[&id], options);
        assert!(!xml.contains("trace-0-marker"));
        options.appearance.traces[0].marker = ChartMarker::Diamond;
        options.appearance.traces[0].marker_size = 12;
        options.appearance.traces[0].marker_every = 250;
        options.appearance.traces[1].marker = ChartMarker::None;
        let xml = replace_chart_with_options(&xml, &id, &spec, &options).unwrap();
        let xml = set_chart_defaults(&xml, &options).unwrap();
        let xml = export_xml(&xml, true).unwrap();
        assert_eq!(chart_options(&xml).unwrap()[&id], options);
        assert_eq!(chart_defaults(&xml).unwrap().as_ref(), Some(&options));

        let malformed = xml
            .replace(r#"trace-0-marker="diamond""#, r#"trace-0-marker="other""#)
            .replace(
                r#"trace-0-marker-size="12""#,
                r#"trace-0-marker-size="100""#,
            )
            .replace(
                r#"trace-0-marker-every="250""#,
                r#"trace-0-marker-every="never""#,
            );
        let options = chart_defaults(&malformed).unwrap().unwrap();
        let style = &options.appearance.traces[0];
        assert_eq!(style.marker, ChartMarker::Auto);
        assert_eq!(style.marker_size, 16);
        assert_eq!(style.marker_every, 0);
        assert_eq!(style.marker_shape(false), None);
        assert_eq!(style.marker_shape(true), Some(ChartMarker::Circle));
    }

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

    #[test]
    fn move_copy_paste_and_delete_regions_preserve_other_coordinates() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, id_a) = insert_region(blank, 12., 20., NewRegion::Formula("a := 2")).unwrap();
        let (xml, id_b) =
            insert_region(&xml, 12., 80., NewRegion::Formula("b := a + 5 =")).unwrap();
        let (xml, id_c) = insert_region(&xml, 200., 50., NewRegion::Text("Заметка")).unwrap();

        // Move definition `a` below `b`
        let moved = move_regions(&xml, &[(id_a.clone(), 12., 140.)]).unwrap();
        let doc = document(&moved).unwrap();
        let regions: Vec<_> = elements(
            elements(doc.root_element())
                .find(|&n| named(n, WS, "regions"))
                .unwrap(),
        )
        .collect();
        let by_id = |id: &str| {
            regions
                .iter()
                .find(|n| n.attribute("region-id") == Some(id))
                .copied()
                .unwrap()
        };
        assert_eq!(by_id(&id_a).attribute("top"), Some("140"));
        assert_eq!(by_id(&id_a).attribute("align-y"), Some("150"));
        assert_eq!(by_id(&id_b).attribute("top"), Some("80"));
        assert_eq!(by_id(&id_c).attribute("left"), Some("200"));
        assert_eq!(by_id(&id_c).attribute("top"), Some("50"));

        // Copy and paste `a` and `c`
        let copied = copy_regions(&xml, &[id_a.clone(), id_c.clone()]).unwrap();
        assert_eq!(copied.len(), 2);
        let (pasted, new_ids) = paste_regions(&xml, &copied, 30., 200.).unwrap();
        assert_eq!(new_ids.len(), 2);

        // Delete pasted regions
        let cleaned = delete_regions(&pasted, &new_ids).unwrap();
        let cleaned_doc = document(&cleaned).unwrap();
        let remaining = elements(
            elements(cleaned_doc.root_element())
                .find(|&n| named(n, WS, "regions"))
                .unwrap(),
        )
        .count();
        assert_eq!(remaining, 3);
    }

    #[test]
    fn imports_native_mathcad_xy_plots_from_binary_content() {
        let xml = include_str!("../fixtures/Complex_numbers_variant_24_calculations.xmcd");
        let parsed = charts(xml).unwrap();
        assert_eq!(parsed.len(), 3);
        match parsed.get("119").unwrap() {
            ChartSpec::Vectors { x, y } => {
                assert_eq!(x, "Xp");
                assert_eq!(y, "Yp");
            }
            _ => panic!("expected vectors chart"),
        }
        match parsed.get("142").unwrap() {
            ChartSpec::Vectors { x, y } => {
                assert_eq!(x, "Xc; Xa");
                assert_eq!(y, "Yc; Ya");
            }
            _ => panic!("expected multi-trace vectors chart"),
        }
        match parsed.get("153").unwrap() {
            ChartSpec::Vectors { x, y } => {
                assert_eq!(x, "Xr");
                assert_eq!(y, "Yr");
            }
            _ => panic!("expected vectors chart"),
        }
    }

    #[test]
    fn sets_and_clears_display_unit_and_sheet_precision() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, _) =
            insert_region(blank, 12., 20., NewRegion::Formula("p := 1 * MPa =")).unwrap();
        let xml = set_display_unit(&xml, "1", Some("kPa")).unwrap();
        let xml = set_sheet_precision(&xml, Some(4)).unwrap();
        assert_eq!(read_sheet_precision(&xml).unwrap(), Some(4));

        let mut sheet = crate::sheet::Sheet::import(&xml).unwrap();
        sheet.full();
        assert_eq!(
            sheet.display_with_precision(0, Some(4)).unwrap(),
            "1000 kPa"
        );

        let cleared = set_display_unit(&xml, "1", None).unwrap();
        let cleared = set_sheet_precision(&cleared, None).unwrap();
        assert_eq!(read_sheet_precision(&cleared).unwrap(), None);
        let mut sheet2 = crate::sheet::Sheet::import(&cleared).unwrap();
        sheet2.full();
        assert_eq!(sheet2.display_unit_label(0), None);
    }

    #[test]
    fn whole_region_format_rejects_text_with_inline_markup() {
        let (xml, id) = insert_region(
            crate::BLANK,
            18.,
            24.,
            NewRegion::StyledText("Исходный текст", TextStyle::Normal),
        )
        .unwrap();
        let xml = xml.replace("Исходный текст", "<b>Исходный</b> текст");
        let format = TextFormat {
            size: 20,
            ..TextFormat::for_heading(0)
        };
        assert!(format_text_region(&xml, &id, format).is_err());
    }

    #[test]
    fn edits_styled_text_page_breaks_and_page_settings() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, id_txt) = insert_region(
            blank,
            12.,
            20.,
            NewRegion::StyledText("Заголовок расчёта", TextStyle::Heading1),
        )
        .unwrap();
        let (xml, _) = insert_region(&xml, 12., 400., NewRegion::PageBreak).unwrap();
        let xml =
            replace_text_region(&xml, &id_txt, "Раздел 1\nОписание", TextStyle::Heading2).unwrap();
        let settings = PageSettings {
            header: "Инженерный отчёт".into(),
            footer: "Проект Лист".into(),
            show_page_numbers: true,
            page_height: 780.0,
        };
        let xml = set_page_settings(&xml, &settings).unwrap();
        assert_eq!(read_page_settings(&xml).unwrap(), settings);
        let sheet = crate::sheet::Sheet::import(&xml).unwrap();
        assert_eq!(sheet.regions[0].visual.heading, 2);
        assert_eq!(sheet.regions[0].text.as_deref(), Some("Раздел 1\nОписание"));
        assert_eq!(sheet.regions[1].text.as_deref(), Some("Разрыв страницы"));
    }

    #[test]
    fn stores_and_reads_3d_surface_and_contour_specs() {
        let blank = include_str!("../examples/blank.xmcd");
        let mut spec = Plot3dSpec::default_surface();
        spec.yaw_deg = 52.0;
        spec.pitch_deg = 24.0;
        let (xml, id) = insert_region(blank, 12., 20., NewRegion::Plot3d(&spec)).unwrap();
        let parsed = plots3d(&xml).unwrap();
        assert_eq!(parsed.get(&id), Some(&spec));
        let updated = Plot3dSpec {
            mode: Plot3dMode::Contour,
            yaw_deg: 75.0,
            ..spec
        };
        let xml2 = replace_plot3d(&xml, &id, &updated).unwrap();
        assert_eq!(plots3d(&xml2).unwrap().get(&id), Some(&updated));
    }

    #[test]
    fn stores_collapsible_areas_and_embedded_images_in_xmcd() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, area_id) = insert_area(blank, "Расчёт балки", 10., 220.).unwrap();
        let list = areas(&xml).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].collapsed);
        let xml = toggle_area(&xml, &area_id).unwrap();
        assert!(areas(&xml).unwrap()[0].collapsed);

        let raw = vec![137u8, 80, 78, 71, 13, 10, 26, 10];
        let b64 = encode_base64(&raw);
        assert_eq!(decode_base64(&b64).unwrap(), raw);
        let img = ImageSpec {
            name: "scheme.png".into(),
            mime: "image/png".into(),
            width: 320,
            height: 200,
            data_base64: b64,
        };
        let (xml, img_id) = insert_region(&xml, 12., 260., NewRegion::Image(&img)).unwrap();
        assert_eq!(images(&xml).unwrap().get(&img_id), Some(&img));
    }

    #[test]
    fn tokenizes_degree_celsius_and_fahrenheit_units() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, id) =
            insert_region(blank, 12., 20., NewRegion::Formula("T := C(100) =")).unwrap();
        let xml_c = set_display_unit(&xml, &id, Some("°C")).unwrap();
        let mut s_c = crate::sheet::Sheet::import(&xml_c).unwrap();
        s_c.full();
        assert_eq!(s_c.display(0).unwrap(), "100 °C");

        let xml_f = set_display_unit(&xml_c, &id, Some("°F")).unwrap();
        let mut s_f = crate::sheet::Sheet::import(&xml_f).unwrap();
        s_f.full();
        assert_eq!(s_f.display(0).unwrap(), "212 °F");
    }
}
