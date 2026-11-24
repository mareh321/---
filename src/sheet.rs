use crate::eval::{ConstraintKind, Datum, Expr, SolveBlockExpr, SolveMode};
use crate::parse::{self, elements, identifier, kind, named, Compiler, Xml, ML};
#[cfg(feature = "mcp")]
pub use crate::presentation::RegionVisual;
use crate::value::{Calc, Value};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const WS: &str = "http://schemas.mathsoft.com/worksheet30";

pub struct Region {
    pub source_id: String,
    pub label: String,
    pub text: Option<String>,
    pub imported: bool,
    #[cfg(feature = "mcp")]
    pub visual: RegionVisual,
    #[cfg(feature = "mcp")]
    defines_name: bool,
    input_unit: Option<Value>,
    formula: Option<Calc<Expr>>,
    snapshot: Option<Calc<Datum>>,
    display_unit: Option<(String, Expr)>,
}

impl Region {
    pub fn has_formula(&self) -> bool {
        self.formula.is_some()
    }

    pub fn is_page_break(&self) -> bool {
        !self.imported && self.formula.is_none() && self.text.as_deref() == Some("Разрыв страницы")
    }
}

#[cfg(feature = "mcp")]
#[derive(Clone, Debug)]
pub struct SurfaceGrid {
    pub rows: usize,
    pub cols: usize,
    pub x_from: f64,
    pub x_to: f64,
    pub y_from: f64,
    pub y_to: f64,
    pub z_min: f64,
    pub z_max: f64,
    pub values: Vec<Option<f64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotReport {
    pub ok: bool,
    pub matched: usize,
    pub failed: usize,
    pub skipped: usize,
    pub mismatches: Vec<String>,
}

pub struct Sheet {
    pub regions: Vec<Region>,
    cache: Vec<Option<Calc<Datum>>>,
    default_origin: Value,
    overrides: Vec<Option<Value>>,
    dependents: Vec<Vec<usize>>,
    dirty: Vec<usize>,
    queued: Vec<bool>,
    eval_order: Vec<usize>,
    eval_rank: Vec<usize>,
    exported_names: HashMap<String, usize>,
}

fn is_global_define_region(node: Xml<'_, '_>) -> bool {
    let Some(math) = elements(node)
        .find(|child| !named(*child, WS, "rendering"))
        .filter(|&child| kind(child) == "math")
    else {
        return false;
    };
    let mut statement = math;
    while let Some(next) =
        elements(statement).find(|child| child.tag_name().namespace() == Some(ML))
    {
        statement = next;
        if !named(statement, ML, "provenance") {
            return named(statement, ML, "globalDefine");
        }
    }
    false
}

fn resolve_reference_path(href: &str, base_dir: Option<&Path>) -> Calc<PathBuf> {
    let cleaned = href
        .trim()
        .strip_prefix("file:///")
        .or_else(|| href.trim().strip_prefix("file://"))
        .unwrap_or(href.trim());
    if cleaned.is_empty() {
        return Err("пустой путь в подключении листа".into());
    }
    let raw = PathBuf::from(cleaned);
    let candidate = if raw.is_absolute() {
        raw
    } else if let Some(dir) = base_dir {
        dir.join(raw)
    } else {
        return Err(format!(
            "невозможно разрешить относительный путь «{cleaned}» без сохранения исходного листа"
        ));
    };
    std::fs::canonicalize(&candidate)
        .map_err(|_| format!("подключаемый лист не найден: {}", candidate.display()))
}

fn unit_label(node: Xml<'_, '_>) -> String {
    if let Ok(name) = identifier(node) {
        return name;
    }
    if named(node, ML, "apply") {
        let items: Vec<_> = elements(node).collect();
        if let [op, a, b] = items.as_slice() {
            let sym = match kind(*op) {
                "div" => "/",
                "mult" | "scale" => "·",
                "pow" => "^",
                _ => "",
            };
            if !sym.is_empty() {
                let left = unit_label(*a);
                let right = if named(*b, ML, "real") {
                    b.text().unwrap_or_default().trim().to_owned()
                } else {
                    unit_label(*b)
                };
                return if sym == "^" {
                    format!("{left}^{right}")
                } else {
                    format!("{left}{sym}{right}")
                };
            }
        }
    }
    "ед. листа".into()
}

fn input_unit(
    node: Xml<'_, '_>,
    names: &HashMap<String, usize>,
    default_origin: Value,
) -> Option<Value> {
    if named(node, ML, "real") {
        return Value::scalar(1.).ok();
    }
    let children: Vec<_> = elements(node).collect();
    if let [op, number, unit] = children.as_slice() {
        if named(node, ML, "apply")
            && (named(*op, ML, "scale") || named(*op, ML, "mult"))
            && named(*number, ML, "real")
        {
            let mut deps = Vec::new();
            let unit = Compiler {
                names,
                deps: &mut deps,
                default_origin,
            }
            .expression(*unit)
            .ok()?;
            if deps.is_empty() {
                return unit
                    .eval(&[])
                    .ok()?
                    .scalar()
                    .ok()
                    .filter(|unit| unit.number != 0. && unit.imaginary == 0.);
            }
        }
    }
    None
}

fn position(region: Xml<'_, '_>, key: &str, fallback: &str) -> Calc<f64> {
    let number = region
        .attribute(key)
        .or_else(|| region.attribute(fallback))
        .unwrap_or("0");
    number
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("некорректная координата {key}={number}"))
}

type PendingConstraint<'a, 'input> = (ConstraintKind, Xml<'a, 'input>, Xml<'a, 'input>);

fn is_given_statement(statement: Xml<'_, '_>) -> bool {
    let target = parse::eval_operand(statement);
    identifier(target)
        .ok()
        .is_some_and(|name| name.eq_ignore_ascii_case("given"))
}

fn extract_constraint<'a, 'input>(
    statement: Xml<'a, 'input>,
) -> Option<PendingConstraint<'a, 'input>> {
    let body = parse::eval_operand(statement);
    if !named(body, ML, "apply") {
        return None;
    }
    let items: Vec<_> = elements(body).collect();
    let [op, lhs, rhs] = items.as_slice() else {
        return None;
    };
    let ckind = match kind(*op) {
        "equal" => ConstraintKind::Eq,
        "lessOrEqual" | "lessThan" => ConstraintKind::Le,
        "greaterOrEqual" | "greaterThan" => ConstraintKind::Ge,
        _ => return None,
    };
    Some((ckind, *lhs, *rhs))
}

fn extract_solve_call<'a, 'input>(
    expr: Xml<'a, 'input>,
) -> Option<(SolveMode, Vec<Xml<'a, 'input>>)> {
    let body = parse::eval_operand(expr);
    if !named(body, ML, "apply") {
        return None;
    }
    let mut items = elements(body);
    let op = items.next()?;
    let name = identifier(op).ok()?;
    let mode = match name.to_ascii_lowercase().as_str() {
        "find" => SolveMode::Find,
        "minerr" => SolveMode::Minerr,
        "minimize" => SolveMode::Minimize,
        "maximize" => SolveMode::Maximize,
        _ => return None,
    };
    let mut args = Vec::new();
    for item in items {
        if named(item, ML, "sequence") {
            args.extend(elements(item));
        } else {
            args.push(item);
        }
    }
    Some((mode, args))
}

