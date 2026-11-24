use crate::value::{Calc, Value};
use num_complex::Complex64;
use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

/// Error function erf(x) via Abramowitz & Stegun 7.1.26 (max error < 1.5e-7).
pub fn erf_f64(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let poly = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
        + 0.254829592)
        * t;
    sign * (1.0 - poly * (-ax * ax).exp())
}

pub fn erfc_f64(x: f64) -> f64 {
    1.0 - erf_f64(x)
}

pub fn dnorm(x: f64, mu: f64, sigma: f64) -> Calc<f64> {
    if sigma <= 0.0 || !sigma.is_finite() {
        return Err("dnorm: стандартное отклонение sigma должно быть > 0".into());
    }
    let z = (x - mu) / sigma;
    Ok((-0.5 * z * z).exp() / (sigma * (2.0 * PI).sqrt()))
}

pub fn pnorm(x: f64, mu: f64, sigma: f64) -> Calc<f64> {
    if sigma <= 0.0 || !sigma.is_finite() {
        return Err("pnorm: стандартное отклонение sigma должно быть > 0".into());
    }
    let z = (x - mu) / (sigma * SQRT_2);
    Ok(0.5 * (1.0 + erf_f64(z)))
}

/// Inverse standard normal CDF (Peter Acklam's rational approximation, accuracy ~1e-9).
pub fn qnorm(p: f64, mu: f64, sigma: f64) -> Calc<f64> {
    if !(0.0 < p && p < 1.0) {
        return Err("qnorm: вероятность p должна лежать строго в интервале (0, 1)".into());
    }
    if sigma <= 0.0 || !sigma.is_finite() {
        return Err("qnorm: стандартное отклонение sigma должно быть > 0".into());
    }
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    let z = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // One Halley/Newton refinement step for high precision
    let err = 0.5 * erfc_f64(-z * FRAC_1_SQRT_2) - p;
    let z = z - err * (2.0 * PI).sqrt() * (0.5 * z * z).exp();
    Ok(mu + sigma * z)
}

/// Cooley-Tukey radix-2 FFT (power-of-two) or direct DFT for general N <= 4096.
pub fn fft_complex(input: &[Complex64], inverse: bool) -> Calc<Vec<Complex64>> {
    let n = input.len();
    if n == 0 || n > 4096 {
        return Err("fft/ifft поддерживает векторы длиной от 1 до 4096".into());
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut out = if n.is_power_of_two() {
        let mut a = input.to_vec();
        // Bit-reversal permutation
        let mut j = 0usize;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                a.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let ang = sign * 2.0 * PI / (len as f64);
            let wlen = Complex64::new(ang.cos(), ang.sin());
            let mut i = 0;
            while i < n {
                let mut w = Complex64::new(1.0, 0.0);
                for k in 0..(len / 2) {
                    let u = a[i + k];
                    let v = a[i + k + len / 2] * w;
                    a[i + k] = u + v;
                    a[i + k + len / 2] = u - v;
                    w *= wlen;
                }
                i += len;
            }
            len <<= 1;
        }
        a
    } else {
        let mut a = vec![Complex64::new(0.0, 0.0); n];
        for (k, slot) in a.iter_mut().enumerate() {
            let mut acc = Complex64::new(0.0, 0.0);
            for (m, &x) in input.iter().enumerate() {
                let ang = sign * 2.0 * PI * (k * m) as f64 / (n as f64);
                acc += x * Complex64::new(ang.cos(), ang.sin());
            }
            *slot = acc;
        }
        a
    };
    if inverse {
        let inv_n = 1.0 / (n as f64);
        for z in &mut out {
            *z *= inv_n;
        }
    }
    for z in &mut out {
        if z.im.abs() < 1e-12 * z.re.abs().max(1.0) {
            z.im = 0.0;
        }
        if z.re.abs() < 1e-12 * z.im.abs().max(1.0) {
            z.re = 0.0;
        }
    }
    Ok(out)
}

