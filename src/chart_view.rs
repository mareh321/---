use eframe::egui::{
    self, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
};
use list_rs::{editing::ChartSpec, number, Calc};

pub type Curves = Vec<(String, Vec<Option<[f64; 2]>>)>;
const COLORS: [Color32; 4] = [
    Color32::from_rgb(0, 111, 105),
    Color32::from_rgb(156, 65, 31),
    Color32::from_rgb(74, 78, 163),
    Color32::from_rgb(128, 57, 116),
];

pub struct Draft {
    vectors: bool,
    expression: String,
    from: String,
    to: String,
    x: String,
    y: String,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            vectors: false,
            expression: "sin(x)".into(),
            from: "-6.28".into(),
            to: "6.28".into(),
            x: "X".into(),
            y: "Y".into(),
        }
    }
}

impl Draft {
    pub fn from_spec(spec: &ChartSpec) -> Self {
        let mut draft = Self::default();
        match spec {
            ChartSpec::Function {
                expression,
                from,
                to,
            } => {
                draft.expression = expression.clone();
                draft.from = from.to_string();
                draft.to = to.to_string();
            }
            ChartSpec::Vectors { x, y } => {
                draft.vectors = true;
                draft.x = x.clone();
                draft.y = y.clone();
            }
        }
        draft
    }

    pub fn fields(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.vectors, false, "Функции от x");
            ui.selectable_value(&mut self.vectors, true, "Данные X и Y");
        });
        ui.add_space(8.);
        if self.vectors {
            field(ui, "Массив X", &mut self.x, 240.);
            field(ui, "Массив Y", &mut self.y, 240.);
            ui.label("Укажите имена массивов или диапазонов, заданных выше графика.");
        } else {
            field(ui, "y =", &mut self.expression, 285.);
            ui.label("До четырёх кривых, например sin(x); cos(x)");
            ui.horizontal(|ui| {
                field(ui, "x от", &mut self.from, 70.);
                field(ui, "до", &mut self.to, 70.);
            });
        }
        ui.add_space(6.);
    }

    pub fn spec(&self) -> Calc<ChartSpec> {
        if self.vectors {
            Ok(ChartSpec::Vectors {
                x: self.x.trim().into(),
                y: self.y.trim().into(),
            })
        } else {
            Ok(ChartSpec::Function {
                expression: self.expression.trim().into(),
                from: number(self.from.trim())?,
                to: number(self.to.trim())?,
            })
        }
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(width)
                .char_limit(512),
        );
    });
}

fn tick(n: f64) -> String {
    if n.abs() >= 10_000. || (n.abs() > 0. && n.abs() < 0.001) {
        format!("{n:.1e}")
    } else {
        let s = format!("{n:.3}");
        s.trim_end_matches('0').trim_end_matches('.').into()
    }
}

pub fn draw(
    painter: &Painter,
    rect: Rect,
    data: &Calc<Curves>,
    hover: Option<Pos2>,
    selected: bool,
) {
    let line = Color32::from_rgb(218, 228, 232);
    let ink = Color32::from_rgb(47, 70, 81);
    let painter = painter.with_clip_rect(rect);
    painter.rect(
        rect,
        7.,
        Color32::WHITE,
        Stroke::new(1_f32, if selected { COLORS[0] } else { line }),
        StrokeKind::Inside,
    );
    let Ok(curves) = data else {
        painter.text(
            rect.left_top() + vec2(10., 12.),
            Align2::LEFT_TOP,
            data.as_ref().unwrap_err(),
            FontId::proportional(12.),
            COLORS[1],
        );
        return;
    };
    let grid = Rect::from_min_max(rect.min + vec2(56., 32.), rect.max - vec2(15., 33.));
    if grid.width() < 20. || grid.height() < 15. {
        return;
    }
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for point in curves
        .iter()
        .flat_map(|(_, points)| points.iter().flatten())
    {
        for (axis, &value) in bounds.iter_mut().zip(point) {
            axis[0] = axis[0].min(value);
            axis[1] = axis[1].max(value);
        }
    }
    for axis in &mut bounds {
        let step = if axis[0] == axis[1] {
            (axis[0].abs() * 0.1).max(1.)
        } else {
            (axis[1] - axis[0]) * 0.05
        };
        axis[0] -= step;
        axis[1] += step;
    }
    let [[xmin, xmax], [ymin, ymax]] = bounds;
    let project = |[x, y]: [f64; 2]| {
        Pos2::new(
            grid.left() + ((x - xmin) / (xmax - xmin)) as f32 * grid.width(),
            grid.bottom() - ((y - ymin) / (ymax - ymin)) as f32 * grid.height(),
        )
    };
    for i in 0..=4 {
        let ratio = i as f32 / 4.;
        let x = egui::lerp(grid.x_range(), ratio);
        let y = egui::lerp(grid.y_range(), 1. - ratio);
        painter.vline(x, grid.y_range(), Stroke::new(1_f32, line));
        painter.hline(grid.x_range(), y, Stroke::new(1_f32, line));
        painter.text(
            Pos2::new(x, grid.bottom() + 6.),
            Align2::CENTER_TOP,
            tick(xmin + (xmax - xmin) * f64::from(ratio)),
            FontId::proportional(10.),
            ink,
        );
        painter.text(
            Pos2::new(grid.left() - 6., y),
            Align2::RIGHT_CENTER,
            tick(ymin + (ymax - ymin) * f64::from(ratio)),
            FontId::proportional(10.),
            ink,
        );
    }
    let axis_color = Color32::from_rgb(144, 162, 170);
    let origin = project([0., 0.]);
    if xmin < 0. && xmax > 0. {
        painter.vline(origin.x, grid.y_range(), Stroke::new(1_f32, axis_color));
    }
    if ymin < 0. && ymax > 0. {
        painter.hline(grid.x_range(), origin.y, Stroke::new(1_f32, axis_color));
    }
    let canvas = painter.with_clip_rect(grid);
    let mut closest = None;
    let mut best = 95.;
    let labels = rect.width() / curves.len().max(1) as f32;
    for (series, (name, points)) in curves.iter().enumerate() {
        let color = COLORS[series % COLORS.len()];
        let label_at = rect.left_top() + vec2(10. + series as f32 * labels, 12.);
        let legend = Rect::from_min_size(label_at, vec2((labels - 12.).max(0.), 16.));
        painter.with_clip_rect(legend).text(
            label_at,
            Align2::LEFT_TOP,
            name,
            FontId::proportional(12.),
            color,
        );
        let mut run = Vec::new();
        for point in points.iter().copied().chain(std::iter::once(None)) {
            if let Some(values) = point {
                let pixel = project(values);
                if let Some(mouse) = hover.filter(|&m| grid.contains(m)) {
                    let distance = mouse.distance_sq(pixel);
                    if distance < best {
                        best = distance;
                        closest = Some((pixel, values, color));
                    }
                }
                run.push(pixel);
            } else if !run.is_empty() {
                if run.len() == 1 {
                    canvas.circle_filled(run[0], 3., color);
                    run.clear();
                } else {
                    canvas.add(Shape::line(
                        std::mem::take(&mut run),
                        Stroke::new(1.7_f32, color),
                    ));
                }
            }
        }
    }
    if let Some((dot, [x, y], color)) = closest {
        canvas.circle_filled(dot, 4., color);
        painter.text(
            rect.right_bottom() - vec2(13., 9.),
            Align2::RIGHT_BOTTOM,
            format!("x = {}   y = {}", tick(x), tick(y)),
            FontId::proportional(10.),
            ink,
        );
    }
}
