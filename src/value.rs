use num_complex::Complex64;
use std::fmt;

pub type Calc<T> = Result<T, String>;
const ZERO: [i16; 7] = [0; 7];
const BASE: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

pub fn display_number(number: f64) -> String {
    display_number_precision(number, 12)
}

pub fn display_number_precision(number: f64, digits: usize) -> String {
    let digits = digits.clamp(1, 15);
    let frac = digits - 1;
    let number = if number == 0. { 0. } else { number };
    let scientific = format!("{number:.frac$e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("формат e содержит показатель степени");
    let exponent: i32 = exponent.parse().expect("показатель степени — целое число");
    let exp_hi = (digits as i32).max(4);
    if (-4..exp_hi).contains(&exponent) {
        let precision = (frac as i32 - exponent).max(0) as usize;
        let decimal = format!("{number:.precision$}");
        if precision > 0 {
            return decimal.trim_end_matches('0').trim_end_matches('.').into();
        }
        return decimal;
    }
    format!(
        "{}e{exponent}",
        mantissa.trim_end_matches('0').trim_end_matches('.')
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value {
    pub number: f64,
    pub imaginary: f64,
    dimensions: [i16; 7],
}

impl Value {
    pub fn scalar(number: f64) -> Calc<Self> {
        Self::new(number, ZERO)
    }

    fn new(number: f64, dimensions: [i16; 7]) -> Calc<Self> {
        Self::from_complex(Complex64::new(number, 0.), dimensions)
    }

    pub fn complex(real: f64, imaginary: f64) -> Calc<Self> {
        Self::from_complex(Complex64::new(real, imaginary), ZERO)
    }

    fn from_complex(z: Complex64, dimensions: [i16; 7]) -> Calc<Self> {
        if !z.re.is_finite() || !z.im.is_finite() {
            return Err("результат не является конечным числом".into());
        }
        Ok(Self {
            number: z.re,
            imaginary: z.im,
            dimensions,
        })
    }

    fn z(self) -> Complex64 {
        Complex64::new(self.number, self.imaginary)
    }

    pub fn real_scalar(self) -> Calc<f64> {
        if self.dimensions == ZERO && self.imaginary == 0. {
            return Ok(self.number);
        }
        Err("требуется действительное безразмерное число".into())
    }

    pub fn dimensionless(self) -> bool {
        self.dimensions == ZERO
    }

    pub fn magnitude(self) -> f64 {
        self.number.hypot(self.imaginary)
    }

    pub fn constant(name: &str) -> Option<Self> {
        if name == "i" {
            return Self::complex(0., 1.).ok();
        }
        let (number, dimensions) = match name {
            "m" => (1., [1, 0, 0, 0, 0, 0, 0]),
            "nm" => (1e-9, [1, 0, 0, 0, 0, 0, 0]),
            "um" | "µm" => (1e-6, [1, 0, 0, 0, 0, 0, 0]),
            "mm" => (0.001, [1, 0, 0, 0, 0, 0, 0]),
            "cm" => (0.01, [1, 0, 0, 0, 0, 0, 0]),
            "dm" => (0.1, [1, 0, 0, 0, 0, 0, 0]),
            "km" => (1000., [1, 0, 0, 0, 0, 0, 0]),
            "in" => (0.0254, [1, 0, 0, 0, 0, 0, 0]),
            "ft" => (0.3048, [1, 0, 0, 0, 0, 0, 0]),
            "yd" => (0.9144, [1, 0, 0, 0, 0, 0, 0]),
            "mi" => (1609.344, [1, 0, 0, 0, 0, 0, 0]),
            "mg" => (1e-6, [0, 1, 0, 0, 0, 0, 0]),
            "g" => (0.001, [0, 1, 0, 0, 0, 0, 0]),
            "kg" => (1., [0, 1, 0, 0, 0, 0, 0]),
            "tonne" => (1000., [0, 1, 0, 0, 0, 0, 0]),
            "lb" | "lbs" => (0.45359237, [0, 1, 0, 0, 0, 0, 0]),
            "ns" => (1e-9, [0, 0, 1, 0, 0, 0, 0]),
            "us" | "µs" => (1e-6, [0, 0, 1, 0, 0, 0, 0]),
            "ms" => (0.001, [0, 0, 1, 0, 0, 0, 0]),
            "s" => (1., [0, 0, 1, 0, 0, 0, 0]),
            "min" => (60., [0, 0, 1, 0, 0, 0, 0]),
            "hr" => (3600., [0, 0, 1, 0, 0, 0, 0]),
            "day" => (86400., [0, 0, 1, 0, 0, 0, 0]),
            "mA" => (0.001, [0, 0, 0, 1, 0, 0, 0]),
            "A" => (1., [0, 0, 0, 1, 0, 0, 0]),
            "kA" => (1000., [0, 0, 0, 1, 0, 0, 0]),
            "K" | "degC" | "°C" => (1., [0, 0, 0, 0, 1, 0, 0]),
            "degF" | "°F" => (5. / 9., [0, 0, 0, 0, 1, 0, 0]),
            "mol" => (1., [0, 0, 0, 0, 0, 1, 0]),
            "cd" => (1., [0, 0, 0, 0, 0, 0, 1]),
            "mN" => (0.001, [1, 1, -2, 0, 0, 0, 0]),
            "N" => (1., [1, 1, -2, 0, 0, 0, 0]),
            "kN" => (1000., [1, 1, -2, 0, 0, 0, 0]),
            "MN" => (1e6, [1, 1, -2, 0, 0, 0, 0]),
            "Pa" => (1., [-1, 1, -2, 0, 0, 0, 0]),
            "kPa" => (1000., [-1, 1, -2, 0, 0, 0, 0]),
            "MPa" => (1e6, [-1, 1, -2, 0, 0, 0, 0]),
            "GPa" => (1e9, [-1, 1, -2, 0, 0, 0, 0]),
            "bar" => (1e5, [-1, 1, -2, 0, 0, 0, 0]),
            "atm" => (101_325., [-1, 1, -2, 0, 0, 0, 0]),
            "psi" => (6894.757293168, [-1, 1, -2, 0, 0, 0, 0]),
            "mmHg" => (133.322387415, [-1, 1, -2, 0, 0, 0, 0]),
            "mJ" => (0.001, [2, 1, -2, 0, 0, 0, 0]),
            "J" => (1., [2, 1, -2, 0, 0, 0, 0]),
            "kJ" => (1000., [2, 1, -2, 0, 0, 0, 0]),
            "MJ" => (1e6, [2, 1, -2, 0, 0, 0, 0]),
            "Wh" => (3600., [2, 1, -2, 0, 0, 0, 0]),
            "kWh" => (3.6e6, [2, 1, -2, 0, 0, 0, 0]),
            "cal" => (4.184, [2, 1, -2, 0, 0, 0, 0]),
            "kcal" => (4184., [2, 1, -2, 0, 0, 0, 0]),
            "mW" => (0.001, [2, 1, -3, 0, 0, 0, 0]),
            "W" => (1., [2, 1, -3, 0, 0, 0, 0]),
            "kW" => (1000., [2, 1, -3, 0, 0, 0, 0]),
            "MW" => (1e6, [2, 1, -3, 0, 0, 0, 0]),
            "hp" => (745.6998715822702, [2, 1, -3, 0, 0, 0, 0]),
            "Hz" => (1., [0, 0, -1, 0, 0, 0, 0]),
            "kHz" => (1000., [0, 0, -1, 0, 0, 0, 0]),
            "MHz" => (1e6, [0, 0, -1, 0, 0, 0, 0]),
            "GHz" => (1e9, [0, 0, -1, 0, 0, 0, 0]),
            "rpm" => (std::f64::consts::TAU / 60., [0, 0, -1, 0, 0, 0, 0]),
            "mV" => (0.001, [2, 1, -3, -1, 0, 0, 0]),
            "V" => (1., [2, 1, -3, -1, 0, 0, 0]),
            "kV" => (1000., [2, 1, -3, -1, 0, 0, 0]),
            "Ω" | "ohm" => (1., [2, 1, -3, -2, 0, 0, 0]),
            "kΩ" => (1000., [2, 1, -3, -2, 0, 0, 0]),
            "MΩ" => (1e6, [2, 1, -3, -2, 0, 0, 0]),
            "mL" => (1e-6, [3, 0, 0, 0, 0, 0, 0]),
            "L" => (0.001, [3, 0, 0, 0, 0, 0, 0]),
            "rad" => (1., ZERO),
            "deg" => (std::f64::consts::PI / 180., ZERO),
            "π" => (std::f64::consts::PI, ZERO),
            "e" => (std::f64::consts::E, ZERO),
            _ => return None,
        };
        Self::new(number, dimensions).ok()
    }

    pub fn compatible(self, other: Self) -> bool {
        self.dimensions == other.dimensions
    }

    pub fn multiply_number(self, number: f64) -> Calc<Self> {
        Self::from_complex(self.z() * number, self.dimensions)
    }

    pub fn with_number(self, number: f64) -> Calc<Self> {
        Self::new(number, self.dimensions)
    }

    pub fn in_units(self, unit: Self) -> Calc<Self> {
        if !self.compatible(unit) || unit.number == 0. || unit.imaginary != 0. {
            return Err("несовместимая или нулевая единица результата".into());
        }
        Self::from_complex(self.z() / unit.number, ZERO)
    }

    pub fn logarithm(self, base: Self) -> Calc<Self> {
        if !self.dimensionless() {
            return Err("log требует безразмерный аргумент".into());
        }
        if self.number == 0. && self.imaginary == 0. {
            return Err("логарифм нуля не определён".into());
        }
        let base = base.real_scalar()?;
        if base <= 0. || base == 1. {
            return Err("log: основание должно быть положительным и отличным от единицы".into());
        }
        Self::from_complex(self.z().ln() / base.ln(), ZERO)
    }

    pub fn round_decimal(self, places: Self) -> Calc<Self> {
        if !self.dimensionless() {
            return Err("round требует безразмерный аргумент".into());
        }
        let requested_places = places.real_scalar()?;
        let places = requested_places.round();
        // Mathcad accepts near-integer precision values from numerical expressions.
        if (requested_places - places).abs() > 1e-11 * places.abs().max(1.) {
            return Err("round: число десятичных знаков должно быть целым".into());
        }
        if places <= -308. {
            return Self::scalar(0.);
        }
        let scale = 10_f64.powf(places.abs());
        let round_part = |number: f64| {
            if places >= 0. {
                let scaled = number * scale;
                if scaled.abs() >= (1_u64 << 52) as f64 || !scale.is_finite() {
                    return number;
                }
                scaled.round() / scale
            } else {
                (number / scale).round() * scale
            }
        };
        Self::complex(round_part(self.number), round_part(self.imaginary))
    }

    pub fn binary(self, op: &str, other: Self) -> Calc<Self> {
        match op {
            "plus" | "minus" => {
                if !self.compatible(other) {
                    return Err("несовместимые размерности при сложении или вычитании".into());
                }
                let z = if op == "plus" {
                    self.z() + other.z()
                } else {
                    self.z() - other.z()
                };
                Self::from_complex(z, self.dimensions)
            }
            "mult" | "scale" | "div" => {
                let divide = op == "div";
                if divide && other.number == 0. && other.imaginary == 0. {
                    return Err("деление на ноль".into());
                }
                let mut dimensions = ZERO;
                for (out, (left, right)) in dimensions
                    .iter_mut()
                    .zip(self.dimensions.into_iter().zip(other.dimensions))
                {
                    *out = if divide {
                        left.checked_sub(right)
                    } else {
                        left.checked_add(right)
                    }
                    .ok_or("слишком большая степень единицы")?;
                }
                let number = if divide && other.imaginary == 0. {
                    self.z() / other.number
                } else if divide {
                    let scale = other.number.abs().max(other.imaginary.abs());
                    let divisor = other.z() / scale;
                    (self.z() / scale) / divisor
                } else {
                    self.z() * other.z()
                };
                Self::from_complex(number, dimensions)
            }
            "pow" => {
                if other.dimensions != ZERO || (self.dimensions != ZERO && other.imaginary != 0.) {
                    return Err("показатель степени должен быть безразмерным".into());
                }
                let mut dimensions = ZERO;
                for (out, original) in dimensions.iter_mut().zip(self.dimensions) {
                    let exponent = f64::from(original) * other.number;
                    if exponent.fract() != 0. || exponent.abs() > f64::from(i16::MAX) {
                        return Err(
                            "результат требует дробных или слишком больших степеней единиц".into(),
                        );
                    }
                    *out = exponent as i16;
                }
                let z = if self.imaginary == 0.
                    && other.imaginary == 0.
                    && (self.number >= 0. || other.number.fract() == 0.)
                {
                    Complex64::new(self.number.powf(other.number), 0.)
                } else if other.imaginary == 0.
                    && other.number.fract() == 0.
                    && other.number.abs() <= f64::from(i32::MAX)
                {
                    self.z().powi(other.number as i32)
                } else {
                    self.z().powc(other.z())
                };
                Self::from_complex(z, dimensions)
            }
            "equal" | "notEqual" => {
                if !self.compatible(other) {
                    return Err("несовместимые размерности при сравнении".into());
                }
                Self::scalar(f64::from((self.z() == other.z()) == (op == "equal")))
            }
            "lessThan" | "lessOrEqual" | "greaterThan" | "greaterOrEqual" => {
                if !self.compatible(other) || self.imaginary != 0. || other.imaginary != 0. {
                    return Err("сравнение требует действительных величин одной размерности".into());
                }
                let matched = match op {
                    "lessThan" => self.number < other.number,
                    "lessOrEqual" => self.number <= other.number,
                    "greaterThan" => self.number > other.number,
                    _ => self.number >= other.number,
                };
                Self::scalar(f64::from(matched))
            }
            "and" | "or" | "xor" => {
                let left = self.real_scalar()? != 0.;
                let right = other.real_scalar()? != 0.;
                let matched = match op {
                    "and" => left && right,
                    "or" => left || right,
                    _ => left ^ right,
                };
                Self::scalar(f64::from(matched))
            }
            "mod" => {
                let (value, divisor) = (self.real_scalar()?, other.real_scalar()?);
                if divisor == 0. {
                    return Err("деление на ноль".into());
                }
                Self::scalar(value - divisor * (value / divisor).floor())
            }
            _ => Err(format!("неподдерживаемая операция {op}")),
        }
    }

    pub fn unary(self, op: &str) -> Calc<Self> {
        match op {
            "neg" | "minus" => return self.multiply_number(-1.),
            "abs" => return Self::new(self.z().norm(), self.dimensions),
            "Re" => return Self::new(self.number, self.dimensions),
            "Im" => return Self::new(self.imaginary, self.dimensions),
            "conj" => return Self::from_complex(self.z().conj(), self.dimensions),
            "arg" => {
                if self.number == 0. && self.imaginary == 0. {
                    return Err("аргумент нуля не определён".into());
                }
                return Self::scalar(self.imaginary.atan2(self.number));
            }
            "sqrt" => return self.binary("pow", Self::scalar(0.5)?),
            _ if self.dimensions != ZERO => {
                return Err(format!("{op} требует безразмерный аргумент"))
            }
            _ => {}
        }
        let z = self.z();
        let number = match op {
            "sin" => z.sin(),
            "cos" => z.cos(),
            "tan" => z.tan(),
            "asin" => z.asin(),
            "acos" => z.acos(),
            "atan" => z.atan(),
            "acot" => std::f64::consts::FRAC_PI_2 - z.atan(),
            "sinh" => z.sinh(),
            "cosh" => z.cosh(),
            "tanh" => z.tanh(),
            "exp" => z.exp(),
            "ln" => z.ln(),
            "log" => return self.logarithm(Self::scalar(10.)?),
            "floor" => return Self::scalar(self.real_scalar()?.floor()),
            "ceil" => return Self::scalar(self.real_scalar()?.ceil()),
            "round" => return self.round_decimal(Self::scalar(0.)?),
            "not" => return Self::scalar(f64::from(self.real_scalar()? == 0.)),
            "factorial" => {
                let n = self.real_scalar()?;
                if !(0. ..=170.).contains(&n) || n.fract() != 0. {
                    return Err("факториал требует целое число от 0 до 170".into());
                }
                return Self::scalar((1..=n as u32).map(f64::from).product());
            }
            _ => return Err(format!("неподдерживаемая функция {op}")),
        };
        Self::from_complex(number, ZERO)
    }
}

impl Value {
    pub fn format_precision(&self, digits: usize) -> String {
        let mut out = if self.imaginary == 0. {
            display_number_precision(self.number, digits)
        } else if self.number == 0. {
            format!("{}i", display_number_precision(self.imaginary, digits))
        } else {
            let sign = if self.imaginary.is_sign_negative() {
                '-'
            } else {
                '+'
            };
            format!(
                "{} {sign} {}i",
                display_number_precision(self.number, digits),
                display_number_precision(self.imaginary.abs(), digits)
            )
        };
        for (unit, exponent) in BASE.into_iter().zip(self.dimensions) {
            match exponent {
                0 => {}
                1 => {
                    out.push(' ');
                    out.push_str(unit);
                }
                _ => {
                    out.push(' ');
                    out.push_str(unit);
                    out.push('^');
                    out.push_str(&exponent.to_string());
                }
            }
        }
        out
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_precision(12))
    }
}