/// Householder QR decomposition for m x n real matrix (m >= n). Returns (Q (m x m), R (m x n)).
pub fn qr_decompose(rows: usize, cols: usize, a_in: &[f64]) -> Calc<(Vec<f64>, Vec<f64>)> {
    if rows == 0 || cols == 0 || rows < cols || rows > 64 || cols > 64 {
        return Err("qr требует матрицу m×n (m >= n, до 64×64)".into());
    }
    let mut r = a_in.to_vec();
    let mut q = vec![0.0; rows * rows];
    for i in 0..rows {
        q[i * rows + i] = 1.0;
    }
    for k in 0..cols.min(rows.saturating_sub(1)) {
        let mut norm_sq = 0.0;
        for i in k..rows {
            norm_sq += r[i * cols + k].powi(2);
        }
        let norm = norm_sq.sqrt();
        if norm < 1e-15 {
            continue;
        }
        let alpha = if r[k * cols + k] >= 0.0 { -norm } else { norm };
        let mut v = vec![0.0; rows - k];
        v[0] = r[k * cols + k] - alpha;
        for i in (k + 1)..rows {
            v[i - k] = r[i * cols + k];
        }
        let v_norm_sq: f64 = v.iter().map(|x| x * x).sum();
        if v_norm_sq < 1e-30 {
            continue;
        }
        let beta = 2.0 / v_norm_sq;
        // Apply H to R from left
        for j in k..cols {
            let mut dot = 0.0;
            for i in k..rows {
                dot += v[i - k] * r[i * cols + j];
            }
            for i in k..rows {
                r[i * cols + j] -= beta * dot * v[i - k];
            }
        }
        // Apply H to Q from right: Q <- Q * H
        for i in 0..rows {
            let mut dot = 0.0;
            for j in k..rows {
                dot += q[i * rows + j] * v[j - k];
            }
            for j in k..rows {
                q[i * rows + j] -= beta * dot * v[j - k];
            }
        }
    }
    for val in &mut r {
        if val.abs() < 1e-13 {
            *val = 0.0;
        }
    }
    Ok((q, r))
}

/// Matrix rank via Gaussian elimination with partial pivoting.
pub fn matrix_rank(rows: usize, cols: usize, a_in: &[f64]) -> usize {
    let mut a = a_in.to_vec();
    let max_val = a.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
    let tol = 1e-11 * max_val.max(1.0);
    let mut rank = 0;
    let mut row = 0;
    for col in 0..cols {
        if row >= rows {
            break;
        }
        let mut pivot = row;
        let mut best = a[row * cols + col].abs();
        for r in (row + 1)..rows {
            if a[r * cols + col].abs() > best {
                best = a[r * cols + col].abs();
                pivot = r;
            }
        }
        if best <= tol {
            continue;
        }
        if pivot != row {
            for c in col..cols {
                a.swap(row * cols + c, pivot * cols + c);
            }
        }
        let diag = a[row * cols + col];
        for r in (row + 1)..rows {
            let factor = a[r * cols + col] / diag;
            for c in (col + 1)..cols {
                a[r * cols + c] -= factor * a[row * cols + c];
            }
        }
        rank += 1;
        row += 1;
    }
    rank
}

