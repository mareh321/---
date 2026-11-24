use crate::editing::{region_text_format, text_heading, FormulaSource, TextFormat};
use crate::parse::{elements, eval_operand, kind, named, Xml, ML};

#[derive(Default)]
pub struct RegionVisual {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub heading: u8,
    pub text_format: Option<TextFormat>,
    pub formula: String,
    pub input_unit: Option<String>,
    pub graphical: bool,
    pub function: bool,
    pub editor: Option<FormulaSource>,
}

impl RegionVisual {
    pub fn new(region: Xml<'_, '_>) -> Self {
        let number = |name, fallback: f32| {
            region
                .attribute(name)
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|number| number.is_finite())
                .map_or(fallback, |number| number.clamp(-2_000_000., 2_000_000.))
        };
        Self {
            left: number("left", 0.),
            top: number("top", 0.),
            width: number("width", 220.).max(8.),
            height: number("height", 26.).max(8.),
            ..Self::default()
        }
    }

    pub fn set_text(&mut self, text: Xml<'_, '_>) -> String {
        let paragraphs: Vec<_> = elements(text).filter(|&node| kind(node) == "p").collect();
        let style = paragraphs
            .first()
            .and_then(|node| node.attribute("style"))
            .unwrap_or_default();
        self.heading = text_heading(style);
        self.text_format = region_text_format(text);
        let line = |paragraph: Xml<'_, '_>| {
            paragraph
                .descendants()
                .filter(|node| node.is_text())
                .filter_map(|node| node.text())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        };
        if paragraphs.is_empty() {
            line(text)
        } else {
            paragraphs
                .into_iter()
                .map(line)
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    pub fn set_formula(&mut self, statement: Xml<'_, '_>, editable: bool) {
        self.formula = notation(statement, 0).0;
        self.editor = FormulaSource::from_statement(statement, editable);
        if !named(statement, ML, "define") && !named(statement, ML, "globalDefine") {
            return;
        }
        let mut parts = elements(statement);
        if let Some(left) = parts.next() {
            self.function = named(left, ML, "function")
                || (named(left, ML, "apply")
                    && elements(left).next().is_some_and(|op| named(op, ML, "id")));
        }
        if editable {
            if let Some(body) = parts.next().map(eval_operand) {
                let operands: Vec<_> = elements(body).collect();
                if let [op, _, unit] = operands.as_slice() {
                    if matches!(kind(*op), "mult" | "scale") {
                        self.input_unit = Some(notation(*unit, 0).0);
                    }
                }
            }
        }
    }
}

fn superscript(s: &str) -> Option<String> {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    s.chars()
        .map(|c| match c {
            '-' | '−' => Some('⁻'),
            '0'..='9' => Some(DIGITS[c as usize - '0' as usize]),
            _ => None,
        })
        .collect()
}

fn notation<'a, 'input>(node: Xml<'a, 'input>, depth: usize) -> (String, u8) {
    if depth > 40 {
        return ("…".into(), 9);
    }
    let descend = |n| notation(n, depth + 1);
    let combine = |items: &[Xml<'a, 'input>], separator: &str| {
        items
            .iter()
            .map(|&n| descend(n).0)
            .collect::<Vec<_>>()
            .join(separator)
    };
    let grouped = |n, required| {
        let (text, precedence) = descend(n);
        if precedence < required {
            format!("({text})")
        } else {
            text
        }
    };
    let tag = kind(node);
    if matches!(tag, "id" | "real" | "imag") {
        let text: String = node
            .text()
            .unwrap_or("?")
            .trim()
            .chars()
            .take(140)
            .collect();
        return match tag {
            "imag" if text == "1" => ("i".into(), 9),
            "imag" => (format!("{text}i"), 9),
            "id" if node.attribute("subscript").is_some() => (
                format!("{text}_{}", node.attribute("subscript").unwrap_or_default()),
                9,
            ),
            "real" if text.starts_with('-') => (text.replacen('-', "−", 1), 4),
            _ => (text, 9),
        };
    }
    if tag == "program" {
        let stmts: Vec<_> = elements(node).take(16).collect();
        return (format!("prog({})", combine(&stmts, "; ")), 9);
    }
    let items: Vec<_> = elements(node).take(34).collect();
    let printed = match (tag, items.as_slice()) {
        ("define", [left, right, ..]) => {
            (format!("{} := {}", descend(*left).0, descend(*right).0), 0)
        }
        ("globalDefine", [left, right, ..]) => {
            (format!("{} ≡ {}", descend(*left).0, descend(*right).0), 0)
        }
        ("eval", [body, ..]) => descend(*body),
        ("symEval", [body, rest @ ..]) => {
            let commands: Vec<_> = rest
                .iter()
                .copied()
                .filter(|&item| named(item, ML, "command"))
                .collect();
            let body = descend(*body).0;
            if commands.is_empty() {
                (format!("{body} →"), 0)
            } else {
                (format!("{body}  {} →", combine(&commands, "; ")), 0)
            }
        }
        ("command", [field]) => descend(*field),
        ("parens", [body]) => (format!("({})", descend(*body).0), 9),
        ("function", [name, vars]) => (format!("{}({})", descend(*name).0, descend(*vars).0), 9),
        ("sequence" | "boundVars", _) => (combine(&items, ", "), 0),
        ("range", _) => (combine(&items, " … "), 1),
        ("matrix", _) => (
            format!(
                "[{} × {}]",
                node.attribute("rows").unwrap_or("?"),
                node.attribute("cols").unwrap_or("?")
            ),
            9,
        ),
        ("complex", [re, im]) => (format!("{} + {}", descend(*re).0, descend(*im).0), 2),
        ("localDefine", [l, r, ..]) => (format!("{} ← {}", descend(*l).0, descend(*r).0), 0),
        ("if", [c, t]) => (format!("if({}, {})", descend(*c).0, descend(*t).0), 0),
        ("if", [c, t, e]) => (
            format!(
                "if({}, {}, {})",
                descend(*c).0,
                descend(*t).0,
                descend(*e).0
            ),
            0,
        ),
        ("otherwise", [b]) => descend(*b),
        ("for", [v, it, b]) => (
            format!(
                "for({}, {}, {})",
                descend(*v).0,
                descend(*it).0,
                descend(*b).0
            ),
            0,
        ),
        ("while", [c, b]) => (format!("while({}, {})", descend(*c).0, descend(*b).0), 0),
        ("return", [v]) => (format!("return {}", descend(*v).0), 0),
        ("break", _) => ("break".into(), 9),
        ("continue", _) => ("continue".into(), 9),
        ("apply", [op, args @ ..]) => match (kind(*op), args) {
            ("indexer", [array, subs @ ..]) => {
                (format!("{}[{}]", descend(*array).0, combine(subs, ", ")), 9)
            }
            ("sqrt", [arg]) => (format!("√({})", descend(*arg).0), 9),
            ("absval", [arg]) => (format!("|{}|", descend(*arg).0), 9),
            ("neg" | "minus", [arg]) => (format!("−{}", grouped(*arg, 4)), 4),
            ("pow", [base, exp]) => {
                let exponent = descend(*exp).0;
                let suffix = superscript(&exponent).unwrap_or_else(|| format!("^({exponent})"));
                (format!("{}{suffix}", grouped(*base, 6)), 5)
            }
            (
                "plus" | "minus" | "mult" | "scale" | "div" | "equal" | "notEqual" | "lessThan"
                | "greaterThan" | "lessOrEqual" | "greaterOrEqual",
                [a, b],
            ) => {
                let (symbol, precedence, strict_right) = match kind(*op) {
                    "plus" => ("+", 2, false),
                    "minus" => ("−", 2, true),
                    "mult" | "scale" => ("·", 3, false),
                    "div" => ("/", 3, true),
                    "equal" => ("=", 1, false),
                    "notEqual" => ("≠", 1, false),
                    "lessThan" => ("<", 1, false),
                    "greaterThan" => (">", 1, false),
                    "lessOrEqual" => ("≤", 1, false),
                    _ => ("≥", 1, false),
                };
                (
                    format!(
                        "{} {symbol} {}",
                        grouped(*a, precedence),
                        grouped(*b, precedence + u8::from(strict_right))
                    ),
                    precedence,
                )
            }
            ("id", _) => (format!("{}({})", descend(*op).0, combine(args, ", ")), 9),
            (name, _) => (format!("{name}({})", combine(args, ", ")), 9),
        },
        (other, _) => (format!("⟨{other}⟩"), 9),
    };
    printed
}
