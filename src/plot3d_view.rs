use eframe::egui::{
    self, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextEdit,
};
use list_rs::{
    editing::{Plot3dMode, Plot3dSource, Plot3dSpec},
    number,
    sheet::SurfaceGrid,
    Calc,
};

pub struct Draft3d {
    pub mode: Plot3dMode,
    pub use_matrix: bool,
    pub expression: String,
    pub matrix_name: String,
    pub x_from: String,
    pub x_to: String,
    pub y_from: String,
    pub y_to: String,
    pub grid: usize,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

impl Draft3d {
    pub fn default_surface() -> Self {
        Self::from_spec(&Plot3dSpec::default_surface())
    }

    pub fn default_contour() -> Self {
        Self::from_spec(&Plot3dSpec::default_contour())
    }

    pub fn from_spec(spec: &Plot3dSpec) -> Self {
        match &spec.source {
            Plot3dSource::Function {
                expression,
                x_from,
                x_to,
                y_from,
                y_to,
            } => Self {
                mode: spec.mode,
                use_matrix: false,
                expression: expression.clone(),
                matrix_name: "M".into(),
                x_from: x_from.to_string(),
                x_to: x_to.to_string(),
                y_from: y_from.to_string(),
                y_to: y_to.to_string(),
                grid: spec.grid,
                yaw_deg: spec.yaw_deg,
                pitch_deg: spec.pitch_deg,
            },
            Plot3dSource::Matrix { name } => Self {
                mode: spec.mode,
                use_matrix: true,
                expression: "sin(x) * cos(y)".into(),
                matrix_name: name.clone(),
                x_from: "-3.14".into(),
                x_to: "3.14".into(),
                y_from: "-3.14".into(),
                y_to: "3.14".into(),
                grid: spec.grid,
                yaw_deg: spec.yaw_deg,
                pitch_deg: spec.pitch_deg,
            },
        }
    }

    pub fn fields(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Вид:");
            ui.selectable_value(&mut self.mode, Plot3dMode::Surface, "3D-поверхность");
            ui.selectable_value(&mut self.mode, Plot3dMode::Contour, "Контуры / теплокарта");
        });
        ui.horizontal(|ui| {
            ui.label("Источник:");
            ui.selectable_value(&mut self.use_matrix, false, "Функция z(x, y)");
            ui.selectable_value(&mut self.use_matrix, true, "Матрица M");
        });
        ui.add_space(4.);
        if !self.use_matrix {
            ui.horizontal(|ui| {
                ui.label("z(x, y) =");
                ui.add(
                    TextEdit::singleline(&mut self.expression)
                        .desired_width(330.)
                        .hint_text("sin(x) * cos(y)"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("X от:");
                ui.add(TextEdit::singleline(&mut self.x_from).desired_width(72.));
                ui.label("до:");
                ui.add(TextEdit::singleline(&mut self.x_to).desired_width(72.));
                ui.separator();
                ui.label("Y от:");
                ui.add(TextEdit::singleline(&mut self.y_from).desired_width(72.));
                ui.label("до:");
                ui.add(TextEdit::singleline(&mut self.y_to).desired_width(72.));
            });
            ui.horizontal(|ui| {
                ui.label("Сетка (N×N):");
                ui.add(egui::DragValue::new(&mut self.grid).range(6..=36));
            });
        } else {
            ui.horizontal(|ui| {
                ui.label("Имя матрицы:");
                ui.add(
                    TextEdit::singleline(&mut self.matrix_name)
                        .desired_width(180.)
                        .hint_text("M"),
                );
            });
        }
        if self.mode == Plot3dMode::Surface {
            ui.horizontal(|ui| {
                ui.label("Поворот (°):");
                ui.add(egui::DragValue::new(&mut self.yaw_deg).range(-180.0..=180.0));
                ui.label("Наклон (°):");
                ui.add(egui::DragValue::new(&mut self.pitch_deg).range(-80.0..=80.0));
            });
        }
    }

    pub fn spec(&self) -> Calc<Plot3dSpec> {
        let source = if self.use_matrix {
            let name = self.matrix_name.trim();
            if name.is_empty() {
                return Err("укажите имя матрицы".into());
            }
            Plot3dSource::Matrix { name: name.into() }
        } else {
            let expr = self.expression.trim();
            if expr.is_empty() {
                return Err("введите выражение z(x, y)".into());
            }
            let x_from = number(self.x_from.trim())?;
            let x_to = number(self.x_to.trim())?;
            let y_from = number(self.y_from.trim())?;
            let y_to = number(self.y_to.trim())?;
            if x_from >= x_to || y_from >= y_to {
                return Err("начала диапазонов X и Y должны быть меньше концов".into());
            }
            Plot3dSource::Function {
                expression: expr.into(),
                x_from,
                x_to,
                y_from,
                y_to,
            }
        };
        Ok(Plot3dSpec {
            mode: self.mode,
            source,
            yaw_deg: self.yaw_deg.clamp(-360.0, 360.0),
            pitch_deg: self.pitch_deg.clamp(-85.0, 85.0),
            grid: self.grid.clamp(6, 40),
        })
    }
}

fn height_color(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    // Teal -> Emerald -> Amber -> Coral
    const STOPS: [(f32, [f32; 3]); 4] = [
        (0.00, [16.0, 78.0, 112.0]),
        (0.38, [18.0, 146.0, 134.0]),
        (0.72, [227.0, 168.0, 56.0]),
        (1.00, [186.0, 58.0, 44.0]),
    ];
    for pair in STOPS.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let r = (c0[0] + (c1[0] - c0[0]) * f) as u8;
            let g = (c0[1] + (c1[1] - c0[1]) * f) as u8;
            let b = (c0[2] + (c1[2] - c0[2]) * f) as u8;
            return Color32::from_rgb(r, g, b);
        }
    }
    Color32::from_rgb(186, 58, 44)
}