/// Eigenvalues of a real square matrix (up to 32x32) via shifted QR iteration + 2x2 Schur blocks.
pub fn eigenvals_real(n: usize, a_in: &[f64]) -> Calc<Vec<Complex64>> {
    if n == 0 || n > 32 {
        return Err("eigenvals поддерживает квадратные матрицы от 1×1 до 32×32".into());
    }
    let mut h = a_in.to_vec();
    // 1. Reduce to upper Hessenberg via Householder reflections
    for k in 0..n.saturating_sub(2) {
        let m = n - k - 1;
        let mut x = vec![0.0; m];
        for i in 0..m {
            x[i] = h[(k + 1 + i) * n + k];
        }
        let norm = x.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm < 1e-15 {
            continue;
        }
        let alpha = if x[0] >= 0.0 { -norm } else { norm };
        x[0] -= alpha;
        let v_sq: f64 = x.iter().map(|v| v * v).sum();
        if v_sq < 1e-30 {
            continue;
        }
        let beta = 2.0 / v_sq;
        // Left multiply
        for j in k..n {
            let mut dot = 0.0;
            for i in 0..m {
                dot += x[i] * h[(k + 1 + i) * n + j];
            }
            for i in 0..m {
                h[(k + 1 + i) * n + j] -= beta * dot * x[i];
            }
        }
        // Right multiply
        for i in 0..n {
            let mut dot = 0.0;
            for j in 0..m {
                dot += h[i * n + (k + 1 + j)] * x[j];
            }
            for j in 0..m {
                h[i * n + (k + 1 + j)] -= beta * dot * x[j];
            }
        }
    }

    // 2. Shifted QR iteration on active submatrix [0..m]
    let mut eigs = Vec::with_capacity(n);
    let mut m = n;
    let mut iter = 0usize;
    let max_iter = 400 * n;
    while m > 0 {
        if m == 1 {
            eigs.push(Complex64::new(h[0], 0.0));
            break;
        }
        iter += 1;
        if iter > max_iter {
            // Extract remaining diagonal/2x2 blocks
            let mut i = 0;
            while i < m {
                if i + 1 < m && h[(i + 1) * n + i].abs() > 1e-9 {
                    let (e1, e2) = eig_2x2(
                        h[i * n + i],
                        h[i * n + i + 1],
                        h[(i + 1) * n + i],
                        h[(i + 1) * n + i + 1],
                    );
                    eigs.push(e1);
                    eigs.push(e2);
                    i += 2;
                } else {
                    eigs.push(Complex64::new(h[i * n + i], 0.0));
                    i += 1;
                }
            }
            break;
        }

        let scale = h[(m - 1) * n + (m - 1)]
            .abs()
            .max(h[(m - 2) * n + (m - 2)].abs())
            .max(1.0);
        if h[(m - 1) * n + (m - 2)].abs() <= 1e-12 * scale {
            eigs.push(Complex64::new(h[(m - 1) * n + (m - 1)], 0.0));
            m -= 1;
            continue;
        }
        if m == 2
            || h[(m - 2) * n + (m - 3)].abs()
                <= 1e-12
                    * (h[(m - 2) * n + (m - 2)].abs() + h[(m - 3) * n + (m - 3)].abs()).max(1.0)
        {
            let (e1, e2) = eig_2x2(
                h[(m - 2) * n + (m - 2)],
                h[(m - 2) * n + (m - 1)],
                h[(m - 1) * n + (m - 2)],
                h[(m - 1) * n + (m - 1)],
            );
            if e1.im.abs() > 1e-12 || m == 2 {
                eigs.push(e1);
                eigs.push(e2);
                m -= 2;
                continue;
            }
        }

        // Wilkinson shift from bottom-right 2x2
        let a = h[(m - 2) * n + (m - 2)];
        let b = h[(m - 2) * n + (m - 1)];
        let c = h[(m - 1) * n + (m - 2)];
        let d = h[(m - 1) * n + (m - 1)];
        let tr = a + d;
        let det = a * d - b * c;
        let disc = tr * tr - 4.0 * det;
        let mu = if disc >= 0.0 {
            let s = disc.sqrt();
            let l1 = 0.5 * (tr + s);
            let l2 = 0.5 * (tr - s);
            if (l1 - d).abs() < (l2 - d).abs() {
                l1
            } else {
                l2
            }
        } else if iter % 15 == 0 {
            // Exceptional shift to break symmetry
            d + 0.75 * (c.abs() + b.abs())
        } else {
            d
        };

        // Shifted QR step on top-left m x m via Givens rotations
        for i in 0..m {
            h[i * n + i] -= mu;
        }
        let mut cs = Vec::with_capacity(m - 1);
        let mut sn = Vec::with_capacity(m - 1);
        for i in 0..(m - 1) {
            let x = h[i * n + i];
            let y = h[(i + 1) * n + i];
            let r = x.hypot(y);
            let (c_g, s_g) = if r < 1e-15 {
                (1.0, 0.0)
            } else {
                (x / r, y / r)
            };
            cs.push(c_g);
            sn.push(s_g);
            for j in i..m {
                let t1 = c_g * h[i * n + j] + s_g * h[(i + 1) * n + j];
                let t2 = -s_g * h[i * n + j] + c_g * h[(i + 1) * n + j];
                h[i * n + j] = t1;
                h[(i + 1) * n + j] = t2;
            }
        }
        for i in 0..(m - 1) {
            let c_g = cs[i];
            let s_g = sn[i];
            let row_lim = (i + 2).min(m);
            for r in 0..row_lim {
                let t1 = c_g * h[r * n + i] + s_g * h[r * n + (i + 1)];
                let t2 = -s_g * h[r * n + i] + c_g * h[r * n + (i + 1)];
                h[r * n + i] = t1;
                h[r * n + (i + 1)] = t2;
            }
        }
        for i in 0..m {
            h[i * n + i] += mu;
        }
    }

    for z in &mut eigs {
        if z.im.abs() < 1e-11 * z.re.abs().max(1.0) {
            z.im = 0.0;
        }
        if z.re.abs() < 1e-12 && z.im == 0.0 {
            z.re = 0.0;
        }
    }
    eigs.sort_by(|a, b| b.re.total_cmp(&a.re).then(a.im.total_cmp(&b.im)));
    Ok(eigs)
}