fn compile_solve_block<'a, 'input>(
    mode: SolveMode,
    args: &[Xml<'a, 'input>],
    raw_constraints: &[PendingConstraint<'a, 'input>],
    names: &HashMap<String, usize>,
    deps: &mut Vec<usize>,
    default_origin: Value,
) -> Calc<Expr> {
    let (obj_node, var_nodes) = match mode {
        SolveMode::Find | SolveMode::Minerr => (None, args),
        SolveMode::Minimize | SolveMode::Maximize => {
            let (first, rest) = args
                .split_first()
                .ok_or("Minimize / Maximize требует целевую функцию и переменные")?;
            (Some(*first), rest)
        }
    };
    if var_nodes.is_empty() {
        return Err("укажите хотя бы одну искомую переменную".into());
    }
    let mut vars = Vec::with_capacity(var_nodes.len());
    let mut var_names = Vec::with_capacity(var_nodes.len());
    for &vn in var_nodes {
        let name = identifier(vn)?;
        let &guess_idx = names
            .get(&name)
            .ok_or_else(|| format!("задайте начальное приближение {name} выше блока Given"))?;
        deps.push(guess_idx);
        vars.push((name.clone(), guess_idx));
        var_names.push(name);
    }
    let mut compiler = Compiler {
        names,
        deps,
        default_origin,
    };
    let objective = if let Some(node) = obj_node {
        if let Ok(fn_name) = identifier(node) {
            if let Some(&fn_idx) = names.get(&fn_name) {
                compiler.deps.push(fn_idx);
                let call_args = var_names.iter().map(|v| Expr::Local(v.clone())).collect();
                Some(Expr::Call(fn_idx, call_args))
            } else {
                Some(compiler.expression_with_locals(node, &var_names)?)
            }
        } else {
            Some(compiler.expression_with_locals(node, &var_names)?)
        }
    } else {
        None
    };
    let mut constraints = Vec::with_capacity(raw_constraints.len());
    for &(ckind, lhs, rhs) in raw_constraints {
        let l = compiler.expression_with_locals(lhs, &var_names)?;
        let r = compiler.expression_with_locals(rhs, &var_names)?;
        constraints.push((ckind, Expr::Builtin("minus".into(), vec![l, r])));
    }
    Ok(Expr::SolveBlock(Box::new(SolveBlockExpr {
        mode,
        vars,
        objective,
        constraints,
    })))
}

impl Sheet {
    pub fn import(xml: &str) -> Calc<Self> {
        Self::import_with_base(xml, None)
    }

    pub fn import_from_path(path: &Path) -> Calc<Self> {
        let canonical =
            std::fs::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let xml = crate::read_xml(&canonical)?;
        let mut visiting = vec![canonical.clone()];
        Self::import_internal(&xml, canonical.parent(), &mut visiting)
    }

    pub fn import_with_base(xml: &str, base_dir: Option<&Path>) -> Calc<Self> {
        let mut visiting = Vec::new();
        Self::import_internal(xml, base_dir, &mut visiting)
    }