fn tick(n: f64) -> String {
    if n.abs() >= 10_000. || (n.abs() > 0. && n.abs() < 0.001) {
        format!("{n:.1e}")
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').into()
    }
}

pub fn draw(
    painter: &Painter,
    rect: Rect,
    grid: &Calc<SurfaceGrid>,
    spec: &Plot3dSpec,
    selected: bool,
) {
    let border = if selected {
        Stroke::new(1.5_f32, Color32::from_rgb(0, 104, 97))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgb(218, 228, 232))
    };
    painter.rect(rect, 7., Color32::WHITE, border, StrokeKind::Inside);

    let clip = painter.with_clip_rect(rect.shrink(2.));
    let ink = Color32::from_rgb(26, 42, 54);
    let muted = Color32::from_rgb(79, 101, 113);

    clip.text(
        rect.left_top() + vec2(10., 7.),
        Align2::LEFT_TOP,
        spec.title(),
        FontId::proportional(11.5),
        ink,
    );

    let grid = match grid {
        Ok(g) => g,
        Err(err) => {
            clip.text(
                rect.center(),
                Align2::CENTER_CENTER,
                err,
                FontId::proportional(12.),
                Color32::from_rgb(161, 41, 33),
            );
            return;
        }
    };

    let z_span = (grid.z_max - grid.z_min).max(1e-12);
    let norm_z = |z: f64| ((z - grid.z_min) / z_span) as f32;

    match spec.mode {
        Plot3dMode::Surface => draw_surface(&clip, rect, grid, spec, norm_z, ink, muted),
        Plot3dMode::Contour => draw_contour(&clip, rect, grid, norm_z, ink, muted),
    }

    // Bottom status bar with z-range
    let info = match spec.mode {
        Plot3dMode::Surface => format!(
            "z ∈ [{}; {}] · ПКМ: вращать",
            tick(grid.z_min),
            tick(grid.z_max)
        ),
        Plot3dMode::Contour => format!(
            "z ∈ [{}; {}] · X:[{}; {}] · Y:[{}; {}]",
            tick(grid.z_min),
            tick(grid.z_max),
            tick(grid.x_from),
            tick(grid.x_to),
            tick(grid.y_from),
            tick(grid.y_to)
        ),
    };
    clip.text(
        Pos2::new(rect.left() + 10., rect.bottom() - 6.),
        Align2::LEFT_BOTTOM,
        info,
        FontId::proportional(10.5),
        muted,
    );
}

