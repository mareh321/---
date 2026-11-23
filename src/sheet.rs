use crate::eval::{Datum, Expr};
use crate::parse::{self, elements, identifier, kind, named, Compiler, Xml, ML};
#[cfg(feature = "desktop")]
pub use crate::presentation::RegionVisual;
use crate::value::{Calc, Value};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::HashMap;

const WS: &str = "http://schemas.mathsoft.com/worksheet30";

pub struct Region {
    pub source_id: String,
    pub label: String,
    pub text: Option<String>,
    #[cfg(feature = "desktop")]
    pub visual: RegionVisual,
    #[cfg(feature = "desktop")]
    defines_name: bool,
    input_unit: Option<Value>,
    formula: Option<Calc<Expr>>,
    snapshot: Option<Calc<Datum>>,
    display_unit: Option<(String, Expr)>,
}

pub struct Sheet {
    pub regions: Vec<Region>,
    cache: Vec<Option<Calc<Datum>>>,
    default_origin: Value,
    overrides: Vec<Option<Value>>,
    dependents: Vec<Vec<usize>>,
    dirty: Vec<usize>,
    queued: Vec<bool>,
}

fn unit_label(node: Xml<'_, '_>) -> String {
    // Use the XML text to make the most common, single-symbol unit override readable.
    if let Ok(name) = identifier(node) {
        return name;
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

impl Sheet {
    pub fn import(xml: &str) -> Calc<Self> {
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
            named(*node, ML, "globalDefine")
                || (node.is_element()
                    && node.tag_name().namespace() == Some(WS)
                    && matches!(
                        kind(*node),
                        "reference" | "include" | "area" | "component" | "solveBlock"
                    ))
        });
        if let Some(node) = nested_effect {
            return Err(format!(
                "лист содержит <{}>, который может менять порядок или значения расчётов",
                kind(node)
            ));
        }

        let mut ordered = Vec::new();
        for region in elements(container) {
            if !named(region, WS, "region") {
                return Err(format!("неподдерживаемый элемент листа <{}>", kind(region)));
            }
            ordered.push((
                position(region, "align-y", "top")?,
                position(region, "left", "left")?,
                region,
            ));
        }
        ordered.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let default_origin = elements(root)
            .find(|&node| named(node, WS, "settings"))
            .and_then(|settings| {
                settings
                    .descendants()
                    .find(|&node| named(node, WS, "calculationBehavior"))
            })
            .and_then(|settings| settings.attribute("array-origin"))
            .unwrap_or("0")
            .parse::<i32>()
            .map_err(|_| "некорректное значение array-origin")?;
        let size = ordered.len();
        let mut sheet = Self {
            regions: Vec::with_capacity(size),
            cache: vec![None; size],
            default_origin: Value::scalar(f64::from(default_origin))?,
            overrides: vec![None; size],
            dependents: vec![Vec::new(); size],
            dirty: Vec::new(),
            queued: vec![false; size],
        };
        let mut names = HashMap::new();
        for (_, _, node) in ordered {
            sheet.import_region(node, &mut names)?;
        }
        Ok(sheet)
    }

    fn import_region(&mut self, node: Xml<'_, '_>, names: &mut HashMap<String, usize>) -> Calc<()> {
        let index = self.regions.len();
        let mut region = Region {
            source_id: node.attribute("region-id").unwrap_or("?").into(),
            label: String::new(),
            text: None,
            #[cfg(feature = "desktop")]
            visual: RegionVisual::new(node),
            #[cfg(feature = "desktop")]
            defines_name: false,
            input_unit: None,
            formula: None,
            snapshot: None,
            display_unit: None,
        };
        let mut deps = Vec::new();
        let content = elements(node).find(|child| !named(*child, WS, "rendering"));
        match content.map(kind) {
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
                #[cfg(feature = "desktop")]
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
                match (kind(statement), children.as_slice()) {
                    ("define", [left, expr, ..]) => {
                        #[cfg(feature = "desktop")]
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
                        });
                        names.insert(name, index);
                    }
                    ("eval", [expr, ..]) => {
                        region.label = identifier(*expr).unwrap_or_else(|_| "Результат".into());
                        region.snapshot = parse::snapshot(statement);
                        region.formula = Some(if disabled {
                            Err("вычисление отключено в исходном листе".into())
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
                        });
                    }
                    ("symEval", _) => {
                        region.label = "Символьное вычисление".into();
                        region.formula =
                            Some(Err("символьные вычисления не поддерживаются".into()));
                    }
                    (
                        "apply" | "parens" | "id" | "real" | "imag" | "complex" | "matrix"
                        | "range",
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
                #[cfg(feature = "desktop")]
                region
                    .visual
                    .set_formula(statement, region.input_unit.is_some());
            }
            Some(tag @ ("graph" | "plot" | "xyPlot" | "image" | "picture")) => {
                region.text = Some(format!("Блок <{tag}> пропущен"));
                #[cfg(feature = "desktop")]
                {
                    region.visual.graphical = true;
                }
            }
            Some("pageBreak") => region.text = Some("Разрыв страницы".into()),
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
        self.regions.push(region);
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
        (0..self.regions.len())
            .filter(|&index| self.evaluate(index))
            .count()
    }

    pub fn incremental(&mut self) -> usize {
        self.dirty.sort_unstable();
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
        self.regions[index].input_unit.is_some()
    }

    #[cfg(feature = "desktop")]
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

    #[cfg(feature = "desktop")]
    pub fn edited(&self, index: usize) -> bool {
        self.overrides.get(index).is_some_and(Option::is_some)
    }

    #[cfg(feature = "desktop")]
    pub fn counts(&self) -> (usize, usize, usize) {
        let inputs = self
            .regions
            .iter()
            .filter(|region| region.input_unit.is_some())
            .count();
        let formulas = self
            .regions
            .iter()
            .filter(|region| region.formula.is_some())
            .count();
        let errors = self
            .cache
            .iter()
            .filter(|result| matches!(result, Some(Err(_))))
            .count();
        (inputs, formulas, errors)
    }

    #[cfg(feature = "desktop")]
    pub fn chart_curves(
        &self,
        before: usize,
        chart: &crate::editing::ChartSpec,
    ) -> Calc<Vec<(String, Vec<Option<[f64; 2]>>)>> {
        use crate::editing::{ChartSpec, FormulaSource};
        let regions = self
            .regions
            .get(..before)
            .ok_or("неверная позиция графика")?;
        let names: HashMap<_, _> = regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.defines_name)
            .map(|(index, r)| (r.label.clone(), index))
            .collect();
        let vector = |name: &str| -> Calc<Vec<Value>> {
            let index = names
                .get(name.trim())
                .ok_or_else(|| format!("Определите {name} выше графика"))?;
            self.cache[*index]
                .as_ref()
                .ok_or("нет значения для графика")?
                .as_ref()
                .map_err(Clone::clone)?
                .vector_values()
        };
        match chart {
            ChartSpec::Vectors { x, y } => {
                let x_values = vector(x)?;
                let y_values = vector(y)?;
                if x_values.len() != y_values.len() || x_values.is_empty() {
                    return Err("массивы X и Y должны иметь одинаковую ненулевую длину".into());
                }
                let valid = |values: &[Value]| {
                    let first = values[0];
                    values
                        .iter()
                        .all(|v| v.imaginary == 0. && v.compatible(first) && v.number.abs() < 1e100)
                };
                if !valid(&x_values) || !valid(&y_values) {
                    return Err("ось требует действительных значений одной размерности".into());
                }
                let points = x_values
                    .into_iter()
                    .zip(y_values)
                    .map(|(a, b)| Some([a.number, b.number]))
                    .collect();
                Ok(vec![(y.clone(), points)])
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
        }
    }

    pub fn first_difference(&self, other: &Self) -> Option<usize> {
        self.cache
            .iter()
            .zip(&other.cache)
            .position(|(left, right)| left != right)
    }

    pub fn details(&self, index: usize) -> Calc<String> {
        let value = self
            .cache
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("нет численного результата")?
            .as_ref()
            .map_err(Clone::clone)?;
        match value {
            Datum::Matrix(_) => Ok(value.details()),
            _ => self.display(index),
        }
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

    pub fn verify_snapshots(&self) -> bool {
        let mut matched = 0;
        let mut failed = 0;
        let mut skipped = 0;
        for (index, region) in self.regions.iter().enumerate() {
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
                    println!(
                        "Расхождение в #{} [{}]: {actual} вместо {expected}",
                        index + 1,
                        region.source_id
                    );
                }
                None => {
                    failed += 1;
                    println!(
                        "Нет вычисленного значения для сверки в #{} [{}]",
                        index + 1,
                        region.source_id
                    );
                }
            }
        }
        println!("Сохранённые результаты XMCD. Совпало: {matched}. Расхождений или пропусков расчёта: {failed}. Неподдерживаемых эталонов: {skipped}.");
        matched > 0 && failed == 0 && skipped == 0
    }

    pub fn display(&self, index: usize) -> Calc<String> {
        let value = self.cache[index]
            .as_ref()
            .ok_or("нет численного результата")?
            .as_ref()
            .map_err(Clone::clone)?;
        if let Some((label, unit)) = &self.regions[index].display_unit {
            let number = value
                .scalar()?
                .in_units(unit.eval(&self.cache)?.scalar()?)?;
            return Ok(format!("{number} {label}"));
        }
        Ok(value.to_string())
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
        assert!(Sheet::import(&xml.replace("m:define", "m:globalDefine")).is_err());
    }
}
