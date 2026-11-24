use crate::eval::{
    CalculusDomain, CalculusExpr, Datum, Expr, Function, IndexedAssignment, Matrix, Stmt, MAX_CELLS,
};
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

fn snapshot_scalar(node: Xml<'_, '_>) -> Calc<Value> {
    if !named(node, ML, "unitedValue") {
        return scalar(node);
    }
    const UNITS: &str = "http://schemas.mathsoft.com/units10";
    let mut parts = elements(node);
    let mut value = scalar(parts.next().ok_or("в эталоне отсутствует число")?)?;
    let monomial = parts
        .next()
        .filter(|&unit| named(unit, UNITS, "unitMonomial"))
        .ok_or("неподдерживаемая запись единиц в эталоне")?;
    let mut factors = elements(monomial).peekable();
    if parts.next().is_some() || factors.peek().is_none() {
        return Err("неверный формат единиц в эталоне".into());
    }
    for factor in factors {
        let unit = match factor.attribute("unit") {
            Some("meter") => "m",
            Some("kilogram") => "kg",
            Some("second") => "s",
            Some("ampere") => "A",
            Some("kelvin") => "K",
            Some("mole") => "mol",
            Some("candela") => "cd",
            _ => return Err("неподдерживаемая единица в эталоне".into()),
        };
        if !named(factor, UNITS, "unitReference")
            || !matches!(factor.attribute("power-denominator"), None | Some("1"))
            || elements(factor).next().is_some()
        {
            return Err("неподдерживаемая степень единицы в эталоне".into());
        }
        let power = factor
            .attribute("power-numerator")
            .unwrap_or("1")
            .parse::<i16>()
            .map_err(|_| "неверная степень единицы в эталоне")?;
        let scale = Value::constant(unit).ok_or("неизвестная базовая единица")?;
        value = value.binary(
            "mult",
            scale.binary("pow", Value::scalar(f64::from(power))?)?,
        )?;
    }
    Ok(value)
}