fn draw_surface(
    painter: &Painter,
    rect: Rect,
    grid: &SurfaceGrid,
    spec: &Plot3dSpec,
    norm_z: impl Fn(f64) -> f32,
    ink: Color32,
    muted: Color32,
) {
    let yaw = spec.yaw_deg.to_radians();
    let pitch = spec.pitch_deg.to_radians();
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();

    let center = Pos2::new(rect.center().x, rect.center().y + 4.);
    let scale = (rect.width().min(rect.height()) * 0.34).max(24.);

    // Project (x, y, z) in [-1, 1]^3 -> (screen Pos2, depth for painter's sort)
    let project = |x: f32, y: f32, z: f32| -> (Pos2, f32) {
        let rx = x * cy - y * sy;
        let ry = x * sy + y * cy;
        let py = ry * sp + z * cp;
        let depth = ry * cp - z * sp;
        (
            Pos2::new(center.x + rx * scale, center.y - py * scale),
            depth,
        )
    };

    // Draw base bounding box edges
    let axis_stroke = Stroke::new(1.0_f32, Color32::from_rgb(176, 192, 200));
    let corners = [
        (-1., -1., -0.7),
        (1., -1., -0.7),
        (1., 1., -0.7),
        (-1., 1., -0.7),
        (-1., -1., 0.7),
    ];
    let p_c: Vec<Pos2> = corners
        .iter()
        .map(|&(x, y, z)| project(x, y, z).0)
        .collect();
    painter.line_segment([p_c[0], p_c[1]], axis_stroke);
    painter.line_segment([p_c[1], p_c[2]], axis_stroke);
    painter.line_segment([p_c[2], p_c[3]], axis_stroke);
    painter.line_segment([p_c[3], p_c[0]], axis_stroke);
    painter.line_segment([p_c[0], p_c[4]], axis_stroke);

    painter.text(
        project(1.12, -1.0, -0.7).0,
        Align2::CENTER_CENTER,
        "X",
        FontId::proportional(10.5),
        muted,
    );
    painter.text(
        project(-1.0, 1.12, -0.7).0,
        Align2::CENTER_CENTER,
        "Y",
        FontId::proportional(10.5),
        muted,
    );
    painter.text(
        project(-1.0, -1.0, 0.85).0,
        Align2::CENTER_CENTER,
        "Z",
        FontId::proportional(10.5),
        muted,
    );

    let rows = grid.rows;
    let cols = grid.cols;
    let mut projected = vec![None; rows * cols];
    for r in 0..rows {
        let ny = -1.0 + 2.0 * (r as f32 / (rows - 1) as f32);
        for c in 0..cols {
            let nx = -1.0 + 2.0 * (c as f32 / (cols - 1) as f32);
            if let Some(z) = grid.values[r * cols + c] {
                let tz = norm_z(z);
                let nz = -0.7 + 1.4 * tz;
                let (pos, depth) = project(nx, ny, nz);
                projected[r * cols + c] = Some((pos, depth, tz));
            }
        }
    }

    let mut quads: Vec<(f32, [Pos2; 4], Color32)> = Vec::with_capacity((rows - 1) * (cols - 1));
    for r in 0..rows - 1 {
        for c in 0..cols - 1 {
            let Some((p00, d00, t00)) = projected[r * cols + c] else {
                continue;
            };
            let Some((p10, d10, t10)) = projected[r * cols + (c + 1)] else {
                continue;
            };
            let Some((p11, d11, t11)) = projected[(r + 1) * cols + (c + 1)] else {
                continue;
            };
            let Some((p01, d01, t01)) = projected[(r + 1) * cols + c] else {
                continue;
            };
            let avg_d = 0.25 * (d00 + d10 + d11 + d01);
            let avg_t = 0.25 * (t00 + t10 + t11 + t01);
            quads.push((avg_d, [p00, p10, p11, p01], height_color(avg_t)));
        }
    }

    // Painter's algorithm: draw farthest quads first (descending depth)
    quads.sort_by(|a, b| b.0.total_cmp(&a.0));
    let wire = Stroke::new(
        0.6_f32,
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 85),
    );
    for (_, pts, fill) in quads {
        painter.add(Shape::convex_polygon(pts.to_vec(), fill, wire));
    }
}