fn eig_2x2(a: f64, b: f64, c: f64, d: f64) -> (Complex64, Complex64) {
    let tr = a + d;
    let det = a * d - b * c;
    let disc = tr * tr - 4.0 * det;
    if disc >= 0.0 {
        let s = disc.sqrt();
        (
            Complex64::new(0.5 * (tr + s), 0.0),
            Complex64::new(0.5 * (tr - s), 0.0),
        )
    } else {
        let s = (-disc).sqrt();
        (
            Complex64::new(0.5 * tr, 0.5 * s),
            Complex64::new(0.5 * tr, -0.5 * s),
        )
    }
}

/// Natural cubic spline second derivatives `y2` for strictly increasing `vx` and matching `vy`.
pub fn cspline_second_derivs(vx: &[f64], vy: &[f64]) -> Calc<Vec<f64>> {
    let n = vx.len();
    if n < 3 || vy.len() != n {
        return Err("cspline требует два вектора одинаковой длины (не менее 3 точек)".into());
    }
    for w in vx.windows(2) {
        if w[1] <= w[0] {
            return Err("cspline: вектор X должен строго возрастать".into());
        }
    }
    let mut y2 = vec![0.0; n];
    let mut u = vec![0.0; n - 1];
    for i in 1..(n - 1) {
        let sig = (vx[i] - vx[i - 1]) / (vx[i + 1] - vx[i - 1]);
        let p = sig * y2[i - 1] + 2.0;
        y2[i] = (sig - 1.0) / p;
        let dd =
            (vy[i + 1] - vy[i]) / (vx[i + 1] - vx[i]) - (vy[i] - vy[i - 1]) / (vx[i] - vx[i - 1]);
        u[i] = (6.0 * dd / (vx[i + 1] - vx[i - 1]) - sig * u[i - 1]) / p;
    }
    for k in (0..(n - 1)).rev() {
        y2[k] = y2[k] * y2[k + 1] + u[k];
    }
    Ok(y2)
}

