#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod chart_view;

use eframe::egui::{
    self, vec2, Align, Color32, FontData, FontDefinitions, FontFamily, FontId, Frame, Key, Margin,
    Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit,
};
use list_rs::{display_number, editing, number, read_xml, sheet::Sheet, Calc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Instant;

const INK: Color32 = Color32::from_rgb(26, 42, 54);
const MUTED: Color32 = Color32::from_rgb(79, 101, 113);
const ACCENT: Color32 = Color32::from_rgb(0, 104, 97);
const LINE: Color32 = Color32::from_rgb(218, 228, 232);
const BG: Color32 = Color32::from_rgb(234, 240, 242);
const PALE: Color32 = Color32::from_rgb(230, 246, 241);
const ERROR: Color32 = Color32::from_rgb(161, 41, 33);
const PAPER: Color32 = Color32::WHITE;

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(windows) = std::env::var_os("WINDIR") {
        let directory = PathBuf::from(windows).join("Fonts");
        for (name, file, family) in [
            ("worksheet_ui", "segoeui.ttf", FontFamily::Proportional),
            ("worksheet_code", "consola.ttf", FontFamily::Monospace),
        ] {
            if let Ok(data) = std::fs::read(directory.join(file)) {
                fonts
                    .font_data
                    .insert(name.into(), Arc::new(FontData::from_owned(data)));
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.into());
            }
        }
        if let Ok(data) = std::fs::read(directory.join("cambria.ttc")) {
            let name = "worksheet_math";
            let mut math_font = FontData::from_owned(data);
            math_font.index = 1;
            fonts.font_data.insert(name.into(), Arc::new(math_font));
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                let order = fonts.families.entry(family).or_default();
                order.insert(1.min(order.len()), name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
    let mut theme = egui::Visuals::light();
    theme.override_text_color = Some(INK);
    theme.selection.bg_fill = PALE;
    theme.selection.stroke = Stroke::new(1_f32, ACCENT);
    theme.panel_fill = PAPER;
    theme.window_fill = PAPER;
    theme.widgets.hovered.weak_bg_fill = PALE;
    theme.widgets.active.weak_bg_fill = PALE;
    ctx.set_visuals(theme);
}

struct Worksheet {
    sheet: Sheet,
    xml: String,
    name: String,
    path: Option<PathBuf>,
    rects: Vec<Rect>,
    input_indices: Vec<usize>,
    extent: egui::Vec2,
    formulas: usize,
    errors: usize,
    changed: usize,
    formula_changed: bool,
    unsaved: bool,
    origin: Pos2,
    plots: HashMap<usize, PlotRegion>,
}

struct PlotRegion {
    spec: editing::ChartSpec,
    curves: Calc<chart_view::Curves>,
}

impl Worksheet {
    fn blank() -> Calc<Self> {
        Self::load(
            include_str!("../examples/blank.xmcd"),
            "Новый лист".into(),
            None,
        )
    }

    #[cfg(test)]
    fn demo() -> Calc<Self> {
        Self::load(list_rs::DEMO, "Давление под опорой".into(), None)
    }

    fn chart_example() -> Calc<Self> {
        use editing::{ChartSpec, NewRegion};
        let mut doc = Self::blank()?;
        doc.name = "Синус и косинус".into();
        doc.insert(
            Pos2::new(14., 12.),
            NewRegion::Text("Измените A, чтобы увидеть, как меняется амплитуда синуса."),
        )?;
        doc.insert(Pos2::new(14., 52.), NewRegion::Formula("A := 2"))?;
        doc.insert(
            Pos2::new(14., 92.),
            NewRegion::Formula("f(x) := A * sin(x)"),
        )?;
        let spec = ChartSpec::Function {
            expression: "f(x); cos(x)".into(),
            from: -6.28,
            to: 6.28,
        };
        doc.insert(Pos2::new(14., 141.), NewRegion::Chart(&spec))?;
        doc.unsaved = false;
        Ok(doc)
    }

    fn from_path(path: PathBuf) -> Calc<Self> {
        let xml = read_xml(&path)?;
        let name = path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string();
        Self::load(&xml, name, Some(path))
    }

    fn load(xml: &str, name: String, path: Option<PathBuf>) -> Calc<Self> {
        let mut sheet = Sheet::import(xml)?;
        sheet.full();
        let (_, formulas, errors) = sheet.counts();
        let mut input_indices = Vec::new();
        let mut rows = HashMap::<i32, Vec<usize>>::new();
        let mut extent = if path.is_none() || sheet.regions.is_empty() {
            vec2(625., 600.)
        } else {
            vec2(540., 410.)
        };
        let left = sheet
            .regions
            .iter()
            .map(|r| r.visual.left)
            .fold(0., f32::min);
        let top = sheet
            .regions
            .iter()
            .map(|r| r.visual.top)
            .fold(0., f32::min);
        let mut rects = Vec::with_capacity(sheet.regions.len());
        for (index, region) in sheet.regions.iter().enumerate() {
            let visual = &region.visual;
            let position = Pos2::new(visual.left - left + 25., visual.top - top + 26.);
            let min_width = if region.text.is_some() { 80. } else { 240. };
            let rect = Rect::from_min_size(
                position,
                vec2(visual.width.max(min_width), visual.height.max(23.)),
            );
            extent = extent.max(rect.max.to_vec2() + vec2(25., 32.));
            rects.push(rect);
            if sheet.editable(index) {
                input_indices.push(index);
            }
            if !visual.graphical {
                rows.entry((visual.top / 3.).round() as i32)
                    .or_default()
                    .push(index);
            }
        }
        for row in rows.values_mut() {
            row.sort_by(|&a, &b| rects[a].left().total_cmp(&rects[b].left()));
            for pair in row.windows(2) {
                let (index, next) = (pair[0], pair[1]);
                let width = rects[next].left() - rects[index].left() - 7.;
                if width > 20. {
                    rects[index].max.x = rects[index].max.x.min(rects[index].left() + width);
                }
            }
        }
        let specs = editing::charts(xml)?;
        let plots = sheet
            .regions
            .iter()
            .enumerate()
            .filter_map(|(index, region)| {
                let spec = specs.get(&region.source_id)?.clone();
                let curves = sheet.chart_curves(index, &spec);
                Some((index, PlotRegion { spec, curves }))
            })
            .collect();
        Ok(Self {
            sheet,
            xml: xml.into(),
            name,
            path,
            rects,
            input_indices,
            extent,
            formulas,
            errors,
            changed: 0,
            formula_changed: false,
            unsaved: false,
            origin: Pos2::new(left, top),
            plots,
        })
    }

    fn update_input(&mut self, index: usize, number: Option<f64>) -> Calc<usize> {
        let was_edited = self.sheet.edited(index);
        self.sheet.set(index, number, None)?;
        match (was_edited, self.sheet.edited(index)) {
            (false, true) => self.changed += 1,
            (true, false) => self.changed -= 1,
            _ => {}
        }
        let count = self.sheet.incremental();
        self.errors = self.sheet.counts().2;
        self.refresh_plots();
        self.unsaved |= count > 0;
        Ok(count)
    }

    fn refresh_plots(&mut self) {
        for (&index, graph) in &mut self.plots {
            graph.curves = self.sheet.chart_curves(index, &graph.spec);
        }
    }

    fn by_id(&self, id: &str) -> Calc<usize> {
        let mut hits = self
            .sheet
            .regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.source_id == id);
        let index = hits
            .next()
            .map(|(index, _)| index)
            .ok_or("не найден регион на листе")?;
        if hits.next().is_some() {
            return Err(format!("Номер региона {id} встречается несколько раз"));
        }
        Ok(index)
    }

    fn rebuild(&self, xml: &str, replacing: Option<usize>) -> Calc<Self> {
        let mut candidate = Self::load(xml, self.name.clone(), self.path.clone())?;
        for &old in &self.input_indices {
            if Some(old) == replacing || !self.sheet.edited(old) {
                continue;
            }
            let other = candidate.by_id(&self.sheet.regions[old].source_id)?;
            let value = self
                .sheet
                .input_value(old)
                .ok_or("не удалось восстановить заданный вход")?;
            if !candidate.sheet.editable(other) {
                return Err(format!("После правки вход #{} потерял единицу или число. Сначала верните его исходное значение.", old + 1));
            }
            candidate.sheet.set(other, Some(value), None)?;
            candidate.changed += 1;
        }
        candidate.sheet.incremental();
        candidate.errors = candidate.sheet.counts().2;
        candidate.refresh_plots();
        candidate.formula_changed = true;
        candidate.unsaved = true;
        Ok(candidate)
    }

    fn insert(&mut self, at: Pos2, region: editing::NewRegion<'_>) -> Calc<usize> {
        let math = matches!(&region, editing::NewRegion::Formula(_));
        let (xml, id) = editing::insert_region(&self.xml, at.x, at.y, region)?;
        let candidate = self.rebuild(&xml, None)?;
        let index = candidate.by_id(&id)?;
        if math {
            candidate.sheet.display(index)?;
        }
        if let Some(graph) = candidate.plots.get(&index) {
            graph.curves.as_ref().map_err(Clone::clone)?;
        }
        *self = candidate;
        Ok(index)
    }

    fn change_chart(&mut self, index: usize, chart: &editing::ChartSpec) -> Calc<()> {
        let id = &self.sheet.regions[index].source_id;
        let xml = editing::replace_chart(&self.xml, id, chart)?;
        let candidate = self.rebuild(&xml, None)?;
        candidate
            .plots
            .get(&index)
            .ok_or("не найден график")?
            .curves
            .as_ref()
            .map_err(Clone::clone)?;
        *self = candidate;
        Ok(())
    }

    fn apply_formula(&mut self, index: usize, source: &str) -> Calc<()> {
        let edit = self
            .sheet
            .regions
            .get(index)
            .and_then(|region| region.visual.editor.as_ref())
            .ok_or("выберите численную формулу на листе")?;
        if edit.source.as_deref() == Some(source.trim()) && !self.sheet.edited(index) {
            return Ok(());
        }
        let edited = edit.replacement(&self.xml, source)?;
        let xml = editing::patch(&self.xml, vec![(edit.statement.clone(), edited)])?;
        let candidate = self.rebuild(&xml, Some(index))?;
        candidate.sheet.display(index)?;
        *self = candidate;
        Ok(())
    }

    fn export(&self) -> Calc<String> {
        let mut changes = Vec::with_capacity(self.changed);
        for &index in &self.input_indices {
            if !self.sheet.edited(index) {
                continue;
            }
            let range = self.sheet.regions[index]
                .visual
                .editor
                .as_ref()
                .and_then(|edit| edit.input_number.clone())
                .ok_or("в исходной формуле не найдено число для записи")?;
            let value = self
                .sheet
                .input_value(index)
                .ok_or("не удалось прочитать изменённый вход")?;
            changes.push((range, editing::literal(value)));
        }
        let xml = editing::patch(&self.xml, changes)?;
        let xml = editing::export_xml(&xml, self.formula_changed || self.changed > 0)?;
        Sheet::import(&xml)?;
        Ok(xml)
    }
}

#[derive(Clone)]
enum Open {
    Picker,
    File(PathBuf),
    Demo,
    Blank,
    Close,
}

enum DraftRegion {
    Formula(String),
    Text(String),
    Chart(Box<chart_view::Draft>),
}

struct Insertion {
    at: Pos2,
    draft: DraftRegion,
    error: Option<String>,
    focus: bool,
    replace_chart: Option<usize>,
}

struct FormulaEditor {
    index: usize,
    draft: String,
    error: Option<String>,
    focus: bool,
}

struct Workbench {
    document: Worksheet,
    load: Option<mpsc::Receiver<Calc<Worksheet>>>,
    pending: Option<Open>,
    generation: u64,
    selected: Option<usize>,
    jump: Option<usize>,
    entry: String,
    details: String,
    zoom: f32,
    fit: bool,
    show_inputs: bool,
    status: String,
    error: Option<String>,
    editor: Option<FormulaEditor>,
    allow_close: bool,
    caret: Option<Pos2>,
    insertion: Option<Insertion>,
}

impl Workbench {
    fn new(ctx: &egui::Context, path: Option<PathBuf>) -> Calc<Self> {
        install_fonts(ctx);
        let (document, error) = match path.map(Worksheet::from_path) {
            Some(Ok(document)) => (document, None),
            Some(Err(error)) => (Worksheet::blank()?, Some(error)),
            None => (Worksheet::blank()?, None),
        };
        let fit = document.extent.x > 760.;
        let show_inputs = !document.sheet.regions.is_empty();
        let status = if document.path.is_some() {
            format!(
                "Лист открыт · рассчитано формул {}",
                document.formulas - document.errors
            )
        } else {
            "Лист открыт".into()
        };
        Ok(Self {
            document,
            load: None,
            pending: None,
            generation: 0,
            selected: None,
            jump: None,
            entry: String::new(),
            details: String::new(),
            zoom: 1.22,
            fit,
            show_inputs,
            status,
            error,
            editor: None,
            allow_close: false,
            caret: None,
            insertion: None,
        })
    }

    fn request_open(&mut self, action: Open, ctx: &egui::Context) {
        if self.load.is_some() {
            return;
        }
        if self.document.unsaved {
            self.pending = Some(action);
        } else {
            self.open(action, ctx);
        }
    }

    fn open(&mut self, action: Open, ctx: &egui::Context) {
        self.pending = None;
        let path = match action {
            Open::Picker => rfd::FileDialog::new()
                .add_filter("Лист Mathcad", &["xmcd", "xmcdz"])
                .pick_file(),
            Open::File(path) => Some(path),
            Open::Demo | Open::Blank => {
                let sheet = if matches!(action, Open::Demo) {
                    Worksheet::chart_example()
                } else {
                    Worksheet::blank()
                };
                match sheet {
                    Ok(sheet) => self.replace(sheet),
                    Err(error) => self.error = Some(error),
                }
                return;
            }
            Open::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        let Some(path) = path else {
            return;
        };
        self.error = None;
        self.status = format!("Открывается {}", file_name(&path));
        let (tx, rx) = mpsc::channel();
        self.load = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Worksheet::from_path(path));
            ctx.request_repaint();
        });
    }

    fn replace(&mut self, document: Worksheet) {
        self.document = document;
        self.show_inputs = !self.document.sheet.regions.is_empty();
        self.generation = self.generation.wrapping_add(1);
        self.selected = None;
        self.jump = None;
        self.entry.clear();
        self.details.clear();
        self.editor = None;
        self.caret = None;
        self.insertion = None;
        self.error = None;
        self.fit = self.document.extent.x > 760.;
        self.zoom = 1.22;
        self.status = format!(
            "Лист открыт · рассчитано формул {}",
            self.document.formulas - self.document.errors
        );
    }

    fn select(&mut self, index: usize) {
        self.caret = None;
        if self.selected == Some(index) {
            return;
        }
        self.selected = Some(index);
        self.entry = self
            .document
            .sheet
            .input_value(index)
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.refresh_details(index);
    }

    fn refresh_details(&mut self, index: usize) {
        let region = &self.document.sheet.regions[index];
        self.details = if let Some(plot) = self.document.plots.get(&index) {
            plot.curves
                .as_ref()
                .map(|_| plot.spec.title())
                .unwrap_or_else(|error| error.clone())
        } else if let Some(text) = &region.text {
            text.clone()
        } else if region.visual.function {
            String::new()
        } else {
            self.document
                .sheet
                .details(index)
                .unwrap_or_else(|error| error)
        };
        const MAX_DETAILS: usize = 24_000;
        if let Some((position, _)) = self.details.char_indices().nth(MAX_DETAILS) {
            self.details.truncate(position);
            self.details.push_str("\n…");
        }
    }

    fn position_for_insert(&self) -> Pos2 {
        self.caret.unwrap_or_else(|| {
            let bottom = self
                .document
                .sheet
                .regions
                .iter()
                .map(|r| r.visual.top + r.visual.height)
                .fold(0., f32::max);
            Pos2::new(12., bottom + 12.)
        })
    }

    fn start_insert(&mut self, draft: DraftRegion) {
        if self.insertion.is_some() {
            return;
        }
        self.insertion = Some(Insertion {
            at: self.position_for_insert(),
            draft,
            error: None,
            focus: true,
            replace_chart: None,
        });
    }

    fn route_sheet_typing(&mut self, ctx: &egui::Context) {
        if self.caret.is_none() || self.insertion.is_some() || ctx.wants_keyboard_input() {
            return;
        }
        let typed = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Text(text) | egui::Event::Paste(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        });
        if !typed.trim().is_empty() {
            self.start_insert(DraftRegion::Formula(typed));
        }
    }

    fn edit_chart(&mut self, index: usize) {
        if let Some(graph) = self.document.plots.get(&index) {
            self.insertion = Some(Insertion {
                at: self.position_for_insert(),
                draft: DraftRegion::Chart(Box::new(chart_view::Draft::from_spec(&graph.spec))),
                error: None,
                focus: false,
                replace_chart: Some(index),
            });
        }
    }

    fn commit_insert(&mut self, insert: &Insertion) -> Calc<()> {
        let index = match &insert.draft {
            DraftRegion::Formula(value) => self
                .document
                .insert(insert.at, editing::NewRegion::Formula(value))?,
            DraftRegion::Text(value) => self
                .document
                .insert(insert.at, editing::NewRegion::Text(value))?,
            DraftRegion::Chart(value) => {
                let spec = value.spec()?;
                if let Some(index) = insert.replace_chart {
                    self.document.change_chart(index, &spec)?;
                    index
                } else {
                    self.document
                        .insert(insert.at, editing::NewRegion::Chart(&spec))?
                }
            }
        };
        self.selected = None;
        if matches!(&insert.draft, DraftRegion::Formula(_)) {
            self.details.clear();
            let visual = &self.document.sheet.regions[index].visual;
            self.caret = Some(Pos2::new(visual.left, visual.top + visual.height + 9.));
        } else {
            self.select(index);
            self.jump = Some(index);
        }
        self.error = None;
        self.status = if insert.replace_chart.is_some() {
            "График обновлён".into()
        } else {
            format!("Добавлен регион #{}", index + 1)
        };
        Ok(())
    }

    fn insert_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut insert) = self.insertion.take() else {
            return;
        };
        if matches!(&insert.draft, DraftRegion::Formula(_)) {
            self.insertion = Some(insert);
            return;
        }
        let response = egui::Modal::new(egui::Id::new("add_region")).show(ctx, |ui| {
            ui.set_width(435.);
            match &mut insert.draft {
                DraftRegion::Text(text) => {
                    ui.heading("Текст на листе");
                    let field = ui.add(
                        TextEdit::multiline(text)
                            .desired_width(435.)
                            .desired_rows(5)
                            .hint_text("Введите текст"),
                    );
                    if insert.focus {
                        field.request_focus();
                        insert.focus = false;
                    }
                }
                DraftRegion::Chart(draft) => {
                    ui.heading("График X–Y");
                    draft.fields(ui);
                }
                DraftRegion::Formula(_) => unreachable!(),
            }
            if let Some(error) = &insert.error {
                ui.label(RichText::new(error).color(ERROR));
            }
            ui.horizontal(|ui| {
                (
                    ui.button("Применить").clicked(),
                    ui.button("Отмена").clicked(),
                )
            })
            .inner
        });
        if response.inner.0 {
            match self.commit_insert(&insert) {
                Ok(()) => {
                    ctx.request_repaint();
                    return;
                }
                Err(error) => insert.error = Some(error),
            }
        } else if response.inner.1 || response.should_close() {
            return;
        }
        self.insertion = Some(insert);
    }

    fn start_formula(&mut self, index: usize) {
        let Some(edit) = self.document.sheet.regions[index].visual.editor.as_ref() else {
            return;
        };
        let draft = if self.document.sheet.edited(index) {
            let label = &self.document.sheet.regions[index].label;
            let unit = self.document.sheet.regions[index]
                .visual
                .input_unit
                .as_deref()
                .map_or(String::new(), |unit| format!(" * {unit}"));
            let value = self
                .document
                .sheet
                .input_value(index)
                .map(|n| n.to_string())
                .unwrap_or_default();
            format!("{label} := {value}{unit}")
        } else {
            edit.source.clone().unwrap_or_default()
        };
        self.editor = Some(FormulaEditor {
            index,
            draft,
            error: None,
            focus: true,
        });
    }

    fn save_copy(&mut self) -> bool {
        let stem = self
            .document
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "Лист".into());
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Лист Mathcad", &["xmcd"])
            .set_file_name(format!("{stem}_копия.xmcd"));
        if let Some(parent) = self.document.path.as_deref().and_then(Path::parent) {
            dialog = dialog.set_directory(parent);
        }
        let Some(mut path) = dialog.save_file() else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("xmcd");
        }
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xmcd"))
        {
            self.error = Some("Для копии выберите расширение .xmcd".into());
            return false;
        }
        match self
            .document
            .export()
            .and_then(|xml| editing::write_copy(&path, &xml))
        {
            Ok(()) => {
                self.document.unsaved = false;
                self.error = None;
                self.status = format!("Копия сохранена · {}", file_name(&path));
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn formula_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.editor.take() else {
            return;
        };
        let result = egui::Modal::new(egui::Id::new("formula_editor")).show(ctx, |ui| {
            ui.set_width(510.);
            ui.heading(format!("Формула #{}", editor.index + 1));
            ui.label(RichText::new("p := F / S  или  sqrt(a^2 + b^2) =").color(MUTED));
            ui.add_space(5.);
            let response = ui.add(
                TextEdit::multiline(&mut editor.draft)
                    .code_editor()
                    .desired_width(510.)
                    .desired_rows(4)
                    .char_limit(16_384)
                    .hint_text("Введите формулу целиком"),
            );
            if editor.focus {
                response.request_focus();
                editor.focus = false;
            }
            if let Some(error) = &editor.error {
                ui.label(RichText::new(error).color(ERROR));
            }
            let mut apply = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                apply = ui.button("Применить").clicked();
                cancel = ui.button("Отмена").clicked();
            });
            (apply, cancel)
        });
        let apply = result.inner.0 || ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Enter));
        if apply {
            match self.document.apply_formula(editor.index, &editor.draft) {
                Ok(()) => {
                    self.selected = None;
                    self.select(editor.index);
                    self.error = None;
                    self.status = format!(
                        "Формула #{} применена · пересчитано {}",
                        editor.index + 1,
                        self.document.formulas
                    );
                    ctx.request_repaint();
                    return;
                }
                Err(error) => editor.error = Some(error),
            }
        } else if result.inner.1 || result.should_close() {
            return;
        }
        self.editor = Some(editor);
    }

    fn apply(&mut self, index: usize, reset: bool) {
        let value = if reset {
            Ok(None)
        } else {
            number(self.entry.trim()).map(Some)
        };
        let value = match value {
            Ok(Some(value)) if self.document.sheet.input_value(index) == Some(value) => return,
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let start = Instant::now();
        match self.document.update_input(index, value) {
            Ok(regions) => {
                self.refresh_details(index);
                self.entry = self
                    .document
                    .sheet
                    .input_value(index)
                    .map(|value| value.to_string())
                    .unwrap_or_default();
                self.error = None;
                self.status = format!(
                    "Пересчитано формул: {regions} · {:.2} мс",
                    start.elapsed().as_secs_f64() * 1e3
                );
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(56.)
            .frame(
                Frame::new()
                    .fill(PAPER)
                    .inner_margin(Margin::symmetric(18, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ЛИСТ").size(20.).strong().color(ACCENT));
                    ui.separator();
                    let title_width = (ui.available_width() - 477.).max(130.);
                    ui.allocate_ui_with_layout(
                        vec2(title_width, 34.),
                        egui::Layout::top_down(Align::Min),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.document.name).strong().size(16.),
                                )
                                .truncate(),
                            );
                            if let Some(path) = &self.document.path {
                                ui.label(RichText::new("Лист Mathcad").size(11.).color(MUTED))
                                    .on_hover_text(path.display().to_string());
                            } else {
                                ui.label(RichText::new("Рабочий лист").size(11.).color(MUTED));
                            }
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let open =
                            egui::Button::new(RichText::new("Открыть…").color(PAPER).strong())
                                .fill(ACCENT);
                        if ui
                            .add_enabled(self.load.is_none(), open)
                            .on_hover_text("Ctrl+O")
                            .clicked()
                        {
                            self.request_open(Open::Picker, ctx);
                        }
                        if ui.button("Новый").on_hover_text("Ctrl+N").clicked() {
                            self.request_open(Open::Blank, ctx);
                        }
                        if ui
                            .add_enabled(self.load.is_none(), egui::Button::new("Сохранить копию…"))
                            .on_hover_text("Ctrl+S")
                            .clicked()
                        {
                            self.save_copy();
                        }
                        if ui
                            .add_enabled(self.load.is_none(), egui::Button::new("Пример"))
                            .clicked()
                        {
                            self.request_open(Open::Demo, ctx);
                        }
                        if ui
                            .selectable_label(self.show_inputs, "Панель входов")
                            .clicked()
                        {
                            self.show_inputs = !self.show_inputs;
                        }
                    });
                });
            });
    }

    fn footer(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                Frame::new()
                    .fill(PAPER)
                    .inner_margin(Margin::symmetric(18, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(error) = &self.error {
                        ui.add(
                            egui::Label::new(RichText::new(error).color(ERROR).size(12.))
                                .truncate(),
                        )
                        .on_hover_text(error);
                    } else {
                        ui.label(RichText::new(&self.status).color(MUTED).size(12.));
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let summary = format!(
                            "{} формул · {} входов · {} ошибок",
                            self.document.formulas,
                            self.document.input_indices.len(),
                            self.document.errors
                        );
                        ui.label(RichText::new(summary).color(MUTED).size(12.));
                        if self.document.unsaved {
                            ui.separator();
                            ui.label(RichText::new("Есть изменения").color(ACCENT).size(12.));
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        if !self.show_inputs {
            return;
        }
        egui::SidePanel::right("inputs")
            .resizable(true)
            .default_width(276.)
            .min_width(225.)
            .frame(Frame::new().fill(PAPER).inner_margin(Margin::same(14)))
            .show(ctx, |ui| {
                ui.heading(RichText::new("Входные значения").size(17.));
                ui.add_space(5.);
                if self.document.input_indices.is_empty() {
                    ui.label(
                        RichText::new("В листе нет доступных для изменения входов.")
                            .color(MUTED)
                            .size(12.),
                    );
                } else {
                    let mut action = None;
                    let item_height = 25.;
                    egui::ScrollArea::vertical()
                        .id_salt(("input_listing", self.generation))
                        .max_height(190.)
                        .auto_shrink([false, true])
                        .show_rows(
                            ui,
                            item_height,
                            self.document.input_indices.len(),
                            |ui, rows| {
                                for row in rows {
                                    let index = self.document.input_indices[row];
                                    let region = &self.document.sheet.regions[index];
                                    let value = self
                                        .document
                                        .sheet
                                        .input_value(index)
                                        .map(display_number)
                                        .unwrap_or_else(|| "—".into());
                                    let unit =
                                        region.visual.input_unit.as_deref().unwrap_or_default();
                                    let label = format!("{}   {} {}", region.label, value, unit);
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), item_height],
                                            egui::Button::selectable(
                                                self.selected == Some(index),
                                                RichText::new(label).size(13.),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        action = Some(index);
                                    }
                                }
                            },
                        );
                    if let Some(index) = action {
                        self.select(index);
                        self.jump = Some(index);
                    }
                }
                ui.add_space(11.);
                ui.separator();
                ui.add_space(8.);
                ui.heading(RichText::new("Выбранный регион").size(16.));
                let Some(index) = self.selected else {
                    ui.label(
                        RichText::new("Выберите формулу или значение на листе.")
                            .size(12.)
                            .color(MUTED),
                    );
                    return;
                };
                let region = &self.document.sheet.regions[index];
                ui.label(
                    RichText::new(format!("#{} · XMCD {}", index + 1, region.source_id))
                        .size(11.)
                        .color(MUTED),
                );
                let input = self.document.sheet.editable(index);
                if !region.visual.formula.is_empty() && !input {
                    ui.add_space(7.);
                    ui.label(RichText::new(&region.visual.formula).size(14.));
                }
                if region.visual.editor.is_some()
                    && ui.button("Править формулу…").on_hover_text("F2").clicked()
                {
                    self.start_formula(index);
                }
                if self.document.plots.contains_key(&index)
                    && ui.button("Настроить график…").clicked()
                {
                    self.edit_chart(index);
                }
                if input {
                    ui.add_space(10.);
                    ui.label(
                        RichText::new("Enter в поле на листе применяет число.")
                            .size(12.)
                            .color(MUTED),
                    );
                    if ui
                        .add_enabled(
                            self.document.sheet.edited(index),
                            egui::Button::new("Вернуть исходное"),
                        )
                        .clicked()
                    {
                        self.apply(index, true);
                    }
                }
                if !self.details.is_empty() {
                    ui.add_space(7.);
                    egui::ScrollArea::both()
                        .id_salt(("details", self.generation, index))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.details).monospace().size(13.),
                                )
                                .wrap(),
                            );
                        });
                }
            });
    }

    fn workspace(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(15, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("+ Формула").clicked() {
                                self.start_insert(DraftRegion::Formula(String::new()));
                            }
                            if ui.button("+ Текст").clicked() {
                                self.start_insert(DraftRegion::Text(String::new()));
                            }
                            if ui.button("+ График").clicked() {
                                self.start_insert(DraftRegion::Chart(Box::default()));
                            }
                            ui.separator();
                            let zoom = self.effective_zoom(ui.available_width());
                            if ui.button("−").on_hover_text("Уменьшить").clicked() {
                                self.fit = false;
                                self.zoom = (zoom / 1.17).max(0.50);
                            }
                            ui.label(
                                RichText::new(format!("{:.0} %", zoom / 1.22 * 100.))
                                    .size(12.)
                                    .color(MUTED),
                            );
                            if ui.button("+").on_hover_text("Увеличить").clicked() {
                                self.fit = false;
                                self.zoom = (zoom * 1.17).min(2.8);
                            }
                            if ui.selectable_label(self.fit, "По ширине").clicked() {
                                self.fit = !self.fit;
                            }
                        });
                    });
                let zoom = self.effective_zoom(ui.available_width());
                egui::ScrollArea::both()
                    .id_salt(("sheet_view", self.generation))
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, _viewport| self.paper(ui, zoom));
            });
    }

    fn effective_zoom(&self, width: f32) -> f32 {
        if !self.fit {
            return self.zoom;
        }
        ((width - 38.) / self.document.extent.x).clamp(0.50, 1.22)
    }

    fn paper(&mut self, ui: &mut egui::Ui, zoom: f32) {
        let potential = self.insertion.as_ref().map(|draft| draft.at).or(self.caret);
        let extended = potential.map_or(self.document.extent, |point| {
            self.document
                .extent
                .max((point - self.document.origin) + vec2(275., 105.))
        });
        let page_size = extended * zoom;
        let margin = 13.;
        let width = ui.available_width().max(page_size.x + 2. * margin);
        let (space, _) =
            ui.allocate_exact_size(vec2(width, page_size.y + 2. * margin), Sense::hover());
        let page = Rect::from_min_size(
            Pos2::new(space.center().x - page_size.x / 2., space.top() + margin),
            page_size,
        );
        let painter = ui.painter();
        painter.rect_filled(page.translate(vec2(2., 3.)), 8., LINE);
        painter.rect(
            page,
            8.,
            PAPER,
            Stroke::new(1_f32, LINE),
            StrokeKind::Inside,
        );
        let sheet_response = ui.interact(
            page,
            egui::Id::new(("paper", self.generation)),
            Sense::click(),
        );
        if self.document.sheet.regions.is_empty()
            && self.insertion.is_none()
            && self.caret.is_none()
        {
            ui.painter().text(
                page.left_top() + vec2(37., 39.),
                egui::Align2::LEFT_TOP,
                "Щёлкните на листе и начните вводить формулу",
                FontId::proportional(17.),
                MUTED,
            );
            ui.painter().text(
                page.left_top() + vec2(37., 67.),
                egui::Align2::LEFT_TOP,
                "Для начала введите x := 2, затем на строке ниже x^2 =",
                FontId::proportional(13.),
                MUTED,
            );
        }
        if let Some(at) = self.caret.filter(|_| self.insertion.is_none()) {
            let pixel = page.min + ((at - self.document.origin) + vec2(25., 26.)) * zoom;
            let stroke = Stroke::new(1.2_f32, ACCENT);
            ui.painter()
                .line_segment([pixel - vec2(5., 0.), pixel + vec2(7., 0.)], stroke);
            ui.painter()
                .line_segment([pixel - vec2(0., 5.), pixel + vec2(0., 7.)], stroke);
        }
        let mut next = None;
        let mut submit = None;
        let mut edit_region = None;
        let clip = ui.clip_rect();
        for index in 0..self.document.rects.len() {
            let original = self.document.rects[index];
            let rect = Rect::from_min_size(
                page.min + original.min.to_vec2() * zoom,
                original.size() * zoom,
            );
            if self.jump == Some(index) {
                ui.scroll_to_rect(rect, Some(Align::Center));
                self.jump = None;
            }
            if !rect.intersects(clip) {
                continue;
            }
            let region = &self.document.sheet.regions[index];
            let selected = self.selected == Some(index);
            if let Some(plot) = self.document.plots.get(&index) {
                let response = ui.interact(
                    rect,
                    egui::Id::new((self.generation, index)),
                    Sense::click(),
                );
                chart_view::draw(
                    ui.painter(),
                    rect,
                    &plot.curves,
                    response.hover_pos(),
                    selected,
                );
                if response.double_clicked() {
                    edit_region = Some((index, true));
                }
                if response.clicked() {
                    next = Some(index);
                }
                continue;
            }
            if self.document.sheet.editable(index) {
                let prefix = format!("{} :=", region.label);
                let font = FontId::proportional(14. * zoom);
                let prefix = ui.painter().layout_no_wrap(prefix, font.clone(), INK);
                let mut chip = Rect::from_min_size(
                    rect.min - vec2(5., 1.),
                    vec2(
                        rect.width().min(170. * zoom),
                        (23. * zoom).min(rect.height()),
                    ),
                );
                if chip.width() < 44.
                    || (selected
                        && self.entry.is_empty()
                        && self.document.sheet.input_value(index).is_none())
                {
                    let caption = format!(
                        "{} := {}",
                        region.label,
                        self.document
                            .sheet
                            .input_value(index)
                            .map(display_number)
                            .unwrap_or_else(|| "—".into())
                    );
                    let response = draw_line(
                        ui,
                        rect,
                        &caption,
                        font,
                        ACCENT,
                        selected,
                        (self.generation, index),
                    );
                    if response.clicked() {
                        next = Some(index);
                    }
                    continue;
                }
                let label_min = prefix.rect.width() + 17.;
                chip.max.x = chip.max.x.max(chip.min.x + label_min + 22.);
                if !selected {
                    let value = self
                        .document
                        .sheet
                        .input_value(index)
                        .map(display_number)
                        .unwrap_or_else(|| "—".into());
                    let unit = region.visual.input_unit.as_deref().unwrap_or_default();
                    let caption = format!("{} := {value} {unit}", region.label);
                    ui.painter().rect_filled(chip, 5., PALE);
                    let response = draw_line(
                        ui,
                        chip.shrink2(vec2(6., 1.)),
                        &caption,
                        font,
                        ACCENT,
                        false,
                        (self.generation, index),
                    );
                    if response.clicked() {
                        next = Some(index);
                    }
                } else {
                    ui.painter().rect(
                        chip,
                        5.,
                        PALE,
                        Stroke::new(1_f32, ACCENT),
                        StrokeKind::Inside,
                    );
                    let top = chip.center().y - prefix.rect.height() / 2.;
                    ui.painter()
                        .galley(Pos2::new(chip.left() + 5., top), prefix, INK);
                    let unit = region.visual.input_unit.as_deref().unwrap_or_default();
                    let unit_width = if unit.is_empty() { 0. } else { 30. * zoom };
                    let origin = Pos2::new(chip.left() + label_min, chip.top() + 2.);
                    let edit_width = (chip.right() - origin.x - unit_width - 3.).max(25.);
                    let edit_rect =
                        Rect::from_min_size(origin, vec2(edit_width, chip.height() - 4.));
                    let id = egui::Id::new(("entry", self.generation, index));
                    let response = ui.put(
                        edit_rect,
                        TextEdit::singleline(&mut self.entry)
                            .id(id)
                            .char_limit(80)
                            .font(font.clone())
                            .desired_width(edit_width)
                            .margin(Margin::symmetric(4, 1)),
                    );
                    if !unit.is_empty() {
                        ui.painter().text(
                            Pos2::new(edit_rect.right() + 4., chip.center().y),
                            egui::Align2::LEFT_CENTER,
                            unit,
                            FontId::proportional(11. * zoom),
                            MUTED,
                        );
                    }
                    if (response.has_focus() || response.lost_focus())
                        && ui.input(|i| i.key_pressed(Key::Enter))
                    {
                        submit = Some(index);
                    }
                }
                continue;
            }
            if let Some(text) = &region.text {
                let color = if region.visual.heading > 0 {
                    INK
                } else {
                    MUTED
                };
                let size = match region.visual.heading {
                    1 => 22.,
                    2 => 17.,
                    _ => 12.5,
                };
                if region.visual.graphical {
                    let box_rect = Rect::from_min_size(
                        rect.min,
                        vec2(rect.width().min(360. * zoom), rect.height().min(70. * zoom)),
                    );
                    ui.painter().rect(
                        box_rect,
                        5.,
                        BG,
                        Stroke::new(1_f32, LINE),
                        StrokeKind::Inside,
                    );
                    let response = draw_line(
                        ui,
                        box_rect.shrink(8.),
                        "Встроенный график Mathcad",
                        FontId::proportional(12. * zoom),
                        MUTED,
                        false,
                        (self.generation, index),
                    );
                    if response.clicked() {
                        next = Some(index);
                    }
                } else {
                    let font = FontId::proportional(size * zoom);
                    let galley =
                        ui.painter()
                            .layout(text.clone(), font, color, rect.width().max(60.));
                    let area = Rect::from_min_size(
                        rect.min,
                        vec2(
                            rect.width(),
                            rect.height().max(galley.rect.height().min(40. * zoom)),
                        ),
                    );
                    if selected {
                        ui.painter().rect_filled(area, 4., PALE);
                    }
                    ui.painter()
                        .with_clip_rect(area)
                        .galley(rect.min, galley, color);
                    let response = ui.interact(
                        area,
                        egui::Id::new((self.generation, index)),
                        Sense::click(),
                    );
                    if response.clicked() {
                        next = Some(index);
                    }
                }
                continue;
            }
            let formula = &region.visual.formula;
            let result = self.document.sheet.display(index);
            let (suffix, color) = match &result {
                Ok(_) if region.visual.function => (String::new(), ACCENT),
                Ok(v) => (format!("  =  {v}"), ACCENT),
                Err(_) => ("  · ошибка".into(), ERROR),
            };
            let mut layout = egui::text::LayoutJob::default();
            layout.wrap.max_width = rect.width();
            layout.wrap.max_rows = if rect.height() >= 34. * zoom { 2 } else { 1 };
            let fmt = |color| egui::TextFormat {
                font_id: FontId::proportional(14. * zoom),
                color,
                ..Default::default()
            };
            layout.append(formula, 0., fmt(INK));
            layout.append(&suffix, 0., fmt(color));
            let galley = ui.painter().layout_job(layout);
            let hit = Rect::from_min_size(
                rect.min,
                vec2((galley.rect.width() + 6.).min(rect.width()), rect.height()),
            );
            if selected {
                ui.painter().rect_filled(hit.expand(3.), 5., PALE);
            }
            ui.painter()
                .with_clip_rect(rect)
                .galley(rect.min + vec2(0., 2.), galley, INK);
            let response =
                ui.interact(hit, egui::Id::new((self.generation, index)), Sense::click());
            let response = response.on_hover_text(format!(
                "{formula}{suffix}\n{}",
                result.err().unwrap_or_default()
            ));
            if response.double_clicked() {
                edit_region = Some((index, false));
            }
            if response.clicked() {
                next = Some(index);
            }
        }
        if let Some(index) = submit {
            self.apply(index, false);
            ui.ctx().request_repaint();
        }
        if let Some(index) = next {
            self.select(index);
            if self.document.sheet.editable(index) {
                ui.memory_mut(|memory| {
                    memory.request_focus(egui::Id::new(("entry", self.generation, index)))
                });
            }
            ui.ctx().request_repaint();
        } else if self.insertion.is_none() && sheet_response.clicked() {
            if let Some(click) = sheet_response.interact_pointer_pos() {
                let relative = (click - page.min) / zoom - vec2(25., 26.);
                self.caret = Some(Pos2::new(
                    (relative.x + self.document.origin.x)
                        .max(0.)
                        .min((extended.x - 190.).max(0.)),
                    (relative.y + self.document.origin.y).max(0.),
                ));
                self.selected = None;
                self.details.clear();
                ui.memory_mut(|memory| {
                    if let Some(focused) = memory.focused() {
                        memory.surrender_focus(focused);
                    }
                });
                ui.ctx().request_repaint();
            }
        }
        if let Some((index, is_chart)) = edit_region {
            if is_chart {
                self.edit_chart(index);
            } else {
                self.start_formula(index);
            }
        }
        self.inline_formula(ui, page, zoom);
    }

    fn inline_formula(&mut self, ui: &mut egui::Ui, page: Rect, zoom: f32) {
        let Some(mut insert) = self.insertion.take() else {
            return;
        };
        let DraftRegion::Formula(text) = &mut insert.draft else {
            self.insertion = Some(insert);
            return;
        };
        let point = page.min + ((insert.at - self.document.origin) + vec2(25., 26.)) * zoom;
        let width = (page.right() - point.x - 35.).clamp(120., 440.);
        let field = Rect::from_min_size(point, vec2(width, 27.));
        let response = ui.put(
            field,
            TextEdit::singleline(text)
                .font(FontId::monospace(15.))
                .char_limit(4096)
                .desired_width(width)
                .hint_text("Формула  x := 2  или  x^2 ="),
        );
        if insert.focus {
            ui.scroll_to_rect(field, Some(Align::Center));
            response.request_focus();
            insert.focus = false;
        }
        let ok = ui
            .put(
                Rect::from_min_size(field.right_top() + vec2(5., 0.), vec2(26., 26.)),
                egui::Button::new("✓").small(),
            )
            .on_hover_text("Применить формулу");
        let enter = (response.has_focus() || response.lost_focus())
            && ui.input(|input| input.key_pressed(Key::Enter));
        if ok.clicked() || enter {
            match self.commit_insert(&insert) {
                Ok(()) => {
                    ui.ctx().request_repaint();
                    return;
                }
                Err(error) => {
                    insert.error = Some(error);
                    response.request_focus();
                }
            }
        }
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            return;
        }
        if let Some(error) = &insert.error {
            ui.painter().text(
                field.left_bottom() + vec2(0., 3.),
                egui::Align2::LEFT_TOP,
                error,
                FontId::proportional(12.),
                ERROR,
            );
        }
        self.insertion = Some(insert);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn draw_line(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    selected: bool,
    id: (u64, usize),
) -> egui::Response {
    if selected {
        ui.painter().rect_filled(rect, 4., PALE);
    }
    ui.painter().with_clip_rect(rect).text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    ui.interact(rect, egui::Id::new(id), Sense::click())
        .on_hover_text(text)
}