fn draw_contour(
    painter: &Painter,
    rect: Rect,
    grid: &SurfaceGrid,
    norm_z: impl Fn(f64) -> f32,
    ink: Color32,
    muted: Color32,
) {
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 36., rect.top() + 26.),
        Pos2::new(rect.right() - 28., rect.bottom() - 24.),
    );
    if !plot.is_positive() {
        return;
    }
    let rows = grid.rows;
    let cols = grid.cols;
    let cw = plot.width() / (cols - 1) as f32;
    let ch = plot.height() / (rows - 1) as f32;

    // 1. Filled heatmap cells
    for r in 0..rows - 1 {
        let y_bot = plot.bottom() - r as f32 * ch;
        let y_top = y_bot - ch;
        for c in 0..cols - 1 {
            let (Some(z00), Some(z10), Some(z11), Some(z01)) = (
                grid.values[r * cols + c],
                grid.values[r * cols + c + 1],
                grid.values[(r + 1) * cols + c + 1],
                grid.values[(r + 1) * cols + c],
            ) else {
                continue;
            };
            let avg = 0.25 * (z00 + z10 + z11 + z01);
            let x_left = plot.left() + c as f32 * cw;
            let cell = Rect::from_min_max(
                Pos2::new(x_left, y_top),
                Pos2::new(x_left + cw + 0.5, y_bot + 0.5),
            );
            painter.rect_filled(cell, 0., height_color(norm_z(avg)));
        }
    }

    // 2. Marching-squares contour isolines (8 levels)
    let iso_stroke = Stroke::new(
        1.0_f32,
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 190),
    );
    let z_span = (grid.z_max - grid.z_min).max(1e-12);
    for level_idx in 1..=8 {
        let iso = grid.z_min + z_span * (level_idx as f64 / 9.0);
        for r in 0..rows - 1 {
            let y0 = plot.bottom() - r as f32 * ch;
            let y1 = y0 - ch;
            for c in 0..cols - 1 {
                let (Some(v0), Some(v1), Some(v2), Some(v3)) = (
                    grid.values[r * cols + c],
                    grid.values[r * cols + c + 1],
                    grid.values[(r + 1) * cols + c + 1],
                    grid.values[(r + 1) * cols + c],
                ) else {
                    continue;
                };
                let x0 = plot.left() + c as f32 * cw;
                let x1 = x0 + cw;
                let mut crossings = Vec::with_capacity(4);
                let interp = |a: f64, b: f64| -> Option<f32> {
                    if (a < iso) != (b < iso) && (b - a).abs() > 1e-15 {
                        Some(((iso - a) / (b - a)) as f32)
                    } else {
                        None
                    }
                };
                if let Some(t) = interp(v0, v1) {
                    crossings.push(Pos2::new(x0 + t * (x1 - x0), y0));
                }
                if let Some(t) = interp(v1, v2) {
                    crossings.push(Pos2::new(x1, y0 + t * (y1 - y0)));
                }
                if let Some(t) = interp(v2, v3) {
                    crossings.push(Pos2::new(x1 + t * (x0 - x1), y1));
                }
                if let Some(t) = interp(v3, v0) {
                    crossings.push(Pos2::new(x0, y1 + t * (y0 - y1)));
                }
                if crossings.len() >= 2 {
                    painter.line_segment([crossings[0], crossings[1]], iso_stroke);
                }
                if crossings.len() == 4 {
                    painter.line_segment([crossings[2], crossings[3]], iso_stroke);
                }
            }
        }
    }

    painter.rect_stroke(plot, 0., Stroke::new(1_f32, muted), StrokeKind::Inside);

    // Colorbar on the right
    let bar = Rect::from_min_max(
        Pos2::new(plot.right() + 7., plot.top()),
        Pos2::new(plot.right() + 17., plot.bottom()),
    );
    let steps = 24;
    let bh = bar.height() / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let y_bot = bar.bottom() - i as f32 * bh;
        let seg = Rect::from_min_max(
            Pos2::new(bar.left(), y_bot - bh - 0.5),
            Pos2::new(bar.right(), y_bot),
        );
        painter.rect_filled(seg, 0., height_color(t));
    }
    painter.rect_stroke(bar, 0., Stroke::new(0.8_f32, muted), StrokeKind::Inside);
}