pub fn eval_cspline(vs: &[f64], vx: &[f64], vy: &[f64], x: f64) -> Calc<f64> {
    let n = vx.len();
    if n < 3 || vy.len() != n || vs.len() != n {
        return Err(
            "interp: длины векторов сплайна vs, vx, vy должны совпадать (не менее 3)".into(),
        );
    }
    for w in vx.windows(2) {
        if w[1] <= w[0] {
            return Err("interp: вектор X должен строго возрастать".into());
        }
    }
    let seg = if x <= vx[0] {
        0
    } else if x >= vx[n - 1] {
        n - 2
    } else {
        let mut lo = 0;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if vx[mid] > x {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        lo
    };
    let h = vx[seg + 1] - vx[seg];
    let a = (vx[seg + 1] - x) / h;
    let b = (x - vx[seg]) / h;
    Ok(a * vy[seg]
        + b * vy[seg + 1]
        + ((a * a * a - a) * vs[seg] + (b * b * b - b) * vs[seg + 1]) * (h * h) / 6.0)
}

/// Polynomial least-squares regression of degree `deg` (1..=8), returning `[c0; c1; ...; c_deg]`.
pub fn poly_regress(vx: &[f64], vy: &[f64], deg: usize) -> Calc<Vec<f64>> {
    if deg == 0 || deg > 8 {
        return Err("regress поддерживает степень полинома от 1 до 8".into());
    }
    let n = vx.len();
    if vy.len() != n || n <= deg {
        return Err("regress: число точек должно быть больше степени полинома".into());
    }
    let m = deg + 1;
    let mut ata = vec![vec![0.0; m]; m];
    let mut aty = vec![0.0; m];
    for (&x, &y) in vx.iter().zip(vy) {
        let mut xp = vec![1.0; m];
        for k in 1..m {
            xp[k] = xp[k - 1] * x;
        }
        for i in 0..m {
            aty[i] += xp[i] * y;
            for j in 0..m {
                ata[i][j] += xp[i] * xp[j];
            }
        }
    }
    // Solve m x m linear system via Gaussian elimination
    for j in 0..m {
        let mut pivot = j;
        let mut best = ata[j][j].abs();
        for i in (j + 1)..m {
            if ata[i][j].abs() > best {
                best = ata[i][j].abs();
                pivot = i;
            }
        }
        if best < 1e-14 {
            return Err("regress: вырожденная матрица нормальных уравнений".into());
        }
        if pivot != j {
            ata.swap(j, pivot);
            aty.swap(j, pivot);
        }
        let diag = ata[j][j];
        for i in (j + 1)..m {
            let f = ata[i][j] / diag;
            for k in (j + 1)..m {
                ata[i][k] -= f * ata[j][k];
            }
            aty[i] -= f * aty[j];
        }
    }
    let mut coeffs = vec![0.0; m];
    for i in (0..m).rev() {
        let mut s = aty[i];
        for k in (i + 1)..m {
            s -= ata[i][k] * coeffs[k];
        }
        coeffs[i] = s / ata[i][i];
    }
    Ok(coeffs)
}

/// Temperature affine conversions: Celsius/Fahrenheit <-> Kelvin Value.
pub fn celsius_to_kelvin(t_c: f64) -> Calc<Value> {
    let k = Value::constant("K").ok_or("нет единицы K")?;
    k.multiply_number(t_c + 273.15)
}

pub fn fahrenheit_to_kelvin(t_f: f64) -> Calc<Value> {
    let k = Value::constant("K").ok_or("нет единицы K")?;
    k.multiply_number((t_f - 32.0) * (5.0 / 9.0) + 273.15)
}

pub fn kelvin_to_celsius(temp: Value) -> Calc<Value> {
    let k = Value::constant("K").ok_or("нет единицы K")?;
    let val_k = temp.in_units(k)?.real_scalar()?;
    Value::scalar(val_k - 273.15)
}

pub fn kelvin_to_fahrenheit(temp: Value) -> Calc<Value> {
    let k = Value::constant("K").ok_or("нет единицы K")?;
    let val_k = temp.in_units(k)?.real_scalar()?;
    Value::scalar((val_k - 273.15) * 1.8 + 32.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigenvals_qr_and_rank_match_known_matrices() {
        // Matrix [[2, 1], [1, 2]] has eigenvalues 3 and 1
        let eigs = eigenvals_real(2, &[2., 1., 1., 2.]).unwrap();
        assert!((eigs[0].re - 3.0).abs() < 1e-8);
        assert!((eigs[1].re - 1.0).abs() < 1e-8);

        // Rotation 90 deg [[0, -1], [1, 0]] has eigenvalues +i and -i
        let rot = eigenvals_real(2, &[0., -1., 1., 0.]).unwrap();
        assert!(rot[0].re.abs() < 1e-8 && (rot[0].im.abs() - 1.0).abs() < 1e-8);

        // QR decomposition: Q*R == A
        let a = [1., 2., 3., 4., 5., 6.]; // 3x2
        let (q, r) = qr_decompose(3, 2, &a).unwrap();
        for i in 0..3 {
            for j in 0..2 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += q[i * 3 + k] * r[k * 2 + j];
                }
                assert!((sum - a[i * 2 + j]).abs() < 1e-9);
            }
        }
        assert_eq!(matrix_rank(2, 2, &[1., 2., 2., 4.]), 1);
    }

    #[test]
    fn fft_ifft_roundtrips_and_cspline_regress_norm_work() {
        let sig: Vec<Complex64> = [1.0, 2.0, 3.0, 4.0]
            .iter()
            .map(|&x| Complex64::new(x, 0.0))
            .collect();
        let spec = fft_complex(&sig, false).unwrap();
        let back = fft_complex(&spec, true).unwrap();
        for (a, b) in sig.iter().zip(&back) {
            assert!((a - b).norm() < 1e-10);
        }

        // Cubic spline on y = x^2 at [0, 1, 2, 3]
        let vx = [0., 1., 2., 3.];
        let vy = [0., 1., 4., 9.];
        let vs = cspline_second_derivs(&vx, &vy).unwrap();
        let mid = eval_cspline(&vs, &vx, &vy, 1.5).unwrap();
        assert!((mid - 2.25).abs() < 0.15);

        // Polynomial regression y = 1 + 2x + 3x^2
        let rx = [0., 1., 2., 3., 4.];
        let ry = [1., 6., 17., 34., 57.];
        let c = poly_regress(&rx, &ry, 2).unwrap();
        assert!((c[0] - 1.0).abs() < 1e-9);
        assert!((c[1] - 2.0).abs() < 1e-9);
        assert!((c[2] - 3.0).abs() < 1e-9);

        // Normal distribution
        assert!((pnorm(0.0, 0.0, 1.0).unwrap() - 0.5).abs() < 1e-6);
        assert!((qnorm(0.975, 0.0, 1.0).unwrap() - 1.959964).abs() < 1e-5);
        assert!((dnorm(0.0, 0.0, 1.0).unwrap() - 0.39894228).abs() < 1e-6);
    }
}