pub fn snapshot(eval: Xml<'_, '_>) -> Option<Calc<Datum>> {
    let node = elements(eval).find(|&n| named(n, ML, "result"))?;
    let node = elements(node).next()?;
    Some(if named(node, ML, "matrix") {
        dimensions(node)
            .and_then(|(r, c)| {
                Matrix::from_values(
                    r,
                    c,
                    elements(node).map(snapshot_scalar).collect::<Calc<_>>()?,
                )
            })
            .map(|matrix| Datum::Matrix(Arc::new(matrix)))
    } else {
        snapshot_scalar(node).map(Datum::from)
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

    fn resolve_name(&mut self, name: String, params: &[String], locals: &[String]) -> Calc<Expr> {
        if locals.contains(&name) {
            return Ok(Expr::Local(name));
        }
        if let Some(index) = params.iter().position(|param| param == &name) {
            return Ok(Expr::Param(index));
        }
        if let Some(index) = self.reference(&name) {
            return Ok(Expr::Ref(index));
        }
        if name == "ORIGIN" {
            return Ok(Expr::Literal(self.default_origin));
        }
        Value::constant(&name)
            .map(Expr::Literal)
            .ok_or_else(|| format!("неизвестное имя {name}"))
    }

    fn compile_calculus(
        &mut self,
        operator: &str,
        args: &[Xml<'_, '_>],
        params: &[String],
        locals: &mut Vec<String>,
        depth: usize,
    ) -> Calc<Expr> {
        let (lambda, limit) = match args {
            [lambda] => (*lambda, None),
            [lambda, limit] => (*lambda, Some(*limit)),
            _ => return Err(format!("{operator}: неверное число полей оператора")),
        };
        if !named(lambda, ML, "lambda") {
            return Err(format!(
                "{operator}: ожидается выражение с переменной оператора"
            ));
        }
        let mut parts = elements(lambda);
        let variables = parts
            .next()
            .filter(|&node| named(node, ML, "boundVars"))
            .ok_or_else(|| format!("{operator}: отсутствует переменная оператора"))?;
        let mut variables = elements(variables);
        let variable = identifier(variables.next().ok_or("не задана переменная оператора")?)?;
        let body = parts.next().ok_or("оператор без выражения")?;
        if variables.next().is_some() || parts.next().is_some() {
            return Err(format!(
                "{operator}: поддерживается одна переменная и одно выражение"
            ));
        }

        // Limits and the differentiation point belong to the enclosing expression.
        let domain = if operator == "derivative" {
            if let Some(degree) = limit {
                let mut values = elements(degree);
                if !named(degree, ML, "degree")
                    || !values
                        .next()
                        .filter(|&value| named(value, ML, "real"))
                        .is_some_and(|value| real(value) == Ok(1.))
                    || values.next().is_some()
                {
                    return Err("поддерживается только производная первого порядка".into());
                }
            }
            CalculusDomain::Derivative(self.resolve_name(variable.clone(), params, locals)?)
        } else {
            let bounds = limit
                .filter(|&node| named(node, ML, "bounds"))
                .ok_or_else(|| format!("{operator}: нужны явные нижняя и верхняя границы"))?;
            let mut limits = elements(bounds);
            let first = limits.next().ok_or("не задана нижняя граница")?;
            let second = limits.next().ok_or("не задана верхняя граница")?;
            if limits.next().is_some() {
                return Err("ожидаются две границы оператора".into());
            }
            let interval = [
                self.compile(first, params, locals, depth + 1)?,
                self.compile(second, params, locals, depth + 1)?,
            ];
            match operator {
                "integral" => CalculusDomain::Integral(interval),
                "summation" => CalculusDomain::Sum(interval),
                "product" => CalculusDomain::Product(interval),
                _ => unreachable!("unknown calculus operator"),
            }
        };

        locals.push(variable.clone());
        let body = self.compile(body, params, locals, depth + 1);
        locals.pop();
        Ok(Expr::Calculus(Box::new(CalculusExpr {
            domain,
            variable,
            body: body?,
        })))
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
                let mut locals = Vec::new();
                let body = self.compile(node, &params, &mut locals, 0)?;
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
        let mut locals = Vec::new();
        self.compile(node, &[], &mut locals, 0)
    }

    pub fn expression_with_locals(
        &mut self,
        node: Xml<'_, '_>,
        initial_locals: &[String],
    ) -> Calc<Expr> {
        let mut locals = initial_locals.to_vec();
        self.compile(node, &[], &mut locals, 0)
    }

    fn compile_block(
        &mut self,
        nodes: &[Xml<'_, '_>],
        params: &[String],
        locals: &mut Vec<String>,
        depth: usize,
    ) -> Calc<Vec<Stmt>> {
        if depth > 96 {
            return Err("слишком большая глубина программного блока".into());
        }
        let mut stmts: Vec<Stmt> = Vec::with_capacity(nodes.len());
        for &node in nodes {
            if node.tag_name().namespace() != Some(ML) {
                return Err("неподдерживаемое пространство имён в программе".into());
            }
            match kind(node) {
                "otherwise" => {
                    let body_nodes: Vec<_> = elements(node).collect();
                    let else_stmts =
                        self.compile_stmt_list(&body_nodes, params, locals, depth + 1)?;
                    if let Some(Stmt::If { else_branch, .. }) = stmts.last_mut() {
                        *else_branch = else_stmts;
                    } else {
                        stmts.extend(else_stmts);
                    }
                }
                _ => {
                    stmts.push(self.compile_stmt(node, params, locals, depth + 1)?);
                }
            }
        }
        Ok(stmts)
    }

    fn compile_stmt_list(
        &mut self,
        nodes: &[Xml<'_, '_>],
        params: &[String],
        locals: &mut Vec<String>,
        depth: usize,
    ) -> Calc<Vec<Stmt>> {
        if let [single] = nodes {
            if named(*single, ML, "program") {
                let inner: Vec<_> = elements(*single).collect();
                return self.compile_block(&inner, params, locals, depth + 1);
            }
        }
        self.compile_block(nodes, params, locals, depth + 1)
    }

    fn compile_stmt(
        &mut self,
        node: Xml<'_, '_>,
        params: &[String],
        locals: &mut Vec<String>,
        depth: usize,
    ) -> Calc<Stmt> {
        if depth > 96 || node.tag_name().namespace() != Some(ML) {
            return Err("неподдерживаемое пространство имён или глубина программы".into());
        }
        match kind(node) {
            "localDefine" => {
                let items: Vec<_> = elements(node).collect();
                let [lhs, rhs, ..] = items.as_slice() else {
                    return Err("localDefine требует переменную и выражение".into());
                };
                if named(*lhs, ML, "apply") {
                    let mut lhs_parts = elements(*lhs);
                    if let Some(op) = lhs_parts.next() {
                        if named(op, ML, "indexer") {
                            let idx_args = expand_sequence(lhs_parts);
                            let (arr_node, idx_nodes) = idx_args
                                .split_first()
                                .ok_or("localDefine: нет имени массива")?;
                            if !(1..=2).contains(&idx_nodes.len()) {
                                return Err("ожидается один или два индекса".into());
                            }
                            let name = identifier(*arr_node)?;
                            let indices = idx_nodes
                                .iter()
                                .map(|&n| self.compile(n, params, locals, depth + 1))
                                .collect::<Calc<Vec<_>>>()?;
                            let value = self.compile(*rhs, params, locals, depth + 1)?;
                            if !locals.contains(&name) {
                                locals.push(name.clone());
                            }
                            return Ok(Stmt::LocalIndexedAssign {
                                name,
                                indices,
                                origin: self.origin(),
                                value,
                            });
                        }
                    }
                }
                let name = identifier(*lhs)?;
                let expr = self.compile(*rhs, params, locals, depth + 1)?;
                if !locals.contains(&name) {
                    locals.push(name.clone());
                }
                Ok(Stmt::LocalAssign(name, expr))
            }
            "if" => {
                let items: Vec<_> = elements(node).collect();
                match items.as_slice() {
                    [cond, then_node] => {
                        let cond = self.compile(*cond, params, locals, depth + 1)?;
                        let then_branch =
                            self.compile_stmt_list(&[*then_node], params, locals, depth + 1)?;
                        Ok(Stmt::If {
                            cond,
                            then_branch,
                            else_branch: Vec::new(),
                        })
                    }
                    [cond, then_node, else_node] => {
                        let cond = self.compile(*cond, params, locals, depth + 1)?;
                        let then_branch =
                            self.compile_stmt_list(&[*then_node], params, locals, depth + 1)?;
                        let else_nodes: Vec<_> = if named(*else_node, ML, "otherwise") {
                            elements(*else_node).collect()
                        } else {
                            vec![*else_node]
                        };
                        let else_branch =
                            self.compile_stmt_list(&else_nodes, params, locals, depth + 1)?;
                        Ok(Stmt::If {
                            cond,
                            then_branch,
                            else_branch,
                        })
                    }
                    _ => Err("if в программе требует условие и ветку (и опционально иначе)".into()),
                }
            }
            "for" => {
                let items: Vec<_> = elements(node).collect();
                let [var_node, iter_node, body_node] = items.as_slice() else {
                    return Err("for требует переменную, диапазон/вектор и тело цикла".into());
                };
                let var = identifier(*var_node)?;
                let iter = self.compile(*iter_node, params, locals, depth + 1)?;
                if !locals.contains(&var) {
                    locals.push(var.clone());
                }
                let body = self.compile_stmt_list(&[*body_node], params, locals, depth + 1)?;
                Ok(Stmt::For { var, iter, body })
            }
            "while" => {
                let items: Vec<_> = elements(node).collect();
                let [cond_node, body_node] = items.as_slice() else {
                    return Err("while требует условие и тело цикла".into());
                };
                let cond = self.compile(*cond_node, params, locals, depth + 1)?;
                let body = self.compile_stmt_list(&[*body_node], params, locals, depth + 1)?;
                Ok(Stmt::While { cond, body })
            }
            "return" => {
                let expr_node = elements(node).next().ok_or("return требует выражение")?;
                Ok(Stmt::Return(self.compile(
                    expr_node,
                    params,
                    locals,
                    depth + 1,
                )?))
            }
            "break" => Ok(Stmt::Break),
            "continue" => Ok(Stmt::Continue),
            _ => Ok(Stmt::Expr(self.compile(node, params, locals, depth + 1)?)),
        }
    }

    fn compile(
        &mut self,
        node: Xml<'_, '_>,
        params: &[String],
        locals: &mut Vec<String>,
        depth: usize,
    ) -> Calc<Expr> {
        if depth > 96 || node.tag_name().namespace() != Some(ML) {
            return Err("неподдерживаемое пространство имён или глубина формулы".into());
        }
        match kind(node) {
            "real" | "imag" | "complex" => Ok(Expr::Literal(scalar(node)?)),
            "id" => self.resolve_name(identifier(node)?, params, locals),
            "parens" | "eval" => {
                let child = elements(node).next().ok_or("пустое выражение")?;
                self.compile(child, params, locals, depth + 1)
            }
            "program" => {
                let children: Vec<_> = elements(node).collect();
                let mut inner_locals = locals.clone();
                let stmts = self.compile_block(&children, params, &mut inner_locals, depth + 1)?;
                Ok(Expr::Program(stmts))
            }
            "matrix" => {
                let (rows, cols) = dimensions(node)?;
                let values = elements(node)
                    .map(|n| self.compile(n, params, locals, depth + 1))
                    .collect::<Calc<_>>()?;
                Ok(Expr::Matrix(rows, cols, values))
            }
            "range" => {
                let items = expand_sequence(elements(node))
                    .into_iter()
                    .map(|n| self.compile(n, params, locals, depth + 1))
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
                let tag = kind(operator);
                if matches!(tag, "derivative" | "integral" | "summation" | "product") {
                    return self.compile_calculus(tag, &args, params, locals, depth);
                }
                if named(operator, ML, "indexer") {
                    let (array, indices) = args.split_first().ok_or("нет массива")?;
                    let array = self.compile(*array, params, locals, depth + 1)?;
                    let indices = indices
                        .iter()
                        .map(|&n| self.compile(n, params, locals, depth + 1))
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
                    .map(|n| self.compile(n, params, locals, depth + 1))
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
