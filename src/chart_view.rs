use eframe::egui::{
    self, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
};
use list_rs::{
    editing::{
        AxisNumber, AxisPlacement, ChartAppearance, ChartLine, ChartOptions, ChartSpec,
        LegendPlacement,
    },
    number, Calc,
};

pub type Curves = Vec<(String, Vec<Option<[f64; 2]>>)>;

#[derive(Clone, Copy, Default)]
pub enum TraceMode {
    #[default]
    Hover,
    Data,
    Cursor,
}

#[derive(Clone, Copy)]
pub struct TraceSample {
    pub coordinates: [f64; 2],
    pub series: Option<usize>,
}
const COLORS: [Color32; 4] = [
    Color32::from_rgb(0, 111, 105),
    Color32::from_rgb(156, 65, 31),
    Color32::from_rgb(74, 78, 163),
    Color32::from_rgb(128, 57, 116),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftMode {
    Function,
    Vectors,
    Parametric,
    Polar,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartTab {
    Axes,
    Traces,
    Numbers,
    Labels,
    Defaults,
    #[default]
    Data,
}

pub struct Draft {
    mode: DraftMode,
    expression: String,
    from: String,
    to: String,
    x: String,
    y: String,
    x_expr: String,
    y_expr: String,
    log_x: bool,
    log_y: bool,
    equal_scale: bool,
    markers: bool,
    show_legend: bool,
    x_min: String,
    x_max: String,
    y_min: String,
    y_max: String,
    x_label: String,
    y_label: String,
    appearance: ChartAppearance,
    pub tab: ChartTab,
    pub save_defaults: bool,
    show_help: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            mode: DraftMode::Function,
            expression: "sin(x)".into(),
            from: "-6.28".into(),
            to: "6.28".into(),
            x: "X".into(),
            y: "Y".into(),
            x_expr: "cos(t)".into(),
            y_expr: "sin(t)".into(),
            log_x: false,
            log_y: false,
            equal_scale: false,
            markers: false,
            show_legend: true,
            x_min: String::new(),
            x_max: String::new(),
            y_min: String::new(),
            y_max: String::new(),
            x_label: String::new(),
            y_label: String::new(),
            appearance: Default::default(),
            tab: ChartTab::Data,
            save_defaults: false,
            show_help: false,
        }
    }
}

impl Draft {
    pub fn vectors_preset() -> Self {
        Self {
            mode: DraftMode::Vectors,
            ..Self::default()
        }
    }

    pub fn parametric_preset() -> Self {
        Self {
            mode: DraftMode::Parametric,
            from: "0".into(),
            to: "6.283185".into(),
            x_expr: "cos(t)".into(),
            y_expr: "sin(t)".into(),
            equal_scale: true,
            ..Self::default()
        }
    }

    pub fn polar_preset() -> Self {
        Self {
            mode: DraftMode::Polar,
            expression: "1 + cos(θ)".into(),
            from: "0".into(),
            to: "6.283185".into(),
            equal_scale: true,
            ..Self::default()
        }
    }

    pub fn from_spec(spec: &ChartSpec, options: &ChartOptions) -> Self {
        let mut draft = Self::default();
        match spec {
            ChartSpec::Function {
                expression,
                from,
                to,
            } => {
                draft.mode = DraftMode::Function;
                draft.expression = expression.clone();
                draft.from = from.to_string();
                draft.to = to.to_string();
            }
            ChartSpec::Vectors { x, y } => {
                draft.mode = DraftMode::Vectors;
                draft.x = x.clone();
                draft.y = y.clone();
            }
            ChartSpec::Parametric {
                x_expr,
                y_expr,
                from,
                to,
            } => {
                draft.mode = DraftMode::Parametric;
                draft.x_expr = x_expr.clone();
                draft.y_expr = y_expr.clone();
                draft.from = from.to_string();
                draft.to = to.to_string();
            }
            ChartSpec::Polar {
                expression,
                from,
                to,
            } => {
                draft.mode = DraftMode::Polar;
                draft.expression = expression.clone();
                draft.from = from.to_string();
                draft.to = to.to_string();
            }
        }
        draft.set_format(options);
        draft.tab = ChartTab::Axes;
        draft
    }

