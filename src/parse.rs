use crate::eval::{Datum, Expr, Function, IndexedAssignment, Matrix, MAX_CELLS};
use crate::value::{Calc, Value};
use roxmltree::Node;
use std::{collections::HashMap, sync::Arc};

pub const ML: &str = "http://schemas.mathsoft.com/math30";
pub type Xml<'a, 'input> = Node<'a, 'input>;

pub fn named(node: Xml<'_, '_>, ns: &str, name: &str) -> bool {
    node.has_tag_name((ns, name))
}

pub fn elements<'a, 'input>(node: Xml<'a, 'input>) -> impl Iterator<Item = Xml<'a, 'input>> {
    node.children().filter(Node::is_element)
}

pub fn kind<'input>(node: Xml<'_, 'input>) -> &'input str {
    node.tag_name().name()
}

pub fn identifier(node: Xml<'_, '_>) -> Calc<String> {
    if !named(node, ML, "id") {
        return Err(format!(
            "идентификатор типа <{}> не поддерживается",
            kind(node)
        ));
    }
    let name = node
        .text()
        .filter(|name| !name.is_empty())
        .ok_or("пустое имя")?;
    if let Some(subscript) = node.attribute("subscript") {
        return Ok(format!("{name}.{subscript}"));
    }
    Ok(name.into())
}

fn real(node: Xml<'_, '_>) -> Calc<f64> {
    let data = node.text().ok_or("пустое число")?.trim();
    data.parse().map_err(|_| format!("неверное число {data}"))
}

fn scalar(node: Xml<'_, '_>) -> Calc<Value> {
    if node.tag_name().namespace() != Some(ML) {
        return Err("неверное пространство имён числа".into());
    }
    match kind(node) {
        "real" => Value::scalar(real(node)?),
        "imag" => Value::complex(0., real(node)?),
        "complex" => {
            let mut items = elements(node);
            let re = items
                .next()
                .filter(|&n| named(n, ML, "real"))
                .ok_or("требуется действительная часть")?;
            let im = items
                .next()
                .filter(|&n| named(n, ML, "imag"))
                .ok_or("требуется мнимая часть")?;
            if items.next().is_some() {
                return Err("лишние поля комплексного числа".into());
            }
            Value::complex(real(re)?, real(im)?)
        }
        _ => Err("ожидается число".into()),
    }
}

fn dimensions(node: Xml<'_, '_>) -> Calc<(usize, usize)> {
    let read = |name| {
        node.attribute(name)
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n > 0 && n <= MAX_CELLS)
            .ok_or("неверный размер массива")
    };
    let (rows, cols) = (read("rows")?, read("cols")?);
    if rows > MAX_CELLS / cols {
        return Err("слишком большой массив".into());
    }
    Ok((rows, cols))
}

fn expand_sequence<'a, 'i>(nodes: impl Iterator<Item = Xml<'a, 'i>>) -> Vec<Xml<'a, 'i>> {
    let mut flat = Vec::new();
    for node in nodes {
        if named(node, ML, "sequence") {
            flat.extend(elements(node));
        } else {
            flat.push(node);
        }
    }
    flat
}

pub fn snapshot(eval: Xml<'_, '_>) -> Option<Calc<Datum>> {
    let node = elements(eval).find(|&n| named(n, ML, "result"))?;
    let node = elements(node).next()?;
    Some(if named(node, ML, "matrix") {
        dimensions(node)
            .and_then(|(r, c)| {
                Matrix::from_values(r, c, elements(node).map(scalar).collect::<Calc<_>>()?)
            })
            .map(|matrix| Datum::Matrix(Arc::new(matrix)))
    } else {
        scalar(node).map(Datum::from)
    })
}

pub fn eval_operand<'a, 'i>(node: Xml<'a, 'i>) -> Xml<'a, 'i> {
    if named(node, ML, "eval") {
        elements(node).next().unwrap_or(node)
    } else {
        node
    }
}

pub fn display_unit<'a, 'i>(node: Xml<'a, 'i>) -> Option<Xml<'a, 'i>> {
    if !named(node, ML, "eval") {
        return None;
    }
    elements(node)
        .find(|&n| named(n, ML, "unitOverride"))
        .and_then(|n| elements(n).next())
        .filter(|&n| !named(n, ML, "placeholder"))
}

pub enum Target<'a, 'i> {
    Variable(String),
    Function(String, Calc<Vec<String>>),
    Indexed(String, Vec<Xml<'a, 'i>>),
}

impl Target<'_, '_> {
    pub fn name(&self) -> &str {
        match self {
            Self::Variable(name) | Self::Function(name, _) | Self::Indexed(name, _) => name,
        }
    }
}

pub fn target<'a, 'i>(node: Xml<'a, 'i>) -> Calc<Target<'a, 'i>> {
    let mut items = elements(node);
    if named(node, ML, "function") {
        let name = identifier(items.next().ok_or("у функции нет имени")?)?;
        let args = items
            .next()
            .filter(|&n| named(n, ML, "boundVars"))
            .ok_or("нет аргументов функции")?;
        return Ok(Target::Function(
            name,
            elements(args).map(identifier).collect(),
        ));
    }
    if named(node, ML, "apply") {
        let op = items.next().ok_or("нет операции в левой части")?;
        if named(op, ML, "id") {
            return Ok(Target::Function(
                identifier(op)?,
                expand_sequence(items).into_iter().map(identifier).collect(),
            ));
        }
        if named(op, ML, "indexer") {
            let name = identifier(items.next().ok_or("у массива нет имени")?)?;
            return Ok(Target::Indexed(name, expand_sequence(items)));
        }
    }
    Ok(Target::Variable(identifier(node)?))
}