    fn import_internal(
        xml: &str,
        base_dir: Option<&Path>,
        visiting: &mut Vec<PathBuf>,
    ) -> Calc<Self> {
        if visiting.len() > 16 {
            return Err("слишком большая глубина подключения листов XMCD".into());
        }
        let document = Document::parse_with_options(
            xml,
            ParsingOptions {
                nodes_limit: 500_000,
                ..ParsingOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
        let root = document.root_element();
        if !named(root, WS, "worksheet") {
            return Err("ожидается лист XMCD Mathcad с пространством имён worksheet30".into());
        }
        let container = elements(root)
            .find(|node| named(*node, WS, "regions"))
            .ok_or("лист не содержит блока regions")?;
        let nested_effect = container.descendants().find(|node| {
            node.is_element()
                && node.tag_name().namespace() == Some(WS)
                && kind(*node) == "component"
        });
        if let Some(node) = nested_effect {
            return Err(format!(
                "лист содержит <{}>, который может менять порядок или значения расчётов",
                kind(node)
            ));
        }

        let mut ordered = Vec::new();
        fn collect_regions<'a, 'i>(
            parent: Xml<'a, 'i>,
            out: &mut Vec<(f64, f64, Xml<'a, 'i>)>,
        ) -> Calc<()> {
            for child in elements(parent) {
                if named(child, WS, "region") {
                    if let Some(area) = elements(child).find(|&n| named(n, WS, "area")) {
                        if let Some(inner_regions) =
                            elements(area).find(|&n| named(n, WS, "regions"))
                        {
                            collect_regions(inner_regions, out)?;
                        } else {
                            collect_regions(area, out)?;
                        }
                    } else {
                        out.push((
                            position(child, "align-y", "top")?,
                            position(child, "left", "left")?,
                            child,
                        ));
                    }
                } else if named(child, WS, "area") {
                    if let Some(inner_regions) = elements(child).find(|&n| named(n, WS, "regions"))
                    {
                        collect_regions(inner_regions, out)?;
                    } else {
                        collect_regions(child, out)?;
                    }
                } else {
                    return Err(format!("неподдерживаемый элемент листа <{}>", kind(child)));
                }
            }
            Ok(())
        }
        collect_regions(container, &mut ordered)?;
        ordered.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let default_origin = elements(root)
            .find(|&node| named(node, WS, "settings"))
            .and_then(|settings| {
                settings
                    .descendants()
                    .find(|&node| named(node, WS, "builtInVariables"))
                    .and_then(|node| node.attribute("array-origin"))
                    .or_else(|| {
                        settings
                            .descendants()
                            .find(|&node| named(node, WS, "calculationBehavior"))
                            .and_then(|node| node.attribute("array-origin"))
                    })
            })
            .unwrap_or("0")
            .parse::<i32>()
            .map_err(|_| "некорректное значение array-origin")?;
        let size = ordered.len();
        let default_origin = Value::scalar(f64::from(default_origin))?;
        let mut sheet = Self {
            regions: (0..size)
                .map(|_| Region {
                    source_id: String::new(),
                    label: String::new(),
                    text: None,
                    imported: false,
                    #[cfg(feature = "mcp")]
                    visual: RegionVisual::default(),
                    #[cfg(feature = "mcp")]
                    defines_name: false,
                    input_unit: None,
                    formula: None,
                    snapshot: None,
                    display_unit: None,
                })
                .collect(),
            cache: vec![None; size],
            default_origin,
            overrides: vec![None; size],
            dependents: vec![Vec::new(); size],
            dirty: Vec::new(),
            queued: vec![false; size],
            eval_order: Vec::with_capacity(size),
            eval_rank: Vec::new(),
            exported_names: HashMap::new(),
        };
        let mut names = HashMap::new();
        let is_global: Vec<bool> = ordered
            .iter()
            .map(|(_, _, node)| is_global_define_region(*node))
            .collect();

        let mut pending_solve: Option<Vec<PendingConstraint<'_, '_>>> = None;
        // Pass 1: global definitions (<ml:globalDefine>) in visual order
        for (index, (_, _, node)) in ordered.iter().enumerate() {
            if is_global[index] {
                sheet.import_region_at(
                    index,
                    *node,
                    &mut names,
                    base_dir,
                    visiting,
                    &mut pending_solve,
                )?;
                sheet.eval_order.push(index);
            }
        }
        // Pass 2: normal regions (including <reference>/<include>) in visual order
        for (index, (_, _, node)) in ordered.iter().enumerate() {
            if !is_global[index] {
                sheet.import_region_at(
                    index,
                    *node,
                    &mut names,
                    base_dir,
                    visiting,
                    &mut pending_solve,
                )?;
                sheet.eval_order.push(index);
            }
        }
        let total = sheet.regions.len();
        sheet.eval_rank = vec![total; total];
        for (rank, &idx) in sheet.eval_order.iter().enumerate() {
            sheet.eval_rank[idx] = rank;
        }
        sheet.exported_names = names;
        Ok(sheet)
    }

    fn import_region_at<'a, 'input>(
        &mut self,
        index: usize,
        node: Xml<'a, 'input>,
        names: &mut HashMap<String, usize>,
        base_dir: Option<&Path>,
        visiting: &mut Vec<PathBuf>,
        pending_solve: &mut Option<Vec<PendingConstraint<'a, 'input>>>,
    ) -> Calc<()> {
        let mut region = Region {
            source_id: node.attribute("region-id").unwrap_or("?").into(),
            label: String::new(),
            text: None,
            imported: false,
            #[cfg(feature = "mcp")]
            visual: RegionVisual::new(node),
            #[cfg(feature = "mcp")]
            defines_name: false,
            input_unit: None,
            formula: None,
            snapshot: None,
            display_unit: None,
        };
        let mut deps = Vec::new();
        let content = elements(node).find(|child| !named(*child, WS, "rendering"));
        match content.map(kind) {
            Some("reference" | "include") => {
                let ref_node = content.ok_or("нет ссылки")?;
                let href = ref_node
                    .attribute("href")
                    .or_else(|| ref_node.attribute("file"))
                    .or_else(|| ref_node.attribute("path"))
                    .ok_or("подключение листа без атрибута href")?;
                let target_path = resolve_reference_path(href, base_dir)?;
                if visiting.iter().any(|p| p == &target_path) {
                    return Err(format!(
                        "обнаружен цикл подключения листов: {}",
                        target_path.display()
                    ));
                }
                let ref_xml = crate::read_xml(&target_path)?;
                visiting.push(target_path.clone());
                let imported = Self::import_internal(&ref_xml, target_path.parent(), visiting);
                visiting.pop();
                let mut child = imported?;
                let offset = self.regions.len();
                for mut child_region in child.regions.drain(..) {
                    child_region.imported = true;
                    if let Some(Ok(expr)) = &mut child_region.formula {
                        expr.shift_refs(offset);
                    }
                    if let Some((_, unit_expr)) = &mut child_region.display_unit {
                        unit_expr.shift_refs(offset);
                    }
                    self.regions.push(child_region);
                }
                let added = self.regions.len() - offset;
                self.cache.resize(self.regions.len(), None);
                self.overrides.resize(self.regions.len(), None);
                self.queued.resize(self.regions.len(), false);
                for mut child_deps in child.dependents.drain(..) {
                    for d in &mut child_deps {
                        *d += offset;
                    }
                    self.dependents.push(child_deps);
                }
                debug_assert_eq!(self.dependents.len(), offset + added);
                for idx in child.eval_order {
                    self.eval_order.push(idx + offset);
                }
                for (name, idx) in child.exported_names {
                    names.insert(name, idx + offset);
                }
                region.label = "Ссылка".into();
                region.text = Some(format!("Ссылка: {href}"));
            }
            Some("text") => {
                let text = content
                    .ok_or("нет текста")?
                    .descendants()
                    .filter(Node::is_text)
                    .filter_map(|part| part.text())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                region.text = Some(text);
                #[cfg(feature = "mcp")]
                {
                    region.text = content.map(|node| region.visual.set_text(node));
                }
            }
            Some("math") => {
                let math = content.ok_or("нет формулы")?;
                let mut statement = math;
                loop {
                    statement = elements(statement)
                        .find(|child| child.tag_name().namespace() == Some(ML))
                        .ok_or_else(|| {
                            format!("регион {} не содержит формулы math30", region.source_id)
                        })?;
                    if !named(statement, ML, "provenance") {
                        break;
                    }
                }
                let children: Vec<_> = elements(statement).collect();
                let disabled = math.attribute("disable-calc") == Some("true");
                if !disabled && is_given_statement(statement) {
                    *pending_solve = Some(Vec::new());
                    region.label = "Given".into();
                    region.formula = Some(Ok(Expr::Literal(Value::scalar(1.)?)));
                    #[cfg(feature = "mcp")]
                    {
                        region.visual.set_formula(statement, false);
                        region.visual.function = true;
                    }
                    self.regions[index] = region;
                    return Ok(());
                }
                if !disabled && pending_solve.is_some() {
                    if let Some(constraint) = extract_constraint(statement) {
                        if let Some(list) = pending_solve.as_mut() {
                            list.push(constraint);
                        }
                        region.label = "Ограничение".into();
                        region.formula = Some(Ok(Expr::Literal(Value::scalar(1.)?)));
                        #[cfg(feature = "mcp")]
                        {
                            region.visual.set_formula(statement, false);
                            region.visual.function = true;
                        }
                        self.regions[index] = region;
                        return Ok(());
                    }
                }
                match (kind(statement), children.as_slice()) {
                    ("define" | "globalDefine", [left, expr, ..]) => {
                        #[cfg(feature = "mcp")]
                        {
                            region.defines_name = true;
                        }
                        let target = parse::target(*left)?;
                        let name = target.name().to_owned();
                        region.label = name.clone();
                        region.snapshot = parse::snapshot(*expr);
                        region.formula = Some(if disabled {
                            Err("вычисление отключено в исходном листе".into())
                        } else {
                            if matches!(target, parse::Target::Variable(_)) {
                                region.input_unit = input_unit(
                                    parse::eval_operand(*expr),
                                    names,
                                    self.default_origin,
                                );
                            }
                            if pending_solve.is_some() {
                                if let Some((mode, args)) = extract_solve_call(*expr) {
                                    let raw_c = pending_solve.take().unwrap_or_default();
                                    compile_solve_block(
                                        mode,
                                        &args,
                                        &raw_c,
                                        names,
                                        &mut deps,
                                        self.default_origin,
                                    )
                                } else {
                                    let mut compiler = Compiler {
                                        names,
                                        deps: &mut deps,
                                        default_origin: self.default_origin,
                                    };
                                    let result = compiler.definition(target, *expr);
                                    region.display_unit = parse::display_unit(*expr)
                                        .map(|unit| {
                                            compiler.expression(unit).map(|e| (unit_label(unit), e))
                                        })
                                        .transpose()?;
                                    result
                                }
                            } else {
                                let mut compiler = Compiler {
                                    names,
                                    deps: &mut deps,
                                    default_origin: self.default_origin,
                                };
                                let result = compiler.definition(target, *expr);
                                region.display_unit = parse::display_unit(*expr)
                                    .map(|unit| {
                                        compiler.expression(unit).map(|e| (unit_label(unit), e))
                                    })
                                    .transpose()?;
                                result
                            }
                        });
                        names.insert(name, index);
                    }
                    ("eval", [expr, ..]) => {
                        region.label = identifier(*expr).unwrap_or_else(|_| "Результат".into());
                        region.snapshot = parse::snapshot(statement);
                        region.formula = Some(if disabled {
                            Err("вычисление отключено в исходном листе".into())
                        } else {
                            if pending_solve.is_some() {
                                if let Some((mode, args)) = extract_solve_call(*expr) {
                                    let raw_c = pending_solve.take().unwrap_or_default();
                                    compile_solve_block(
                                        mode,
                                        &args,
                                        &raw_c,
                                        names,
                                        &mut deps,
                                        self.default_origin,
                                    )
                                } else {
                                    let mut compiler = Compiler {
                                        names,
                                        deps: &mut deps,
                                        default_origin: self.default_origin,
                                    };
                                    compiler.expression(*expr).and_then(|expr| {
                                        region.display_unit = parse::display_unit(statement)
                                            .map(|unit| {
                                                compiler
                                                    .expression(unit)
                                                    .map(|e| (unit_label(unit), e))
                                            })
                                            .transpose()?;
                                        Ok(expr)
                                    })
                                }
                            } else {
                                let mut compiler = Compiler {
                                    names,
                                    deps: &mut deps,
                                    default_origin: self.default_origin,
                                };
                                compiler.expression(*expr).and_then(|expr| {
                                    region.display_unit = parse::display_unit(statement)
                                        .map(|unit| {
                                            compiler.expression(unit).map(|e| (unit_label(unit), e))
                                        })
                                        .transpose()?;
                                    Ok(expr)
                                })
                            }
                        });
                    }
                    ("symEval", _) => {
                        region.label = "Символьное вычисление".into();
                        region.formula = Some(if disabled {
                            Err("вычисление отключено в исходном листе".into())
                        } else {
                            crate::symbolic::from_ml_node(statement, 0).map(|sym| {
                                let mut used = std::collections::HashSet::new();
                                sym.collect_vars(&mut used);
                                let mut bindings = Vec::new();
                                for var_name in used {
                                    if let Some(&ref_idx) = names.get(&var_name) {
                                        deps.push(ref_idx);
                                        bindings.push((var_name, ref_idx));
                                    }
                                }
                                Expr::Symbolic(Box::new(sym), bindings)
                            })
                        });
                    }
                    (
                        "apply" | "parens" | "id" | "real" | "imag" | "complex" | "matrix"
                        | "range" | "program",
                        _,
                    ) => {
                        region.label = "Выражение".into();
                        region.formula = Some(if disabled {
                            Err("вычисление отключено в исходном листе".into())
                        } else {
                            Compiler {
                                names,
                                deps: &mut deps,
                                default_origin: self.default_origin,
                            }
                            .expression(statement)
                        });
                    }
                    (tag, _) => {
                        return Err(format!(
                            "регион {} содержит неподдерживаемую операцию <{tag}>",
                            region.source_id
                        ))
                    }
                }
                #[cfg(feature = "mcp")]
                region
                    .visual
                    .set_formula(statement, region.input_unit.is_some());
            }
            Some(tag @ ("graph" | "plot" | "xyPlot" | "image" | "picture")) => {
                region.text = Some(format!("Блок <{tag}> пропущен"));
                #[cfg(feature = "mcp")]
                {
                    region.visual.graphical = true;
                }
            }
            Some("pageBreak") => region.text = Some("Разрыв страницы".into()),
            Some("solveBlock") => region.text = Some("Блок решения".into()),
            Some(tag) => {
                return Err(format!(
                    "регион {} содержит неизвестный блок <{tag}>",
                    region.source_id
                ))
            }
            None => region.text = Some("Пустой регион".into()),
        }
        deps.sort_unstable();
        deps.dedup();
        for dep in deps {
            self.dependents[dep].push(index);
        }
        self.regions[index] = region;
        Ok(())
    }

    fn evaluate(&mut self, index: usize) -> bool {
        if let Some(formula) = &self.regions[index].formula {
            let result = match (self.overrides[index], formula) {
                (Some(value), _) => Ok(value.into()),
                (None, Ok(expr)) => expr.eval(&self.cache),
                (None, Err(reason)) => Err(reason.clone()),
            };
            self.cache[index] = Some(result);
            return true;
        }
        false
    }

    pub fn full(&mut self) -> usize {
        for index in self.dirty.drain(..) {
            self.queued[index] = false;
        }
        let mut count = 0;
        for i in 0..self.eval_order.len() {
            let index = self.eval_order[i];
            if self.evaluate(index) {
                count += 1;
            }
        }
        count
    }

    pub fn incremental(&mut self) -> usize {
        let ranks = &self.eval_rank;
        self.dirty.sort_unstable_by_key(|&idx| ranks[idx]);
        let count = self.dirty.len();
        for cursor in 0..count {
            let index = self.dirty[cursor];
            self.evaluate(index);
            self.queued[index] = false;
        }
        self.dirty.clear();
        count
    }

    fn replace(&mut self, index: usize, number: Option<f64>, unit: Option<&str>) -> Calc<bool> {
        let original = self
            .regions
            .get(index)
            .and_then(|region| region.input_unit)
            .ok_or("выберите номер изменяемого входа (*)")?;
        let replacement = if let Some(number) = number {
            let scale = match unit {
                Some(name) => {
                    Value::constant(name).ok_or_else(|| format!("неизвестная единица {name}"))?
                }
                None => original,
            };
            if !original.compatible(scale) {
                return Err("единица не совпадает с размерностью входа".into());
            }
            Some(scale.multiply_number(number)?)
        } else {
            None
        };
        let changed = replacement != self.overrides[index];
        self.overrides[index] = replacement;
        Ok(changed)
    }

    pub fn set(&mut self, index: usize, number: Option<f64>, unit: Option<&str>) -> Calc<()> {
        if self.replace(index, number, unit)? && !self.queued[index] {
            self.queued[index] = true;
            let mut cursor = self.dirty.len();
            self.dirty.push(index);
            while cursor < self.dirty.len() {
                let region = self.dirty[cursor];
                cursor += 1;
                for &dependent in &self.dependents[region] {
                    if !self.queued[dependent] {
                        self.queued[dependent] = true;
                        self.dirty.push(dependent);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn set_and_recalculate_all(&mut self, index: usize, number: f64) -> Calc<usize> {
        self.replace(index, Some(number), None)?;
        Ok(self.full())
    }

    pub fn editable(&self, index: usize) -> bool {
        !self.regions[index].imported && self.regions[index].input_unit.is_some()
    }

    #[cfg(feature = "mcp")]
    pub fn input_value(&self, index: usize) -> Option<f64> {
        let unit = self.regions.get(index)?.input_unit?;
        let actual = self
            .cache
            .get(index)?
            .as_ref()?
            .as_ref()
            .ok()?
            .scalar()
            .ok()?;
        actual.in_units(unit).ok()?.real_scalar().ok()
    }

    #[cfg(feature = "mcp")]
    pub fn edited(&self, index: usize) -> bool {
        self.overrides.get(index).is_some_and(Option::is_some)
    }

    #[cfg(feature = "mcp")]
    pub fn counts(&self) -> (usize, usize, usize) {
        let inputs = self
            .regions
            .iter()
            .filter(|region| !region.imported && region.input_unit.is_some())
            .count();
        let formulas = self
            .regions
            .iter()
            .filter(|region| !region.imported && region.formula.is_some())
            .count();
        let errors = self
            .regions
            .iter()
            .zip(&self.cache)
            .filter(|(region, result)| !region.imported && matches!(result, Some(Err(_))))
            .count();
        (inputs, formulas, errors)
    }

    #[cfg(feature = "mcp")]
    pub fn skipped_visual_count(&self) -> usize {
        self.regions
            .iter()
            .filter(|region| !region.imported && region.visual.graphical)
            .count()
    }

    #[cfg(feature = "mcp")]
    pub fn chart_curves(
        &self,
        before: usize,
        chart: &crate::editing::ChartSpec,
    ) -> Calc<Vec<(String, Vec<Option<[f64; 2]>>)>> {
        use crate::editing::{ChartSpec, FormulaSource};
        let chart_rank = *self
            .eval_rank
            .get(before)
            .ok_or("неверная позиция графика")?;
        let mut names = HashMap::new();
        for &idx in &self.eval_order[..chart_rank] {
            let r = &self.regions[idx];
            if r.defines_name {
                names.insert(r.label.clone(), idx);
            }
        }
        let series_values = |name: &str| -> Calc<Vec<Value>> {
            let trimmed = name.trim();
            let index = names
                .get(trimmed)
                .ok_or_else(|| format!("Определите {trimmed} выше графика"))?;
            let datum = self.cache[*index]
                .as_ref()
                .ok_or("нет значения для графика")?
                .as_ref()
                .map_err(Clone::clone)?;
            match datum {
                Datum::Scalar(value) => Ok(vec![*value]),
                other => other.vector_values(),
            }
        };
        match chart {
            ChartSpec::Vectors { x, y } => {
                let x_parts: Vec<_> = x
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                let y_parts: Vec<_> = y
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                if x_parts.is_empty() || y_parts.is_empty() {
                    return Err("укажите массивы X и Y для графика".into());
                }
                if x_parts.len() != y_parts.len() && x_parts.len() != 1 && y_parts.len() != 1 {
                    return Err("число серий по X и Y должно совпадать".into());
                }
                let count = x_parts.len().max(y_parts.len()).min(8);
                let valid = |values: &[Value]| {
                    let first = values[0];
                    values
                        .iter()
                        .all(|v| v.imaginary == 0. && v.compatible(first) && v.number.abs() < 1e100)
                };
                let mut curves = Vec::with_capacity(count);
                for i in 0..count {
                    let x_name = x_parts[i.min(x_parts.len() - 1)];
                    let y_name = y_parts[i.min(y_parts.len() - 1)];
                    let x_values = series_values(x_name)?;
                    let y_values = series_values(y_name)?;
                    if x_values.len() != y_values.len() || x_values.is_empty() {
                        return Err("массивы X и Y должны иметь одинаковую ненулевую длину".into());
                    }
                    if !valid(&x_values) || !valid(&y_values) {
                        return Err("ось требует действительных значений одной размерности".into());
                    }
                    let points = x_values
                        .into_iter()
                        .zip(y_values)
                        .map(|(a, b)| Some([a.number, b.number]))
                        .collect();
                    curves.push((y_name.into(), points));
                }
                Ok(curves)
            }
            ChartSpec::Function {
                expression,
                from,
                to,
            } => {
                if !from.is_finite()
                    || !to.is_finite()
                    || from >= to
                    || from.abs() >= 1e100
                    || to.abs() >= 1e100
                {
                    return Err("начало диапазона должно быть меньше конца".into());
                }
                let parts: Vec<_> = expression
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                if parts.is_empty() || parts.len() > 4 {
                    return Err("введите от одной до четырёх функций".into());
                }
                let mut curves = Vec::new();
                for part in parts {
                    let xml = FormulaSource::default()
                        .replacement("", &format!("plot_fn(x) := {part}"))?;
                    let doc = Document::parse(&xml).map_err(|error| error.to_string())?;
                    let mut children = elements(doc.root_element());
                    let target =
                        parse::target(children.next().ok_or("не найдена функция для графика")?)?;
                    let body = children.next().ok_or("пустая функция для графика")?;
                    let mut deps = Vec::new();
                    let function = Compiler {
                        names: &names,
                        deps: &mut deps,
                        default_origin: self.default_origin,
                    }
                    .definition(target, body)?;
                    let Datum::Function(function) = function.eval(&self.cache)? else {
                        return Err("не удалось подготовить функцию".into());
                    };
                    let mut points = Vec::new();
                    for step in 0..=320 {
                        let x = from + (to - from) * (f64::from(step) / 320.);
                        let y = function
                            .sample(&self.cache, Value::scalar(x)?)
                            .ok()
                            .filter(|y| y.imaginary == 0. && y.number.abs() < 1e100);
                        points.push(y.map(|y| [x, y.number]));
                    }
                    if points.iter().all(Option::is_none) {
                        return Err(format!(
                            "У {part} нет действительных значений на этом интервале"
                        ));
                    }
                    curves.push((part.into(), points));
                }
                Ok(curves)
            }
            ChartSpec::Parametric {
                x_expr,
                y_expr,
                from,
                to,
            } => {
                if !from.is_finite()
                    || !to.is_finite()
                    || from >= to
                    || from.abs() >= 1e100
                    || to.abs() >= 1e100
                {
                    return Err("начало диапазона параметра должно быть меньше конца".into());
                }
                let x_parts: Vec<_> = x_expr
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                let y_parts: Vec<_> = y_expr
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                if x_parts.is_empty()
                    || y_parts.is_empty()
                    || x_parts.len() != y_parts.len()
                    || x_parts.len() > 4
                {
                    return Err("укажите от одной до четырёх пар выражений x(t) и y(t)".into());
                }
                let compile_param =
                    |expr: &str, param: &str| -> Calc<std::sync::Arc<crate::eval::Function>> {
                        let xml = FormulaSource::default()
                            .replacement("", &format!("plot_p({param}) := {expr}"))?;
                        let doc = Document::parse(&xml).map_err(|error| error.to_string())?;
                        let mut children = elements(doc.root_element());
                        let target = parse::target(
                            children.next().ok_or("не найдена функция для графика")?,
                        )?;
                        let body = children.next().ok_or("пустая функция для графика")?;
                        let mut deps = Vec::new();
                        let function = Compiler {
                            names: &names,
                            deps: &mut deps,
                            default_origin: self.default_origin,
                        }
                        .definition(target, body)?;
                        match function.eval(&self.cache)? {
                            Datum::Function(f) => Ok(f),
                            _ => Err("не удалось подготовить параметрическую функцию".into()),
                        }
                    };
                let mut curves = Vec::with_capacity(x_parts.len());
                for (xp, yp) in x_parts.into_iter().zip(y_parts) {
                    let param = if xp.contains('θ') || yp.contains('θ') {
                        "θ"
                    } else {
                        "t"
                    };
                    let fx = compile_param(xp, param)?;
                    let fy = compile_param(yp, param)?;
                    let mut points = Vec::with_capacity(321);
                    for step in 0..=320 {
                        let t = from + (to - from) * (f64::from(step) / 320.);
                        let tv = Value::scalar(t)?;
                        let x_val = fx
                            .sample(&self.cache, tv)
                            .ok()
                            .filter(|v| v.imaginary == 0. && v.number.abs() < 1e100);
                        let y_val = fy
                            .sample(&self.cache, tv)
                            .ok()
                            .filter(|v| v.imaginary == 0. && v.number.abs() < 1e100);
                        points.push(match (x_val, y_val) {
                            (Some(xv), Some(yv)) => Some([xv.number, yv.number]),
                            _ => None,
                        });
                    }
                    if points.iter().all(Option::is_none) {
                        return Err(format!(
                            "У кривой ({xp}; {yp}) нет действительных значений на этом интервале"
                        ));
                    }
                    curves.push((format!("({xp}; {yp})"), points));
                }
                Ok(curves)
            }
            ChartSpec::Polar {
                expression,
                from,
                to,
            } => {
                if !from.is_finite()
                    || !to.is_finite()
                    || from >= to
                    || from.abs() >= 1e100
                    || to.abs() >= 1e100
                {
                    return Err("начало диапазона угла должно быть меньше конца".into());
                }
                let parts: Vec<_> = expression
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                if parts.is_empty() || parts.len() > 4 {
                    return Err("введите от одной до четырёх полярных функций".into());
                }
                let mut curves = Vec::with_capacity(parts.len());
                for part in parts {
                    let param = if part.contains('θ') { "θ" } else { "t" };
                    let xml = FormulaSource::default()
                        .replacement("", &format!("plot_r({param}) := {part}"))?;
                    let doc = Document::parse(&xml).map_err(|error| error.to_string())?;
                    let mut children = elements(doc.root_element());
                    let target =
                        parse::target(children.next().ok_or("не найдена функция для графика")?)?;
                    let body = children.next().ok_or("пустая функция для графика")?;
                    let mut deps = Vec::new();
                    let function = Compiler {
                        names: &names,
                        deps: &mut deps,
                        default_origin: self.default_origin,
                    }
                    .definition(target, body)?;
                    let Datum::Function(function) = function.eval(&self.cache)? else {
                        return Err("не удалось подготовить полярную функцию".into());
                    };
                    let mut points = Vec::with_capacity(321);
                    for step in 0..=320 {
                        let angle = from + (to - from) * (f64::from(step) / 320.);
                        let r = function
                            .sample(&self.cache, Value::scalar(angle)?)
                            .ok()
                            .filter(|v| v.imaginary == 0. && v.number.abs() < 1e100);
                        points.push(r.map(|rv| [rv.number * angle.cos(), rv.number * angle.sin()]));
                    }
                    if points.iter().all(Option::is_none) {
                        return Err(format!(
                            "У r = {part} нет действительных значений на этом интервале"
                        ));
                    }
                    curves.push((format!("r = {part}"), points));
                }
                Ok(curves)
            }
        }
    }

    #[cfg(feature = "mcp")]
    pub fn surface_grid(
        &self,
        before: usize,
        spec: &crate::editing::Plot3dSpec,
    ) -> Calc<SurfaceGrid> {
        use crate::editing::{FormulaSource, Plot3dSource};
        let chart_rank = *self
            .eval_rank
            .get(before)
            .ok_or("неверная позиция 3D-графика")?;
        let mut names = HashMap::new();
        for &idx in &self.eval_order[..chart_rank] {
            let r = &self.regions[idx];
            if r.defines_name {
                names.insert(r.label.clone(), idx);
            }
        }
        match &spec.source {
            Plot3dSource::Matrix { name } => {
                let trimmed = name.trim();
                let index = *names
                    .get(trimmed)
                    .ok_or_else(|| format!("Определите матрицу {trimmed} выше графика"))?;
                let datum = self.cache[index]
                    .as_ref()
                    .ok_or("нет вычисленной матрицы для 3D-графика")?
                    .as_ref()
                    .map_err(Clone::clone)?;
                let (rows, cols, nums) = datum.as_surface_matrix()?;
                let mut z_min = f64::INFINITY;
                let mut z_max = f64::NEG_INFINITY;
                let mut values = Vec::with_capacity(nums.len());
                for v in nums {
                    if v.is_finite() && v.abs() < 1e100 {
                        z_min = z_min.min(v);
                        z_max = z_max.max(v);
                        values.push(Some(v));
                    } else {
                        values.push(None);
                    }
                }
                if !z_min.is_finite() {
                    return Err("матрица не содержит конечных значений".into());
                }
                Ok(SurfaceGrid {
                    rows,
                    cols,
                    x_from: 0.0,
                    x_to: (cols - 1) as f64,
                    y_from: 0.0,
                    y_to: (rows - 1) as f64,
                    z_min,
                    z_max,
                    values,
                })
            }
            Plot3dSource::Function {
                expression,
                x_from,
                x_to,
                y_from,
                y_to,
            } => {
                if !x_from.is_finite()
                    || !x_to.is_finite()
                    || !y_from.is_finite()
                    || !y_to.is_finite()
                    || x_from >= x_to
                    || y_from >= y_to
                    || x_from.abs() >= 1e100
                    || x_to.abs() >= 1e100
                    || y_from.abs() >= 1e100
                    || y_to.abs() >= 1e100
                {
                    return Err("начала диапазонов X и Y должны быть меньше концов".into());
                }
                let expr = expression.trim();
                if expr.is_empty() {
                    return Err("введите выражение z(x, y)".into());
                }
                let xml = FormulaSource::default()
                    .replacement("", &format!("plot3d_fn(x, y) := {expr}"))?;
                let doc = Document::parse(&xml).map_err(|error| error.to_string())?;
                let mut children = elements(doc.root_element());
                let target = parse::target(children.next().ok_or("не найдена функция z(x, y)")?)?;
                let body = children.next().ok_or("пустая функция z(x, y)")?;
                let mut deps = Vec::new();
                let function = Compiler {
                    names: &names,
                    deps: &mut deps,
                    default_origin: self.default_origin,
                }
                .definition(target, body)?;
                let Datum::Function(function) = function.eval(&self.cache)? else {
                    return Err("не удалось подготовить функцию z(x, y)".into());
                };
                let n = spec.grid.clamp(6, 40);
                let mut values = Vec::with_capacity(n * n);
                let mut z_min = f64::INFINITY;
                let mut z_max = f64::NEG_INFINITY;
                for r in 0..n {
                    let y = y_from + (y_to - y_from) * (r as f64 / (n - 1) as f64);
                    let yv = Value::scalar(y)?;
                    for c in 0..n {
                        let x = x_from + (x_to - x_from) * (c as f64 / (n - 1) as f64);
                        let z = function
                            .sample2(&self.cache, Value::scalar(x)?, yv)
                            .ok()
                            .filter(|v| {
                                v.imaginary == 0. && v.number.is_finite() && v.number.abs() < 1e100
                            })
                            .map(|v| v.number);
                        if let Some(zv) = z {
                            z_min = z_min.min(zv);
                            z_max = z_max.max(zv);
                        }
                        values.push(z);
                    }
                }
                if !z_min.is_finite() {
                    return Err(format!(
                        "У {expr} нет действительных значений на заданной области"
                    ));
                }
                Ok(SurfaceGrid {
                    rows: n,
                    cols: n,
                    x_from: *x_from,
                    x_to: *x_to,
                    y_from: *y_from,
                    y_to: *y_to,
                    z_min,
                    z_max,
                    values,
                })
            }
        }
    }

    pub fn first_difference(&self, other: &Self) -> Option<usize> {
        self.cache
            .iter()
            .zip(&other.cache)
            .position(|(left, right)| left != right)
    }

    pub fn details(&self, index: usize) -> Calc<String> {
        self.details_with_precision(index, None)
    }

    pub fn details_with_precision(&self, index: usize, precision: Option<usize>) -> Calc<String> {
        let value = self
            .cache
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("нет численного результата")?
            .as_ref()
            .map_err(Clone::clone)?;
        let digits = precision.unwrap_or(12);
        match value {
            Datum::Matrix(_) => Ok(value.details_precision(digits)),
            _ => self.display_with_precision(index, precision),
        }
    }

    pub fn display_unit_label(&self, index: usize) -> Option<&str> {
        self.regions
            .get(index)?
            .display_unit
            .as_ref()
            .map(|(label, _)| label.as_str())
    }

    pub fn export_csv(
        &self,
        index: usize,
        delimiter: char,
        precision: Option<usize>,
    ) -> Calc<String> {
        let value = self
            .cache
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("нет численного результата для экспорта")?
            .as_ref()
            .map_err(Clone::clone)?;
        value.to_csv(delimiter, precision.unwrap_or(12))
    }

    #[cfg(feature = "mcp")]
    pub fn region_real_matrix(&self, index: usize) -> Option<(usize, usize, Vec<f64>)> {
        self.cache
            .get(index)?
            .as_ref()?
            .as_ref()
            .ok()?
            .as_real_matrix()
    }

    pub fn plot_series(&self, token: &str) -> Calc<(&str, Vec<Value>)> {
        let index = match token.trim_start_matches('#').parse::<usize>() {
            Ok(index) => index
                .checked_sub(1)
                .ok_or("номера регионов начинаются с 1")?,
            Err(_) => self
                .regions
                .iter()
                .rposition(|region| region.label == token)
                .ok_or_else(|| format!("неизвестное имя региона {token}"))?,
        };
        let value = self
            .cache
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("нет численного результата для графика")?
            .as_ref()
            .map_err(Clone::clone)?;
        Ok((&self.regions[index].label, value.vector_values()?))
    }

    pub fn verify_snapshots_report(&self) -> SnapshotReport {
        let mut matched = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut mismatches = Vec::new();
        for (index, region) in self.regions.iter().enumerate() {
            if region.imported {
                continue;
            }
            let Some(snapshot) = &region.snapshot else {
                continue;
            };
            let Ok(expected) = snapshot else {
                skipped += 1;
                continue;
            };
            let actual = self.cache[index]
                .as_ref()
                .and_then(|result| result.as_ref().ok());
            match actual {
                Some(actual) if actual.near(expected) => matched += 1,
                Some(actual) => {
                    failed += 1;
                    mismatches.push(format!(
                        "Расхождение в #{} [{}]: {actual} вместо {expected}",
                        index + 1,
                        region.source_id
                    ));
                }
                None => {
                    failed += 1;
                    mismatches.push(format!(
                        "Нет вычисленного значения для сверки в #{} [{}]",
                        index + 1,
                        region.source_id
                    ));
                }
            }
        }
        SnapshotReport {
            ok: matched > 0 && failed == 0 && skipped == 0,
            matched,
            failed,
            skipped,
            mismatches,
        }
    }

    pub fn verify_snapshots(&self) -> bool {
        let report = self.verify_snapshots_report();
        for line in &report.mismatches {
            println!("{line}");
        }
        println!(
            "Сохранённые результаты XMCD. Совпало: {}. Расхождений или пропусков расчёта: {}. Неподдерживаемых эталонов: {}.",
            report.matched, report.failed, report.skipped
        );
        report.ok
    }

    pub fn display(&self, index: usize) -> Calc<String> {
        self.display_with_precision(index, None)
    }

    pub fn display_with_precision(&self, index: usize, precision: Option<usize>) -> Calc<String> {
        let value = self.cache[index]
            .as_ref()
            .ok_or("нет численного результата")?
            .as_ref()
            .map_err(Clone::clone)?;
        let digits = precision.unwrap_or(12);
        if let Some((label, unit)) = &self.regions[index].display_unit {
            let scalar = value.scalar()?;
            if matches!(label.as_str(), "°C" | "degC") {
                let c = crate::engineering::kelvin_to_celsius(scalar)?;
                return Ok(format!("{} {label}", c.format_precision(digits)));
            }
            if matches!(label.as_str(), "°F" | "degF") {
                let f = crate::engineering::kelvin_to_fahrenheit(scalar)?;
                return Ok(format!("{} {label}", f.format_precision(digits)));
            }
            let number = scalar.in_units(unit.eval(&self.cache)?.scalar()?)?;
            return Ok(format!("{} {label}", number.format_precision(digits)));
        }
        Ok(value.format_precision(digits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_function_range_and_array_follow_input_changes() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <settings><calculation><calculationBehavior array-origin="1"/></calculation></settings>
              <regions>
                <region top="1"><math><m:define><m:id>a</m:id><m:real>2</m:real></m:define></math></region>
                <region top="2"><math><m:define>
                  <m:function><m:id>f</m:id><m:boundVars><m:id>x</m:id><m:id>y</m:id></m:boundVars></m:function>
                  <m:apply><m:plus/><m:apply><m:mult/><m:id>a</m:id><m:id>x</m:id></m:apply><m:id>y</m:id></m:apply>
                </m:define></math></region>
                <region top="3"><math><m:define><m:id>k</m:id>
                  <m:range><m:sequence><m:real>1</m:real><m:real>3</m:real></m:sequence><m:real>5</m:real></m:range>
                </m:define></math></region>
                <region top="4"><math><m:define>
                  <m:apply><m:indexer/><m:id>z</m:id><m:id>k</m:id></m:apply>
                  <m:apply><m:id>f</m:id><m:sequence><m:id>k</m:id><m:imag>1</m:imag></m:sequence></m:apply>
                </m:define></math></region>
                <region top="5"><math><m:eval>
                  <m:apply><m:indexer/><m:id>z</m:id><m:real>3</m:real></m:apply>
                  <m:result><m:complex><m:real>6</m:real><m:imag>1</m:imag></m:complex></m:result>
                </m:eval></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        assert_eq!(sheet.full(), 5);
        assert_eq!(sheet.display(4).unwrap(), "6 + 1i");
        assert!(sheet.verify_snapshots());
        sheet.set(0, Some(4.), None).unwrap();
        assert_eq!(sheet.incremental(), 4);
        assert_eq!(sheet.display(4).unwrap(), "12 + 1i");
        assert_eq!(sheet.details(3).unwrap().lines().count(), 5);
        assert!(!sheet.verify_snapshots());
    }

    #[test]
    fn recalculates_only_dependent_regions_and_matches_full_calculation() {
        let mut incremental = Sheet::import(crate::DEMO).unwrap();
        let mut full = Sheet::import(crate::DEMO).unwrap();
        assert_eq!(incremental.full(), 7);
        assert_eq!(full.full(), 7);
        for pressure in [60000., 32000., 13000.] {
            assert!(incremental.set(1, Some(pressure), Some("mm")).is_err());
            incremental.set(1, Some(pressure), Some("N")).unwrap();
            full.set(1, Some(pressure), Some("N")).unwrap();
            assert_eq!(incremental.incremental(), 3);
            assert_eq!(full.full(), 7);
            assert_eq!(incremental.cache, full.cache);
            let result = incremental.display(5).unwrap();
            let (number, unit) = result.split_once(' ').unwrap();
            assert!((number.parse::<f64>().unwrap() - pressure / 40000.).abs() < 1e-12);
            assert_eq!(unit, "MPa");
        }
        incremental.set(1, None, None).unwrap();
        assert_eq!(incremental.incremental(), 3);
        assert_eq!(incremental.incremental(), 0);
        let result = incremental.cache[1].as_ref().unwrap().as_ref().unwrap();
        assert_eq!(result.scalar().unwrap().number, 40000.);
    }

    #[test]
    fn sorts_aligned_regions_and_keeps_prior_bindings() {
        let mut sheet = Sheet::import(r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <region align-y="1" left="5"><math><m:eval><m:id>x</m:id></m:eval></math></region>
                <region align-y="2" left="2"><math><m:define><m:id>x</m:id><m:real>8</m:real></m:define></math></region>
                <region align-y="1" left="0"><math><m:define><m:id>x</m:id><m:real>2</m:real></m:define></math></region>
                <region align-y="2" left="9"><math><m:eval><m:id>x</m:id></m:eval></math></region>
              </regions>
            </worksheet>"#).unwrap();
        assert_eq!(sheet.full(), 4);
        assert_eq!(sheet.display(1).unwrap(), "2");
        assert_eq!(sheet.display(3).unwrap(), "8");
        sheet.set(0, Some(7.), None).unwrap();
        assert_eq!(sheet.incremental(), 2);
        assert_eq!(sheet.display(1).unwrap(), "7");
        assert_eq!(sheet.display(3).unwrap(), "8");
    }

    #[test]
    fn propagates_an_unsupported_definition_to_its_consumers() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <region top="1"><math><m:provenance><m:define><m:id>x</m:id><m:matrix /></m:define></m:provenance></math></region>
                <region top="2"><math><m:eval><m:id>x</m:id></m:eval></math></region>
                <region top="3"><math><m:eval><m:real>42</m:real><m:result><m:real>7</m:real></m:result></m:eval></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        sheet.full();
        assert!(sheet.display(0).unwrap_err().contains("размер массива"));
        assert!(sheet.display(1).unwrap_err().ends_with("#1"));
        assert_eq!(sheet.display(2).unwrap(), "42");
        assert!(Sheet::import(&xml.replace(
            "<regions>",
            "<regions><region top=\"4\"><component/></region>"
        ))
        .is_err());
    }

    #[test]
    fn reads_array_origin_from_builtin_variables() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <settings>
                <calculation>
                  <builtInVariables array-origin="1"/>
                  <calculationBehavior automatic-recalculation="true"/>
                </calculation>
              </settings>
              <regions>
                <region top="1"><math><m:define><m:id>v</m:id><m:matrix rows="2" cols="1"><m:real>10</m:real><m:real>20</m:real></m:matrix></m:define></math></region>
                <region top="2"><math><m:eval><m:apply><m:indexer/><m:id>v</m:id><m:real>2</m:real></m:apply></m:eval></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        sheet.full();
        assert_eq!(sheet.display(1).unwrap(), "20");
    }

    #[test]
    fn global_define_applies_above_and_can_be_overridden_locally() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <region region-id="1" top="10"><math><m:eval><m:apply><m:mult/><m:id>g</m:id><m:real>2</m:real></m:apply></m:eval></math></region>
                <region region-id="2" top="30"><math><m:define><m:id>g</m:id><m:real>10</m:real></m:define></math></region>
                <region region-id="3" top="50"><math><m:eval><m:apply><m:mult/><m:id>g</m:id><m:real>2</m:real></m:apply></m:eval></math></region>
                <region region-id="4" top="90"><math><m:globalDefine><m:id>g</m:id><m:real>9.8</m:real></m:globalDefine></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        sheet.full();
        // Region 0 (above local define) uses global g = 9.8
        assert_eq!(sheet.display(0).unwrap(), "19.6");
        // Region 2 (below local define) uses local g = 10
        assert_eq!(sheet.display(2).unwrap(), "20");
        // Editing global input (region 3) updates region 0 incrementally without affecting region 2
        sheet.set(3, Some(5.), None).unwrap();
        assert_eq!(sheet.incremental(), 2);
        assert_eq!(sheet.display(0).unwrap(), "10");
        assert_eq!(sheet.display(2).unwrap(), "20");
    }

    #[test]
    fn references_resolve_relative_paths_overrides_cycles_missing_and_updates() {
        let dir = std::env::temp_dir().join(format!("list-rs-ref-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let lib_path = dir.join("lib.xmcd");
        let main_path = dir.join("main.xmcd");
        let cyc_a = dir.join("cyc_a.xmcd");
        let cyc_b = dir.join("cyc_b.xmcd");

        let wrap = |body: &str| {
            format!(
                r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet30" xmlns:m="http://schemas.mathsoft.com/math30"><regions>{body}</regions></worksheet>"#
            )
        };

        // 1. Relative reference + override + live update after editing source
        std::fs::write(
            &lib_path,
            wrap(
                r#"<region region-id="1" top="10"><math><m:define><m:id>k</m:id><m:real>3</m:real></m:define></math></region>
                   <region region-id="2" top="30"><math><m:define><m:function><m:id>scale</m:id><m:boundVars><m:id>x</m:id></m:boundVars></m:function><m:apply><m:mult/><m:id>k</m:id><m:id>x</m:id></m:apply></m:define></math></region>"#,
            ),
        )
        .unwrap();
        std::fs::write(
            &main_path,
            wrap(
                r#"<region region-id="1" top="10"><reference href="lib.xmcd"/></region>
                   <region region-id="2" top="30"><math><m:eval><m:apply><m:id>scale</m:id><m:real>4</m:real></m:apply></m:eval></math></region>
                   <region region-id="3" top="50"><math><m:define><m:id>k</m:id><m:real>100</m:real></m:define></math></region>
                   <region region-id="4" top="70"><math><m:eval><m:id>k</m:id></m:eval></math></region>"#,
            ),
        )
        .unwrap();

        let mut sheet = Sheet::import_from_path(&main_path).unwrap();
        sheet.full();
        assert_eq!(sheet.display(1).unwrap(), "12");
        assert_eq!(sheet.display(3).unwrap(), "100");

        // Modify lib.xmcd source: k := 5 -> scale(4) becomes 20
        std::fs::write(
            &lib_path,
            wrap(
                r#"<region region-id="1" top="10"><math><m:define><m:id>k</m:id><m:real>5</m:real></m:define></math></region>
                   <region region-id="2" top="30"><math><m:define><m:function><m:id>scale</m:id><m:boundVars><m:id>x</m:id></m:boundVars></m:function><m:apply><m:mult/><m:id>k</m:id><m:id>x</m:id></m:apply></m:define></math></region>"#,
            ),
        )
        .unwrap();
        let mut reloaded = Sheet::import_from_path(&main_path).unwrap();
        reloaded.full();
        assert_eq!(reloaded.display(1).unwrap(), "20");

        // 2. Missing referenced file fails with clear error
        let missing_xml =
            wrap(r#"<region region-id="1" top="10"><reference href="nope.xmcd"/></region>"#);
        let err = Sheet::import_with_base(&missing_xml, Some(&dir))
            .err()
            .unwrap();
        assert!(err.contains("не найден"));

        // 3. Reference cycle (cyc_a -> cyc_b -> cyc_a) fails with cycle error
        std::fs::write(
            &cyc_a,
            wrap(r#"<region region-id="1" top="10"><reference href="cyc_b.xmcd"/></region>"#),
        )
        .unwrap();
        std::fs::write(
            &cyc_b,
            wrap(r#"<region region-id="1" top="10"><reference href="cyc_a.xmcd"/></region>"#),
        )
        .unwrap();
        let cyc_err = Sheet::import_from_path(&cyc_a).err().unwrap();
        assert!(cyc_err.contains("цикл"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn given_find_and_given_minimize_solve_blocks_recalculate() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <region region-id="1" top="10"><math><m:define><m:id>R</m:id><m:real>5</m:real></m:define></math></region>
                <region region-id="2" top="30"><math><m:define><m:id>x</m:id><m:real>3</m:real></m:define></math></region>
                <region region-id="3" top="50"><math><m:define><m:id>y</m:id><m:real>2</m:real></m:define></math></region>
                <region region-id="4" top="70"><math><m:id>Given</m:id></math></region>
                <region region-id="5" top="90"><math><m:apply><m:equal/><m:apply><m:plus/><m:apply><m:pow/><m:id>x</m:id><m:real>2</m:real></m:apply><m:apply><m:pow/><m:id>y</m:id><m:real>2</m:real></m:apply></m:apply><m:apply><m:pow/><m:id>R</m:id><m:real>2</m:real></m:apply></m:apply></math></region>
                <region region-id="6" top="110"><math><m:apply><m:equal/><m:apply><m:minus/><m:id>x</m:id><m:id>y</m:id></m:apply><m:real>1</m:real></m:apply></math></region>
                <region region-id="7" top="130"><math><m:define><m:id>sol</m:id><m:apply><m:id>Find</m:id><m:sequence><m:id>x</m:id><m:id>y</m:id></m:sequence></m:apply></m:define></math></region>
                <region region-id="8" top="150"><math><m:eval><m:apply><m:indexer/><m:id>sol</m:id><m:real>0</m:real></m:apply></m:eval></math></region>
                <region region-id="9" top="170"><math><m:eval><m:apply><m:indexer/><m:id>sol</m:id><m:real>1</m:real></m:apply></m:eval></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        sheet.full();
        let x_val: f64 = sheet.display(7).unwrap().parse().unwrap();
        let y_val: f64 = sheet.display(8).unwrap().parse().unwrap();
        assert!((x_val - 4.0).abs() < 1e-6);
        assert!((y_val - 3.0).abs() < 1e-6);

        // Constrained optimization: minimize x^2 + y^2 subject to x + y >= 4 -> (2, 2)
        let opt_xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <region region-id="1" top="10"><math><m:define><m:id>x</m:id><m:real>0</m:real></m:define></math></region>
                <region region-id="2" top="30"><math><m:define><m:id>y</m:id><m:real>0</m:real></m:define></math></region>
                <region region-id="3" top="50"><math><m:define><m:function><m:id>f</m:id><m:boundVars><m:id>a</m:id><m:id>b</m:id></m:boundVars></m:function><m:apply><m:plus/><m:apply><m:pow/><m:id>a</m:id><m:real>2</m:real></m:apply><m:apply><m:pow/><m:id>b</m:id><m:real>2</m:real></m:apply></m:apply></m:define></math></region>
                <region region-id="4" top="70"><math><m:id>Given</m:id></math></region>
                <region region-id="5" top="90"><math><m:apply><m:greaterOrEqual/><m:apply><m:plus/><m:id>x</m:id><m:id>y</m:id></m:apply><m:real>4</m:real></m:apply></math></region>
                <region region-id="6" top="110"><math><m:define><m:id>opt</m:id><m:apply><m:id>Minimize</m:id><m:sequence><m:id>f</m:id><m:id>x</m:id><m:id>y</m:id></m:sequence></m:apply></m:define></math></region>
                <region region-id="7" top="130"><math><m:eval><m:apply><m:indexer/><m:id>opt</m:id><m:real>0</m:real></m:apply></m:eval></math></region>
              </regions>
            </worksheet>"#;
        let mut opt_sheet = Sheet::import(opt_xml).unwrap();
        opt_sheet.full();
        let ox: f64 = opt_sheet.display(6).unwrap().parse().unwrap();
        assert!((ox - 2.0).abs() < 1e-3);
    }

    #[test]
    fn given_minimize_rejects_infeasible_constraints() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <region region-id="1" top="10"><math><m:define><m:id>x</m:id><m:real>0</m:real></m:define></math></region>
                <region region-id="2" top="30"><math><m:define><m:function><m:id>f</m:id><m:boundVars><m:id>a</m:id></m:boundVars></m:function><m:apply><m:pow/><m:id>a</m:id><m:real>2</m:real></m:apply></m:define></math></region>
                <region region-id="3" top="50"><math><m:id>Given</m:id></math></region>
                <region region-id="4" top="70"><math><m:apply><m:greaterOrEqual/><m:id>x</m:id><m:real>2</m:real></m:apply></math></region>
                <region region-id="5" top="90"><math><m:apply><m:lessOrEqual/><m:id>x</m:id><m:real>0</m:real></m:apply></math></region>
                <region region-id="6" top="110"><math><m:define><m:id>opt</m:id><m:apply><m:id>Minimize</m:id><m:sequence><m:id>f</m:id><m:id>x</m:id></m:sequence></m:apply></m:define></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        sheet.full();
        let error = sheet.display(5).unwrap_err();
        assert!(error.contains("удовлетворяющую ограничениям"), "{error}");
    }

    #[test]
    fn imports_collapsible_area_regions_and_affine_temperature_units() {
        let xml = r#"
            <worksheet xmlns="http://schemas.mathsoft.com/worksheet30"
                       xmlns:m="http://schemas.mathsoft.com/math30">
              <regions>
                <area>
                  <regions>
                    <region region-id="1" top="10"><math><m:define><m:id>T</m:id><m:apply><m:id>C</m:id><m:real>25</m:real></m:apply></m:define></math></region>
                  </regions>
                </area>
                <region region-id="2" top="50"><math><m:eval><m:id>T</m:id><m:unitOverride><m:id>°C</m:id></m:unitOverride></m:eval></math></region>
                <region region-id="3" top="90"><math><m:eval><m:id>T</m:id><m:unitOverride><m:id>°F</m:id></m:unitOverride></m:eval></math></region>
              </regions>
            </worksheet>"#;
        let mut sheet = Sheet::import(xml).unwrap();
        sheet.full();
        assert_eq!(sheet.display(1).unwrap(), "25 °C");
        assert_eq!(sheet.display(2).unwrap(), "77 °F");
    }
}