    pub fn set_format(&mut self, options: &ChartOptions) {
        self.appearance = options.appearance.clone();
        self.log_x = options.log_x;
        self.log_y = options.log_y;
        self.equal_scale = options.equal_scale;
        self.markers = options.markers;
        self.show_legend = options.show_legend;
        self.x_min = options.x_min.map(|v| v.to_string()).unwrap_or_default();
        self.x_max = options.x_max.map(|v| v.to_string()).unwrap_or_default();
        self.y_min = options.y_min.map(|v| v.to_string()).unwrap_or_default();
        self.y_max = options.y_max.map(|v| v.to_string()).unwrap_or_default();
        self.x_label = options.x_label.clone();
        self.y_label = options.y_label.clone();
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn fields(&mut self, ui: &mut egui::Ui, defaults: Option<&ChartOptions>, editing: bool) {
        ui.horizontal_wrapped(|ui| {
            for (tab, name) in [
                (ChartTab::Axes, "Оси X, Y"),
                (ChartTab::Traces, "Трассировка"),
                (ChartTab::Numbers, "Формат числа"),
                (ChartTab::Labels, "Подписи"),
                (ChartTab::Defaults, "По умолчанию"),
                (ChartTab::Data, "Данные"),
            ] {
                ui.selectable_value(&mut self.tab, tab, name);
            }
        });
        ui.separator();
        if self.show_help {
            ui.label(match self.tab {
                ChartTab::Axes => {
                    "Оставьте границу пустой, чтобы определить её автоматически по данным."
                }
                ChartTab::Traces => {
                    "Параметры строк относятся к кривым в порядке ввода на вкладке «Данные»."
                }
                ChartTab::Numbers => {
                    "Формат меняет подписи делений. Дроби подбираются со знаменателем до 100."
                }
                ChartTab::Labels => "Оставьте подпись пустой, чтобы скрыть её.",
                ChartTab::Defaults => {
                    "Умолчания хранятся в листе и применяются при вставке новых графиков."
                }
                ChartTab::Data => {
                    "Выберите способ построения и введите выражения или имена массивов."
                }
            });
        }
        match self.tab {
            ChartTab::Axes => {
                for (i, (name, log, min, max)) in [
                    ("X", &mut self.log_x, &mut self.x_min, &mut self.x_max),
                    ("Y", &mut self.log_y, &mut self.y_min, &mut self.y_max),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.group(|ui| {
                        ui.strong(format!("Ось {name}"));
                        ui.horizontal_wrapped(|ui| {
                            if self.mode != DraftMode::Polar {
                                ui.checkbox(log, "Логарифмическая");
                            }
                            ui.checkbox(&mut self.appearance.grid[i], "Линии сетки");
                            ui.checkbox(&mut self.appearance.numbers[i], "Нумерация");
                        });
                        ui.horizontal_wrapped(|ui| {
                            field(ui, "Мин", min, 76.);
                            field(ui, "Макс", max, 76.);
                            ui.label("Делений");
                            ui.add(
                                egui::DragValue::new(&mut self.appearance.divisions[i])
                                    .range(1..=20),
                            );
                        });
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Отображение осей");
                    for (placement, name) in AxisPlacement::ALL {
                        ui.radio_value(&mut self.appearance.axes, placement, name);
                    }
                });
                ui.checkbox(&mut self.equal_scale, "Одинаковый масштаб");
            }
            ChartTab::Traces => {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.show_legend, "Показывать легенду");
                    ui.checkbox(&mut self.markers, "Маркеры точек");
                });
                ui.add_enabled_ui(self.show_legend, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Положение легенды");
                        let placement = &mut self.appearance.legend_placement;
                        let selected = LegendPlacement::ALL
                            .iter()
                            .find(|(value, _)| value == placement)
                            .map_or("", |(_, name)| *name);
                        egui::ComboBox::from_id_salt("legend_placement")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (value, name) in LegendPlacement::ALL {
                                    ui.selectable_value(placement, value, name);
                                }
                            });
                    });
                });
                egui::Grid::new("chart_trace_styles")
                    .striped(true)
                    .show(ui, |ui| {
                        for heading in ["Кривая", "Подпись в легенде", "Линия", "Толщина", "Цвет"]
                        {
                            ui.strong(heading);
                        }
                        ui.end_row();
                        for (i, trace) in self.appearance.traces.iter_mut().enumerate() {
                            ui.label((i + 1).to_string());
                            ui.add(
                                egui::TextEdit::singleline(&mut trace.label)
                                    .desired_width(118.)
                                    .char_limit(120),
                            );
                            let name = ChartLine::ALL
                                .iter()
                                .find(|(style, _)| *style == trace.line)
                                .map_or("", |(_, name)| *name);
                            egui::ComboBox::from_id_salt(("trace_line", i))
                                .selected_text(name)
                                .show_ui(ui, |ui| {
                                    for (style, name) in ChartLine::ALL {
                                        ui.selectable_value(&mut trace.line, style, name);
                                    }
                                });
                            ui.add_enabled(
                                trace.line != ChartLine::Points,
                                egui::DragValue::new(&mut trace.width)
                                    .range(0..=8)
                                    .custom_formatter(|value, _| {
                                        if value == 0. {
                                            "Авто".into()
                                        } else {
                                            format!("{value:.0}")
                                        }
                                    }),
                            )
                            .on_hover_text("Толщина линии в пунктах. 0 — стандартная.");
                            ui.horizontal(|ui| {
                                let [r, g, b, _] = COLORS[i].to_array();
                                let mut rgb = trace.color.unwrap_or([r, g, b]);
                                if ui.color_edit_button_srgb(&mut rgb).changed() {
                                    trace.color = Some(rgb);
                                }
                                if trace.color.is_some() && ui.small_button("Сброс").clicked()
                                {
                                    trace.color = None;
                                }
                            });
                            ui.end_row();
                        }
                    });
            }
            ChartTab::Numbers => {
                for (kind, name) in AxisNumber::ALL {
                    ui.radio_value(&mut self.appearance.number, kind, name);
                }
                ui.horizontal(|ui| {
                    ui.label("Десятичных знаков");
                    ui.add(egui::DragValue::new(&mut self.appearance.digits).range(0..=12));
                });
                ui.checkbox(
                    &mut self.appearance.trailing_zeros,
                    "Показывать конечные нули",
                );
            }
            ChartTab::Labels => {
                field(ui, "Заголовок", &mut self.appearance.title, 260.);
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.appearance.title_below, false, "Сверху");
                    ui.radio_value(&mut self.appearance.title_below, true, "Снизу");
                });
                field(ui, "Ось X", &mut self.x_label, 280.);
                field(ui, "Ось Y", &mut self.y_label, 280.);
            }
            ChartTab::Defaults => {
                if ui.button("Использовать умолчания").clicked() {
                    self.set_format(defaults.unwrap_or(&ChartOptions::default()));
                }
                if editing {
                    ui.checkbox(
                        &mut self.save_defaults,
                        "Сохранить формат как умолчание этого листа",
                    );
                }
            }
            ChartTab::Data => self.data_fields(ui),
        }
    }

    fn data_fields(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.mode, DraftMode::Function, "y(x)");
            ui.selectable_value(&mut self.mode, DraftMode::Vectors, "Данные X, Y");
            ui.selectable_value(&mut self.mode, DraftMode::Parametric, "Параметр.");
            ui.selectable_value(&mut self.mode, DraftMode::Polar, "Полярный");
        });
        ui.add_space(6.);
        match self.mode {
            DraftMode::Function => {
                field(ui, "y =", &mut self.expression, 310.);
                ui.label("До четырёх кривых через «;», например sin(x); cos(x)");
                ui.horizontal(|ui| {
                    field(ui, "x от", &mut self.from, 80.);
                    field(ui, "до", &mut self.to, 80.);
                });
            }
            DraftMode::Vectors => {
                field(ui, "Массив X", &mut self.x, 260.);
                field(ui, "Массив Y", &mut self.y, 260.);
                ui.label("Имена массивов/диапазонов выше графика (несколько серий через «;»).");
            }
            DraftMode::Parametric => {
                field(ui, "x(t) =", &mut self.x_expr, 295.);
                field(ui, "y(t) =", &mut self.y_expr, 295.);
                ui.label("Параметр t или θ (до четырёх пар через «;»).");
                ui.horizontal(|ui| {
                    field(ui, "t от", &mut self.from, 80.);
                    field(ui, "до", &mut self.to, 80.);
                });
            }
            DraftMode::Polar => {
                field(ui, "r(θ) =", &mut self.expression, 295.);
                ui.label("Угол θ (или t) в радианах, до четырёх кривых через «;».");
                ui.horizontal(|ui| {
                    field(ui, "θ от", &mut self.from, 80.);
                    field(ui, "до", &mut self.to, 80.);
                });
            }
        }
    }

    pub fn spec(&self) -> Calc<ChartSpec> {
        match self.mode {
            DraftMode::Function => Ok(ChartSpec::Function {
                expression: self.expression.trim().into(),
                from: number(self.from.trim())?,
                to: number(self.to.trim())?,
            }),
            DraftMode::Vectors => Ok(ChartSpec::Vectors {
                x: self.x.trim().into(),
                y: self.y.trim().into(),
            }),
            DraftMode::Parametric => Ok(ChartSpec::Parametric {
                x_expr: self.x_expr.trim().into(),
                y_expr: self.y_expr.trim().into(),
                from: number(self.from.trim())?,
                to: number(self.to.trim())?,
            }),
            DraftMode::Polar => Ok(ChartSpec::Polar {
                expression: self.expression.trim().into(),
                from: number(self.from.trim())?,
                to: number(self.to.trim())?,
            }),
        }
    }

    pub fn options(&self) -> Calc<ChartOptions> {
        let opt = |s: &str| -> Calc<Option<f64>> {
            let t = s.trim();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(number(t)?))
            }
        };
        let x_min = opt(&self.x_min)?;
        let x_max = opt(&self.x_max)?;
        let y_min = opt(&self.y_min)?;
        let y_max = opt(&self.y_max)?;
        if let (Some(a), Some(b)) = (x_min, x_max) {
            if a >= b {
                return Err("X мин должен быть меньше X макс".into());
            }
        }
        if let (Some(a), Some(b)) = (y_min, y_max) {
            if a >= b {
                return Err("Y мин должен быть меньше Y макс".into());
            }
        }
        let log_x = self.mode != DraftMode::Polar && self.log_x;
        let log_y = self.mode != DraftMode::Polar && self.log_y;
        if log_x && (x_min.is_some_and(|v| v <= 0.) || x_max.is_some_and(|v| v <= 0.)) {
            return Err("Для логарифмической шкалы X пределы должны быть > 0".into());
        }
        if log_y && (y_min.is_some_and(|v| v <= 0.) || y_max.is_some_and(|v| v <= 0.)) {
            return Err("Для логарифмической шкалы Y пределы должны быть > 0".into());
        }
        Ok(ChartOptions {
            log_x,
            log_y,
            equal_scale: self.equal_scale,
            markers: self.markers,
            show_legend: self.show_legend,
            x_min,
            x_max,
            y_min,
            y_max,
            x_label: self.x_label.trim().into(),
            y_label: self.y_label.trim().into(),
            appearance: self.appearance.clone(),
        })
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

