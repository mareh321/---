use crate::sheet::Sheet;
use crate::value::{Calc, Value};
use std::{fmt::Write as _, fs::OpenOptions, io::Write, path::Path};

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn real_vector(values: Vec<Value>) -> Calc<Vec<f64>> {
    let first = values.first().ok_or("пустой вектор")?;
    if values
        .iter()
        .any(|value| value.imaginary != 0. || !first.compatible(*value))
    {
        return Err(
            "координаты оси должны быть действительными величинами одной размерности".into(),
        );
    }
    Ok(values.into_iter().map(|value| value.number).collect())
}

pub fn write(sheet: &Sheet, x: &str, y: Option<&str>, path: &Path) -> Calc<()> {
    let (x_label, values) = sheet.plot_series(x)?;
    let (title, x_axis, y_axis, x, y, equal) = if let Some(y) = y {
        let (y_label, data) = sheet.plot_series(y)?;
        (
            format!("{y_label} от {x_label}"),
            escape(x_label),
            escape(y_label),
            real_vector(values)?,
            real_vector(data)?,
            "false",
        )
    } else {
        let real = values.iter().map(|v| v.number).collect();
        let imaginary = values.iter().map(|v| v.imaginary).collect();
        let first = values.first().ok_or("пустой вектор")?;
        if values.iter().any(|value| !first.compatible(*value)) {
            return Err("все комплексные точки должны иметь одну размерность".into());
        }
        (
            format!("{x_label} на комплексной плоскости"),
            "Re".into(),
            "Im".into(),
            real,
            imaginary,
            "true",
        )
    };
    if x.is_empty() || x.len() != y.len() {
        return Err("векторы X и Y должны иметь одинаковую ненулевую длину".into());
    }
    let mut points = String::from("[");
    for (i, (&x, &y)) in x.iter().zip(&y).enumerate() {
        if i > 0 {
            points.push(',');
        }
        write!(points, "[{x:e},{y:e}]").map_err(|e| e.to_string())?;
    }
    points.push(']');
    let template = include_str!("plot.html");
    let (before, after) = template
        .split_once("/*POINT_DATA*/[]")
        .ok_or("нет шаблона графика")?;
    let (setup, end) = after
        .split_once("/*EQUAL_SCALE*/false")
        .ok_or("нет настроек графика")?;
    let before = before
        .replace("__X_AXIS__", &x_axis)
        .replace("__Y_AXIS__", &y_axis)
        .replace("__TITLE__", &escape(&title));
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    for part in [before.as_str(), &points, setup, equal, end] {
        out.write_all(part.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
