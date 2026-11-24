use crate::value::{display_number, display_number_precision, Calc, Value};
use num_complex::Complex64;
use std::{cell::Cell, fmt, sync::Arc};

pub const MAX_CELLS: usize = 100_000;
pub type Cache = [Option<Calc<Datum>>];

#[derive(Clone, Debug, PartialEq)]
pub enum Datum {
    Scalar(Value),
    Matrix(Arc<Matrix>),
    Range(Span),
    Function(Arc<Function>),
    Symbolic(Arc<crate::symbolic::SymResult>),
}

impl From<Value> for Datum {
    fn from(value: Value) -> Self {
        Self::Scalar(value)
    }
}

impl Datum {
    pub fn scalar(&self) -> Calc<Value> {
        if let Self::Scalar(value) = self {
            Ok(*value)
        } else {
            Err("требуется скаляр".into())
        }
    }

    pub fn near(&self, other: &Self) -> bool {
        fn near(a: Value, b: Value) -> bool {
            let scale = a.number.hypot(a.imaginary).max(b.number.hypot(b.imaginary));
            a.compatible(b)
                && (a.number - b.number).hypot(a.imaginary - b.imaginary) <= 1e-11 + 1e-9 * scale
        }
        match (self, other) {
            (Self::Scalar(a), Self::Scalar(b)) => near(*a, *b),
            (Self::Matrix(a), Self::Matrix(b)) => {
                a.rows == b.rows
                    && a.cols == b.cols
                    && a.values.iter().zip(&b.values).all(|(&x, &y)| near(x, y))
            }
            _ => self == other,
        }
    }

    pub fn format_precision(&self, digits: usize) -> String {
        match self {
            Self::Scalar(value) => value.format_precision(digits),
            Self::Function(function) => format!("функция ({})", function.params.join(", ")),
            Self::Symbolic(sym) => sym.to_string(),
            Self::Range(span) => format!(
                "{} … {} (шаг {}, {} знач.)",
                display_number_precision(span.start, digits),
                display_number_precision(span.start + span.step * (span.count - 1) as f64, digits),
                display_number_precision(span.step, digits),
                span.count
            ),
            Self::Matrix(matrix) => {
                let mut out = format!("{}×{} [", matrix.rows, matrix.cols);
                for (i, value) in matrix.values.iter().take(12).enumerate() {
                    if i > 0 {
                        out.push_str(if i % matrix.cols == 0 { "; " } else { ", " });
                    }
                    out.push_str(&value.format_precision(digits));
                }
                if matrix.values.len() > 12 {
                    out.push_str(", …");
                }
                out.push(']');
                out
            }
        }
    }

