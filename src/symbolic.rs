use crate::parse::{elements, identifier, kind, named, Xml, ML};
use crate::value::{display_number, Calc};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum SymExpr {
    Num(f64),
    Var(String),
    Add(Vec<SymExpr>),
    Mul(Vec<SymExpr>),
    Pow(Box<SymExpr>, Box<SymExpr>),
    Func(String, Vec<SymExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SymResult {
    pub display: String,
    pub expr: Option<SymExpr>,
    pub domain: Vec<String>,
}

impl fmt::Display for SymResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display)?;
        if !self.domain.is_empty() {
            write!(f, "  [{}]", self.domain.join(", "))?;
        }
        Ok(())
    }
}

fn is_zero(v: f64) -> bool {
    v.abs() < 1e-12
}

fn is_one(v: f64) -> bool {
    (v - 1.0).abs() < 1e-12
}

fn clean_num(v: f64) -> f64 {
    if is_zero(v) {
        0.0
    } else if (v - v.round()).abs() < 1e-12 {
        v.round()
    } else {
        v
    }
}

impl SymExpr {
    pub fn num(v: f64) -> Calc<Self> {
        if !v.is_finite() {
            return Err("символьное выражение содержит неконечное число".into());
        }
        Ok(Self::Num(clean_num(v)))
    }

    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }

    pub fn add(a: Self, b: Self) -> Self {
        Self::Add(vec![a, b])
    }

    pub fn sub(a: Self, b: Self) -> Self {
        Self::Add(vec![a, Self::Mul(vec![Self::Num(-1.0), b])])
    }

    pub fn mul(a: Self, b: Self) -> Self {
        Self::Mul(vec![a, b])
    }

    pub fn div(a: Self, b: Self) -> Self {
        Self::Mul(vec![a, Self::Pow(Box::new(b), Box::new(Self::Num(-1.0)))])
    }

    pub fn pow(a: Self, b: Self) -> Self {
        Self::Pow(Box::new(a), Box::new(b))
    }

    pub fn neg(a: Self) -> Self {
        Self::Mul(vec![Self::Num(-1.0), a])
    }

    pub fn contains_var(&self, name: &str) -> bool {
        match self {
            Self::Num(_) => false,
            Self::Var(v) => v == name,
            Self::Add(items) | Self::Mul(items) | Self::Func(_, items) => {
                items.iter().any(|e| e.contains_var(name))
            }
            Self::Pow(a, b) => a.contains_var(name) || b.contains_var(name),
        }
    }

    pub fn has_any_var(&self) -> bool {
        match self {
            Self::Num(_) => false,
            Self::Var(_) => true,
            Self::Add(items) | Self::Mul(items) | Self::Func(_, items) => {
                items.iter().any(Self::has_any_var)
            }
            Self::Pow(a, b) => a.has_any_var() || b.has_any_var(),
        }
    }

    pub fn collect_vars(&self, out: &mut HashSet<String>) {
        match self {
            Self::Num(_) => {}
            Self::Var(v) => {
                out.insert(v.clone());
            }
            Self::Add(items) | Self::Mul(items) | Self::Func(_, items) => {
                for item in items {
                    item.collect_vars(out);
                }
            }
            Self::Pow(a, b) => {
                a.collect_vars(out);
                b.collect_vars(out);
            }
        }
    }

    pub fn collect_domain(&self) -> Vec<String> {
        let mut notes = Vec::new();
        self.gather_domain(&mut notes);
        notes.sort();
        notes.dedup();
        notes
    }

    fn gather_domain(&self, out: &mut Vec<String>) {
        match self {
            Self::Num(_) | Self::Var(_) => {}
            Self::Add(items) | Self::Mul(items) => {
                for item in items {
                    item.gather_domain(out);
                }
            }
            Self::Pow(base, exp) => {
                base.gather_domain(out);
                exp.gather_domain(out);
                if let Self::Num(e) = exp.as_ref() {
                    if *e < 0.0 && base.has_any_var() {
                        out.push(format!("{base} ≠ 0"));
                    } else if e.fract() != 0.0 && *e > 0.0 && base.has_any_var() {
                        out.push(format!("{base} ≥ 0"));
                    }
                }
            }
            Self::Func(name, args) => {
                for arg in args {
                    arg.gather_domain(out);
                }
                if let [arg] = args.as_slice() {
                    if arg.has_any_var() {
                        match name.as_str() {
                            "ln" | "log" => out.push(format!("{arg} > 0")),
                            "sqrt" => out.push(format!("{arg} ≥ 0")),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    pub fn substitute(&self, var: &str, replacement: &Self) -> Self {
        match self {
            Self::Num(n) => Self::Num(*n),
            Self::Var(v) if v == var => replacement.clone(),
            Self::Var(v) => Self::Var(v.clone()),
            Self::Add(items) => Self::Add(
                items
                    .iter()
                    .map(|e| e.substitute(var, replacement))
                    .collect(),
            ),
            Self::Mul(items) => Self::Mul(
                items
                    .iter()
                    .map(|e| e.substitute(var, replacement))
                    .collect(),
            ),
            Self::Pow(a, b) => Self::Pow(
                Box::new(a.substitute(var, replacement)),
                Box::new(b.substitute(var, replacement)),
            ),
            Self::Func(name, args) => Self::Func(
                name.clone(),
                args.iter()
                    .map(|e| e.substitute(var, replacement))
                    .collect(),
            ),
        }
    }

    pub fn substitute_map(&self, env: &HashMap<String, Self>) -> Self {
        if env.is_empty() {
            return self.clone();
        }
        match self {
            Self::Num(n) => Self::Num(*n),
            Self::Var(v) => env.get(v).cloned().unwrap_or_else(|| Self::Var(v.clone())),
            Self::Add(items) => Self::Add(items.iter().map(|e| e.substitute_map(env)).collect()),
            Self::Mul(items) => Self::Mul(items.iter().map(|e| e.substitute_map(env)).collect()),
            Self::Pow(a, b) => Self::Pow(
                Box::new(a.substitute_map(env)),
                Box::new(b.substitute_map(env)),
            ),
            Self::Func(name, args) => Self::Func(
                name.clone(),
                args.iter().map(|e| e.substitute_map(env)).collect(),
            ),
        }
    }

    pub fn simplify(&self) -> Calc<Self> {
        self.simplify_depth(0)
    }

    fn simplify_depth(&self, depth: usize) -> Calc<Self> {
        if depth > 64 {
            return Err("слишком большая глубина символьного упрощения".into());
        }
        match self {
            Self::Num(n) => Self::num(*n),
            Self::Var(v) => Ok(Self::Var(v.clone())),
            Self::Func(name, args) => {
                let mut s_args = Vec::with_capacity(args.len());
                for a in args {
                    s_args.push(a.simplify_depth(depth + 1)?);
                }
                match (name.as_str(), s_args.as_slice()) {
                    ("simplify", [e]) => e.simplify_depth(depth + 1),
                    ("expand", [e]) => e.expand(),
                    ("sdiff", [e, Self::Var(v)]) => e.diff(v),
                    ("subst", [e, Self::Var(v), val]) => {
                        e.substitute(v, val).simplify_depth(depth + 1)
                    }
                    ("sqrt", [Self::Num(n)]) if *n >= 0.0 => {
                        let r = n.sqrt();
                        if (r - r.round()).abs() < 1e-12 {
                            Self::num(r.round())
                        } else {
                            Ok(Self::Func("sqrt".into(), s_args))
                        }
                    }
                    ("sqrt", [Self::Pow(base, exp)]) if matches!(exp.as_ref(), Self::Num(n) if (*n - 2.0).abs() < 1e-12) => {
                        Ok(Self::Func("abs".into(), vec![(**base).clone()]))
                    }
                    ("sin", [Self::Num(n)]) if is_zero(*n) => Self::num(0.0),
                    ("cos", [Self::Num(n)]) if is_zero(*n) => Self::num(1.0),
                    ("tan", [Self::Num(n)]) if is_zero(*n) => Self::num(0.0),
                    ("exp", [Self::Num(n)]) if is_zero(*n) => Self::num(1.0),
                    ("ln", [Self::Num(n)]) if is_one(*n) => Self::num(0.0),
                    ("ln", [Self::Num(n)]) if *n <= 0.0 => {
                        Err("ln определён только для положительных аргументов".into())
                    }
                    ("abs", [Self::Num(n)]) => Self::num(n.abs()),
                    _ => Ok(Self::Func(name.clone(), s_args)),
                }
            }
            Self::Pow(base, exp) => {
                let b = base.simplify_depth(depth + 1)?;
                let e = exp.simplify_depth(depth + 1)?;
                match (&b, &e) {
                    (Self::Num(bv), Self::Num(ev)) => {
                        if is_zero(*bv) && *ev < 0.0 {
                            return Err("деление на ноль".into());
                        }
                        if is_zero(*bv) && is_zero(*ev) {
                            return Self::num(1.0);
                        }
                        if *bv < 0.0 && ev.fract() != 0.0 {
                            return Ok(Self::Pow(Box::new(b), Box::new(e)));
                        }
                        let val = bv.powf(*ev);
                        if val.is_finite()
                            && (ev.fract() == 0.0 || (val - val.round()).abs() < 1e-12)
                        {
                            return Self::num(val);
                        }
                        Ok(Self::Pow(Box::new(b), Box::new(e)))
                    }
                    (_, Self::Num(ev)) if is_zero(*ev) => Self::num(1.0),
                    (_, Self::Num(ev)) if is_one(*ev) => Ok(b),
                    (Self::Num(bv), _) if is_one(*bv) => Self::num(1.0),
                    (Self::Pow(inner_b, inner_e), Self::Num(ev)) => {
                        if let Self::Num(ie) = inner_e.as_ref() {
                            if ie.fract() == 0.0 && ev.fract() == 0.0 {
                                return Self::Pow(
                                    inner_b.clone(),
                                    Box::new(Self::Num(clean_num(ie * ev))),
                                )
                                .simplify_depth(depth + 1);
                            }
                        }
                        Ok(Self::Pow(Box::new(b), Box::new(e)))
                    }
                    _ => Ok(Self::Pow(Box::new(b), Box::new(e))),
                }
            }
            Self::Mul(items) => {
                let mut coeff = 1.0f64;
                let mut factors: Vec<(Self, Self)> = Vec::new();
                for item in items {
                    let s = item.simplify_depth(depth + 1)?;
                    let flat = match s {
                        Self::Mul(sub) => sub,
                        other => vec![other],
                    };
                    for f in flat {
                        match f {
                            Self::Num(n) => {
                                if is_zero(n) {
                                    return Self::num(0.0);
                                }
                                coeff *= n;
                            }
                            Self::Pow(base, exp) => {
                                if let Some((_, existing_exp)) =
                                    factors.iter_mut().find(|(b, _)| b == base.as_ref())
                                {
                                    *existing_exp = Self::add(existing_exp.clone(), (*exp).clone())
                                        .simplify_depth(depth + 1)?;
                                } else {
                                    factors.push((*base, *exp));
                                }
                            }
                            other => {
                                if let Some((_, existing_exp)) =
                                    factors.iter_mut().find(|(b, _)| *b == other)
                                {
                                    *existing_exp = Self::add(existing_exp.clone(), Self::Num(1.0))
                                        .simplify_depth(depth + 1)?;
                                } else {
                                    factors.push((other, Self::Num(1.0)));
                                }
                            }
                        }
                    }
                }
                coeff = clean_num(coeff);
                if is_zero(coeff) {
                    return Self::num(0.0);
                }
                let mut out = Vec::new();
                if !is_one(coeff) {
                    out.push(Self::Num(coeff));
                }
                factors.sort_by(|a, b| a.0.sort_key().cmp(&b.0.sort_key()));
                for (base, exp) in factors {
                    match exp {
                        Self::Num(n) if is_zero(n) => {}
                        Self::Num(n) if is_one(n) => out.push(base),
                        other => out.push(Self::Pow(Box::new(base), Box::new(other))),
                    }
                }
                match out.len() {
                    0 => Self::num(1.0),
                    1 => Ok(out.into_iter().next().unwrap()),
                    _ => Ok(Self::Mul(out)),
                }
            }
            Self::Add(items) => {
                let mut constant = 0.0f64;
                let mut terms: Vec<(f64, Self)> = Vec::new();
                for item in items {
                    let s = item.simplify_depth(depth + 1)?;
                    let flat = match s {
                        Self::Add(sub) => sub,
                        other => vec![other],
                    };
                    for t in flat {
                        if let Self::Num(n) = t {
                            constant += n;
                        } else {
                            let (c, base) = split_coeff(t);
                            if let Some((existing_c, _)) =
                                terms.iter_mut().find(|(_, b)| *b == base)
                            {
                                *existing_c += c;
                            } else {
                                terms.push((c, base));
                            }
                        }
                    }
                }
                terms.retain(|(c, _)| !is_zero(*c));
                terms.sort_by(|a, b| a.1.sort_key().cmp(&b.1.sort_key()));
                let mut out = Vec::new();
                for (c, base) in terms {
                    let c = clean_num(c);
                    if is_one(c) {
                        out.push(base);
                    } else {
                        out.push(Self::mul(Self::Num(c), base).simplify_depth(depth + 1)?);
                    }
                }
                constant = clean_num(constant);
                if !is_zero(constant) {
                    out.push(Self::Num(constant));
                }
                match out.len() {
                    0 => Self::num(0.0),
                    1 => Ok(out.into_iter().next().unwrap()),
                    _ => Ok(Self::Add(out)),
                }
            }
        }
    }

    pub fn expand(&self) -> Calc<Self> {
        self.expand_depth(0)?.simplify()
    }

    fn expand_depth(&self, depth: usize) -> Calc<Self> {
        if depth > 32 {
            return Err("слишком большая глубина раскрытия скобок".into());
        }
        match self {
            Self::Num(_) | Self::Var(_) => Ok(self.clone()),
            Self::Func(name, args) => Ok(Self::Func(
                name.clone(),
                args.iter()
                    .map(|a| a.expand_depth(depth + 1))
                    .collect::<Calc<_>>()?,
            )),
            Self::Add(items) => Ok(Self::Add(
                items
                    .iter()
                    .map(|a| a.expand_depth(depth + 1))
                    .collect::<Calc<_>>()?,
            )),
            Self::Pow(base, exp) => {
                let b = base.expand_depth(depth + 1)?.simplify()?;
                let e = exp.expand_depth(depth + 1)?.simplify()?;
                if let Self::Num(n) = e {
                    if n.fract() == 0.0 && (0.0..=12.0).contains(&n) {
                        let k = n as usize;
                        if k == 0 {
                            return Self::num(1.0);
                        }
                        let mut acc = b.clone();
                        for _ in 1..k {
                            acc = multiply_expanded(acc, b.clone())?.simplify()?;
                        }
                        return Ok(acc);
                    }
                }
                Ok(Self::Pow(Box::new(b), Box::new(e)))
            }
            Self::Mul(items) => {
                let mut acc = Self::Num(1.0);
                for item in items {
                    let part = item.expand_depth(depth + 1)?;
                    acc = multiply_expanded(acc, part)?;
                }
                Ok(acc)
            }
        }
    }

    pub fn diff(&self, var: &str) -> Calc<Self> {
        let raw = self.diff_raw(var, 0)?;
        raw.simplify()
    }

    fn diff_raw(&self, var: &str, depth: usize) -> Calc<Self> {
        if depth > 64 {
            return Err("слишком большая глубина дифференцирования".into());
        }
        match self {
            Self::Num(_) => Self::num(0.0),
            Self::Var(v) => Self::num(if v == var { 1.0 } else { 0.0 }),
            Self::Add(items) => Ok(Self::Add(
                items
                    .iter()
                    .map(|e| e.diff_raw(var, depth + 1))
                    .collect::<Calc<_>>()?,
            )),
            Self::Mul(items) => {
                let mut terms = Vec::with_capacity(items.len());
                for i in 0..items.len() {
                    let mut prod = Vec::with_capacity(items.len());
                    for (j, item) in items.iter().enumerate() {
                        if i == j {
                            prod.push(item.diff_raw(var, depth + 1)?);
                        } else {
                            prod.push(item.clone());
                        }
                    }
                    terms.push(Self::Mul(prod));
                }
                Ok(Self::Add(terms))
            }
            Self::Pow(base, exp) => {
                let b_has = base.contains_var(var);
                let e_has = exp.contains_var(var);
                match (b_has, e_has) {
                    (false, false) => Self::num(0.0),
                    (true, false) => {
                        let du = base.diff_raw(var, depth + 1)?;
                        let new_exp = Self::sub((**exp).clone(), Self::Num(1.0));
                        Ok(Self::Mul(vec![
                            (**exp).clone(),
                            Self::Pow(base.clone(), Box::new(new_exp)),
                            du,
                        ]))
                    }
                    (false, true) => {
                        let dv = exp.diff_raw(var, depth + 1)?;
                        Ok(Self::Mul(vec![
                            Self::Pow(base.clone(), exp.clone()),
                            Self::Func("ln".into(), vec![(**base).clone()]),
                            dv,
                        ]))
                    }
                    (true, true) => {
                        let du = base.diff_raw(var, depth + 1)?;
                        let dv = exp.diff_raw(var, depth + 1)?;
                        let term1 = Self::mul(dv, Self::Func("ln".into(), vec![(**base).clone()]));
                        let term2 = Self::div(Self::mul((**exp).clone(), du), (**base).clone());
                        Ok(Self::mul(
                            Self::Pow(base.clone(), exp.clone()),
                            Self::add(term1, term2),
                        ))
                    }
                }
            }
            Self::Func(name, args) => {
                let [u] = args.as_slice() else {
                    return Err(format!("символьная производная {name} не поддерживается"));
                };
                if !u.contains_var(var) {
                    return Self::num(0.0);
                }
                let du = u.diff_raw(var, depth + 1)?;
                let outer = match name.as_str() {
                    "sin" => Self::Func("cos".into(), vec![u.clone()]),
                    "cos" => Self::neg(Self::Func("sin".into(), vec![u.clone()])),
                    "tan" => Self::pow(Self::Func("cos".into(), vec![u.clone()]), Self::Num(-2.0)),
                    "exp" => Self::Func("exp".into(), vec![u.clone()]),
                    "ln" => Self::pow(u.clone(), Self::Num(-1.0)),
                    "sqrt" => Self::div(
                        Self::Num(1.0),
                        Self::mul(Self::Num(2.0), Self::Func("sqrt".into(), vec![u.clone()])),
                    ),
                    "sinh" => Self::Func("cosh".into(), vec![u.clone()]),
                    "cosh" => Self::Func("sinh".into(), vec![u.clone()]),
                    "tanh" => {
                        Self::pow(Self::Func("cosh".into(), vec![u.clone()]), Self::Num(-2.0))
                    }
                    "atan" => Self::div(
                        Self::Num(1.0),
                        Self::add(Self::Num(1.0), Self::pow(u.clone(), Self::Num(2.0))),
                    ),
                    _ => return Err(format!("символьная производная {name} не поддерживается")),
                };
                Ok(Self::mul(outer, du))
            }
        }
    }

    pub fn solve(&self, var: &str) -> Calc<Vec<Self>> {
        let expanded = self.expand()?;
        let terms = match expanded {
            Self::Add(items) => items,
            other => vec![other],
        };
        let mut c0 = Vec::new();
        let mut c1 = Vec::new();
        let mut c2 = Vec::new();
        for term in terms {
            match extract_poly_degree(&term, var) {
                Some((0, coeff)) => c0.push(coeff),
                Some((1, coeff)) => c1.push(coeff),
                Some((2, coeff)) => c2.push(coeff),
                _ => {
                    return Err(format!(
                        "символьное решение по {var} поддерживает линейные и квадратные уравнения"
                    ))
                }
            }
        }
        let sum_or_zero = |list: Vec<Self>| -> Calc<Self> {
            if list.is_empty() {
                Self::num(0.0)
            } else {
                Self::Add(list).simplify()
            }
        };
        let a = sum_or_zero(c2)?;
        let b = sum_or_zero(c1)?;
        let c = sum_or_zero(c0)?;

        let a_zero = matches!(a, Self::Num(n) if is_zero(n));
        let b_zero = matches!(b, Self::Num(n) if is_zero(n));
        if a_zero {
            if b_zero {
                return Err(format!("уравнение не зависит от {var}"));
            }
            let root = Self::div(Self::neg(c), b).simplify()?;
            return Ok(vec![root]);
        }
        let disc = Self::sub(
            Self::pow(b.clone(), Self::Num(2.0)),
            Self::Mul(vec![Self::Num(4.0), a.clone(), c]),
        )
        .simplify()?;
        let two_a = Self::mul(Self::Num(2.0), a).simplify()?;
        if matches!(disc, Self::Num(n) if is_zero(n)) {
            let r = Self::div(Self::neg(b), two_a).simplify()?;
            return Ok(vec![r]);
        }
        let sqrt_d = Self::Func("sqrt".into(), vec![disc]).simplify()?;
        let r1 = Self::div(
            Self::sub(Self::neg(b.clone()), sqrt_d.clone()),
            two_a.clone(),
        )
        .simplify()?;
        let r2 = Self::div(Self::add(Self::neg(b), sqrt_d), two_a).simplify()?;
        Ok(vec![r1, r2])
    }

    fn sort_key(&self) -> (u8, String) {
        match self {
            Self::Var(v) => (0, v.clone()),
            Self::Pow(b, e) => (1, format!("{b}^{e}")),
            Self::Func(n, _) => (2, format!("{n}({self})")),
            Self::Mul(_) => (3, self.to_string()),
            Self::Add(_) => (4, self.to_string()),
            Self::Num(_) => (5, self.to_string()),
        }
    }

    fn precedence(&self) -> u8 {
        match self {
            Self::Add(_) => 1,
            Self::Mul(_) => 2,
            Self::Pow(_, _) => 3,
            Self::Num(n) if *n < 0.0 => 2,
            Self::Num(_) | Self::Var(_) | Self::Func(_, _) => 4,
        }
    }
}

fn split_coeff(expr: SymExpr) -> (f64, SymExpr) {
    match expr {
        SymExpr::Num(n) => (n, SymExpr::Num(1.0)),
        SymExpr::Mul(mut items) => {
            if let Some(SymExpr::Num(n)) = items.first().copied_num() {
                items.remove(0);
                let rest = match items.len() {
                    0 => SymExpr::Num(1.0),
                    1 => items.into_iter().next().unwrap(),
                    _ => SymExpr::Mul(items),
                };
                (n, rest)
            } else {
                (1.0, SymExpr::Mul(items))
            }
        }
        other => (1.0, other),
    }
}

trait OptNum {
    fn copied_num(&self) -> Option<SymExpr>;
}

impl OptNum for Option<&SymExpr> {
    fn copied_num(&self) -> Option<SymExpr> {
        match self {
            Some(SymExpr::Num(n)) => Some(SymExpr::Num(*n)),
            _ => None,
        }
    }
}

fn multiply_expanded(a: SymExpr, b: SymExpr) -> Calc<SymExpr> {
    let left = match a {
        SymExpr::Add(items) => items,
        other => vec![other],
    };
    let right = match b {
        SymExpr::Add(items) => items,
        other => vec![other],
    };
    if left.len().saturating_mul(right.len()) > 256 {
        return Err("раскрытие скобок даёт слишком много слагаемых".into());
    }
    let mut out = Vec::with_capacity(left.len() * right.len());
    for l in &left {
        for r in &right {
            out.push(SymExpr::mul(l.clone(), r.clone()));
        }
    }
    Ok(if out.len() == 1 {
        out.into_iter().next().unwrap()
    } else {
        SymExpr::Add(out)
    })
}

fn extract_poly_degree(term: &SymExpr, var: &str) -> Option<(usize, SymExpr)> {
    if !term.contains_var(var) {
        return Some((0, term.clone()));
    }
    match term {
        SymExpr::Var(v) if v == var => Some((1, SymExpr::Num(1.0))),
        SymExpr::Pow(b, e) if matches!(b.as_ref(), SymExpr::Var(v) if v == var) => {
            if let SymExpr::Num(n) = e.as_ref() {
                if n.fract() == 0.0 && (*n == 0.0 || *n == 1.0 || *n == 2.0) {
                    return Some((*n as usize, SymExpr::Num(1.0)));
                }
            }
            None
        }
        SymExpr::Mul(factors) => {
            let mut deg = 0usize;
            let mut coeff_factors = Vec::new();
            for f in factors {
                if !f.contains_var(var) {
                    coeff_factors.push(f.clone());
                } else if matches!(f, SymExpr::Var(v) if v == var) {
                    deg += 1;
                } else if let SymExpr::Pow(b, e) = f {
                    if matches!(b.as_ref(), SymExpr::Var(v) if v == var) {
                        if let SymExpr::Num(n) = e.as_ref() {
                            if n.fract() == 0.0 && (0.0..=2.0).contains(n) {
                                deg += *n as usize;
                                continue;
                            }
                        }
                    }
                    return None;
                } else {
                    return None;
                }
            }
            if deg > 2 {
                return None;
            }
            let coeff = match coeff_factors.len() {
                0 => SymExpr::Num(1.0),
                1 => coeff_factors.into_iter().next().unwrap(),
                _ => SymExpr::Mul(coeff_factors),
            };
            Some((deg, coeff))
        }
        _ => None,
    }
}

impl fmt::Display for SymExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(n) => write!(f, "{}", display_number(*n)),
            Self::Var(v) => write!(f, "{v}"),
            Self::Add(items) => {
                for (i, item) in items.iter().enumerate() {
                    let (neg, body) = match item {
                        Self::Num(n) if *n < 0.0 => (true, Self::Num(-*n)),
                        Self::Mul(factors) if matches!(factors.first(), Some(Self::Num(n)) if *n < 0.0) =>
                        {
                            let mut copy = factors.clone();
                            if let Self::Num(n) = &mut copy[0] {
                                *n = -*n;
                                if is_one(*n) && copy.len() > 1 {
                                    copy.remove(0);
                                }
                            }
                            let rest = if copy.len() == 1 {
                                copy.into_iter().next().unwrap()
                            } else {
                                Self::Mul(copy)
                            };
                            (true, rest)
                        }
                        other => (false, other.clone()),
                    };
                    if i == 0 {
                        if neg {
                            write!(f, "−{body}")?;
                        } else {
                            write!(f, "{body}")?;
                        }
                    } else if neg {
                        write!(f, " − {body}")?;
                    } else {
                        write!(f, " + {body}")?;
                    }
                }
                Ok(())
            }
            Self::Mul(items) => {
                let mut num = Vec::new();
                let mut den = Vec::new();
                let mut sign = false;
                for item in items {
                    match item {
                        Self::Num(n) if (*n + 1.0).abs() < 1e-12 => sign = !sign,
                        Self::Num(n) if *n < 0.0 => {
                            sign = !sign;
                            num.push(Self::Num(-*n));
                        }
                        Self::Pow(b, e) if matches!(e.as_ref(), Self::Num(n) if *n < 0.0) => {
                            if let Self::Num(n) = e.as_ref() {
                                let pos = -*n;
                                if is_one(pos) {
                                    den.push((**b).clone());
                                } else {
                                    den.push(Self::Pow(b.clone(), Box::new(Self::Num(pos))));
                                }
                            }
                        }
                        other => num.push(other.clone()),
                    }
                }
                if sign {
                    write!(f, "−")?;
                }
                if num.is_empty() {
                    write!(f, "1")?;
                } else {
                    for (i, factor) in num.iter().enumerate() {
                        if i > 0 {
                            write!(f, "·")?;
                        }
                        if factor.precedence() < 2 {
                            write!(f, "({factor})")?;
                        } else {
                            write!(f, "{factor}")?;
                        }
                    }
                }
                if !den.is_empty() {
                    write!(f, " / ")?;
                    if den.len() == 1 {
                        let d = &den[0];
                        if d.precedence() <= 2 {
                            write!(f, "({d})")?;
                        } else {
                            write!(f, "{d}")?;
                        }
                    } else {
                        write!(f, "(")?;
                        for (i, d) in den.iter().enumerate() {
                            if i > 0 {
                                write!(f, "·")?;
                            }
                            if d.precedence() < 2 {
                                write!(f, "({d})")?;
                            } else {
                                write!(f, "{d}")?;
                            }
                        }
                        write!(f, ")")?;
                    }
                }
                Ok(())
            }
            Self::Pow(base, exp) => {
                if base.precedence() < 3 {
                    write!(f, "({base})")?;
                } else {
                    write!(f, "{base}")?;
                }
                write!(f, "^")?;
                if exp.precedence() < 3 {
                    write!(f, "({exp})")
                } else {
                    write!(f, "{exp}")
                }
            }
            Self::Func(name, args) => {
                write!(f, "{name}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
        }
    }
}

pub fn from_ml_node(node: Xml<'_, '_>, depth: usize) -> Calc<SymExpr> {
    if depth > 64 || node.tag_name().namespace() != Some(ML) {
        return Err("неподдерживаемое символьное выражение".into());
    }
    match kind(node) {
        "real" => {
            let txt = node.text().ok_or("пустое число")?.trim();
            let val: f64 = txt.parse().map_err(|_| format!("неверное число {txt}"))?;
            SymExpr::num(val)
        }
        "id" => Ok(SymExpr::var(identifier(node)?)),
        "parens" | "eval" | "symEval" => {
            let child = elements(node).next().ok_or("пустое символьное выражение")?;
            from_ml_node(child, depth + 1)
        }
        "apply" => {
            let mut items = elements(node);
            let op = items.next().ok_or("нет операции")?;
            let mut args = Vec::new();
            for item in items {
                if named(item, ML, "sequence") {
                    for sub in elements(item) {
                        args.push(from_ml_node(sub, depth + 1)?);
                    }
                } else {
                    args.push(from_ml_node(item, depth + 1)?);
                }
            }
            let op_name = if named(op, ML, "id") {
                identifier(op)?
            } else {
                kind(op).to_owned()
            };
            match (op_name.as_str(), args.as_slice()) {
                ("plus", [a, b]) => Ok(SymExpr::add(a.clone(), b.clone())),
                ("minus", [a, b]) => Ok(SymExpr::sub(a.clone(), b.clone())),
                ("neg" | "minus", [a]) => Ok(SymExpr::neg(a.clone())),
                ("mult" | "scale", [a, b]) => Ok(SymExpr::mul(a.clone(), b.clone())),
                ("div", [a, b]) => Ok(SymExpr::div(a.clone(), b.clone())),
                ("pow", [a, b]) => Ok(SymExpr::pow(a.clone(), b.clone())),
                ("equal", [a, b]) => Ok(SymExpr::sub(a.clone(), b.clone())),
                ("absval", [a]) => Ok(SymExpr::Func("abs".into(), vec![a.clone()])),
                _ => Ok(SymExpr::Func(op_name, args)),
            }
        }
        other => Err(format!("символьный движок пока не поддерживает <{other}>")),
    }
}

pub fn eval_symbolic(expr: &SymExpr, env: &HashMap<String, SymExpr>) -> Calc<SymResult> {
    let substituted = expr.substitute_map(env);
    let domain = substituted.collect_domain();
    if let SymExpr::Func(name, args) = &substituted {
        if name == "ssolve" {
            let [eq, SymExpr::Var(var)] = args.as_slice() else {
                return Err("ssolve требует уравнение и переменную: ssolve(expr, x)".into());
            };
            let roots = eq.solve(var)?;
            let display = if roots.len() == 1 {
                format!("{var} = {}", roots[0])
            } else {
                let list: Vec<String> = roots.iter().map(ToString::to_string).collect();
                format!("{var} ∈ {{{}}}", list.join("; "))
            };
            return Ok(SymResult {
                display,
                expr: roots.into_iter().next(),
                domain,
            });
        }
    }
    let simplified = substituted.simplify()?;
    let display = simplified.to_string();
    Ok(SymResult {
        display,
        expr: Some(simplified),
        domain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplifies_expands_differentiates_and_solves_symbolically() {
        // expand((x + 1)^2) -> x^2 + 2·x + 1
        let sq = SymExpr::pow(
            SymExpr::add(SymExpr::var("x"), SymExpr::Num(1.0)),
            SymExpr::Num(2.0),
        );
        let exp = sq.expand().unwrap();
        assert_eq!(exp.to_string(), "2·x + x^2 + 1");

        // diff(x^3 + 2*x, x) -> 3*x^2 + 2
        let poly = SymExpr::add(
            SymExpr::pow(SymExpr::var("x"), SymExpr::Num(3.0)),
            SymExpr::mul(SymExpr::Num(2.0), SymExpr::var("x")),
        );
        let d = poly.diff("x").unwrap();
        assert_eq!(d.to_string(), "3·x^2 + 2");

        // solve(2*x - 6, x) -> x = 3
        let lin = SymExpr::sub(
            SymExpr::mul(SymExpr::Num(2.0), SymExpr::var("x")),
            SymExpr::Num(6.0),
        );
        let roots = lin.solve("x").unwrap();
        assert_eq!(roots, vec![SymExpr::Num(3.0)]);

        // solve(x^2 - 5*x + 6, x) -> 2, 3
        let quad = SymExpr::Add(vec![
            SymExpr::pow(SymExpr::var("x"), SymExpr::Num(2.0)),
            SymExpr::mul(SymExpr::Num(-5.0), SymExpr::var("x")),
            SymExpr::Num(6.0),
        ]);
        let qroots = quad.solve("x").unwrap();
        assert_eq!(qroots, vec![SymExpr::Num(2.0), SymExpr::Num(3.0)]);
    }

    #[test]
    fn preserves_domain_constraints_on_simplification() {
        // (x / x) -> 1, domain: [x ≠ 0]
        let expr = SymExpr::div(SymExpr::var("x"), SymExpr::var("x"));
        let res = eval_symbolic(&expr, &HashMap::new()).unwrap();
        assert_eq!(res.display, "1");
        assert_eq!(res.domain, vec!["x ≠ 0"]);

        // ln(x) -> domain: [x > 0]
        let log_expr = SymExpr::Func("ln".into(), vec![SymExpr::var("x")]);
        let res2 = eval_symbolic(&log_expr, &HashMap::new()).unwrap();
        assert_eq!(res2.domain, vec!["x > 0"]);
    }
}