fn map_coord(v: f64, log: bool) -> Option<f64> {
    if log {
        (v > 0. && v.is_finite()).then(|| v.log10())
    } else {
        v.is_finite().then_some(v)
    }
}

fn unmap_coord(u: f64, log: bool) -> f64 {
    if log {
        10_f64.powf(u)
    } else {
        u
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    painter: &Painter,
    rect: Rect,
    data: &Calc<Curves>,
    options: &ChartOptions,
    is_polar: bool,
    zoom: f64,
    hover: Option<Pos2>,
    selected: bool,
    trace_mode: TraceMode,
) -> Option<TraceSample> {
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
        return None;
    };
    let appearance = &options.appearance;
    let title = appearance.title.trim();
    let top_title = if !title.is_empty() && !appearance.title_below {
        20.
    } else {
        0.
    };
    let bottom_title = if !title.is_empty() && appearance.title_below {
        20.
    } else {
        0.
    };
    let placement = options.show_legend.then_some(appearance.legend_placement);
    let top_pad = if placement == Some(LegendPlacement::Above) {
        32.
    } else {
        14.
    } + top_title;
    let bottom_legend = if placement == Some(LegendPlacement::Below) {
        20.
    } else {
        0.
    };
    let bottom_pad =
        if options.x_label.is_empty() { 33. } else { 44. } + bottom_title + bottom_legend;
    let left_pad = if options.y_label.is_empty() { 56. } else { 66. };
    let grid = Rect::from_min_max(
        rect.min + vec2(left_pad, top_pad),
        rect.max - vec2(15., bottom_pad),
    );
    if grid.width() < 20. || grid.height() < 15. {
        return None;
    }

    let log_x = !is_polar && options.log_x;
    let log_y = !is_polar && options.log_y;

    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for point in curves
        .iter()
        .flat_map(|(_, points)| points.iter().flatten())
    {
        if let (Some(mx), Some(my)) = (map_coord(point[0], log_x), map_coord(point[1], log_y)) {
            bounds[0][0] = bounds[0][0].min(mx);
            bounds[0][1] = bounds[0][1].max(mx);
            bounds[1][0] = bounds[1][0].min(my);
            bounds[1][1] = bounds[1][1].max(my);
        }
    }
    if !bounds[0][0].is_finite() || !bounds[1][0].is_finite() {
        painter.text(
            grid.left_top() + vec2(8., 8.),
            Align2::LEFT_TOP,
            "Нет положительных точек для выбранной шкалы",
            FontId::proportional(12.),
            COLORS[1],
        );
        return None;
    }
    for (axis, (manual_min, manual_max, is_log)) in bounds.iter_mut().zip([
        (options.x_min, options.x_max, log_x),
        (options.y_min, options.y_max, log_y),
    ]) {
        let step = if (axis[1] - axis[0]).abs() < 1e-12 {
            if is_log {
                0.5
            } else {
                (axis[0].abs() * 0.1).max(1.)
            }
        } else {
            (axis[1] - axis[0]) * 0.05
        };
        axis[0] -= step;
        axis[1] += step;
        if let Some(v) = manual_min.and_then(|v| map_coord(v, is_log)) {
            axis[0] = v;
        }
        if let Some(v) = manual_max.and_then(|v| map_coord(v, is_log)) {
            axis[1] = v;
        }
        if axis[0] >= axis[1] {
            axis[1] = axis[0] + 1.0;
        }
    }

    // Apply equal scale (1:1) when requested or in polar mode (without log axes)
    if (options.equal_scale || is_polar) && !log_x && !log_y {
        let cx = 0.5 * (bounds[0][0] + bounds[0][1]);
        let cy = 0.5 * (bounds[1][0] + bounds[1][1]);
        let half_x = 0.5 * (bounds[0][1] - bounds[0][0]);
        let half_y = 0.5 * (bounds[1][1] - bounds[1][0]);
        let px_per_x = f64::from(grid.width()) / (2. * half_x);
        let px_per_y = f64::from(grid.height()) / (2. * half_y);
        let scale = px_per_x.min(px_per_y);
        let new_hx = f64::from(grid.width()) / (2. * scale);
        let new_hy = f64::from(grid.height()) / (2. * scale);
        bounds[0] = [cx - new_hx, cx + new_hx];
        bounds[1] = [cy - new_hy, cy + new_hy];
    }

    // Apply interactive zoom around center
    let zoom = zoom.clamp(0.25, 16.0);
    if (zoom - 1.0).abs() > 1e-6 {
        for axis in &mut bounds {
            let center = 0.5 * (axis[0] + axis[1]);
            let half = 0.5 * (axis[1] - axis[0]) / zoom;
            axis[0] = center - half;
            axis[1] = center + half;
        }
    }

    let [[xmin, xmax], [ymin, ymax]] = bounds;
    let project = |[x, y]: [f64; 2]| -> Option<Pos2> {
        let mx = map_coord(x, log_x)?;
        let my = map_coord(y, log_y)?;
        Some(Pos2::new(
            grid.left() + ((mx - xmin) / (xmax - xmin)) as f32 * grid.width(),
            grid.bottom() - ((my - ymin) / (ymax - ymin)) as f32 * grid.height(),
        ))
    };

    for axis in 0..2 {
        let count = appearance.divisions[axis].clamp(1, 20);
        for i in 0..=count {
            let ratio = f32::from(i) / f32::from(count);
            let (pos, align, value, log) = if axis == 0 {
                let x = egui::lerp(grid.x_range(), ratio);
                if appearance.grid[axis] {
                    painter.vline(x, grid.y_range(), Stroke::new(1_f32, line));
                }
                (
                    Pos2::new(x, grid.bottom() + 6.),
                    Align2::CENTER_TOP,
                    xmin + (xmax - xmin) * f64::from(ratio),
                    log_x,
                )
            } else {
                let y = egui::lerp(grid.y_range(), 1. - ratio);
                if appearance.grid[axis] {
                    painter.hline(grid.x_range(), y, Stroke::new(1_f32, line));
                }
                (
                    Pos2::new(grid.left() - 6., y),
                    Align2::RIGHT_CENTER,
                    ymin + (ymax - ymin) * f64::from(ratio),
                    log_y,
                )
            };
            if appearance.numbers[axis] {
                painter.text(
                    pos,
                    align,
                    appearance.tick(unmap_coord(value, log)),
                    FontId::proportional(10.),
                    ink,
                );
            }
        }
    }
    if !title.is_empty() {
        let y = if appearance.title_below {
            rect.bottom() - 11.
        } else {
            rect.top() + 11.
        };
        painter.text(
            Pos2::new(rect.center().x, y),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(13.),
            ink,
        );
    }

    let axis = Stroke::new(1_f32, Color32::from_rgb(144, 162, 170));
    let [at_x, at_y] = appearance.axes.positions(bounds, [log_x, log_y]);
    if let Some(x) = at_x {
        painter.vline(egui::lerp(grid.x_range(), x as f32), grid.y_range(), axis);
    }
    if let Some(y) = at_y {
        painter.hline(
            grid.x_range(),
            egui::lerp(grid.y_range(), 1. - y as f32),
            axis,
        );
    }

    let canvas = painter.with_clip_rect(grid);
    if is_polar {
        if let Some(origin) = project([0., 0.]) {
            let max_r = xmax.abs().max(xmin.abs()).max(ymax.abs()).max(ymin.abs());
            let px_per_unit = grid.width() / (xmax - xmin) as f32;
            for k in 1..=4 {
                let r_px = (max_r * (k as f64 / 4.0)) as f32 * px_per_unit;
                if r_px > 4.0 && r_px < grid.width().max(grid.height()) * 1.5 {
                    canvas.circle_stroke(origin, r_px, Stroke::new(0.8_f32, line));
                }
            }
        }
    }

    if !options.x_label.is_empty() {
        painter.text(
            Pos2::new(
                grid.center().x,
                rect.bottom() - 4. - bottom_title - bottom_legend,
            ),
            Align2::CENTER_BOTTOM,
            &options.x_label,
            FontId::proportional(11.),
            ink,
        );
    }
    if !options.y_label.is_empty() {
        painter.text(
            Pos2::new(rect.left() + 6., grid.top() - 2.),
            Align2::LEFT_BOTTOM,
            &options.y_label,
            FontId::proportional(11.),
            ink,
        );
    }

    let mut closest = None;
    let mut best = match trace_mode {
        TraceMode::Hover => 120.,
        _ => f32::INFINITY,
    };
    let mouse = hover.filter(|&pos| grid.contains(pos));
    let snap = mouse.filter(|_| !matches!(trace_mode, TraceMode::Cursor));
    let default_style = list_rs::editing::ChartTraceStyle::default();
    for (series, (_, points)) in curves.iter().enumerate() {
        let style = appearance.traces.get(series).unwrap_or(&default_style);
        let color = style
            .color
            .map(|[r, g, b]| Color32::from_rgb(r, g, b))
            .unwrap_or(COLORS[series % COLORS.len()]);
        let mut run = Vec::new();
        let valid_count = points.iter().flatten().count();
        let marker_step = (valid_count / 60).max(1);
        let mut drawn_pts = 0usize;
        for point in points.iter().copied().chain(std::iter::once(None)) {
            if let Some(values) = point {
                if let Some(pixel) = project(values) {
                    if let Some(mouse) = snap.filter(|_| grid.contains(pixel)) {
                        let distance = mouse.distance_sq(pixel);
                        if distance < best {
                            best = distance;
                            closest = Some((
                                pixel,
                                TraceSample {
                                    coordinates: values,
                                    series: Some(series),
                                },
                                color,
                            ));
                        }
                    }
                    if style.line == ChartLine::Points
                        || (options.markers && drawn_pts % marker_step == 0)
                    {
                        canvas.circle_filled(pixel, 2.8, color);
                    }
                    drawn_pts += 1;
                    if style.line != ChartLine::Points {
                        run.push(pixel);
                    }
                    continue;
                }
            }
            if !run.is_empty() {
                if run.len() == 1 {
                    canvas.circle_filled(run[0], 3., color);
                    run.clear();
                } else if style.line == ChartLine::Dashed {
                    canvas.extend(Shape::dashed_line(
                        &run,
                        Stroke::new(style.line_width(1.7), color),
                        7.,
                        4.,
                    ));
                    run.clear();
                } else {
                    canvas.add(Shape::line(
                        std::mem::take(&mut run),
                        Stroke::new(style.line_width(1.7), color),
                    ));
                }
            }
        }
    }

    if let Some(mouse) = mouse.filter(|_| matches!(trace_mode, TraceMode::Cursor)) {
        let x_ratio = f64::from((mouse.x - grid.left()) / grid.width());
        let y_ratio = f64::from((grid.bottom() - mouse.y) / grid.height());
        closest = Some((
            mouse,
            TraceSample {
                coordinates: [
                    unmap_coord(xmin + (xmax - xmin) * x_ratio, log_x),
                    unmap_coord(ymin + (ymax - ymin) * y_ratio, log_y),
                ],
                series: None,
            },
            ink,
        ));
    }
    if let Some((dot, sample, color)) = closest {
        let [x, y] = sample.coordinates;
        let guide = Stroke::new(0.8_f32, Color32::from_rgb(180, 194, 200));
        canvas.vline(dot.x, grid.y_range(), guide);
        canvas.hline(grid.x_range(), dot.y, guide);
        canvas.circle_filled(dot, 4., color);
        let readout = if is_polar {
            let r = x.hypot(y);
            let deg = y.atan2(x).to_degrees();
            format!(
                "x = {}   y = {}   r = {}   θ = {}°",
                tick(x),
                tick(y),
                tick(r),
                tick(deg)
            )
        } else {
            format!("x = {}   y = {}", tick(x), tick(y))
        };
        painter.text(
            rect.right_bottom() - vec2(12., 7. + bottom_title + bottom_legend),
            Align2::RIGHT_BOTTOM,
            readout,
            FontId::proportional(10.),
            ink,
        );
    }
    if let Some(placement) = placement {
        let corner = placement.corner_box(
            [
                f64::from(grid.left()),
                f64::from(grid.top()),
                f64::from(grid.right()),
                f64::from(grid.bottom()),
            ],
            curves.len(),
        );
        if let Some(panel) = corner.as_ref() {
            painter.rect(
                Rect::from_min_size(
                    Pos2::new(panel.x as f32, panel.y as f32),
                    vec2(panel.width as f32, panel.height as f32),
                ),
                3.,
                Color32::from_white_alpha(245),
                Stroke::new(1_f32, line),
                StrokeKind::Inside,
            );
        }
        let column_width = rect.width() / curves.len().max(1) as f32;
        for (series, (name, _)) in curves.iter().enumerate() {
            let (label_at, width, row_height) = if let Some(panel) = corner.as_ref() {
                (
                    Pos2::new(
                        panel.x as f32 + 5.,
                        panel.row_y(series, curves.len()) as f32,
                    ),
                    panel.width as f32 - 10.,
                    (panel.height as f32 - 8.) / curves.len() as f32,
                )
            } else if !placement.is_corner() {
                let y = if placement == LegendPlacement::Below {
                    rect.bottom() - bottom_title - 17.
                } else {
                    rect.top() + 12. + top_title
                };
                (
                    Pos2::new(rect.left() + 10. + series as f32 * column_width, y),
                    (column_width - 12.).max(0.),
                    16.,
                )
            } else {
                break;
            };
            let style = appearance.traces.get(series).unwrap_or(&default_style);
            let color = style
                .color
                .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                .unwrap_or(COLORS[series % COLORS.len()]);
            let label = if style.label.trim().is_empty() {
                name
            } else {
                &style.label
            };
            let legend = Rect::from_min_size(label_at, vec2(width, row_height));
            let swatch_y = label_at.y + 6.;
            let swatch = [
                Pos2::new(label_at.x, swatch_y),
                Pos2::new(label_at.x + 12., swatch_y),
            ];
            let stroke = Stroke::new(style.line_width(2.), color);
            match style.line {
                ChartLine::Points => {
                    painter.circle_filled(swatch[0].lerp(swatch[1], 0.5), 2.8, color);
                }
                ChartLine::Dashed => painter.extend(Shape::dashed_line(&swatch, stroke, 4., 3.)),
                ChartLine::Solid => {
                    painter.line_segment(swatch, stroke);
                }
            }
            painter.with_clip_rect(legend).text(
                label_at + vec2(16., 0.),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(11.5),
                color,
            );
        }
    }
    closest.map(|(_, sample, _)| sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_trace_snaps_across_series_and_inverts_the_zoomed_log_scale() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(400., 300.));
            let options = ChartOptions {
                log_x: true,
                show_legend: false,
                x_min: Some(1.),
                x_max: Some(1000.),
                y_min: Some(0.),
                y_max: Some(10.),
                ..Default::default()
            };
            let curves = Ok(vec![
                ("a".into(), vec![Some([1., 1.]), None, Some([1.0e8, 5.])]),
                ("b".into(), vec![Some([-1., 5.]), Some([31.6, 5.])]),
            ]);
            let center = Pos2::new(220.5, 140.5);
            let snap = draw(
                &painter,
                rect,
                &curves,
                &options,
                false,
                2.,
                Some(center),
                false,
                TraceMode::Data,
            )
            .unwrap();
            assert_eq!(snap.series, Some(1));
            assert_eq!(snap.coordinates, [31.6, 5.]);
            let cursor = draw(
                &painter,
                rect,
                &curves,
                &options,
                false,
                2.,
                Some(Pos2::new(56. + (385. - 56.) / 4., center.y)),
                false,
                TraceMode::Cursor,
            )
            .unwrap();
            assert!(cursor.series.is_none());
            assert!((cursor.coordinates[0] - 10_f64.powf(1.125)).abs() < 1e-10);
            assert_eq!(cursor.coordinates[1], 5.);
            assert!(draw(
                &painter,
                rect,
                &curves,
                &options,
                false,
                2.,
                Some(Pos2::new(20., center.y)),
                false,
                TraceMode::Data
            )
            .is_none());
        });
    }
}