    pub fn details_precision(&self, digits: usize) -> String {
        let Self::Matrix(matrix) = self else {
            return self.format_precision(digits);
        };
        let mut lines = Vec::with_capacity(matrix.rows);
        for row in matrix.values.chunks_exact(matrix.cols) {
            lines.push(
                row.iter()
                    .map(|v| v.format_precision(digits))
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
        }
        lines.join("\n")
    }

    pub fn vector_values(&self) -> Calc<Vec<Value>> {
        match self {
            Self::Matrix(matrix) if matrix.rows == 1 || matrix.cols == 1 => {
                Ok(matrix.values.clone())
            }
            Self::Range(span) => span.values().collect(),
            _ => Err("для графика требуется вектор или диапазон".into()),
        }
    }

    pub fn to_csv(&self, delimiter: char, digits: usize) -> Calc<String> {
        let mut out = String::new();
        match self {
            Self::Matrix(matrix) => {
                for row in matrix.values.chunks_exact(matrix.cols) {
                    for (c, val) in row.iter().enumerate() {
                        if c > 0 {
                            out.push(delimiter);
                        }
                        out.push_str(&val.format_precision(digits));
                    }
                    out.push('\n');
                }
                Ok(out)
            }
            Self::Range(span) => {
                for val in span.values() {
                    out.push_str(&val?.format_precision(digits));
                    out.push('\n');
                }
                Ok(out)
            }
            Self::Scalar(value) => {
                out.push_str(&value.format_precision(digits));
                out.push('\n');
                Ok(out)
            }
            Self::Function(_) => Err("функцию нельзя выгрузить в CSV".into()),
            Self::Symbolic(sym) => Ok(format!("{sym}\n")),
        }
    }

    pub fn as_real_matrix(&self) -> Option<(usize, usize, Vec<f64>)> {
        let Self::Matrix(matrix) = self else {
            return None;
        };
        if matrix.rows > 64 || matrix.cols > 32 {
            return None;
        }
        let nums: Option<Vec<f64>> = matrix
            .values
            .iter()
            .map(|v| (v.dimensionless() && v.imaginary == 0.).then_some(v.number))
            .collect();
        Some((matrix.rows, matrix.cols, nums?))
    }

    #[cfg(feature = "desktop")]
    pub fn as_surface_matrix(&self) -> Calc<(usize, usize, Vec<f64>)> {
        let Self::Matrix(matrix) = self else {
            return Err("для графика по матрице требуется матрица не меньше 2×2".into());
        };
        if matrix.rows < 2 || matrix.cols < 2 {
            return Err("матрица для 3D/контурного графика должна быть не меньше 2×2".into());
        }
        let first = matrix.values[0];
        if matrix
            .values
            .iter()
            .any(|v| v.imaginary != 0. || !first.compatible(*v) || !v.number.is_finite())
        {
            return Err("матрица для 3D/контурного графика должна содержать действительные числа одной размерности".into());
        }
        Ok((
            matrix.rows,
            matrix.cols,
            matrix.values.iter().map(|v| v.number).collect(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    values: Vec<Value>,
}

impl Matrix {
    pub fn from_values(rows: usize, cols: usize, values: Vec<Value>) -> Calc<Self> {
        if rows == 0
            || cols == 0
            || rows.checked_mul(cols) != Some(values.len())
            || values.len() > MAX_CELLS
        {
            return Err("неверный размер матрицы или превышен предел числа ячеек".into());
        }
        Ok(Self { rows, cols, values })
    }

    fn get(&self, row: usize, col: usize) -> Calc<Value> {
        if row >= self.rows || col >= self.cols {
            return Err("индекс за пределами массива".into());
        }
        Ok(self.values[row * self.cols + col])
    }

    fn write(&mut self, row: usize, col: usize, value: Value) -> Calc<()> {
        let rows = self.rows.max(row + 1);
        let cols = self.cols.max(col + 1);
        if rows > MAX_CELLS / cols {
            return Err("слишком много ячеек в массиве".into());
        }
        if cols != self.cols {
            let mut values = vec![Value::scalar(0.)?; rows * cols];
            for (r, old_row) in self.values.chunks(self.cols.max(1)).enumerate() {
                values[r * cols..r * cols + self.cols].copy_from_slice(old_row);
            }
            self.values = values;
        } else if rows != self.rows {
            self.values.resize(rows * cols, Value::scalar(0.)?);
        }
        self.rows = rows;
        self.cols = cols;
        self.values[row * cols + col] = value;
        Ok(())
    }

    fn transpose(&self) -> Self {
        let mut values = Vec::with_capacity(self.values.len());
        for col in 0..self.cols {
            for row in 0..self.rows {
                values.push(self.values[row * self.cols + col]);
            }
        }
        Self {
            rows: self.cols,
            cols: self.rows,
            values,
        }
    }

    fn zip(&self, other: &Self, op: &str) -> Calc<Self> {
        if self.rows != other.rows || self.cols != other.cols {
            return Err("размеры матриц должны совпадать".into());
        }
        let values = self
            .values
            .iter()
            .zip(&other.values)
            .map(|(&a, &b)| a.binary(op, b))
            .collect::<Calc<_>>()?;
        Self::from_values(self.rows, self.cols, values)
    }

    fn multiply(&self, other: &Self) -> Calc<Datum> {
        if self.cols == 1 && other.cols == 1 {
            if self.rows != other.rows {
                return Err("длины векторов должны совпадать".into());
            }
            let terms = self
                .values
                .iter()
                .zip(&other.values)
                .map(|(&a, &b)| a.binary("mult", b.unary("conj")?));
            return Ok(sum(terms)?.into());
        }
        if self.cols != other.rows
            || self.rows > MAX_CELLS / other.cols
            || self.rows * other.cols > 5_000_000 / self.cols
        {
            return Err("несовместимые размеры или слишком большое произведение матриц".into());
        }
        let mut values = Vec::with_capacity(self.rows * other.cols);
        for row in 0..self.rows {
            for col in 0..other.cols {
                let terms =
                    (0..self.cols).map(|i| self.get(row, i)?.binary("mult", other.get(i, col)?));
                values.push(sum(terms)?);
            }
        }
        Ok(Datum::Matrix(Arc::new(Self::from_values(
            self.rows, other.cols, values,
        )?)))
    }

    fn identity(size: usize) -> Calc<Self> {
        let cells = size
            .checked_mul(size)
            .filter(|&n| n > 0 && n <= MAX_CELLS)
            .ok_or("неверный размер единичной матрицы")?;
        let mut values = vec![Value::scalar(0.)?; cells];
        for i in 0..size {
            values[i * size + i] = Value::scalar(1.)?;
        }
        Self::from_values(size, size, values)
    }

    fn column(&self, col: usize) -> Calc<Self> {
        if col >= self.cols {
            return Err("индекс столбца за пределами матрицы".into());
        }
        let mut values = Vec::with_capacity(self.rows);
        for row in 0..self.rows {
            values.push(self.values[row * self.cols + col]);
        }
        Self::from_values(self.rows, 1, values)
    }

    fn row_vec(&self, row: usize) -> Calc<Self> {
        if row >= self.rows {
            return Err("индекс строки за пределами матрицы".into());
        }
        let start = row * self.cols;
        Self::from_values(1, self.cols, self.values[start..start + self.cols].to_vec())
    }

    fn augment(&self, other: &Self) -> Calc<Self> {
        if self.rows != other.rows {
            return Err("augment требует одинаковое число строк".into());
        }
        let cols = self
            .cols
            .checked_add(other.cols)
            .ok_or("слишком много столбцов")?;
        let mut values = Vec::with_capacity(self.rows * cols);
        for r in 0..self.rows {
            values.extend_from_slice(&self.values[r * self.cols..(r + 1) * self.cols]);
            values.extend_from_slice(&other.values[r * other.cols..(r + 1) * other.cols]);
        }
        Self::from_values(self.rows, cols, values)
    }

    fn stack(&self, other: &Self) -> Calc<Self> {
        if self.cols != other.cols {
            return Err("stack требует одинаковое число столбцов".into());
        }
        let rows = self
            .rows
            .checked_add(other.rows)
            .ok_or("слишком много строк")?;
        let mut values = Vec::with_capacity(rows * self.cols);
        values.extend_from_slice(&self.values);
        values.extend_from_slice(&other.values);
        Self::from_values(rows, self.cols, values)
    }

    fn eliminate(&self, rhs: Option<&Self>) -> Calc<Datum> {
        let n = self.rows;
        let columns = rhs.map_or(0, |matrix| matrix.cols);
        if n != self.cols || rhs.is_some_and(|b| b.rows != n) || n * n > 5_000_000 / (n + columns) {
            return Err("несовместимые размеры системы либо слишком большая матрица".into());
        }
        if self.values.iter().any(|v| !v.dimensionless()) {
            return Err("матрица коэффициентов должна быть безразмерной".into());
        }
        let mut a = self.values.clone();
        let mut b = rhs.map(|rhs| rhs.values.clone()).unwrap_or_default();
        let mut det = Value::scalar(1.)?;
        for j in 0..n {
            let pivot = (j..n)
                .max_by(|&x, &y| {
                    a[x * n + j]
                        .magnitude()
                        .total_cmp(&a[y * n + j].magnitude())
                })
                .ok_or("пустая система")?;
            if a[pivot * n + j].magnitude() == 0. {
                if rhs.is_none() {
                    return Ok(Value::scalar(0.)?.into());
                }
                return Err("матрица вырождена".into());
            }
            if pivot != j {
                for k in j..n {
                    a.swap(j * n + k, pivot * n + k);
                }
                for k in 0..columns {
                    b.swap(j * columns + k, pivot * columns + k);
                }
                det = det.unary("neg")?;
            }
            let divisor = a[j * n + j];
            if rhs.is_none() {
                det = det.binary("mult", divisor)?;
            }
            for r in j + 1..n {
                let factor = a[r * n + j].binary("div", divisor)?;
                if factor.magnitude() == 0. {
                    continue;
                }
                for k in j + 1..n {
                    a[r * n + k] =
                        a[r * n + k].binary("minus", factor.binary("mult", a[j * n + k])?)?;
                }
                for k in 0..columns {
                    b[r * columns + k] = b[r * columns + k]
                        .binary("minus", factor.binary("mult", b[j * columns + k])?)?;
                }
            }
        }
        if rhs.is_none() {
            return Ok(det.into());
        }
        for j in (0..n).rev() {
            for k in 0..columns {
                let mut value = b[j * columns + k];
                for i in j + 1..n {
                    value =
                        value.binary("minus", a[j * n + i].binary("mult", b[i * columns + k])?)?;
                }
                b[j * columns + k] = value.binary("div", a[j * n + j])?;
            }
        }
        Ok(Datum::Matrix(Arc::new(Self::from_values(n, columns, b)?)))
    }
}

fn sum(mut terms: impl Iterator<Item = Calc<Value>>) -> Calc<Value> {
    let first = terms.next().ok_or("нет элементов")??;
    terms.try_fold(first, |a, b| a.binary("plus", b?))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    start: f64,
    step: f64,
    count: usize,
}

impl Span {
    fn new(start: f64, next: Option<f64>, end: f64) -> Calc<Self> {
        let step = next.map_or(1., |value| value - start);
        let distance = (end - start) / step;
        let count = (distance + 1e-12 * distance.abs()).floor() + 1.;
        if step == 0.
            || !step.is_finite()
            || !count.is_finite()
            || count < 1.
            || count > MAX_CELLS as f64
        {
            return Err("неверный шаг или превышен размер диапазона".into());
        }
        Ok(Self {
            start,
            step,
            count: count as usize,
        })
    }

    fn values(self) -> impl Iterator<Item = Calc<Value>> {
        (0..self.count).map(move |i| Value::scalar(self.start + self.step * i as f64))
    }
}

impl fmt::Display for Datum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(value) => write!(f, "{value}"),
            Self::Function(function) => write!(f, "функция ({})", function.params.join(", ")),
            Self::Symbolic(sym) => write!(f, "{sym}"),
            Self::Range(span) => write!(
                f,
                "{} … {} (шаг {}, {} знач.)",
                display_number(span.start),
                display_number(span.start + span.step * (span.count - 1) as f64),
                display_number(span.step),
                span.count
            ),
            Self::Matrix(matrix) => {
                write!(f, "{}×{} [", matrix.rows, matrix.cols)?;
                for (i, value) in matrix.values.iter().take(12).enumerate() {
                    if i > 0 {
                        write!(f, "{}", if i % matrix.cols == 0 { "; " } else { ", " })?;
                    }
                    write!(f, "{value}")?;
                }
                if matrix.values.len() > 12 {
                    write!(f, ", …")?;
                }
                write!(f, "]")
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Function {
    pub params: Vec<String>,
    pub body: Expr,
}

#[cfg(feature = "desktop")]
impl Function {
    pub fn sample(&self, cache: &Cache, x: Value) -> Calc<Value> {
        self.body
            .evaluate(&Context {
                cache,
                params: &[x.into()],
                bindings: &[],
                locals: &[],
                budget: &Cell::new(16_000),
                call_depth: 0,
            })?
            .scalar()
    }

    pub fn sample2(&self, cache: &Cache, x: Value, y: Value) -> Calc<Value> {
        self.body
            .evaluate(&Context {
                cache,
                params: &[x.into(), y.into()],
                bindings: &[],
                locals: &[],
                budget: &Cell::new(16_000),
                call_depth: 0,
            })?
            .scalar()
    }
}

#[derive(Debug, PartialEq)]
pub enum Stmt {
    Expr(Expr),
    LocalAssign(String, Expr),
    LocalIndexedAssign {
        name: String,
        indices: Vec<Expr>,
        origin: Expr,
        value: Expr,
    },
    If {
        cond: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    For {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Return(Expr),
    Break,
    Continue,
}

impl Stmt {
    pub fn shift_refs(&mut self, offset: usize) {
        match self {
            Self::Expr(e) | Self::LocalAssign(_, e) | Self::Return(e) => e.shift_refs(offset),
            Self::LocalIndexedAssign {
                indices,
                origin,
                value,
                ..
            } => {
                for idx in indices {
                    idx.shift_refs(offset);
                }
                origin.shift_refs(offset);
                value.shift_refs(offset);
            }
            Self::If {
                cond,
                then_branch,
                else_branch,
            } => {
                cond.shift_refs(offset);
                for s in then_branch {
                    s.shift_refs(offset);
                }
                for s in else_branch {
                    s.shift_refs(offset);
                }
            }
            Self::For { iter, body, .. } => {
                iter.shift_refs(offset);
                for s in body {
                    s.shift_refs(offset);
                }
            }
            Self::While { cond, body } => {
                cond.shift_refs(offset);
                for s in body {
                    s.shift_refs(offset);
                }
            }
            Self::Break | Self::Continue => {}
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Expr {
    Literal(Value),
    Ref(usize),
    Param(usize),
    Local(String),
    Builtin(String, Vec<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Function(Arc<Function>),
    Call(usize, Vec<Expr>),
    Matrix(usize, usize, Vec<Expr>),
    Range(Vec<Expr>),
    Index(Box<Expr>, Vec<Expr>, Box<Expr>),
    Assign(Box<IndexedAssignment>),
    Program(Vec<Stmt>),
    Symbolic(Box<crate::symbolic::SymExpr>, Vec<(String, usize)>),
    SolveBlock(Box<SolveBlockExpr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveMode {
    Find,
    Minerr,
    Minimize,
    Maximize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintKind {
    Eq,
    Le,
    Ge,
}

#[derive(Debug, PartialEq)]
pub struct SolveBlockExpr {
    pub mode: SolveMode,
    pub vars: Vec<(String, usize)>,
    pub objective: Option<Expr>,
    pub constraints: Vec<(ConstraintKind, Expr)>,
}

#[derive(Debug, PartialEq)]
pub struct IndexedAssignment {
    pub previous: Option<usize>,
    pub indices: Vec<Expr>,
    pub origin: Expr,
    pub body: Expr,
}

struct Context<'a> {
    cache: &'a Cache,
    params: &'a [Datum],
    bindings: &'a [(usize, Value)],
    locals: &'a [(String, Datum)],
    budget: &'a Cell<usize>,
    call_depth: usize,
}

impl Context<'_> {
    fn get(&self, index: usize) -> Calc<Datum> {
        if let Some((_, value)) = self.bindings.iter().rev().find(|(at, _)| *at == index) {
            return Ok((*value).into());
        }
        self.cache[index]
            .as_ref()
            .ok_or("зависимость ещё не рассчитана")?
            .as_ref()
            .cloned()
            .map_err(|_| format!("ошибка в зависимой формуле #{}", index + 1))
    }

    fn spend(&self, count: usize) -> Calc<()> {
        let remaining = self
            .budget
            .get()
            .checked_sub(count)
            .ok_or("превышен предел операций для одной формулы")?;
        self.budget.set(remaining);
        Ok(())
    }
}

fn index(value: Value, origin: f64) -> Calc<usize> {
    let n = value.real_scalar()? - origin;
    if n.fract() != 0. || n < 0. || n >= MAX_CELLS as f64 {
        return Err("целый индекс находится за пределами поддерживаемого диапазона".into());
    }
    Ok(n as usize)
}

fn as_indices(array: &Matrix, indices: &[usize]) -> Calc<(usize, usize)> {
    match indices {
        [row, col] => Ok((*row, *col)),
        [i] if array.cols == 1 => Ok((*i, 0)),
        [i] if array.rows == 1 => Ok((0, *i)),
        _ => Err("у матрицы требуются два индекса".into()),
    }
}

impl IndexedAssignment {
    fn evaluate(&self, ctx: &Context<'_>) -> Calc<Datum> {
        let origin = self.origin.evaluate(ctx)?.scalar()?.real_scalar()?;
        if origin.fract() != 0. {
            return Err("ORIGIN должен быть целым".into());
        }
        let mut array = match self.previous.map(|i| ctx.get(i)).transpose()? {
            Some(Datum::Matrix(array)) => (*array).clone(),
            None => Matrix {
                rows: 0,
                cols: 0,
                values: Vec::new(),
            },
            _ => return Err("индексированное присваивание требует массива".into()),
        };
        let mut axes = Vec::new();
        for expr in &self.indices {
            axes.push(match expr.evaluate(ctx)? {
                Datum::Scalar(value) => (None, vec![value]),
                Datum::Range(span) => {
                    let Expr::Ref(id) = expr else {
                        return Err("для записи укажите имя диапазона".into());
                    };
                    (Some(*id), span.values().collect::<Calc<_>>()?)
                }
                _ => return Err("неверный индекс массива".into()),
            });
        }
        let [(first_binding, first), rem @ ..] = axes.as_slice() else {
            return Err("нет индекса".into());
        };
        let zero = [Value::scalar(origin)?];
        let (second_binding, second) = match rem {
            [(binding, values)] => (*binding, values.as_slice()),
            [] => (None, zero.as_slice()),
            _ => return Err("у массива не более двух индексов".into()),
        };
        if first_binding.is_some() && *first_binding == second_binding {
            return Err("для двух измерений укажите разные диапазоны".into());
        }
        ctx.spend(
            first
                .len()
                .checked_mul(second.len())
                .ok_or("слишком большой диапазон индексов")?,
        )?;
        let mut bindings = ctx.bindings.to_vec();
        let base = bindings.len();
        for &a in first {
            let i = index(a, origin)?;
            for &b in second {
                let j = index(b, origin)?;
                if let Some(id) = first_binding {
                    bindings.push((*id, a));
                }
                if let Some(id) = second_binding {
                    bindings.push((id, b));
                }
                let value = self
                    .body
                    .evaluate(&Context {
                        bindings: &bindings,
                        ..*ctx
                    })?
                    .scalar()?;
                bindings.truncate(base);
                let (row, col) = if rem.is_empty() && array.rows == 1 && array.cols > 1 {
                    (0, i)
                } else {
                    if rem.is_empty() && array.cols > 1 {
                        return Err("у матрицы требуются два индекса".into());
                    }
                    (i, j)
                };
                array.write(row, col, value)?;
            }
        }
        Ok(Datum::Matrix(Arc::new(array)))
    }
}

impl Expr {
    pub fn shift_refs(&mut self, offset: usize) {
        if offset == 0 {
            return;
        }
        match self {
            Self::Literal(_) | Self::Param(_) | Self::Local(_) => {}
            Self::Ref(idx) => *idx += offset,
            Self::Builtin(_, args) | Self::Matrix(_, _, args) | Self::Range(args) => {
                for arg in args {
                    arg.shift_refs(offset);
                }
            }
            Self::Conditional(a, b, c) => {
                a.shift_refs(offset);
                b.shift_refs(offset);
                c.shift_refs(offset);
            }
            Self::Function(func) => {
                if let Some(inner) = Arc::get_mut(func) {
                    inner.body.shift_refs(offset);
                }
            }
            Self::Call(idx, args) => {
                *idx += offset;
                for arg in args {
                    arg.shift_refs(offset);
                }
            }
            Self::Index(arr, indices, origin) => {
                arr.shift_refs(offset);
                for idx in indices {
                    idx.shift_refs(offset);
                }
                origin.shift_refs(offset);
            }
            Self::Assign(assign) => {
                if let Some(prev) = &mut assign.previous {
                    *prev += offset;
                }
                for idx in &mut assign.indices {
                    idx.shift_refs(offset);
                }
                assign.origin.shift_refs(offset);
                assign.body.shift_refs(offset);
            }
            Self::Program(stmts) => {
                for stmt in stmts {
                    stmt.shift_refs(offset);
                }
            }
            Self::Symbolic(_, bindings) => {
                for (_, idx) in bindings {
                    *idx += offset;
                }
            }
            Self::SolveBlock(sb) => {
                for (_, idx) in &mut sb.vars {
                    *idx += offset;
                }
                if let Some(obj) = &mut sb.objective {
                    obj.shift_refs(offset);
                }
                for (_, c) in &mut sb.constraints {
                    c.shift_refs(offset);
                }
            }
        }
    }

    pub fn eval(&self, cache: &Cache) -> Calc<Datum> {
        self.evaluate(&Context {
            cache,
            params: &[],
            bindings: &[],
            locals: &[],
            budget: &Cell::new(1_000_000),
            call_depth: 0,
        })
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Calc<Datum> {
        ctx.spend(1)?;
        match self {
            Self::Literal(value) => Ok((*value).into()),
            Self::Ref(index) => ctx.get(*index),
            Self::Param(index) => ctx
                .params
                .get(*index)
                .cloned()
                .ok_or_else(|| "нет аргумента функции".into()),
            Self::Local(name) => ctx
                .locals
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("локальная переменная {name} ещё не задана")),
            Self::Function(function) => Ok(Datum::Function(function.clone())),
            Self::Call(index, args) => {
                let Datum::Function(function) = ctx.get(*index)? else {
                    return Err("имя не является функцией".into());
                };
                if args.len() != function.params.len() || ctx.call_depth >= 32 {
                    return Err(
                        "неверное число аргументов или слишком много вложенных вызовов".into(),
                    );
                }
                let params = args
                    .iter()
                    .map(|arg| arg.evaluate(ctx))
                    .collect::<Calc<Vec<_>>>()?;
                function.body.evaluate(&Context {
                    params: &params,
                    locals: &[],
                    call_depth: ctx.call_depth + 1,
                    ..*ctx
                })
            }
            Self::Builtin(op, args) if args.len() == 1 => {
                builtin(op, &[args[0].evaluate(ctx)?], ctx)
            }
            Self::Builtin(op, args) if args.len() == 2 => {
                builtin(op, &[args[0].evaluate(ctx)?, args[1].evaluate(ctx)?], ctx)
            }
            Self::Builtin(op, args) => {
                let args = args
                    .iter()
                    .map(|arg| arg.evaluate(ctx))
                    .collect::<Calc<Vec<_>>>()?;
                builtin(op, &args, ctx)
            }
            Self::Conditional(test, yes, no) => {
                let branch = if test.evaluate(ctx)?.scalar()?.real_scalar()? != 0. {
                    yes
                } else {
                    no
                };
                branch.evaluate(ctx)
            }
            Self::Matrix(rows, cols, items) => {
                let values = items
                    .iter()
                    .map(|expr| expr.evaluate(ctx)?.scalar())
                    .collect::<Calc<_>>()?;
                Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                    *rows, *cols, values,
                )?)))
            }
            Self::Range(items) => {
                let values = items
                    .iter()
                    .map(|e| e.evaluate(ctx)?.scalar()?.real_scalar())
                    .collect::<Calc<Vec<_>>>()?;
                let span = match values.as_slice() {
                    [start, end] => Span::new(*start, None, *end)?,
                    [start, next, end] => Span::new(*start, Some(*next), *end)?,
                    _ => return Err("неверный синтаксис диапазона".into()),
                };
                Ok(Datum::Range(span))
            }
            Self::Index(array, indices, origin) => {
                let origin = origin.evaluate(ctx)?.scalar()?.real_scalar()?;
                if origin.fract() != 0. {
                    return Err("ORIGIN должен быть целым".into());
                }
                let Datum::Matrix(array) = array.evaluate(ctx)? else {
                    return Err("индексируется только массив".into());
                };
                let indices = indices
                    .iter()
                    .map(|e| index(e.evaluate(ctx)?.scalar()?, origin))
                    .collect::<Calc<Vec<_>>>()?;
                let (row, col) = as_indices(&array, &indices)?;
                Ok(array.get(row, col)?.into())
            }
            Self::Assign(assignment) => assignment.evaluate(ctx),
            Self::Program(stmts) => eval_program(stmts, ctx),
            Self::Symbolic(sym, bindings) => {
                let mut env = std::collections::HashMap::new();
                for (name, idx) in bindings {
                    if let Ok(datum) = ctx.get(*idx) {
                        match datum {
                            Datum::Scalar(v) if v.dimensionless() && v.imaginary == 0. => {
                                env.insert(name.clone(), crate::symbolic::SymExpr::num(v.number)?);
                            }
                            Datum::Symbolic(sr) => {
                                if let Some(expr) = &sr.expr {
                                    env.insert(name.clone(), expr.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let result = crate::symbolic::eval_symbolic(sym, &env)?;
                Ok(Datum::Symbolic(Arc::new(result)))
            }
            Self::SolveBlock(sb) => eval_solve_block(sb, ctx),
        }
    }
}

fn call_unary(function: &Function, x: Value, ctx: &Context<'_>) -> Calc<Value> {
    if function.params.len() != 1 || ctx.call_depth >= 32 {
        return Err("требуется функция одного аргумента".into());
    }
    function
        .body
        .evaluate(&Context {
            params: &[x.into()],
            locals: &[],
            call_depth: ctx.call_depth + 1,
            ..*ctx
        })?
        .scalar()
}

fn numeric_diff(function: &Function, x0: Value, ctx: &Context<'_>) -> Calc<Value> {
    if x0.imaginary != 0. {
        return Err("diff требует действительную точку дифференцирования".into());
    }
    let h = (x0.number.abs() * 1e-4).max(1e-5);
    let h_val = x0.with_number(h)?;
    let h2_val = x0.with_number(h * 0.5)?;
    let f_plus = call_unary(function, x0.with_number(x0.number + h)?, ctx)?;
    let f_minus = call_unary(function, x0.with_number(x0.number - h)?, ctx)?;
    let d1 = f_plus
        .binary("minus", f_minus)?
        .binary("div", h_val.multiply_number(2.)?)?;
    let f_plus2 = call_unary(function, x0.with_number(x0.number + h * 0.5)?, ctx)?;
    let f_minus2 = call_unary(function, x0.with_number(x0.number - h * 0.5)?, ctx)?;
    let d2 = f_plus2
        .binary("minus", f_minus2)?
        .binary("div", h2_val.multiply_number(2.)?)?;
    d2.multiply_number(4.)?
        .binary("minus", d1)?
        .multiply_number(1. / 3.)
}

fn numeric_integ(function: &Function, a: Value, b: Value, ctx: &Context<'_>) -> Calc<Value> {
    if a.imaginary != 0. || b.imaginary != 0. || !a.compatible(b) {
        return Err(
            "integ: пределы интегрирования должны быть действительными величинами одной размерности"
                .into(),
        );
    }
    if a.number == b.number {
        let fa = call_unary(function, a, ctx)?;
        return fa.binary("mult", b.binary("minus", a)?);
    }
    // Composite 4-point Gauss-Legendre quadrature over 32 subintervals (128 evaluations)
    const NODES: [f64; 4] = [
        -0.8611363115940526,
        -0.33998104358485626,
        0.33998104358485626,
        0.8611363115940526,
    ];
    const WEIGHTS: [f64; 4] = [
        0.34785484513745385,
        0.6521451548625461,
        0.6521451548625461,
        0.34785484513745385,
    ];
    const PANELS: usize = 32;
    ctx.spend(PANELS * NODES.len())?;
    let step = (b.number - a.number) / PANELS as f64;
    let half = 0.5 * step;
    let mut acc: Option<Value> = None;
    for panel in 0..PANELS {
        let mid = a.number + (panel as f64 + 0.5) * step;
        for (&node, &weight) in NODES.iter().zip(&WEIGHTS) {
            let x = a.with_number(mid + half * node)?;
            let fx = call_unary(function, x, ctx)?;
            let term = fx.binary("mult", a.with_number(half * weight)?)?;
            acc = Some(match acc {
                Some(sum) => sum.binary("plus", term)?,
                None => term,
            });
        }
    }
    acc.ok_or_else(|| "integ: пустой интервал".into())
}

fn indexed_fold(
    function: &Function,
    start: Value,
    end: Value,
    op: &str,
    ctx: &Context<'_>,
) -> Calc<Value> {
    let s = start.real_scalar()?;
    let e = end.real_scalar()?;
    if s.fract() != 0. || e.fract() != 0. || s > e {
        return Err("пределы суммы или произведения должны быть целыми числами (от <= до)".into());
    }
    let count = (e - s) + 1.;
    if !count.is_finite() || count > MAX_CELLS as f64 {
        return Err("слишком много шагов в сумме или произведении".into());
    }
    let count = count as usize;
    ctx.spend(count)?;
    let mut acc = call_unary(function, Value::scalar(s)?, ctx)?;
    for step in 1..count {
        let term = call_unary(function, Value::scalar(s + step as f64)?, ctx)?;
        acc = acc.binary(op, term)?;
    }
    Ok(acc)
}

fn solve_root_guess(function: &Function, guess: Value, ctx: &Context<'_>) -> Calc<Value> {
    if guess.imaginary != 0. {
        return Err("root: начальное приближение должно быть действительным".into());
    }
    let mut x = guess.number;
    let mut fx = call_unary(function, guess, ctx)?;
    if fx.imaginary != 0. {
        return Err("root: функция должна возвращать действительное значение".into());
    }
    let scale = fx.number.abs().max(1.);
    for _ in 0..80 {
        ctx.spend(4)?;
        if fx.number.abs() <= 1e-10 * scale {
            return guess.with_number(x);
        }
        let h = (x.abs() * 1e-6).max(1e-7);
        let fp = call_unary(function, guess.with_number(x + h)?, ctx)?;
        let fm = call_unary(function, guess.with_number(x - h)?, ctx)?;
        if fp.imaginary != 0. || fm.imaginary != 0. {
            return Err("root: функция вернула комплексное значение".into());
        }
        let df = (fp.number - fm.number) / (2. * h);
        if !df.is_finite() || df.abs() < 1e-15 {
            return Err("root: производная близка к нулю, уточните начальное приближение".into());
        }
        let step = fx.number / df;
        let mut factor = 1.0;
        let mut accepted = false;
        for _ in 0..8 {
            let next_x = x - factor * step;
            if next_x.is_finite() {
                if let Ok(next_fx) = call_unary(function, guess.with_number(next_x)?, ctx) {
                    if next_fx.imaginary == 0.
                        && (next_fx.number.abs() < fx.number.abs() || factor < 0.02)
                    {
                        x = next_x;
                        fx = next_fx;
                        accepted = true;
                        break;
                    }
                }
            }
            factor *= 0.5;
        }
        if !accepted {
            x -= step;
            fx = call_unary(function, guess.with_number(x)?, ctx)?;
        }
    }
    if fx.number.abs() <= 1e-7 * scale {
        guess.with_number(x)
    } else {
        Err("root: метод не сошёлся за 80 итераций (проверьте начальное приближение)".into())
    }
}

fn solve_root_bracket(function: &Function, a: Value, b: Value, ctx: &Context<'_>) -> Calc<Value> {
    if a.imaginary != 0. || b.imaginary != 0. || !a.compatible(b) {
        return Err(
            "root: границы интервала должны быть действительными величинами одной размерности"
                .into(),
        );
    }
    let mut left = a.number;
    let mut right = b.number;
    let mut fl = call_unary(function, a, ctx)?;
    let fr = call_unary(function, b, ctx)?;
    if fl.imaginary != 0. || fr.imaginary != 0. || !fl.compatible(fr) {
        return Err(
            "root: функция на границах должна быть действительной одной размерности".into(),
        );
    }
    if fl.number.abs() <= 1e-12 {
        return Ok(a);
    }
    if fr.number.abs() <= 1e-12 {
        return Ok(b);
    }
    if fl.number.signum() == fr.number.signum() {
        return Err("root: значения функции на концах интервала должны иметь разные знаки".into());
    }
    for _ in 0..100 {
        ctx.spend(2)?;
        let mid = 0.5 * (left + right);
        let fm = call_unary(function, a.with_number(mid)?, ctx)?;
        if fm.imaginary != 0. {
            return Err("root: функция вернула комплексное значение".into());
        }
        if fm.number.abs() <= 1e-12
            || (right - left).abs() <= 1e-12 * (1. + left.abs() + right.abs())
        {
            return a.with_number(mid);
        }
        if fl.number.signum() == fm.number.signum() {
            left = mid;
            fl = fm;
        } else {
            right = mid;
        }
    }
    a.with_number(0.5 * (left + right))
}

fn solve_polyroots(matrix: &Matrix) -> Calc<Datum> {
    if matrix.rows != 1 && matrix.cols != 1 {
        return Err("polyroots требует вектор коэффициентов [c0; c1; ...; cn]".into());
    }
    if matrix.values.iter().any(|v| !v.dimensionless()) {
        return Err("polyroots требует безразмерные коэффициенты".into());
    }
    let mut coeffs: Vec<Complex64> = matrix
        .values
        .iter()
        .map(|v| Complex64::new(v.number, v.imaginary))
        .collect();
    while coeffs.len() > 1 && coeffs.last().is_some_and(|c| c.norm() == 0.) {
        coeffs.pop();
    }
    let degree = coeffs.len().saturating_sub(1);
    if degree == 0 {
        return Err("polyroots: степень полинома должна быть не меньше 1".into());
    }
    if degree > 64 {
        return Err("polyroots поддерживает полиномы степени не выше 64".into());
    }
    let lead = coeffs[degree];
    let monic: Vec<Complex64> = coeffs.into_iter().map(|c| c / lead).collect();
    let radius = 1.0
        + monic[..degree]
            .iter()
            .map(|value| value.norm())
            .fold(0.0f64, f64::max);
    let seed = Complex64::new(0.4, 0.9);
    let mut roots: Vec<Complex64> = (0..degree)
        .map(|k| {
            let mut p = Complex64::new(radius, 0.);
            for _ in 0..k {
                p *= seed;
            }
            p
        })
        .collect();
    let eval_poly = |z: Complex64| -> Complex64 {
        let mut acc = monic[degree];
        for &c in monic[..degree].iter().rev() {
            acc = acc * z + c;
        }
        acc
    };
    let mut converged = false;
    for _ in 0..500 {
        let mut max_delta = 0.0f64;
        for i in 0..degree {
            let zi = roots[i];
            let mut denom = Complex64::new(1., 0.);
            for (j, &zj) in roots.iter().enumerate() {
                if i != j {
                    let diff = zi - zj;
                    if diff.norm() == 0. {
                        denom *= Complex64::new(1e-9, 1e-9);
                    } else {
                        denom *= diff;
                    }
                }
            }
            let delta = eval_poly(zi) / denom;
            roots[i] -= delta;
            max_delta = max_delta.max(delta.norm());
        }
        if max_delta <= 1e-13 {
            converged = true;
            break;
        }
    }
    if !converged {
        let worst = roots
            .iter()
            .map(|&r| eval_poly(r).norm())
            .fold(0.0f64, f64::max);
        if worst > 1e-6 {
            return Err("polyroots: метод не сошёлся к корням с требуемой точностью".into());
        }
    }
    for r in &mut roots {
        if r.im.abs() < 1e-11 * r.re.abs().max(1.) {
            r.im = 0.;
        }
        if r.re.abs() < 1e-13 && r.im == 0. {
            r.re = 0.;
        }
    }
    roots.sort_by(|a, b| a.re.total_cmp(&b.re).then(a.im.total_cmp(&b.im)));
    let values = roots
        .into_iter()
        .map(|z| Value::complex(z.re, z.im))
        .collect::<Calc<Vec<_>>>()?;
    Ok(Datum::Matrix(Arc::new(Matrix::from_values(
        degree, 1, values,
    )?)))
}

fn real_slice_same_dim(values: &[Value], name: &str) -> Calc<Value> {
    let first = *values
        .first()
        .ok_or_else(|| format!("{name}: пустой массив"))?;
    if values
        .iter()
        .any(|v| v.imaginary != 0. || !first.compatible(*v))
    {
        return Err(format!(
            "{name} требует действительные значения одной размерности"
        ));
    }
    Ok(first)
}

fn stat_extremum(values: &[Value], is_max: bool) -> Calc<Value> {
    let first = real_slice_same_dim(values, if is_max { "max" } else { "min" })?;
    let mut best = first.number;
    for v in &values[1..] {
        if is_max {
            best = best.max(v.number);
        } else {
            best = best.min(v.number);
        }
    }
    first.with_number(best)
}

fn stat_median(values: &[Value]) -> Calc<Value> {
    let first = real_slice_same_dim(values, "median")?;
    let mut nums: Vec<f64> = values.iter().map(|v| v.number).collect();
    nums.sort_by(f64::total_cmp);
    let n = nums.len();
    let med = if n % 2 == 1 {
        nums[n / 2]
    } else {
        0.5 * (nums[n / 2 - 1] + nums[n / 2])
    };
    first.with_number(med)
}

fn stat_variance(values: &[Value], sample: bool) -> Calc<Value> {
    let first = real_slice_same_dim(values, if sample { "stdev" } else { "var" })?;
    let n = values.len();
    if n == 0 || (sample && n < 2) {
        return Err("недостаточно элементов для вычисления разброса".into());
    }
    let mean = values.iter().map(|v| v.number).sum::<f64>() / n as f64;
    let denom = if sample { (n - 1) as f64 } else { n as f64 };
    let v = values
        .iter()
        .map(|x| (x.number - mean).powi(2))
        .sum::<f64>()
        / denom;
    let unit_sq = first.binary("pow", Value::scalar(2.)?)?;
    unit_sq.with_number(v)
}

fn matrix_norm(values: &[Value]) -> Calc<Value> {
    let first = *values.first().ok_or("norm: пустой массив")?;
    if values.iter().any(|v| !first.compatible(*v)) {
        return Err("norm требует элементы одной размерности".into());
    }
    let sum_sq: f64 = values.iter().map(|v| v.magnitude().powi(2)).sum();
    first.with_number(sum_sq.sqrt())
}

fn linear_interp(vx: &[Value], vy: &[Value], x: Value) -> Calc<Value> {
    if vx.len() < 2 || vx.len() != vy.len() {
        return Err("linterp требует два вектора одинаковой длины (не менее 2 точек)".into());
    }
    let x0 = real_slice_same_dim(vx, "linterp")?;
    let y0 = real_slice_same_dim(vy, "linterp")?;
    if x.imaginary != 0. || !x0.compatible(x) {
        return Err("linterp: размерность аргумента должна совпадать с вектором X".into());
    }
    for pair in vx.windows(2) {
        if pair[1].number <= pair[0].number {
            return Err("linterp: вектор X должен строго возрастать".into());
        }
    }
    let target = x.number;
    let seg = if target <= vx[0].number {
        0
    } else if target >= vx[vx.len() - 1].number {
        vx.len() - 2
    } else {
        let mut idx = 0;
        for i in 0..vx.len() - 1 {
            if target >= vx[i].number && target <= vx[i + 1].number {
                idx = i;
                break;
            }
        }
        idx
    };
    let t = (target - vx[seg].number) / (vx[seg + 1].number - vx[seg].number);
    let y = vy[seg].number + t * (vy[seg + 1].number - vy[seg].number);
    y0.with_number(y)
}

enum Flow {
    Next(Option<Datum>),
    Return(Datum),
    Break,
    Continue,
}

fn set_local(locals: &mut Vec<(String, Datum)>, name: &str, value: Datum) {
    if let Some((_, slot)) = locals.iter_mut().rev().find(|(n, _)| n == name) {
        *slot = value;
    } else {
        locals.push((name.to_owned(), value));
    }
}

fn eval_program(stmts: &[Stmt], ctx: &Context<'_>) -> Calc<Datum> {
    let mut locals = ctx.locals.to_vec();
    match eval_block(stmts, &mut locals, ctx)? {
        Flow::Return(val) | Flow::Next(Some(val)) => Ok(val),
        Flow::Next(None) | Flow::Break | Flow::Continue => {
            Err("программа не вернула значение".into())
        }
    }
}

fn eval_block(stmts: &[Stmt], locals: &mut Vec<(String, Datum)>, ctx: &Context<'_>) -> Calc<Flow> {
    let mut last = None;
    for stmt in stmts {
        ctx.spend(1)?;
        match stmt {
            Stmt::Expr(expr) => {
                let val = expr.evaluate(&Context { locals, ..*ctx })?;
                last = Some(val);
            }
            Stmt::LocalAssign(name, expr) => {
                let val = expr.evaluate(&Context { locals, ..*ctx })?;
                set_local(locals, name, val.clone());
                last = Some(val);
            }
            Stmt::LocalIndexedAssign {
                name,
                indices,
                origin,
                value,
            } => {
                let sub_ctx = Context { locals, ..*ctx };
                let orig = origin.evaluate(&sub_ctx)?.scalar()?.real_scalar()?;
                if orig.fract() != 0. {
                    return Err("ORIGIN должен быть целым".into());
                }
                let idxs = indices
                    .iter()
                    .map(|e| index(e.evaluate(&sub_ctx)?.scalar()?, orig))
                    .collect::<Calc<Vec<_>>>()?;
                let val = value.evaluate(&sub_ctx)?.scalar()?;
                let mut array = match locals
                    .iter()
                    .rev()
                    .find(|(n, _)| n == name)
                    .map(|(_, d)| d.clone())
                {
                    Some(Datum::Matrix(m)) => (*m).clone(),
                    None => Matrix {
                        rows: 0,
                        cols: 0,
                        values: Vec::new(),
                    },
                    _ => {
                        return Err(
                            "индексированное присваивание в программе требует массива".into()
                        )
                    }
                };
                let (row, col) = match idxs.as_slice() {
                    [r, c] => (*r, *c),
                    [i] if array.rows == 1 && array.cols > 1 => (0, *i),
                    [i] if array.cols <= 1 => (*i, 0),
                    [_] => return Err("у матрицы требуются два индекса".into()),
                    _ => return Err("у массива не более двух индексов".into()),
                };
                array.write(row, col, val)?;
                let updated = Datum::Matrix(Arc::new(array));
                set_local(locals, name, updated.clone());
                last = Some(updated);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let take_then = cond
                    .evaluate(&Context { locals, ..*ctx })?
                    .scalar()?
                    .real_scalar()?
                    != 0.;
                let branch = if take_then { then_branch } else { else_branch };
                match eval_block(branch, locals, ctx)? {
                    Flow::Next(Some(v)) => last = Some(v),
                    Flow::Next(None) => {}
                    flow => return Ok(flow),
                }
            }
            Stmt::For { var, iter, body } => {
                let iterable = iter.evaluate(&Context { locals, ..*ctx })?;
                let items: Vec<Datum> = match iterable {
                    Datum::Range(span) => span
                        .values()
                        .map(|v| v.map(Datum::from))
                        .collect::<Calc<_>>()?,
                    Datum::Matrix(m) => m.values.iter().copied().map(Datum::from).collect(),
                    Datum::Scalar(v) => vec![v.into()],
                    Datum::Function(_) | Datum::Symbolic(_) => {
                        return Err("for требует диапазон, вектор или скаляр".into())
                    }
                };
                for item in items {
                    ctx.spend(1)?;
                    set_local(locals, var, item);
                    match eval_block(body, locals, ctx)? {
                        Flow::Next(Some(v)) => last = Some(v),
                        Flow::Next(None) | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
            }
            Stmt::While { cond, body } => {
                while cond
                    .evaluate(&Context { locals, ..*ctx })?
                    .scalar()?
                    .real_scalar()?
                    != 0.
                {
                    ctx.spend(1)?;
                    match eval_block(body, locals, ctx)? {
                        Flow::Next(Some(v)) => last = Some(v),
                        Flow::Next(None) | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
            }
            Stmt::Return(expr) => {
                let val = expr.evaluate(&Context { locals, ..*ctx })?;
                return Ok(Flow::Return(val));
            }
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Continue => return Ok(Flow::Continue),
        }
    }
    Ok(Flow::Next(last))
}

fn call_binary(function: &Function, x: Datum, y: Datum, ctx: &Context<'_>) -> Calc<Datum> {
    if function.params.len() != 2 || ctx.call_depth >= 32 {
        return Err("odesolve требует функцию двух аргументов f(x, y)".into());
    }
    function.body.evaluate(&Context {
        params: &[x, y],
        locals: &[],
        call_depth: ctx.call_depth + 1,
        ..*ctx
    })
}

fn ode_add_scaled(y: &Datum, k: &Datum, scale: f64) -> Calc<Datum> {
    match (y, k) {
        (Datum::Scalar(yv), Datum::Scalar(kv)) => {
            Ok(yv.binary("plus", kv.multiply_number(scale)?)?.into())
        }
        (Datum::Matrix(ym), Datum::Matrix(km)) => {
            if ym.rows != km.rows || ym.cols != km.cols {
                return Err("odesolve: размерность f(x, y) должна совпадать с y".into());
            }
            let values = ym
                .values
                .iter()
                .zip(&km.values)
                .map(|(&yv, &kv)| yv.binary("plus", kv.multiply_number(scale)?))
                .collect::<Calc<_>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                ym.rows, ym.cols, values,
            )?)))
        }
        _ => Err("odesolve: тип f(x, y) должен совпадать с типом начального значения y0".into()),
    }
}

fn ode_rk4_step(
    function: &Function,
    x: Value,
    y: &Datum,
    h: f64,
    ctx: &Context<'_>,
) -> Calc<Datum> {
    let k1 = call_binary(function, x.into(), y.clone(), ctx)?;
    let x_mid = x.with_number(x.number + 0.5 * h)?;
    let y2 = ode_add_scaled(y, &k1, 0.5 * h)?;
    let k2 = call_binary(function, x_mid.into(), y2, ctx)?;
    let y3 = ode_add_scaled(y, &k2, 0.5 * h)?;
    let k3 = call_binary(function, x_mid.into(), y3, ctx)?;
    let x_end = x.with_number(x.number + h)?;
    let y4 = ode_add_scaled(y, &k3, h)?;
    let k4 = call_binary(function, x_end.into(), y4, ctx)?;

    let combined = ode_add_scaled(&k1, &k2, 2.0)?;
    let combined = ode_add_scaled(&combined, &k3, 2.0)?;
    let combined = ode_add_scaled(&combined, &k4, 1.0)?;
    ode_add_scaled(y, &combined, h / 6.0)
}

fn solve_ode(
    function: &Function,
    x0: Value,
    y0: &Datum,
    x1: Value,
    steps: usize,
    return_table: bool,
    ctx: &Context<'_>,
) -> Calc<Datum> {
    if x0.imaginary != 0. || x1.imaginary != 0. || !x0.compatible(x1) {
        return Err(
            "odesolve: границы x0 и x1 должны быть действительными одной размерности".into(),
        );
    }
    if steps == 0 || steps > 20_000 {
        return Err("odesolve: число шагов должно быть от 1 до 20000".into());
    }
    let h = (x1.number - x0.number) / steps as f64;
    let mut y = y0.clone();

    if !return_table {
        for step in 0..steps {
            ctx.spend(4)?;
            let x = x0.with_number(x0.number + step as f64 * h)?;
            y = ode_rk4_step(function, x, &y, h, ctx)?;
        }
        return Ok(y);
    }

    let y_cols = match &y {
        Datum::Scalar(_) => 1,
        Datum::Matrix(m) if m.rows == 1 || m.cols == 1 => m.values.len(),
        _ => return Err("odesolve_table требует скалярное или векторное y0".into()),
    };
    let cols = 1 + y_cols;
    let rows = steps + 1;
    if rows.checked_mul(cols).is_none_or(|n| n > MAX_CELLS) {
        return Err("odesolve_table: слишком большая результирующая таблица".into());
    }
    let mut table = Vec::with_capacity(rows * cols);
    let push_row = |table: &mut Vec<Value>, xv: Value, yv: &Datum| -> Calc<()> {
        table.push(xv);
        match yv {
            Datum::Scalar(v) => table.push(*v),
            Datum::Matrix(m) => table.extend_from_slice(&m.values),
            _ => return Err("odesolve_table: некорректное значение состояния".into()),
        }
        Ok(())
    };
    push_row(&mut table, x0, &y)?;
    for step in 0..steps {
        ctx.spend(4)?;
        let x = x0.with_number(x0.number + step as f64 * h)?;
        y = ode_rk4_step(function, x, &y, h, ctx)?;
        let x_next = x0.with_number(x0.number + (step + 1) as f64 * h)?;
        push_row(&mut table, x_next, &y)?;
    }
    Ok(Datum::Matrix(Arc::new(Matrix::from_values(
        rows, cols, table,
    )?)))
}

fn solve_linear_f64(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    if n == 0 || a.len() != n || a.iter().any(|row| row.len() != n) {
        return None;
    }
    for j in 0..n {
        let mut pivot = j;
        let mut best = a[j][j].abs();
        for i in (j + 1)..n {
            if a[i][j].abs() > best {
                best = a[i][j].abs();
                pivot = i;
            }
        }
        if !best.is_finite() || best < 1e-14 {
            return None;
        }
        if pivot != j {
            a.swap(j, pivot);
            b.swap(j, pivot);
        }
        let diag = a[j][j];
        for i in (j + 1)..n {
            let factor = a[i][j] / diag;
            if factor == 0.0 {
                continue;
            }
            for k in (j + 1)..n {
                a[i][k] -= factor * a[j][k];
            }
            b[i] -= factor * b[j];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in (i + 1)..n {
            s -= a[i][k] * x[k];
        }
        let v = s / a[i][i];
        if !v.is_finite() {
            return None;
        }
        x[i] = v;
    }
    Some(x)
}

fn datum_to_f64_vec(datum: &Datum, name: &str) -> Calc<(Vec<f64>, bool)> {
    match datum {
        Datum::Scalar(v) => Ok((vec![v.real_scalar()?], true)),
        Datum::Matrix(m) if m.rows == 1 || m.cols == 1 => {
            if m.values.is_empty() || m.values.len() > 32 {
                return Err(format!("{name}: поддерживается от 1 до 32 переменных"));
            }
            let vals = m
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            Ok((vals, false))
        }
        _ => Err(format!(
            "{name} требует скалярное или векторное приближение"
        )),
    }
}

fn f64_vec_to_datum(vals: &[f64], scalar_output: bool) -> Calc<Datum> {
    if scalar_output && vals.len() == 1 {
        return Ok(Value::scalar(vals[0])?.into());
    }
    let items = vals
        .iter()
        .map(|&v| Value::scalar(v))
        .collect::<Calc<Vec<_>>>()?;
    Ok(Datum::Matrix(Arc::new(Matrix::from_values(
        vals.len(),
        1,
        items,
    )?)))
}

fn call_vec_fn(function: &Function, x: &[f64], ctx: &Context<'_>) -> Calc<Datum> {
    if ctx.call_depth >= 32 {
        return Err("превышена глубина вложенных вызовов".into());
    }
    let params: Vec<Datum> = if function.params.len() == x.len() {
        x.iter()
            .map(|&v| Value::scalar(v).map(Datum::from))
            .collect::<Calc<_>>()?
    } else if function.params.len() == 1 {
        let vals = x
            .iter()
            .map(|&v| Value::scalar(v))
            .collect::<Calc<Vec<_>>>()?;
        vec![Datum::Matrix(Arc::new(Matrix::from_values(
            x.len(),
            1,
            vals,
        )?))]
    } else {
        return Err(format!(
            "функция принимает {} аргум., а вектор приближения имеет длину {}",
            function.params.len(),
            x.len()
        ));
    };
    function.body.evaluate(&Context {
        params: &params,
        locals: &[],
        call_depth: ctx.call_depth + 1,
        ..*ctx
    })
}

fn eval_residuals(
    eval_fn: &mut impl FnMut(&[f64]) -> Calc<Vec<f64>>,
    x: &[f64],
) -> Calc<(Vec<f64>, f64)> {
    let r = eval_fn(x)?;
    if r.is_empty() || r.iter().any(|v| !v.is_finite()) {
        return Err("решатель получил неконечное значение невязки".into());
    }
    let cost = 0.5 * r.iter().map(|v| v * v).sum::<f64>();
    Ok((r, cost))
}

/// Damped Levenberg-Marquardt solver for F(x) = 0 or nonlinear least squares min 0.5*||F(x)||^2.
fn solve_lm(
    mut eval_fn: impl FnMut(&[f64]) -> Calc<Vec<f64>>,
    x0: &[f64],
    require_exact: bool,
    ctx: &Context<'_>,
) -> Calc<Vec<f64>> {
    let n = x0.len();
    if n == 0 || n > 32 {
        return Err("решатель поддерживает от 1 до 32 неизвестных".into());
    }
    let mut x = x0.to_vec();
    let (mut r, mut cost) = eval_residuals(&mut eval_fn, &x)?;
    let m = r.len();
    let mut lambda = 1e-3f64;

    for _ in 0..120 {
        ctx.spend(n * 4 + m)?;
        let max_r = r.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
        if max_r <= 1e-10 {
            return Ok(x);
        }
        // Central-difference Jacobian (m x n)
        let mut jac = vec![vec![0.0; n]; m];
        for j in 0..n {
            let h = (x[j].abs() * 1e-6).max(1e-7);
            let mut xp = x.clone();
            let mut xm = x.clone();
            xp[j] += h;
            xm[j] -= h;
            let rp = eval_fn(&xp)?;
            let rm = eval_fn(&xm)?;
            if rp.len() != m || rm.len() != m {
                return Err("размерность вектора уравнений изменилась во время решения".into());
            }
            for i in 0..m {
                jac[i][j] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        // Normal equations: (J^T J + lambda * diag) dx = -J^T r
        let mut jtj = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for i in 0..m {
            for j in 0..n {
                g[j] += jac[i][j] * r[i];
                for k in j..n {
                    jtj[j][k] += jac[i][j] * jac[i][k];
                }
            }
        }
        for j in 0..n {
            for k in 0..j {
                jtj[j][k] = jtj[k][j];
            }
        }
        let grad_norm = g.iter().map(|v| v * v).sum::<f64>().sqrt();
        if grad_norm <= 1e-11 && (!require_exact || max_r <= 1e-7) {
            return Ok(x);
        }

        let mut accepted = false;
        for _ in 0..12 {
            let mut a = jtj.clone();
            for j in 0..n {
                let diag = a[j][j].abs().max(1.0);
                a[j][j] += lambda * diag;
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            if let Some(dx) = solve_linear_f64(a, rhs) {
                let step_norm = dx.iter().map(|v| v * v).sum::<f64>().sqrt();
                let x_try: Vec<f64> = x.iter().zip(&dx).map(|(a, b)| a + b).collect();
                if let Ok((r_try, cost_try)) = eval_residuals(&mut eval_fn, &x_try) {
                    if cost_try < cost {
                        x = x_try;
                        r = r_try;
                        let rel_improv = (cost - cost_try) / cost.max(1e-15);
                        cost = cost_try;
                        lambda = (lambda * 0.3).max(1e-12);
                        accepted = true;
                        if step_norm <= 1e-12 * (1.0 + x.iter().map(|v| v.abs()).sum::<f64>())
                            && rel_improv < 1e-12
                        {
                            let final_r = r.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
                            if !require_exact || final_r <= 1e-6 {
                                return Ok(x);
                            }
                        }
                        break;
                    }
                }
            }
            lambda = (lambda * 5.0).min(1e12);
        }
        if !accepted {
            break;
        }
    }

    let final_r = r.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
    if !require_exact || final_r <= 1e-6 {
        Ok(x)
    } else {
        Err(format!(
            "решатель не сошёлся (макс. невязка {final_r:.2e}); уточните начальное приближение или используйте minerr"
        ))
    }
}

fn clamp_box(x: &mut [f64], bounds: Option<(&[f64], &[f64])>) {
    if let Some((lo, hi)) = bounds {
        for i in 0..x.len() {
            x[i] = x[i].clamp(lo[i], hi[i]);
        }
    }
}

/// Damped BFGS optimizer with backtracking Armijo line search and optional box constraints.
fn solve_bfgs(
    mut eval_obj: impl FnMut(&[f64]) -> Calc<f64>,
    x0: &[f64],
    bounds: Option<(&[f64], &[f64])>,
    ctx: &Context<'_>,
) -> Calc<Vec<f64>> {
    let n = x0.len();
    if n == 0 || n > 32 {
        return Err("оптимизатор поддерживает от 1 до 32 переменных".into());
    }
    let mut x = x0.to_vec();
    clamp_box(&mut x, bounds);
    let mut fx = eval_obj(&x)?;
    if !fx.is_finite() {
        return Err("целевая функция не определена в начальной точке".into());
    }

    let grad = |pt: &[f64], f: &mut dyn FnMut(&[f64]) -> Calc<f64>| -> Calc<Vec<f64>> {
        let mut g = vec![0.0; n];
        for j in 0..n {
            let h = (pt[j].abs() * 1e-5).max(1e-6);
            let mut xp = pt.to_vec();
            let mut xm = pt.to_vec();
            xp[j] += h;
            xm[j] -= h;
            if let Some((lo, hi)) = bounds {
                xp[j] = xp[j].min(hi[j]);
                xm[j] = xm[j].max(lo[j]);
            }
            let span = xp[j] - xm[j];
            if span.abs() < 1e-14 {
                g[j] = 0.0;
            } else {
                g[j] = (f(&xp)? - f(&xm)?) / span;
            }
        }
        Ok(g)
    };

    let mut g = grad(&x, &mut eval_obj)?;
    // Inverse Hessian approximation H = I
    let mut h_inv = vec![vec![0.0; n]; n];
    for i in 0..n {
        h_inv[i][i] = 1.0;
    }

    for _ in 0..160 {
        ctx.spend(n * 4 + 4)?;
        // Projected gradient norm check
        let mut pg_norm_sq = 0.0;
        for i in 0..n {
            let mut gi = g[i];
            if let Some((lo, hi)) = bounds {
                if (x[i] <= lo[i] + 1e-12 && gi > 0.0) || (x[i] >= hi[i] - 1e-12 && gi < 0.0) {
                    gi = 0.0;
                }
            }
            pg_norm_sq += gi * gi;
        }
        if pg_norm_sq.sqrt() <= 1e-8 {
            return Ok(x);
        }

        // Direction d = -H * g
        let mut d = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                d[i] -= h_inv[i][j] * g[j];
            }
        }
        let mut dg: f64 = d.iter().zip(&g).map(|(a, b)| a * b).sum();
        if !dg.is_finite() || dg >= 0.0 {
            // Reset to steepest descent
            for i in 0..n {
                for j in 0..n {
                    h_inv[i][j] = if i == j { 1.0 } else { 0.0 };
                }
                d[i] = -g[i];
            }
            dg = -g.iter().map(|v| v * v).sum::<f64>();
        }

        // Backtracking line search
        let mut alpha = 1.0;
        let mut accepted = false;
        let mut x_next = x.clone();
        let mut f_next = fx;
        for _ in 0..24 {
            for i in 0..n {
                x_next[i] = x[i] + alpha * d[i];
            }
            clamp_box(&mut x_next, bounds);
            if let Ok(val) = eval_obj(&x_next) {
                if val.is_finite() && val <= fx + 1e-4 * alpha * dg.min(0.0) {
                    f_next = val;
                    accepted = true;
                    break;
                }
            }
            alpha *= 0.5;
        }
        if !accepted {
            break;
        }

        let s: Vec<f64> = x_next.iter().zip(&x).map(|(a, b)| a - b).collect();
        let s_norm = s.iter().map(|v| v * v).sum::<f64>().sqrt();
        x = x_next;
        fx = f_next;
        if s_norm <= 1e-11 * (1.0 + x.iter().map(|v| v.abs()).sum::<f64>()) {
            return Ok(x);
        }
        let g_next = grad(&x, &mut eval_obj)?;
        let y: Vec<f64> = g_next.iter().zip(&g).map(|(a, b)| a - b).collect();
        let sy: f64 = s.iter().zip(&y).map(|(a, b)| a * b).sum();
        if sy > 1e-12 {
            let rho = 1.0 / sy;
            // H <- (I - rho s y^T) H (I - rho y s^T) + rho s s^T
            let mut hy = vec![0.0; n];
            for i in 0..n {
                for j in 0..n {
                    hy[i] += h_inv[i][j] * y[j];
                }
            }
            let yhy: f64 = y.iter().zip(&hy).map(|(a, b)| a * b).sum();
            let factor = (1.0 + rho * yhy) * rho;
            for i in 0..n {
                for j in 0..n {
                    h_inv[i][j] += factor * s[i] * s[j] - rho * (s[i] * hy[j] + hy[i] * s[j]);
                }
            }
        }
        g = g_next;
    }
    Ok(x)
}

fn solve_system_builtin(
    function: &Function,
    guess: &Datum,
    require_exact: bool,
    ctx: &Context<'_>,
) -> Calc<Datum> {
    let name = if require_exact { "nsolve" } else { "minerr" };
    let (x0, scalar_out) = datum_to_f64_vec(guess, name)?;
    let sol = solve_lm(
        |x| {
            let res = call_vec_fn(function, x, ctx)?;
            match res {
                Datum::Scalar(v) => Ok(vec![v.real_scalar()?]),
                Datum::Matrix(m) => m
                    .values
                    .iter()
                    .map(|v| v.real_scalar())
                    .collect::<Calc<Vec<_>>>(),
                _ => Err(format!(
                    "{name}: функция должна возвращать скаляр или вектор невязок"
                )),
            }
        },
        &x0,
        require_exact,
        ctx,
    )?;
    f64_vec_to_datum(&sol, scalar_out)
}

fn optimize_builtin(
    function: &Function,
    guess: &Datum,
    bounds: Option<(&Datum, &Datum)>,
    maximize: bool,
    ctx: &Context<'_>,
) -> Calc<Datum> {
    let name = if maximize { "maximize" } else { "minimize" };
    let (x0, scalar_out) = datum_to_f64_vec(guess, name)?;
    let box_vecs = if let Some((lo_d, hi_d)) = bounds {
        let (lo, _) = datum_to_f64_vec(lo_d, name)?;
        let (hi, _) = datum_to_f64_vec(hi_d, name)?;
        if lo.len() != x0.len() || hi.len() != x0.len() {
            return Err(format!(
                "{name}: границы должны совпадать по длине с приближением"
            ));
        }
        for i in 0..lo.len() {
            if lo[i] > hi[i] {
                return Err(format!("{name}: нижняя граница больше верхней"));
            }
        }
        Some((lo, hi))
    } else {
        None
    };
    let bounds_ref = box_vecs
        .as_ref()
        .map(|(lo, hi)| (lo.as_slice(), hi.as_slice()));
    let sign = if maximize { -1.0 } else { 1.0 };
    let sol = solve_bfgs(
        |x| {
            let v = call_vec_fn(function, x, ctx)?.scalar()?.real_scalar()?;
            Ok(sign * v)
        },
        &x0,
        bounds_ref,
        ctx,
    )?;
    f64_vec_to_datum(&sol, scalar_out)
}

fn eval_solve_block(sb: &SolveBlockExpr, ctx: &Context<'_>) -> Calc<Datum> {
    if sb.vars.is_empty() {
        return Err("блок решения требует хотя бы одну искомую переменную".into());
    }
    let mut x0 = Vec::with_capacity(sb.vars.len());
    for (_, guess_idx) in &sb.vars {
        let v = ctx.get(*guess_idx)?.scalar()?.real_scalar()?;
        x0.push(v);
    }
    let scalar_out = x0.len() == 1;

    let eval_with_vars = |expr: &Expr, x: &[f64]| -> Calc<f64> {
        let mut locals = ctx.locals.to_vec();
        for ((name, _), &val) in sb.vars.iter().zip(x) {
            set_local(&mut locals, name, Value::scalar(val)?.into());
        }
        expr.evaluate(&Context {
            locals: &locals,
            ..*ctx
        })?
        .scalar()?
        .real_scalar()
    };

    match sb.mode {
        SolveMode::Find | SolveMode::Minerr => {
            if sb.constraints.is_empty() {
                return Err(
                    "блок Given … Find требует хотя бы одно уравнение или ограничение".into(),
                );
            }
            let sol = solve_lm(
                |x| {
                    let mut out = Vec::with_capacity(sb.constraints.len());
                    for (kind, expr) in &sb.constraints {
                        let v = eval_with_vars(expr, x)?;
                        let r = match kind {
                            ConstraintKind::Eq => v,
                            ConstraintKind::Le => v.max(0.0),
                            ConstraintKind::Ge => (-v).max(0.0),
                        };
                        out.push(r);
                    }
                    Ok(out)
                },
                &x0,
                sb.mode == SolveMode::Find,
                ctx,
            )?;
            f64_vec_to_datum(&sol, scalar_out)
        }
        SolveMode::Minimize | SolveMode::Maximize => {
            let obj = sb
                .objective
                .as_ref()
                .ok_or("Minimize / Maximize требует целевую функцию")?;
            let sign = if sb.mode == SolveMode::Maximize {
                -1.0
            } else {
                1.0
            };
            if sb.constraints.is_empty() {
                let sol = solve_bfgs(|x| Ok(sign * eval_with_vars(obj, x)?), &x0, None, ctx)?;
                return f64_vec_to_datum(&sol, scalar_out);
            }
            let violation = |kind: &ConstraintKind, expr: &Expr, pt: &[f64]| -> Calc<f64> {
                let value = eval_with_vars(expr, pt)?;
                Ok(match kind {
                    ConstraintKind::Eq => value.abs(),
                    ConstraintKind::Le => value.max(0.0),
                    ConstraintKind::Ge => (-value).max(0.0),
                })
            };
            const FEASIBILITY_TOLERANCE: f64 = 1e-6;
            let mut x = x0;
            for mu in [10.0, 1e3, 1e5, 1e7] {
                x = solve_bfgs(
                    |pt| {
                        let mut val = sign * eval_with_vars(obj, pt)?;
                        let mut pen = 0.0;
                        for (kind, expr) in &sb.constraints {
                            pen += violation(kind, expr, pt)?.powi(2);
                        }
                        val += mu * pen;
                        Ok(val)
                    },
                    &x,
                    None,
                    ctx,
                )?;
                let mut max_violation = 0.0_f64;
                for (kind, expr) in &sb.constraints {
                    max_violation = max_violation.max(violation(kind, expr, &x)?);
                }
                if max_violation <= FEASIBILITY_TOLERANCE {
                    return f64_vec_to_datum(&x, scalar_out);
                }
            }
            Err("Оптимизация не нашла точку, удовлетворяющую ограничениям".into())
        }
    }
}

fn builtin(op: &str, args: &[Datum], ctx: &Context<'_>) -> Calc<Datum> {
    match (op, args) {
        ("erf", [Datum::Scalar(x)]) => {
            Ok(Value::scalar(crate::engineering::erf_f64(x.real_scalar()?))?.into())
        }
        ("erfc", [Datum::Scalar(x)]) => {
            Ok(Value::scalar(crate::engineering::erfc_f64(x.real_scalar()?))?.into())
        }
        ("dnorm", [Datum::Scalar(x), Datum::Scalar(mu), Datum::Scalar(sigma)]) => {
            Ok(Value::scalar(crate::engineering::dnorm(
                x.real_scalar()?,
                mu.real_scalar()?,
                sigma.real_scalar()?,
            )?)?
            .into())
        }
        ("pnorm", [Datum::Scalar(x), Datum::Scalar(mu), Datum::Scalar(sigma)]) => {
            Ok(Value::scalar(crate::engineering::pnorm(
                x.real_scalar()?,
                mu.real_scalar()?,
                sigma.real_scalar()?,
            )?)?
            .into())
        }
        ("qnorm", [Datum::Scalar(p), Datum::Scalar(mu), Datum::Scalar(sigma)]) => {
            Ok(Value::scalar(crate::engineering::qnorm(
                p.real_scalar()?,
                mu.real_scalar()?,
                sigma.real_scalar()?,
            )?)?
            .into())
        }
        ("C" | "degC", [Datum::Scalar(t)]) => {
            Ok(crate::engineering::celsius_to_kelvin(t.real_scalar()?)?.into())
        }
        ("F" | "degF", [Datum::Scalar(t)]) => {
            Ok(crate::engineering::fahrenheit_to_kelvin(t.real_scalar()?)?.into())
        }
        ("toC", [Datum::Scalar(t)]) => Ok(crate::engineering::kelvin_to_celsius(*t)?.into()),
        ("toF", [Datum::Scalar(t)]) => Ok(crate::engineering::kelvin_to_fahrenheit(*t)?.into()),
        ("eigenvals", [Datum::Matrix(m)]) => {
            if m.rows != m.cols {
                return Err("eigenvals требует квадратную матрицу".into());
            }
            let nums = m
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let eigs = crate::engineering::eigenvals_real(m.rows, &nums)?;
            let vals = eigs
                .into_iter()
                .map(|z| Value::complex(z.re, z.im))
                .collect::<Calc<Vec<_>>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                m.rows, 1, vals,
            )?)))
        }
        ("qr", [Datum::Matrix(m)]) => {
            let nums = m
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let (q, r) = crate::engineering::qr_decompose(m.rows, m.cols, &nums)?;
            let q_vals = q.into_iter().map(Value::scalar).collect::<Calc<Vec<_>>>()?;
            let r_vals = r.into_iter().map(Value::scalar).collect::<Calc<Vec<_>>>()?;
            let qm = Matrix::from_values(m.rows, m.rows, q_vals)?;
            let rm = Matrix::from_values(m.rows, m.cols, r_vals)?;
            Ok(Datum::Matrix(Arc::new(qm.augment(&rm)?)))
        }
        ("rank", [Datum::Matrix(m)]) => {
            let nums = m
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let rk = crate::engineering::matrix_rank(m.rows, m.cols, &nums);
            Ok(Value::scalar(rk as f64)?.into())
        }
        ("fft" | "cfft", [Datum::Matrix(v)]) => {
            if v.rows != 1 && v.cols != 1 {
                return Err("fft требует вектор".into());
            }
            if v.values.iter().any(|x| !x.dimensionless()) {
                return Err("fft требует безразмерные числа".into());
            }
            let input: Vec<Complex64> = v
                .values
                .iter()
                .map(|x| Complex64::new(x.number, x.imaginary))
                .collect();
            let out = crate::engineering::fft_complex(&input, false)?;
            let vals = out
                .into_iter()
                .map(|z| Value::complex(z.re, z.im))
                .collect::<Calc<Vec<_>>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                vals.len(),
                1,
                vals,
            )?)))
        }
        ("ifft" | "icfft", [Datum::Matrix(v)]) => {
            if v.rows != 1 && v.cols != 1 {
                return Err("ifft требует вектор".into());
            }
            if v.values.iter().any(|x| !x.dimensionless()) {
                return Err("ifft требует безразмерные числа".into());
            }
            let input: Vec<Complex64> = v
                .values
                .iter()
                .map(|x| Complex64::new(x.number, x.imaginary))
                .collect();
            let out = crate::engineering::fft_complex(&input, true)?;
            let vals = out
                .into_iter()
                .map(|z| Value::complex(z.re, z.im))
                .collect::<Calc<Vec<_>>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                vals.len(),
                1,
                vals,
            )?)))
        }
        ("cspline", [Datum::Matrix(vx), Datum::Matrix(vy)]) => {
            let xs = vx
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let ys = vy
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let s = crate::engineering::cspline_second_derivs(&xs, &ys)?;
            let vals = s.into_iter().map(Value::scalar).collect::<Calc<Vec<_>>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                vals.len(),
                1,
                vals,
            )?)))
        }
        ("interp", [Datum::Matrix(vs), Datum::Matrix(vx), Datum::Matrix(vy), Datum::Scalar(x)]) => {
            let ss = vs
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let xs = vx
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let ys = vy
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let y = crate::engineering::eval_cspline(&ss, &xs, &ys, x.real_scalar()?)?;
            Ok(Value::scalar(y)?.into())
        }
        ("line", [Datum::Matrix(vx), Datum::Matrix(vy)]) => {
            let xs = vx
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let ys = vy
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let c = crate::engineering::poly_regress(&xs, &ys, 1)?;
            let vals = c.into_iter().map(Value::scalar).collect::<Calc<Vec<_>>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(2, 1, vals)?)))
        }
        ("regress", [Datum::Matrix(vx), Datum::Matrix(vy), Datum::Scalar(k)]) => {
            let xs = vx
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let ys = vy
                .values
                .iter()
                .map(|v| v.real_scalar())
                .collect::<Calc<Vec<_>>>()?;
            let deg = index(*k, 0.)?;
            let c = crate::engineering::poly_regress(&xs, &ys, deg)?;
            let vals = c.into_iter().map(Value::scalar).collect::<Calc<Vec<_>>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                vals.len(),
                1,
                vals,
            )?)))
        }
        ("min", [Datum::Matrix(a)]) => Ok(stat_extremum(&a.values, false)?.into()),
        ("max", [Datum::Matrix(a)]) => Ok(stat_extremum(&a.values, true)?.into()),
        ("min", [Datum::Scalar(a), Datum::Scalar(b)]) => {
            Ok(stat_extremum(&[*a, *b], false)?.into())
        }
        ("max", [Datum::Scalar(a), Datum::Scalar(b)]) => Ok(stat_extremum(&[*a, *b], true)?.into()),
        ("median", [Datum::Matrix(a)]) => Ok(stat_median(&a.values)?.into()),
        ("var", [Datum::Matrix(a)]) => Ok(stat_variance(&a.values, false)?.into()),
        ("stdev", [Datum::Matrix(a)]) => Ok(stat_variance(&a.values, true)?.unary("sqrt")?.into()),
        ("norm", [Datum::Matrix(a)]) => Ok(matrix_norm(&a.values)?.into()),
        ("linterp", [Datum::Matrix(vx), Datum::Matrix(vy), Datum::Scalar(x)]) => {
            Ok(linear_interp(&vx.values, &vy.values, *x)?.into())
        }
        ("col", [Datum::Matrix(m), Datum::Scalar(j)]) => {
            let col_idx = index(*j, 0.)?;
            Ok(Datum::Matrix(Arc::new(m.column(col_idx)?)))
        }
        ("row", [Datum::Matrix(m), Datum::Scalar(i)]) => {
            let row_idx = index(*i, 0.)?;
            Ok(Datum::Matrix(Arc::new(m.row_vec(row_idx)?)))
        }
        ("augment", [Datum::Matrix(a), Datum::Matrix(b)]) => {
            Ok(Datum::Matrix(Arc::new(a.augment(b)?)))
        }
        ("stack", [Datum::Matrix(a), Datum::Matrix(b)]) => Ok(Datum::Matrix(Arc::new(a.stack(b)?))),
        ("diff", [Datum::Function(f), Datum::Scalar(x0)]) => Ok(numeric_diff(f, *x0, ctx)?.into()),
        ("integ", [Datum::Function(f), Datum::Scalar(a), Datum::Scalar(b)]) => {
            Ok(numeric_integ(f, *a, *b, ctx)?.into())
        }
        ("isum", [Datum::Function(f), Datum::Scalar(a), Datum::Scalar(b)]) => {
            Ok(indexed_fold(f, *a, *b, "plus", ctx)?.into())
        }
        ("iprod", [Datum::Function(f), Datum::Scalar(a), Datum::Scalar(b)]) => {
            Ok(indexed_fold(f, *a, *b, "mult", ctx)?.into())
        }
        ("root", [Datum::Function(f), Datum::Scalar(guess)]) => {
            Ok(solve_root_guess(f, *guess, ctx)?.into())
        }
        ("root", [Datum::Function(f), Datum::Scalar(a), Datum::Scalar(b)]) => {
            Ok(solve_root_bracket(f, *a, *b, ctx)?.into())
        }
        ("polyroots", [Datum::Matrix(v)]) => solve_polyroots(v),
        ("nsolve" | "find", [Datum::Function(f), guess]) => {
            solve_system_builtin(f, guess, true, ctx)
        }
        ("minerr", [Datum::Function(f), guess]) => solve_system_builtin(f, guess, false, ctx),
        ("minimize", [Datum::Function(f), guess]) => optimize_builtin(f, guess, None, false, ctx),
        ("minimize", [Datum::Function(f), guess, lo, hi]) => {
            optimize_builtin(f, guess, Some((lo, hi)), false, ctx)
        }
        ("maximize", [Datum::Function(f), guess]) => optimize_builtin(f, guess, None, true, ctx),
        ("maximize", [Datum::Function(f), guess, lo, hi]) => {
            optimize_builtin(f, guess, Some((lo, hi)), true, ctx)
        }
        ("odesolve", [Datum::Function(f), Datum::Scalar(x0), y0, Datum::Scalar(x1)]) => {
            solve_ode(f, *x0, y0, *x1, 256, false, ctx)
        }
        (
            "odesolve",
            [Datum::Function(f), Datum::Scalar(x0), y0, Datum::Scalar(x1), Datum::Scalar(n)],
        ) => {
            let steps = index(*n, 0.)?;
            solve_ode(f, *x0, y0, *x1, steps, false, ctx)
        }
        ("odesolve_table", [Datum::Function(f), Datum::Scalar(x0), y0, Datum::Scalar(x1)]) => {
            solve_ode(f, *x0, y0, *x1, 100, true, ctx)
        }
        (
            "odesolve_table",
            [Datum::Function(f), Datum::Scalar(x0), y0, Datum::Scalar(x1), Datum::Scalar(n)],
        ) => {
            let steps = index(*n, 0.)?;
            solve_ode(f, *x0, y0, *x1, steps, true, ctx)
        }
        ("identity", [Datum::Scalar(n)]) => {
            let n = index(*n, 0.)?;
            Ok(Datum::Matrix(Arc::new(Matrix::identity(n)?)))
        }
        ("determinant" | "det", [Datum::Matrix(a)]) => a.eliminate(None),
        ("lsolve", [Datum::Matrix(a), Datum::Matrix(b)]) => a.eliminate(Some(b)),
        ("pow", [Datum::Matrix(a), Datum::Scalar(n)]) if n.real_scalar()? == -1. => {
            a.eliminate(Some(&Matrix::identity(a.rows)?))
        }
        ("tr", [Datum::Matrix(a)]) if a.rows == a.cols => {
            Ok(sum((0..a.rows).map(|i| a.get(i, i)))?.into())
        }
        ("atan2", [a, b]) => {
            Ok(Value::scalar(a.scalar()?.real_scalar()?.atan2(b.scalar()?.real_scalar()?))?.into())
        }
        ("rows" | "cols" | "length", [Datum::Matrix(a)]) => {
            let size = match op {
                "rows" => a.rows,
                "cols" => a.cols,
                _ => a.values.len(),
            };
            Ok(Value::scalar(size as f64)?.into())
        }
        ("transpose", [Datum::Matrix(a)]) => Ok(Datum::Matrix(Arc::new(a.transpose()))),
        ("sum" | "mean", [Datum::Matrix(a)]) => {
            let result = sum(a.values.iter().copied().map(Ok))?;
            Ok(if op == "mean" {
                result.multiply_number(1. / a.values.len() as f64)?
            } else {
                result
            }
            .into())
        }
        ("sum" | "mean", [Datum::Range(span)]) => {
            let result = sum(span.values())?;
            Ok(if op == "mean" {
                result.multiply_number(1. / span.count as f64)?
            } else {
                result
            }
            .into())
        }
        ("plus" | "minus", [Datum::Matrix(a), Datum::Matrix(b)]) => {
            Ok(Datum::Matrix(Arc::new(a.zip(b, op)?)))
        }
        ("mult" | "scale", [Datum::Matrix(a), Datum::Matrix(b)]) => a.multiply(b),
        ("mult" | "scale" | "div", [Datum::Matrix(a), Datum::Scalar(b)])
        | ("mult" | "scale", [Datum::Scalar(b), Datum::Matrix(a)]) => {
            let values = a
                .values
                .iter()
                .map(|&value| value.binary(op, *b))
                .collect::<Calc<_>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                a.rows, a.cols, values,
            )?)))
        }
        ("neg" | "minus", [Datum::Matrix(a)]) => {
            let values = a
                .values
                .iter()
                .map(|&value| value.unary("neg"))
                .collect::<Calc<_>>()?;
            Ok(Datum::Matrix(Arc::new(Matrix::from_values(
                a.rows, a.cols, values,
            )?)))
        }
        (_, [Datum::Scalar(a)]) => Ok(a.unary(op)?.into()),
        (_, [Datum::Scalar(a), Datum::Scalar(b)]) => Ok(a.binary(op, *b)?.into()),
        _ => Err(format!(
            "неподдерживаемая операция или тип аргументов: {op}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(rows: usize, cols: usize, entries: &[f64]) -> Arc<Matrix> {
        let values = entries.iter().map(|&x| Value::scalar(x).unwrap()).collect();
        Arc::new(Matrix::from_values(rows, cols, values).unwrap())
    }

    #[test]
    fn determinant_and_linear_solve_allow_row_swaps() {
        let a = matrix(2, 2, &[0., 2., 1., 3.]);
        let b = matrix(2, 1, &[8., 15.]);
        assert_eq!(a.eliminate(None).unwrap().scalar().unwrap().number, -2.);
        let solution = a.eliminate(Some(&b)).unwrap();
        assert!(solution.near(&Datum::Matrix(matrix(2, 1, &[3., 4.]))));
        assert!(matrix(2, 2, &[1., 2., 2., 4.]).eliminate(Some(&b)).is_err());
    }

    #[test]
    fn conditional_evaluates_only_the_selected_branch() {
        let v = |x| Expr::Literal(Value::scalar(x).unwrap());
        let expr = Expr::Conditional(
            Box::new(v(0.)),
            Box::new(Expr::Builtin("div".into(), vec![v(1.), v(0.)])),
            Box::new(v(4.)),
        );
        assert_eq!(expr.eval(&[]).unwrap().scalar().unwrap().number, 4.);
    }

    #[test]
    fn polynomial_diff_and_integ_preserve_values_and_dimensions() {
        // f(x) = x^2 + 3*x -> f'(2) = 7, integral_0^3 f(x) dx = 9 + 13.5 = 22.5
        let poly = Arc::new(Function {
            params: vec!["x".into()],
            body: Expr::Builtin(
                "plus".into(),
                vec![
                    Expr::Builtin(
                        "pow".into(),
                        vec![Expr::Param(0), Expr::Literal(Value::scalar(2.).unwrap())],
                    ),
                    Expr::Builtin(
                        "mult".into(),
                        vec![Expr::Literal(Value::scalar(3.).unwrap()), Expr::Param(0)],
                    ),
                ],
            ),
        });
        let deriv = Expr::Builtin(
            "diff".into(),
            vec![
                Expr::Function(poly.clone()),
                Expr::Literal(Value::scalar(2.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert!((deriv.number - 7.0).abs() < 1e-8);

        let area = Expr::Builtin(
            "integ".into(),
            vec![
                Expr::Function(poly),
                Expr::Literal(Value::scalar(0.).unwrap()),
                Expr::Literal(Value::scalar(3.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert!((area.number - 22.5).abs() < 1e-10);

        // Dimensional test: v(t) = 2 * t -> integ(v, 0 s, 3 s) = 9 s^2, incompatible units fail
        let lin = Arc::new(Function {
            params: vec!["t".into()],
            body: Expr::Builtin(
                "mult".into(),
                vec![Expr::Literal(Value::scalar(2.).unwrap()), Expr::Param(0)],
            ),
        });
        let sec = Value::constant("s").unwrap();
        let meter = Value::constant("m").unwrap();
        let dim_int = Expr::Builtin(
            "integ".into(),
            vec![
                Expr::Function(lin.clone()),
                Expr::Literal(sec.multiply_number(0.).unwrap()),
                Expr::Literal(sec.multiply_number(3.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        let s2 = sec.binary("pow", Value::scalar(2.).unwrap()).unwrap();
        assert!(dim_int.compatible(s2));
        assert!((dim_int.number - 9.0).abs() < 1e-10);

        let bad_units = Expr::Builtin(
            "integ".into(),
            vec![
                Expr::Function(lin),
                Expr::Literal(sec),
                Expr::Literal(meter),
            ],
        )
        .eval(&[]);
        assert!(bad_units.is_err());
    }

    #[test]
    fn indexed_sum_and_product_work() {
        let id_fn = Arc::new(Function {
            params: vec!["k".into()],
            body: Expr::Param(0),
        });
        let s = Expr::Builtin(
            "isum".into(),
            vec![
                Expr::Function(id_fn.clone()),
                Expr::Literal(Value::scalar(1.).unwrap()),
                Expr::Literal(Value::scalar(5.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert_eq!(s.number, 15.0);

        let p = Expr::Builtin(
            "iprod".into(),
            vec![
                Expr::Function(id_fn),
                Expr::Literal(Value::scalar(1.).unwrap()),
                Expr::Literal(Value::scalar(5.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert_eq!(p.number, 120.0);
    }

    #[test]
    fn root_and_polyroots_solve_equations_and_report_nonconvergence() {
        // f(x) = x^2 - 2
        let f = Arc::new(Function {
            params: vec!["x".into()],
            body: Expr::Builtin(
                "minus".into(),
                vec![
                    Expr::Builtin(
                        "pow".into(),
                        vec![Expr::Param(0), Expr::Literal(Value::scalar(2.).unwrap())],
                    ),
                    Expr::Literal(Value::scalar(2.).unwrap()),
                ],
            ),
        });
        let r1 = Expr::Builtin(
            "root".into(),
            vec![
                Expr::Function(f.clone()),
                Expr::Literal(Value::scalar(1.5).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert!((r1.number - std::f64::consts::SQRT_2).abs() < 1e-9);

        let r2 = Expr::Builtin(
            "root".into(),
            vec![
                Expr::Function(f),
                Expr::Literal(Value::scalar(0.).unwrap()),
                Expr::Literal(Value::scalar(2.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert!((r2.number - std::f64::consts::SQRT_2).abs() < 1e-9);

        // No real root: x^2 + 1 = 0
        let no_root = Arc::new(Function {
            params: vec!["x".into()],
            body: Expr::Builtin(
                "plus".into(),
                vec![
                    Expr::Builtin(
                        "pow".into(),
                        vec![Expr::Param(0), Expr::Literal(Value::scalar(2.).unwrap())],
                    ),
                    Expr::Literal(Value::scalar(1.).unwrap()),
                ],
            ),
        });
        assert!(Expr::Builtin(
            "root".into(),
            vec![
                Expr::Function(no_root),
                Expr::Literal(Value::scalar(1.).unwrap())
            ],
        )
        .eval(&[])
        .is_err());

        // polyroots([1; 0; 1]) -> -i, +i (x^2 + 1 = 0)
        let coeffs = matrix(3, 1, &[1., 0., 1.]);
        let Datum::Matrix(roots) = solve_polyroots(&coeffs).unwrap() else {
            panic!("expected matrix of roots");
        };
        assert_eq!(roots.rows, 2);
        assert!((roots.values[0].number).abs() < 1e-9);
        assert!((roots.values[0].imaginary + 1.0).abs() < 1e-9);
        assert!((roots.values[1].imaginary - 1.0).abs() < 1e-9);
    }

    #[test]
    fn statistics_norm_and_linterp_work() {
        let v = matrix(4, 1, &[1., 2., 3., 4.]);
        let min_v = builtin_eval(
            "min",
            vec![
                Expr::Literal(Value::scalar(5.).unwrap()),
                Expr::Literal(Value::scalar(2.).unwrap()),
            ],
        );
        assert_eq!(min_v, 2.0);
        assert_eq!(stat_extremum(&v.values, true).unwrap().number, 4.0);
        assert_eq!(stat_median(&v.values).unwrap().number, 2.5);
        assert!((stat_variance(&v.values, false).unwrap().number - 1.25).abs() < 1e-12);
        let std = stat_variance(&v.values, true)
            .unwrap()
            .unary("sqrt")
            .unwrap()
            .number;
        assert!((std - (5.0_f64 / 3.0).sqrt()).abs() < 1e-12);
        let vec34 = matrix(2, 1, &[3., 4.]);
        assert!((matrix_norm(&vec34.values).unwrap().number - 5.0).abs() < 1e-12);

        let vx = matrix(3, 1, &[0., 2., 4.]);
        let vy = matrix(3, 1, &[0., 10., 30.]);
        assert!(
            (linear_interp(&vx.values, &vy.values, Value::scalar(1.).unwrap())
                .unwrap()
                .number
                - 5.0)
                .abs()
                < 1e-12
        );
        assert!(
            (linear_interp(&vx.values, &vy.values, Value::scalar(3.).unwrap())
                .unwrap()
                .number
                - 20.0)
                .abs()
                < 1e-12
        );
    }

    fn builtin_eval(op: &str, args: Vec<Expr>) -> f64 {
        Expr::Builtin(op.into(), args)
            .eval(&[])
            .unwrap()
            .scalar()
            .unwrap()
            .number
    }

    #[test]
    fn col_row_augment_stack_and_csv_export_work() {
        let m = matrix(2, 3, &[1., 2., 3., 4., 5., 6.]);
        let c1 = m.column(1).unwrap();
        assert_eq!(c1.rows, 2);
        assert_eq!(c1.cols, 1);
        assert_eq!(c1.values[0].number, 2.0);
        assert_eq!(c1.values[1].number, 5.0);

        let r0 = m.row_vec(0).unwrap();
        assert_eq!(r0.rows, 1);
        assert_eq!(r0.cols, 3);
        assert_eq!(r0.values[2].number, 3.0);

        let aug = c1.augment(&m.column(2).unwrap()).unwrap();
        assert_eq!((aug.rows, aug.cols), (2, 2));
        let csv = Datum::Matrix(Arc::new(aug)).to_csv(',', 6).unwrap();
        assert_eq!(csv, "2,3\n5,6\n");

        let st = r0.stack(&m.row_vec(1).unwrap()).unwrap();
        assert_eq!((st.rows, st.cols), (2, 3));
    }

    #[test]
    fn program_piecewise_accumulation_and_odesolve_match_analytics() {
        // Program: s <- 0; for k in 1..5 { if k > 2 { s <- s + k } else { s <- s + 1 } }; return s
        // Expected: (1 + 1) + (3 + 4 + 5) = 14
        let prog = Expr::Program(vec![
            Stmt::LocalAssign("s".into(), Expr::Literal(Value::scalar(0.).unwrap())),
            Stmt::For {
                var: "k".into(),
                iter: Expr::Range(vec![
                    Expr::Literal(Value::scalar(1.).unwrap()),
                    Expr::Literal(Value::scalar(5.).unwrap()),
                ]),
                body: vec![Stmt::If {
                    cond: Expr::Builtin(
                        "greaterThan".into(),
                        vec![
                            Expr::Local("k".into()),
                            Expr::Literal(Value::scalar(2.).unwrap()),
                        ],
                    ),
                    then_branch: vec![Stmt::LocalAssign(
                        "s".into(),
                        Expr::Builtin(
                            "plus".into(),
                            vec![Expr::Local("s".into()), Expr::Local("k".into())],
                        ),
                    )],
                    else_branch: vec![Stmt::LocalAssign(
                        "s".into(),
                        Expr::Builtin(
                            "plus".into(),
                            vec![
                                Expr::Local("s".into()),
                                Expr::Literal(Value::scalar(1.).unwrap()),
                            ],
                        ),
                    )],
                }],
            },
            Stmt::Return(Expr::Local("s".into())),
        ]);
        assert_eq!(prog.eval(&[]).unwrap().scalar().unwrap().number, 14.0);

        // Cauchy ODE: y'(x) = y, y(0) = 1 -> y(1) = e
        let exp_rhs = Arc::new(Function {
            params: vec!["x".into(), "y".into()],
            body: Expr::Param(1),
        });
        let y1 = Expr::Builtin(
            "odesolve".into(),
            vec![
                Expr::Function(exp_rhs.clone()),
                Expr::Literal(Value::scalar(0.).unwrap()),
                Expr::Literal(Value::scalar(1.).unwrap()),
                Expr::Literal(Value::scalar(1.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap()
        .scalar()
        .unwrap();
        assert!((y1.number - std::f64::consts::E).abs() < 1e-8);

        // Table output: 10 steps -> 11x2 matrix
        let Datum::Matrix(tbl) = Expr::Builtin(
            "odesolve_table".into(),
            vec![
                Expr::Function(exp_rhs),
                Expr::Literal(Value::scalar(0.).unwrap()),
                Expr::Literal(Value::scalar(1.).unwrap()),
                Expr::Literal(Value::scalar(1.).unwrap()),
                Expr::Literal(Value::scalar(10.).unwrap()),
            ],
        )
        .eval(&[])
        .unwrap() else {
            panic!("expected matrix table");
        };
        assert_eq!((tbl.rows, tbl.cols), (11, 2));
        assert!((tbl.get(10, 1).unwrap().number - std::f64::consts::E).abs() < 1e-5);
    }

    #[test]
    fn nsolve_minerr_minimize_and_maximize_work() {
        // System: x^2 + y^2 - 25 = 0, x - y - 1 = 0 -> (4, 3) from guess [3; 2]
        let sys = Arc::new(Function {
            params: vec!["x".into(), "y".into()],
            body: Expr::Matrix(
                2,
                1,
                vec![
                    Expr::Builtin(
                        "minus".into(),
                        vec![
                            Expr::Builtin(
                                "plus".into(),
                                vec![
                                    Expr::Builtin(
                                        "pow".into(),
                                        vec![
                                            Expr::Param(0),
                                            Expr::Literal(Value::scalar(2.).unwrap()),
                                        ],
                                    ),
                                    Expr::Builtin(
                                        "pow".into(),
                                        vec![
                                            Expr::Param(1),
                                            Expr::Literal(Value::scalar(2.).unwrap()),
                                        ],
                                    ),
                                ],
                            ),
                            Expr::Literal(Value::scalar(25.).unwrap()),
                        ],
                    ),
                    Expr::Builtin(
                        "minus".into(),
                        vec![
                            Expr::Builtin("minus".into(), vec![Expr::Param(0), Expr::Param(1)]),
                            Expr::Literal(Value::scalar(1.).unwrap()),
                        ],
                    ),
                ],
            ),
        });
        let guess = Expr::Matrix(
            2,
            1,
            vec![
                Expr::Literal(Value::scalar(3.).unwrap()),
                Expr::Literal(Value::scalar(2.).unwrap()),
            ],
        );
        let Datum::Matrix(sol) = Expr::Builtin("nsolve".into(), vec![Expr::Function(sys), guess])
            .eval(&[])
            .unwrap()
        else {
            panic!("expected matrix solution");
        };
        assert!((sol.get(0, 0).unwrap().number - 4.0).abs() < 1e-7);
        assert!((sol.get(1, 0).unwrap().number - 3.0).abs() < 1e-7);

        // Minimize f(x, y) = (x - 2)^2 + (y + 1)^2 -> (2, -1)
        let obj = Arc::new(Function {
            params: vec!["x".into(), "y".into()],
            body: Expr::Builtin(
                "plus".into(),
                vec![
                    Expr::Builtin(
                        "pow".into(),
                        vec![
                            Expr::Builtin(
                                "minus".into(),
                                vec![Expr::Param(0), Expr::Literal(Value::scalar(2.).unwrap())],
                            ),
                            Expr::Literal(Value::scalar(2.).unwrap()),
                        ],
                    ),
                    Expr::Builtin(
                        "pow".into(),
                        vec![
                            Expr::Builtin(
                                "plus".into(),
                                vec![Expr::Param(1), Expr::Literal(Value::scalar(1.).unwrap())],
                            ),
                            Expr::Literal(Value::scalar(2.).unwrap()),
                        ],
                    ),
                ],
            ),
        });
        let g2 = Expr::Matrix(
            2,
            1,
            vec![
                Expr::Literal(Value::scalar(0.).unwrap()),
                Expr::Literal(Value::scalar(0.).unwrap()),
            ],
        );
        let Datum::Matrix(opt) = Expr::Builtin("minimize".into(), vec![Expr::Function(obj), g2])
            .eval(&[])
            .unwrap()
        else {
            panic!("expected matrix optimum");
        };
        assert!((opt.get(0, 0).unwrap().number - 2.0).abs() < 1e-6);
        assert!((opt.get(1, 0).unwrap().number + 1.0).abs() < 1e-6);
    }
}