pub struct Compiler<'a> {
    pub names: &'a HashMap<String, usize>,
    pub deps: &'a mut Vec<usize>,
    pub default_origin: Value,
}

impl Compiler<'_> {
    fn reference(&mut self, name: &str) -> Option<usize> {
        let &index = self.names.get(name)?;
        self.deps.push(index);
        Some(index)
    }

    fn origin(&mut self) -> Expr {
        self.reference("ORIGIN")
            .map(Expr::Ref)
            .unwrap_or(Expr::Literal(self.default_origin))
    }

    pub fn definition(&mut self, target: Target<'_, '_>, node: Xml<'_, '_>) -> Calc<Expr> {
        match target {
            Target::Variable(_) => self.expression(node),
            Target::Function(_, params) => {
                let params = params?;
                if params.len() > 32
                    || params
                        .iter()
                        .enumerate()
                        .any(|(i, p)| params[..i].contains(p))
                {
                    return Err("слишком много или повторяющиеся аргументы функции".into());
                }
                let body = self.compile(node, &params, 0)?;
                Ok(Expr::Function(Arc::new(Function { params, body })))
            }
            Target::Indexed(name, args) => {
                if !(1..=2).contains(&args.len()) {
                    return Err("ожидается один или два индекса".into());
                }
                let previous = self.reference(&name);
                let indices = args
                    .into_iter()
                    .map(|n| self.expression(n))
                    .collect::<Calc<_>>()?;
                let origin = self.origin();
                let body = self.expression(node)?;
                Ok(Expr::Assign(Box::new(IndexedAssignment {
                    previous,
                    indices,
                    origin,
                    body,
                })))
            }
        }
    }

    pub fn expression(&mut self, node: Xml<'_, '_>) -> Calc<Expr> {
        self.compile(node, &[], 0)
    }

    fn compile(&mut self, node: Xml<'_, '_>, params: &[String], depth: usize) -> Calc<Expr> {
        if depth > 96 || node.tag_name().namespace() != Some(ML) {
            return Err("неподдерживаемое пространство имён или глубина формулы".into());
        }
        match kind(node) {
            "real" | "imag" | "complex" => Ok(Expr::Literal(scalar(node)?)),
            "id" => {
                let name = identifier(node)?;
                if let Some(i) = params.iter().position(|p| p == &name) {
                    return Ok(Expr::Param(i));
                }
                if let Some(index) = self.reference(&name) {
                    return Ok(Expr::Ref(index));
                }
                if name == "ORIGIN" {
                    return Ok(self.origin());
                }
                Value::constant(&name)
                    .map(Expr::Literal)
                    .ok_or_else(|| format!("неизвестное имя {name}"))
            }
            "parens" | "eval" => {
                let child = elements(node).next().ok_or("пустое выражение")?;
                self.compile(child, params, depth + 1)
            }
            "matrix" => {
                let (rows, cols) = dimensions(node)?;
                let values = elements(node)
                    .map(|n| self.compile(n, params, depth + 1))
                    .collect::<Calc<_>>()?;
                Ok(Expr::Matrix(rows, cols, values))
            }
            "range" => {
                let items = expand_sequence(elements(node))
                    .into_iter()
                    .map(|n| self.compile(n, params, depth + 1))
                    .collect::<Calc<_>>()?;
                Ok(Expr::Range(items))
            }
            "apply" => {
                let mut children = elements(node);
                let operator = children.next().ok_or("нет операции")?;
                if operator.tag_name().namespace() != Some(ML) {
                    return Err("неизвестное пространство имён операции".into());
                }
                let args = expand_sequence(children);
                if args.len() > 32 {
                    return Err("слишком много аргументов".into());
                }
                if named(operator, ML, "indexer") {
                    let (array, indices) = args.split_first().ok_or("нет массива")?;
                    let array = self.compile(*array, params, depth + 1)?;
                    let indices = indices
                        .iter()
                        .map(|&n| self.compile(n, params, depth + 1))
                        .collect::<Calc<_>>()?;
                    return Ok(Expr::Index(
                        Box::new(array),
                        indices,
                        Box::new(self.origin()),
                    ));
                }
                let is_call = named(operator, ML, "id");
                let name = if is_call {
                    identifier(operator)?
                } else {
                    kind(operator).into()
                };
                let args = args
                    .into_iter()
                    .map(|n| self.compile(n, params, depth + 1))
                    .collect::<Calc<Vec<_>>>()?;
                if is_call {
                    if let Some(index) = self.reference(&name) {
                        return Ok(Expr::Call(index, args));
                    }
                }
                if name == "if" {
                    let [test, yes, no] =
                        <[_; 3]>::try_from(args).map_err(|_| "if требует три аргумента")?;
                    return Ok(Expr::Conditional(
                        Box::new(test),
                        Box::new(yes),
                        Box::new(no),
                    ));
                }
                let op = match name.as_str() {
                    "absval" => "abs",
                    "conjugate" => "conj",
                    other => other,
                };
                Ok(Expr::Builtin(op.into(), args))
            }
            other => Err(format!("неподдерживаемое выражение <{other}>")),
        }
    }
}
