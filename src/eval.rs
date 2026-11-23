use crate::value::{display_number, Calc, Value};
use std::{cell::Cell, fmt, sync::Arc};

pub const MAX_CELLS: usize = 100_000;
pub type Cache = [Option<Calc<Datum>>];

#[derive(Clone, Debug, PartialEq)]
pub enum Datum {
    Scalar(Value),
    Matrix(Arc<Matrix>),
    Range(Span),
    Function(Arc<Function>),
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

    pub fn details(&self) -> String {
        let Self::Matrix(matrix) = self else {
            return self.to_string();
        };
        let mut lines = Vec::with_capacity(matrix.rows);
        for row in matrix.values.chunks_exact(matrix.cols) {
            lines.push(
                row.iter()
                    .map(ToString::to_string)
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
                budget: &Cell::new(16_000),
                call_depth: 0,
            })?
            .scalar()
    }
}

#[derive(Debug, PartialEq)]
pub enum Expr {
    Literal(Value),
    Ref(usize),
    Param(usize),
    Builtin(String, Vec<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Function(Arc<Function>),
    Call(usize, Vec<Expr>),
    Matrix(usize, usize, Vec<Expr>),
    Range(Vec<Expr>),
    Index(Box<Expr>, Vec<Expr>, Box<Expr>),
    Assign(Box<IndexedAssignment>),
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
    pub fn eval(&self, cache: &Cache) -> Calc<Datum> {
        self.evaluate(&Context {
            cache,
            params: &[],
            bindings: &[],
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
                    call_depth: ctx.call_depth + 1,
                    ..*ctx
                })
            }
            Self::Builtin(op, args) if args.len() == 1 => builtin(op, &[args[0].evaluate(ctx)?]),
            Self::Builtin(op, args) if args.len() == 2 => {
                builtin(op, &[args[0].evaluate(ctx)?, args[1].evaluate(ctx)?])
            }
            Self::Builtin(op, args) => {
                let args = args
                    .iter()
                    .map(|arg| arg.evaluate(ctx))
                    .collect::<Calc<Vec<_>>>()?;
                builtin(op, &args)
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
        }
    }
}

fn builtin(op: &str, args: &[Datum]) -> Calc<Datum> {
    match (op, args) {
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
}