impl eframe::App for Workbench {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(receiver) = &self.load {
            match receiver.try_recv() {
                Ok(Ok(document)) => {
                    self.load = None;
                    self.replace(document);
                }
                Ok(Err(error)) => {
                    self.load = None;
                    self.error = Some(error);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.load = None;
                    self.error = Some("Не удалось загрузить лист".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.allow_close
            && self.document.unsaved
            && ctx.input(|input| input.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Open::Close);
            self.editor = None;
        }
        if self.pending.is_none() && self.editor.is_none() && self.load.is_none() {
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::O)) {
                self.request_open(Open::Picker, ctx);
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::S)) {
                self.save_copy();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::N)) {
                self.request_open(Open::Blank, ctx);
            }
            self.route_sheet_typing(ctx);
            if ctx.input(|i| i.key_pressed(Key::F2)) {
                if let Some(index) = self.selected {
                    self.start_formula(index);
                }
            }
            if let Some(path) =
                ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()))
            {
                self.request_open(Open::File(path), ctx);
            }
        }
        self.toolbar(ctx);
        self.footer(ctx);
        self.sidebar(ctx);
        self.workspace(ctx);
        if let Some(action) = self.pending.clone() {
            let response = egui::Modal::new(egui::Id::new("discard_inputs")).show(ctx, |ui| {
                ui.heading("Есть несохранённые изменения");
                ui.label("Сохранить текущий лист в отдельный файл?");
                ui.horizontal(|ui| {
                    (
                        ui.button("Сохранить копию…").clicked(),
                        ui.button("Продолжить без сохранения").clicked(),
                        ui.button("Отмена").clicked(),
                    )
                })
                .inner
            });
            if response.inner.1 || (response.inner.0 && self.save_copy()) {
                self.open(action, ctx);
            } else if response.inner.2 || response.should_close() {
                self.pending = None;
            }
        } else {
            self.formula_dialog(ctx);
            self.insert_dialog(ctx);
        }
    }
}

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let example = path.as_ref().is_some_and(|p| p.as_os_str() == "--example");
    eframe::run_native(
        concat!("Лист ", env!("CARGO_PKG_VERSION"), " · рабочая среда"),
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 800.])
                .with_min_inner_size([780., 560.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = Workbench::new(&cc.egui_ctx, path.filter(|_| !example))?;
            if example {
                app.replace(Worksheet::chart_example()?);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_and_type_on_an_empty_page_then_press_enter() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let mut tick = |events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1080., 740.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.route_sheet_typing(ctx);
                    app.workspace(ctx);
                },
            );
        };
        let pos = Pos2::new(380., 180.);
        tick(Vec::new());
        for pressed in [true, false] {
            tick(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        tick(vec![egui::Event::Text("a := 2".into())]);
        tick(vec![egui::Event::Key {
            key: Key::Enter,
            physical_key: Some(Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(app.document.sheet.input_value(0), Some(2.));
        assert!(app.caret.is_some());
    }

    #[test]
    fn chart_mode_aligns_arrays_by_index() {
        let mut doc = Worksheet::blank().unwrap();
        doc.insert(Pos2::new(10., 0.), editing::NewRegion::Formula("X := 0..2"))
            .unwrap();
        doc.insert(
            Pos2::new(10., 40.),
            editing::NewRegion::Formula("Y := [0; 1; 4]"),
        )
        .unwrap();
        let spec = editing::ChartSpec::Vectors {
            x: "X".into(),
            y: "Y".into(),
        };
        let plot = doc
            .insert(Pos2::new(10., 83.), editing::NewRegion::Chart(&spec))
            .unwrap();
        assert_eq!(
            doc.plots[&plot].curves.as_ref().unwrap()[0].1,
            vec![Some([0., 0.]), Some([1., 1.]), Some([2., 4.])]
        );
    }

    #[test]
    fn creates_a_sheet_with_live_graph_and_reopens_it() {
        let mut doc = Worksheet::blank().unwrap();
        assert!(doc.sheet.regions.is_empty());
        let a = doc
            .insert(Pos2::new(18., 46.), editing::NewRegion::Formula("a := 2"))
            .unwrap();
        doc.update_input(a, Some(4.)).unwrap();
        let result = doc
            .insert(Pos2::new(18., 86.), editing::NewRegion::Formula("a^2 ="))
            .unwrap();
        assert_eq!(doc.sheet.display(result).unwrap(), "16");
        doc.insert(Pos2::new(18., 2.), editing::NewRegion::Text("Расчёт A & B"))
            .unwrap();
        let chart = editing::ChartSpec::Function {
            expression: "a*x; x^2".into(),
            from: -2.,
            to: 2.,
        };
        let plot = doc
            .insert(Pos2::new(18., 134.), editing::NewRegion::Chart(&chart))
            .unwrap();
        let point = |doc: &Worksheet| doc.plots[&plot].curves.as_ref().unwrap()[0].1[320].unwrap();
        assert_eq!(point(&doc), [2., 8.]);
        let a = doc
            .sheet
            .regions
            .iter()
            .position(|r| r.label == "a")
            .unwrap();
        doc.update_input(a, Some(9.)).unwrap();
        assert_eq!(point(&doc), [2., 18.]);
        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(point(&reopened), [2., 18.]);
        assert_eq!(
            reopened.sheet.regions[0].text.as_deref(),
            Some("Расчёт A & B")
        );
    }

    #[test]
    fn formula_edits_and_inline_inputs_survive_xmcd_roundtrip() {
        let mut doc = Worksheet::demo().unwrap();
        doc.update_input(1, Some(60.)).unwrap();
        assert!(doc.apply_formula(4, "p := F /").is_err());
        assert_eq!(doc.sheet.display(5).unwrap(), "1.5 MPa");
        assert!(doc.apply_formula(4, "p := F / 0").is_err());
        assert_eq!(doc.sheet.display(5).unwrap(), "1.5 MPa");
        doc.apply_formula(4, "p := 2 * F / S").unwrap();
        assert_eq!(doc.sheet.display(5).unwrap(), "3 MPa");
        assert_eq!(doc.sheet.input_value(1), Some(60.));
        assert!(doc.unsaved);
        assert!(list_rs::DEMO.contains("<ml:real>40</ml:real>"));
        let xml = doc.export().unwrap();
        let reopened = Worksheet::load(&xml, "Копия".into(), None).unwrap();
        assert_eq!(reopened.sheet.input_value(1), Some(60.));
        assert_eq!(reopened.sheet.display(5).unwrap(), "3 MPa");
        assert!(xml.contains("Измените нагрузку"));
    }

    #[test]
    fn formula_dialog_applies_a_draft_with_ctrl_enter() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        app.replace(Worksheet::demo().unwrap());
        app.start_formula(4);
        app.editor.as_mut().unwrap().draft = "p := 2 * F / S".into();
        let size = Some(Rect::from_min_size(Pos2::ZERO, vec2(1080., 740.)));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: size,
                ..Default::default()
            },
            |ctx| app.formula_dialog(ctx),
        );
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: size,
                modifiers: egui::Modifiers::CTRL,
                events: vec![egui::Event::Key {
                    key: Key::Enter,
                    physical_key: Some(Key::Enter),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::CTRL,
                }],
                ..Default::default()
            },
            |ctx| app.formula_dialog(ctx),
        );
        assert!(app.editor.is_none());
        assert_eq!(app.document.sheet.display(5).unwrap(), "2 MPa");
    }

    #[test]
    fn worksheet_uses_imported_coordinates_and_keeps_unit_when_editing() {
        let mut document = Worksheet::demo().unwrap();
        assert_eq!(document.input_indices, [1, 2]);
        assert!(document.rects[2].left() > document.rects[1].left());
        assert_eq!(document.rects[2].top(), document.rects[1].top());
        assert_eq!(document.sheet.regions[1].visual.formula, "F := 40 · kN");
        assert_eq!(document.sheet.input_value(1), Some(40.));
        assert_eq!(document.update_input(1, Some(60.)).unwrap(), 3);
        assert_eq!(document.changed, 1);
        assert_eq!(document.sheet.display(5).unwrap(), "1.5 MPa");
        assert_eq!(document.update_input(1, None).unwrap(), 3);
        assert_eq!(document.changed, 0);
        assert_eq!(document.sheet.display(5).unwrap(), "1 MPa");
    }

    #[test]
    fn pressing_enter_in_the_inline_editor_updates_the_result() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        app.replace(Worksheet::demo().unwrap());
        app.select(1);
        app.entry = "60".into();
        let raw_input = |events| egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1080., 740.))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(raw_input(Vec::new()), |ctx| app.workspace(ctx));
        ctx.memory_mut(|memory| {
            memory.request_focus(egui::Id::new(("entry", app.generation, 1usize)))
        });
        let _ = ctx.run(
            raw_input(vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: Some(Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]),
            |ctx| app.workspace(ctx),
        );
        assert_eq!(app.document.sheet.display(5).unwrap(), "1.5 MPa");
    }
}
