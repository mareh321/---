#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod chart_view;
mod command_icons;
mod math_view;
mod plot3d_view;
mod tool_ribbon;

use eframe::egui::{
    self, vec2, Align, Color32, FontData, FontDefinitions, FontFamily, FontId, Frame, Key, Margin,
    Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit,
};
use list_rs::{
    data_io, display_number, editing,
    editing::{TextAlignment, TextFont, TextFormat},
    math_editor::{parse_source_to_slot, Cursor, MathEditor},
    number, read_xml, report,
    rich_text::RichText as StyledText,
    sheet::Sheet,
    Calc,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime};

const INK: Color32 = Color32::from_rgb(22, 34, 52);
const MUTED: Color32 = Color32::from_rgb(76, 94, 116);
const ACCENT: Color32 = Color32::from_rgb(21, 94, 184);
const LINE: Color32 = Color32::from_rgb(206, 218, 232);
const PAPER_CHIP_BG: Color32 = Color32::from_rgb(236, 242, 250);
const PALE: Color32 = Color32::from_rgb(228, 240, 255);
const ERROR: Color32 = Color32::from_rgb(176, 36, 36);
const PAPER: Color32 = Color32::WHITE;
const INDEX_INK: Color32 = Color32::from_rgb(56, 72, 168);
const CROSSHAIR: Color32 = Color32::from_rgb(196, 43, 28);
const GRID_DOT: Color32 = Color32::from_rgb(212, 222, 236);

const NORMAL_ZOOM: f32 = 1.22;
const RESULT_PRECISIONS: [Option<usize>; 5] = [None, Some(3), Some(4), Some(6), Some(8)];

const MENU_BG: Color32 = Color32::from_rgb(13, 22, 38);
const ERROR_DARK: Color32 = Color32::from_rgb(255, 138, 128);
const WARN_DARK: Color32 = Color32::from_rgb(255, 222, 138);

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
    let windows_fonts = std::env::var_os("WINDIR").map(|path| PathBuf::from(path).join("Fonts"));
    for (font, prefix) in [
        (TextFont::Arial, "arial"),
        (TextFont::Times, "times"),
        (TextFont::Courier, "cour"),
    ] {
        for (bold, italic, suffix) in [
            (false, false, ""),
            (true, false, "bd"),
            (false, true, "i"),
            (true, true, "bi"),
        ] {
            let family = text_font_family(font, bold, italic);
            let mut order = fonts.families[&FontFamily::Proportional].clone();
            if let Some(data) = windows_fonts
                .as_ref()
                .and_then(|path| std::fs::read(path.join(format!("{prefix}{suffix}.ttf"))).ok())
            {
                let name = format!("{prefix}{suffix}");
                fonts
                    .font_data
                    .insert(name.clone(), Arc::new(FontData::from_owned(data)));
                order.insert(0, name);
            }
            fonts.families.insert(family, order);
        }
    }
    ctx.set_fonts(fonts);
    let mut theme = egui::Visuals::dark();
    theme.override_text_color = Some(tool_ribbon::TEXT_WHITE);
    theme.panel_fill = tool_ribbon::PANEL_DARK;
    theme.window_fill = tool_ribbon::PANEL_SURFACE;
    theme.window_stroke = Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK);
    theme.extreme_bg_color = tool_ribbon::INPUT_BG;
    theme.faint_bg_color = tool_ribbon::PANEL_SURFACE;
    theme.code_bg_color = tool_ribbon::INPUT_BG;
    theme.selection.bg_fill = tool_ribbon::BLUE_ACCENT_SOFT;
    theme.selection.stroke = Stroke::new(1.0_f32, tool_ribbon::BLUE_ACCENT_BORDER);
    theme.hyperlink_color = tool_ribbon::BLUE_ACCENT_BORDER;
    theme.error_fg_color = ERROR_DARK;
    theme.warn_fg_color = WARN_DARK;
    theme.widgets.noninteractive.bg_fill = tool_ribbon::PANEL_DARK;
    theme.widgets.noninteractive.weak_bg_fill = tool_ribbon::PANEL_SURFACE;
    theme.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK);
    theme.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, tool_ribbon::TEXT_WHITE);
    theme.widgets.inactive.bg_fill = tool_ribbon::PANEL_ELEVATED;
    theme.widgets.inactive.weak_bg_fill = tool_ribbon::PANEL_SURFACE;
    theme.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK);
    theme.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, tool_ribbon::TEXT_WHITE);
    theme.widgets.hovered.bg_fill = tool_ribbon::PANEL_HOVER;
    theme.widgets.hovered.weak_bg_fill = tool_ribbon::PANEL_HOVER;
    theme.widgets.hovered.bg_stroke = Stroke::new(1.1_f32, tool_ribbon::BLUE_ACCENT_BORDER);
    theme.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, tool_ribbon::TEXT_WHITE);
    theme.widgets.active.bg_fill = tool_ribbon::BLUE_ACCENT_SOFT;
    theme.widgets.active.weak_bg_fill = tool_ribbon::BLUE_ACCENT_SOFT;
    theme.widgets.active.bg_stroke = Stroke::new(1.2_f32, tool_ribbon::BLUE_ACCENT_BORDER);
    theme.widgets.active.fg_stroke = Stroke::new(1.0_f32, tool_ribbon::TEXT_WHITE);
    theme.widgets.open.bg_fill = tool_ribbon::PANEL_HOVER;
    theme.widgets.open.weak_bg_fill = tool_ribbon::PANEL_HOVER;
    theme.widgets.open.bg_stroke = Stroke::new(1.1_f32, tool_ribbon::BLUE_ACCENT_BORDER);
    theme.widgets.open.fg_stroke = Stroke::new(1.0_f32, tool_ribbon::TEXT_WHITE);
    ctx.set_visuals_of(egui::Theme::Dark, theme);
    ctx.set_theme(egui::Theme::Dark);
}

fn text_font_family(font: TextFont, bold: bool, italic: bool) -> FontFamily {
    FontFamily::Name(Arc::from(format!("{}-{bold}-{italic}", font.name())))
}

fn append_rich_text(
    job: &mut egui::text::LayoutJob,
    rich: &StyledText,
    color: Color32,
    scale: f32,
) {
    for (text, format) in rich.runs() {
        let font = FontId::new(
            f32::from(format.size) * scale,
            text_font_family(format.font, format.bold, format.italic),
        );
        let underline = if format.underline {
            Stroke::new(scale, color)
        } else {
            Stroke::NONE
        };
        job.append(
            text,
            0.,
            egui::TextFormat {
                font_id: font,
                color,
                underline,
                ..Default::default()
            },
        );
    }
}

fn fragment_format_controls(ui: &mut egui::Ui, format: &mut TextFormat) {
    egui::ComboBox::from_id_salt("fragment_font")
        .width(122.)
        .selected_text(format.font.name())
        .show_ui(ui, |ui| {
            for font in TextFont::OPTIONS {
                ui.selectable_value(&mut format.font, font, font.name());
            }
        })
        .response
        .on_hover_text("Шрифт выделенного фрагмента");
    egui::ComboBox::from_id_salt("fragment_size")
        .width(54.)
        .selected_text(format!("{} пт", format.size))
        .show_ui(ui, |ui| {
            for size in [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 72] {
                ui.selectable_value(&mut format.size, size, format!("{size} пт"));
            }
        })
        .response
        .on_hover_text("Размер выделенного фрагмента");
    for (value, label, tip) in [
        (&mut format.bold, RichText::new("B").strong(), "Полужирный"),
        (&mut format.italic, RichText::new("I").italics(), "Курсив"),
        (
            &mut format.underline,
            RichText::new("U").underline(),
            "Подчёркивание",
        ),
    ] {
        ui.toggle_value(value, label).on_hover_text(tip);
    }
}

fn paper_edit_visuals(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(INK);
    v.extreme_bg_color = PAPER;
    v.selection.bg_fill = PALE;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.inactive.bg_fill = PAPER;
    v.widgets.inactive.weak_bg_fill = PAPER;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, INK);
    v.widgets.hovered.bg_fill = PAPER;
    v.widgets.hovered.weak_bg_fill = PALE;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, INK);
    v.widgets.active.bg_fill = PAPER;
    v.widgets.active.weak_bg_fill = PALE;
    v.widgets.active.bg_stroke = Stroke::new(1.2_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, INK);
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
    skipped_visual: usize,
    changed: usize,
    formula_changed: bool,
    unsaved: bool,
    origin: Pos2,
    plots: HashMap<usize, PlotRegion>,
    plots3d: HashMap<usize, Plot3dRegion>,
    images: HashMap<usize, editing::ImageSpec>,
    areas: Vec<editing::AreaSpec>,
    precision: Option<usize>,
    page_settings: editing::PageSettings,
}

struct PlotRegion {
    spec: editing::ChartSpec,
    options: editing::ChartOptions,
    curves: Calc<chart_view::Curves>,
    zoom: f64,
}

struct Plot3dRegion {
    spec: editing::Plot3dSpec,
    grid: Calc<list_rs::sheet::SurfaceGrid>,
}

#[derive(Clone)]
struct SheetSnapshot {
    xml: String,
    overrides: Vec<(String, f64)>,
    formula_changed: bool,
    unsaved: bool,
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
        let mut sheet = Sheet::import_with_base(xml, path.as_deref().and_then(Path::parent))?;
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
            .filter(|r| !r.imported)
            .map(|r| r.visual.left)
            .fold(0., f32::min);
        let top = sheet
            .regions
            .iter()
            .filter(|r| !r.imported)
            .map(|r| r.visual.top)
            .fold(0., f32::min);
        let mut rects = Vec::with_capacity(sheet.regions.len());
        for (index, region) in sheet.regions.iter().enumerate() {
            if region.imported {
                rects.push(Rect::NOTHING);
                continue;
            }
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
        let opts = editing::chart_options(xml)?;
        let plots: HashMap<usize, PlotRegion> = sheet
            .regions
            .iter()
            .enumerate()
            .filter_map(|(index, region)| {
                if region.imported {
                    return None;
                }
                let spec = specs.get(&region.source_id)?.clone();
                let options = opts.get(&region.source_id).cloned().unwrap_or_default();
                let curves = sheet.chart_curves(index, &spec);
                Some((
                    index,
                    PlotRegion {
                        spec,
                        options,
                        curves,
                        zoom: 1.0,
                    },
                ))
            })
            .collect();
        let specs3d = editing::plots3d(xml)?;
        let plots3d: HashMap<usize, Plot3dRegion> = sheet
            .regions
            .iter()
            .enumerate()
            .filter_map(|(index, region)| {
                if region.imported {
                    return None;
                }
                let spec = specs3d.get(&region.source_id)?.clone();
                let grid = sheet.surface_grid(index, &spec);
                Some((index, Plot3dRegion { spec, grid }))
            })
            .collect();
        let img_specs = editing::images(xml)?;
        let images: HashMap<usize, editing::ImageSpec> = sheet
            .regions
            .iter()
            .enumerate()
            .filter_map(|(index, region)| {
                if region.imported {
                    return None;
                }
                let spec = img_specs.get(&region.source_id)?.clone();
                Some((index, spec))
            })
            .collect();
        let areas = editing::areas(xml)?;
        let skipped_visual = sheet
            .regions
            .iter()
            .enumerate()
            .filter(|(index, region)| {
                region.visual.graphical
                    && !plots.contains_key(index)
                    && !plots3d.contains_key(index)
                    && !images.contains_key(index)
            })
            .count();
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
            skipped_visual,
            changed: 0,
            formula_changed: false,
            unsaved: false,
            origin: Pos2::new(left, top),
            plots,
            plots3d,
            images,
            areas,
            precision: editing::read_sheet_precision(xml)?,
            page_settings: editing::read_page_settings(xml)?,
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
        for (&index, graph) in &mut self.plots3d {
            graph.grid = self.sheet.surface_grid(index, &graph.spec);
        }
    }

    fn by_id(&self, id: &str) -> Calc<usize> {
        let mut hits = self
            .sheet
            .regions
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.imported && r.source_id == id);
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
        self.insert_sized(at, region, None)
    }

    fn insert_sized(
        &mut self,
        at: Pos2,
        region: editing::NewRegion<'_>,
        size: Option<egui::Vec2>,
    ) -> Calc<usize> {
        let math = matches!(&region, editing::NewRegion::Formula(_));
        let (xml, id) = editing::insert_region(&self.xml, at.x, at.y, region)?;
        let xml = match size {
            Some(size) => editing::grow_formula_region(&xml, &id, size.x, size.y)?,
            None => xml,
        };
        let candidate = self.rebuild(&xml, None)?;
        let index = candidate.by_id(&id)?;
        if math {
            candidate.sheet.display(index)?;
        }
        if let Some(graph) = candidate.plots.get(&index) {
            graph.curves.as_ref().map_err(Clone::clone)?;
        }
        if let Some(graph3d) = candidate.plots3d.get(&index) {
            graph3d.grid.as_ref().map_err(Clone::clone)?;
        }
        *self = candidate;
        Ok(index)
    }

    fn change_plot3d(&mut self, index: usize, spec: &editing::Plot3dSpec) -> Calc<()> {
        let id = &self.sheet.regions[index].source_id;
        let xml = editing::replace_plot3d(&self.xml, id, spec)?;
        let candidate = self.rebuild(&xml, None)?;
        candidate
            .plots3d
            .get(&index)
            .ok_or("не найден 3D-график")?
            .grid
            .as_ref()
            .map_err(Clone::clone)?;
        *self = candidate;
        Ok(())
    }

    fn save_plot3d_angle(&mut self, index: usize, yaw_deg: f32, pitch_deg: f32) -> Calc<()> {
        let Some(mut spec) = self.plots3d.get(&index).map(|p| p.spec.clone()) else {
            return Ok(());
        };
        spec.yaw_deg = yaw_deg.clamp(-360.0, 360.0);
        spec.pitch_deg = pitch_deg.clamp(-85.0, 85.0);
        let id = self.sheet.regions[index].source_id.clone();
        self.xml = editing::replace_plot3d(&self.xml, &id, &spec)?;
        if let Some(plot) = self.plots3d.get_mut(&index) {
            plot.spec = spec;
        }
        self.unsaved = true;
        Ok(())
    }

    fn change_chart(
        &mut self,
        index: usize,
        chart: &editing::ChartSpec,
        options: &editing::ChartOptions,
    ) -> Calc<()> {
        let id = &self.sheet.regions[index].source_id;
        let xml = editing::replace_chart_with_options(&self.xml, id, chart, options)?;
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
        self.apply_formula_sized(index, source, None)
    }

    fn apply_formula_sized(
        &mut self,
        index: usize,
        source: &str,
        size: Option<egui::Vec2>,
    ) -> Calc<()> {
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
        let xml = match size {
            Some(size) => editing::grow_formula_region(
                &xml,
                &self.sheet.regions[index].source_id,
                size.x,
                size.y,
            )?,
            None => xml,
        };
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
        Sheet::import_with_base(&xml, self.path.as_deref().and_then(Path::parent))?;
        Ok(xml)
    }

    fn snapshot(&self) -> SheetSnapshot {
        let overrides = self
            .input_indices
            .iter()
            .filter(|&&index| self.sheet.edited(index))
            .filter_map(|&index| {
                Some((
                    self.sheet.regions[index].source_id.clone(),
                    self.sheet.input_value(index)?,
                ))
            })
            .collect();
        SheetSnapshot {
            xml: self.xml.clone(),
            overrides,
            formula_changed: self.formula_changed,
            unsaved: self.unsaved,
        }
    }

    fn restore(&mut self, snap: SheetSnapshot) -> Calc<()> {
        let mut candidate = Self::load(&snap.xml, self.name.clone(), self.path.clone())?;
        for (id, value) in snap.overrides {
            if let Ok(index) = candidate.by_id(&id) {
                if candidate.sheet.editable(index) {
                    candidate.sheet.set(index, Some(value), None)?;
                    candidate.changed += 1;
                }
            }
        }
        candidate.sheet.incremental();
        candidate.errors = candidate.sheet.counts().2;
        candidate.refresh_plots();
        candidate.formula_changed = snap.formula_changed;
        candidate.unsaved = snap.unsaved;
        *self = candidate;
        Ok(())
    }

    fn move_regions(&mut self, moves: &[(String, f32, f32)]) -> Calc<()> {
        let xml = editing::move_regions(&self.xml, moves)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(())
    }

    fn delete_regions(&mut self, ids: &[String]) -> Calc<()> {
        let xml = editing::delete_regions(&self.xml, ids)?;
        let mut candidate = Self::load(&xml, self.name.clone(), self.path.clone())?;
        for &old in &self.input_indices {
            if !self.sheet.edited(old) {
                continue;
            }
            let id = &self.sheet.regions[old].source_id;
            if ids.iter().any(|removed| removed == id) {
                continue;
            }
            if let Ok(other) = candidate.by_id(id) {
                if let Some(value) = self.sheet.input_value(old) {
                    if candidate.sheet.editable(other) {
                        candidate.sheet.set(other, Some(value), None)?;
                        candidate.changed += 1;
                    }
                }
            }
        }
        candidate.sheet.incremental();
        candidate.errors = candidate.sheet.counts().2;
        candidate.refresh_plots();
        candidate.formula_changed = true;
        candidate.unsaved = true;
        *self = candidate;
        Ok(())
    }

    fn paste_regions(
        &mut self,
        items: &[editing::ClipboardRegion],
        base_left: f32,
        base_top: f32,
    ) -> Calc<Vec<String>> {
        let (xml, new_ids) = editing::paste_regions(&self.xml, items, base_left, base_top)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(new_ids)
    }

    fn set_display_unit(&mut self, index: usize, unit: Option<&str>) -> Calc<()> {
        let id = self
            .sheet
            .regions
            .get(index)
            .ok_or("выберите формулу на листе")?
            .source_id
            .clone();
        let xml = editing::set_display_unit(&self.xml, &id, unit)?;
        let candidate = self.rebuild(&xml, None)?;
        let new_idx = candidate.by_id(&id)?;
        candidate
            .sheet
            .display_with_precision(new_idx, candidate.precision)?;
        *self = candidate;
        Ok(())
    }

    fn set_precision(&mut self, precision: Option<usize>) -> Calc<()> {
        let xml = editing::set_sheet_precision(&self.xml, precision)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(())
    }

    fn replace_text(&mut self, index: usize, text: &str, style: editing::TextStyle) -> Calc<()> {
        let id = self
            .sheet
            .regions
            .get(index)
            .ok_or("выберите текстовый регион")?
            .source_id
            .clone();
        let xml = editing::replace_text_region(&self.xml, &id, text, style)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(())
    }

    fn insert_area(&mut self, title: &str, top: f32, bottom: f32) -> Calc<String> {
        let (xml, id) = editing::insert_area(&self.xml, title, top, bottom)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(id)
    }

    fn toggle_area(&mut self, area_id: &str) -> Calc<()> {
        let xml = editing::toggle_area(&self.xml, area_id)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(())
    }

    fn delete_area(&mut self, area_id: &str) -> Calc<()> {
        let xml = editing::delete_area(&self.xml, area_id)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(())
    }

    fn set_page_settings(&mut self, settings: &editing::PageSettings) -> Calc<()> {
        let xml = editing::set_page_settings(&self.xml, settings)?;
        let candidate = self.rebuild(&xml, None)?;
        *self = candidate;
        Ok(())
    }

    fn build_html_report(&self) -> String {
        let titles: HashMap<usize, String> = self
            .plots
            .iter()
            .map(|(&idx, p)| (idx, p.spec.title()))
            .collect();
        let charts: HashMap<usize, report::ChartReport<'_>> = self
            .plots
            .iter()
            .map(|(&idx, p)| {
                (
                    idx,
                    report::ChartReport {
                        curves: &p.curves,
                        options: &p.options,
                        is_polar: p.spec.is_polar(),
                        title: titles.get(&idx).map_or("", String::as_str),
                    },
                )
            })
            .collect();
        let plots3d: HashMap<usize, report::Plot3dReport<'_>> = self
            .plots3d
            .iter()
            .map(|(&idx, p)| {
                (
                    idx,
                    report::Plot3dReport {
                        grid: &p.grid,
                        spec: &p.spec,
                    },
                )
            })
            .collect();
        report::render_html_report_full(
            &self.name,
            &self.sheet,
            &charts,
            &plots3d,
            &self.images,
            &self.page_settings,
            self.precision,
        )
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
    Formula {
        math: MathEditor,
        text: String,
        text_mode: bool,
    },
    Text {
        body: String,
        style: editing::TextStyle,
        replace_region: Option<usize>,
        rich: Option<StyledText>,
        rich_active: bool,
        selection: Option<egui::text::CCursorRange>,
    },
    Chart(Box<chart_view::Draft>),
    Plot3d(Box<plot3d_view::Draft3d>),
}

fn new_text_draft() -> DraftRegion {
    DraftRegion::Text {
        body: String::new(),
        style: editing::TextStyle::Normal,
        replace_region: None,
        rich: Some(StyledText::plain("", TextFormat::for_heading(0))),
        rich_active: false,
        selection: None,
    }
}

fn formula_draft(initial: String) -> DraftRegion {
    let mut math = MathEditor::empty();
    if !initial.is_empty() {
        math.insert_text(&initial);
    }
    let text = math.to_source();
    DraftRegion::Formula {
        math,
        text,
        text_mode: false,
    }
}

struct Insertion {
    at: Pos2,
    draft: DraftRegion,
    error: Option<String>,
    focus: bool,
    replace_chart: Option<usize>,
    replace_formula: Option<usize>,
}

struct FormulaEditor {
    index: usize,
    draft: String,
    synced_draft: String,
    math: MathEditor,
    text_mode: bool,
    error: Option<String>,
    focus: bool,
}

struct TableEditor {
    name: String,
    headers: Vec<String>,
    rows: usize,
    cols: usize,
    cells: Vec<String>,
    replace_region: Option<usize>,
    error: Option<String>,
}

impl TableEditor {
    fn from_csv(name: String, table: data_io::CsvTable, replace_region: Option<usize>) -> Self {
        let cells = table.values.into_iter().map(display_number).collect();
        Self {
            name,
            headers: table.headers,
            rows: table.rows,
            cols: table.cols,
            cells,
            replace_region,
            error: None,
        }
    }

    fn empty(name: &str, rows: usize, cols: usize) -> Self {
        Self::from_csv(name.into(), data_io::CsvTable::empty(rows, cols), None)
    }

    fn resize(&mut self, new_rows: usize, new_cols: usize) {
        let new_rows = new_rows.clamp(1, 64);
        let new_cols = new_cols.clamp(1, 32);
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }
        let mut next = vec!["0".to_owned(); new_rows * new_cols];
        for r in 0..self.rows.min(new_rows) {
            for c in 0..self.cols.min(new_cols) {
                next[r * new_cols + c] = self.cells[r * self.cols + c].clone();
            }
        }
        self.rows = new_rows;
        self.cols = new_cols;
        self.cells = next;
        if !self.headers.is_empty() {
            self.headers.resize(new_cols, String::new());
        }
    }

    fn to_formula(&self) -> Calc<String> {
        let mut values = Vec::with_capacity(self.cells.len());
        for (idx, cell) in self.cells.iter().enumerate() {
            let r = idx / self.cols + 1;
            let c = idx % self.cols + 1;
            let val = number(cell.trim()).map_err(|_| {
                format!("некорректное число в ячейке ({r}, {c}): «{}»", cell.trim())
            })?;
            values.push(val);
        }
        let table = data_io::CsvTable {
            headers: self.headers.clone(),
            rows: self.rows,
            cols: self.cols,
            values,
        };
        table.to_formula(&self.name)
    }
}

struct DragState {
    ids: Vec<String>,
    origins: Vec<(String, f32, f32)>,
    start_pointer: Pos2,
    delta_sheet: egui::Vec2,
    moved: bool,
}

struct MarqueeState {
    start: Pos2,
    current: Pos2,
}

struct AreaDialog {
    title: String,
    top: f32,
    height: f32,
    error: Option<String>,
}

struct AnimationState {
    input_index: usize,
    from: String,
    to: String,
    steps: usize,
    fps: f32,
    frame: usize,
    playing: bool,
    last_tick: Instant,
    error: Option<String>,
}

struct Workbench {
    document: Worksheet,
    load: Option<mpsc::Receiver<Calc<Worksheet>>>,
    pdf_export: Option<mpsc::Receiver<Calc<PathBuf>>>,
    pending: Option<Open>,
    generation: u64,
    selected: Option<usize>,
    selected_ids: Vec<String>,
    jump: Option<usize>,
    entry: String,
    unit_entry: String,
    details: String,
    zoom: f32,
    sheet_width: f32,
    last_inline_caret: Option<(Cursor, egui::Vec2)>,
    fit: bool,
    show_inputs: bool,
    status: String,
    error: Option<String>,
    editor: Option<FormulaEditor>,
    table_editor: Option<TableEditor>,
    allow_close: bool,
    caret: Option<Pos2>,
    insertion: Option<Insertion>,
    undo_stack: Vec<SheetSnapshot>,
    redo_stack: Vec<SheetSnapshot>,
    clipboard: Vec<editing::ClipboardRegion>,
    drag: Option<DragState>,
    marquee: Option<MarqueeState>,
    show_palette: bool,
    ribbon_expanded: bool,
    palette_tab: math_view::PaletteTab,
    tool_search: tool_ribbon::SearchState,
    matrix_rows: usize,
    matrix_cols: usize,
    show_grid: bool,
    show_regions: bool,
    print_preview: bool,
    page_dialog: Option<editing::PageSettings>,
    animation: Option<AnimationState>,
    area_dialog: Option<AreaDialog>,
    textures: HashMap<String, Option<egui::TextureHandle>>,
    auto_reload: bool,
    file_stamp: Option<(SystemTime, u64)>,
    last_watch_check: Instant,
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
        let file_stamp = document.path.as_deref().and_then(file_stamp);
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
            pdf_export: None,
            pending: None,
            generation: 0,
            selected: None,
            selected_ids: Vec::new(),
            jump: None,
            entry: String::new(),
            unit_entry: String::new(),
            details: String::new(),
            zoom: NORMAL_ZOOM,
            sheet_width: ctx.content_rect().width(),
            last_inline_caret: None,
            fit,
            show_inputs: false,
            status,
            error,
            editor: None,
            table_editor: None,
            allow_close: false,
            caret: None,
            insertion: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            clipboard: Vec::new(),
            drag: None,
            marquee: None,
            show_palette: true,
            ribbon_expanded: false,
            palette_tab: math_view::PaletteTab::Operators,
            tool_search: tool_ribbon::SearchState::default(),
            matrix_rows: 2,
            matrix_cols: 2,
            show_grid: false,
            show_regions: false,
            print_preview: false,
            page_dialog: None,
            animation: None,
            area_dialog: None,
            textures: HashMap::new(),
            auto_reload: true,
            file_stamp,
            last_watch_check: Instant::now(),
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
        self.generation = self.generation.wrapping_add(1);
        self.selected = None;
        self.selected_ids.clear();
        self.jump = None;
        self.entry.clear();
        self.unit_entry.clear();
        self.details.clear();
        self.editor = None;
        self.table_editor = None;
        self.page_dialog = None;
        self.animation = None;
        self.area_dialog = None;
        self.textures.clear();
        self.caret = None;
        self.insertion = None;
        self.last_inline_caret = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.drag = None;
        self.marquee = None;
        self.error = None;
        self.file_stamp = self.document.path.as_deref().and_then(file_stamp);
        self.last_watch_check = Instant::now();
        self.fit = self.document.extent.x > 760.;
        self.zoom = NORMAL_ZOOM;
        self.status = format!(
            "Лист открыт · рассчитано формул {}",
            self.document.formulas - self.document.errors
        );
    }

    fn reload_from_disk_if_changed(&mut self) -> bool {
        let Some(path) = self.document.path.clone() else {
            return false;
        };
        let Some(stamp) = file_stamp(&path) else {
            return false;
        };
        if self.file_stamp == Some(stamp) {
            return false;
        }
        if self.document.unsaved {
            self.status = "Файл изменён на диске (Ctrl+R — перезагрузить)".into();
            return false;
        }
        let Ok(updated) = Worksheet::from_path(path) else {
            return false;
        };
        self.file_stamp = Some(stamp);
        self.document = updated;
        self.generation = self.generation.wrapping_add(1);
        self.jump = None;
        self.textures.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.sync_selection_from_ids();
        self.error = None;
        self.status = format!(
            "Лист обновлён с диска · рассчитано формул {}",
            self.document.formulas - self.document.errors
        );
        true
    }

    fn check_external_file_change(&mut self, ctx: &egui::Context) {
        if !self.auto_reload
            || self.document.path.is_none()
            || self.load.is_some()
            || self.pending.is_some()
            || self.insertion.is_some()
            || self.editor.is_some()
            || self.table_editor.is_some()
            || self.page_dialog.is_some()
            || self.area_dialog.is_some()
            || self.animation.is_some()
            || self.drag.is_some()
            || self.marquee.is_some()
        {
            return;
        }
        if self.last_watch_check.elapsed() >= Duration::from_millis(500) {
            self.last_watch_check = Instant::now();
            if self.reload_from_disk_if_changed() {
                ctx.request_repaint();
            }
        }
        ctx.request_repaint_after(Duration::from_millis(600));
    }

    fn reload_from_disk(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.document.path.clone() {
            self.request_open(Open::File(path), ctx);
        }
    }

    fn select(&mut self, index: usize) {
        self.caret = None;
        let id = self.document.sheet.regions[index].source_id.clone();
        self.selected_ids = vec![id];
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
        self.unit_entry = self
            .document
            .sheet
            .display_unit_label(index)
            .unwrap_or_default()
            .to_owned();
        self.refresh_details(index);
    }

    fn toggle_select(&mut self, index: usize) {
        self.caret = None;
        let id = self.document.sheet.regions[index].source_id.clone();
        if let Some(pos) = self
            .selected_ids
            .iter()
            .position(|existing| existing == &id)
        {
            self.selected_ids.remove(pos);
        } else {
            self.selected_ids.push(id);
        }
        self.sync_selection_from_ids();
    }

    fn sync_selection_from_ids(&mut self) {
        self.selected_ids
            .retain(|id| self.document.by_id(id).is_ok());
        self.selected = self
            .selected_ids
            .last()
            .and_then(|id| self.document.by_id(id).ok());
        if let Some(index) = self.selected {
            self.entry = self
                .document
                .sheet
                .input_value(index)
                .map(|value| value.to_string())
                .unwrap_or_default();
            self.unit_entry = self
                .document
                .sheet
                .display_unit_label(index)
                .unwrap_or_default()
                .to_owned();
            self.refresh_details(index);
        } else {
            self.entry.clear();
            self.unit_entry.clear();
            self.details.clear();
        }
    }

    fn is_selected_index(&self, index: usize) -> bool {
        let id = &self.document.sheet.regions[index].source_id;
        self.selected_ids.iter().any(|s| s == id)
    }

    fn record_undo(&mut self, snap: SheetSnapshot) {
        const MAX_HISTORY: usize = 64;
        if self.undo_stack.len() >= MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snap);
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        let Some(prev) = self.undo_stack.pop() else {
            return;
        };
        let current = self.document.snapshot();
        match self.document.restore(prev) {
            Ok(()) => {
                self.redo_stack.push(current);
                self.sync_selection_from_ids();
                self.error = None;
                self.status = "Изменение отменено".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        let current = self.document.snapshot();
        match self.document.restore(next) {
            Ok(()) => {
                self.undo_stack.push(current);
                self.sync_selection_from_ids();
                self.error = None;
                self.status = "Изменение возвращено".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn remove_selected(&mut self, cut: bool) {
        if self.selected_ids.is_empty() {
            return;
        }
        let ids = self.selected_ids.clone();
        let cut_items = if cut {
            match editing::copy_regions(&self.document.xml, &ids) {
                Ok(items) if items.len() == ids.len() => Some(items),
                Ok(_) => {
                    self.error = Some("Не удалось скопировать все выбранные регионы".into());
                    return;
                }
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        } else {
            None
        };
        let snap = self.document.snapshot();
        match self.document.delete_regions(&ids) {
            Ok(()) => {
                self.record_undo(snap);
                self.selected = None;
                self.selected_ids.clear();
                self.entry.clear();
                self.details.clear();
                self.error = None;
                self.status = if let Some(items) = cut_items {
                    self.clipboard = items;
                    format!("Вырезано регионов: {}", ids.len())
                } else {
                    format!("Удалено регионов: {}", ids.len())
                };
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn copy_selected(&mut self) {
        if self.selected_ids.is_empty() {
            return;
        }
        match editing::copy_regions(&self.document.xml, &self.selected_ids) {
            Ok(items) if !items.is_empty() => {
                let count = items.len();
                self.clipboard = items;
                self.error = None;
                self.status = format!("Скопировано регионов: {count}");
            }
            Ok(_) => {}
            Err(error) => self.error = Some(error),
        }
    }

    fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let target = if let Some(caret) = self.caret {
            caret
        } else if let Some(first) = self.clipboard.first() {
            Pos2::new(first.old_left + 24., first.old_top + 24.)
        } else {
            self.position_for_insert()
        };
        let snap = self.document.snapshot();
        match self
            .document
            .paste_regions(&self.clipboard, target.x, target.y)
        {
            Ok(new_ids) => {
                self.record_undo(snap);
                let count = new_ids.len();
                self.selected_ids = new_ids;
                self.sync_selection_from_ids();
                self.caret = None;
                self.error = None;
                self.status = format!("Вставлено регионов: {count}");
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn move_selected_to(&mut self, moves: &[(String, f32, f32)]) {
        if moves.is_empty() {
            return;
        }
        let snap = self.document.snapshot();
        match self.document.move_regions(moves) {
            Ok(()) => {
                self.record_undo(snap);
                self.sync_selection_from_ids();
                self.error = None;
                self.status = format!("Перемещено регионов: {}", moves.len());
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn refresh_details(&mut self, index: usize) {
        let region = &self.document.sheet.regions[index];
        self.details = if let Some(plot) = self.document.plots.get(&index) {
            plot.curves
                .as_ref()
                .map(|_| plot.spec.title())
                .unwrap_or_else(|error| error.clone())
        } else if let Some(plot3d) = self.document.plots3d.get(&index) {
            plot3d
                .grid
                .as_ref()
                .map(|_| plot3d.spec.title())
                .unwrap_or_else(|error| error.clone())
        } else if let Some(text) = &region.text {
            text.clone()
        } else if region.visual.function {
            String::new()
        } else {
            self.document
                .sheet
                .details_with_precision(index, self.document.precision)
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
                .filter(|r| !r.imported)
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
            replace_formula: None,
        });
    }

    fn route_sheet_typing(&mut self, ctx: &egui::Context) {
        if self.caret.is_none()
            || self.insertion.is_some()
            || ctx.wants_keyboard_input()
            || self.tool_search.is_blocking_sheet(ctx)
        {
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
            self.start_insert(formula_draft(typed));
        }
    }

    fn edit_text(&mut self, index: usize) {
        let Some(region) = self.document.sheet.regions.get(index) else {
            return;
        };
        let Some(body) = &region.text else {
            return;
        };
        if region.visual.graphical || region.is_page_break() || region.label == "Ссылка" {
            return;
        }
        self.insertion = Some(Insertion {
            at: self.position_for_insert(),
            draft: DraftRegion::Text {
                body: body.clone(),
                style: editing::TextStyle::from_heading_level(region.visual.heading),
                replace_region: Some(index),
                rich: region.visual.rich_text.clone().or_else(|| {
                    region
                        .visual
                        .text_format
                        .map(|format| StyledText::plain(body, format))
                }),
                rich_active: region.visual.rich_text.is_some(),
                selection: None,
            },
            error: None,
            focus: true,
            replace_chart: None,
            replace_formula: None,
        });
    }

    fn insert_page_break(&mut self) {
        let before = self.document.snapshot();
        let at = self.position_for_insert();
        match self.document.insert(at, editing::NewRegion::PageBreak) {
            Ok(index) => {
                self.record_undo(before);
                self.select(index);
                self.jump = Some(index);
                self.error = None;
                self.status = "Добавлен разрыв страницы".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn export_html_report_picker(&mut self) {
        let html = self.document.build_html_report();
        let stem = self.document.name.trim();
        let file_stem = if stem.is_empty() { "Отчёт" } else { stem };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Отчёт HTML (печать / PDF)", &["html"])
            .set_file_name(format!("{file_stem}_отчёт.html"))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("html");
        }
        match report::write_html_report(&path, &html) {
            Ok(()) => {
                self.error = None;
                self.status = format!("Отчёт сохранён · {}", file_name(&path));
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn export_pdf_report_picker(&mut self, ctx: &egui::Context) {
        let stem = self.document.name.trim();
        let file_stem = if stem.is_empty() { "Отчёт" } else { stem };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Документ PDF", &["pdf"])
            .set_file_name(format!("{file_stem}.pdf"))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        let html = self.document.build_html_report();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = report::export_pdf_via_browser(&html, &path).map(|()| path);
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.pdf_export = Some(receiver);
        self.error = None;
        self.status = "Экспорт PDF…".into();
    }

    fn page_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut settings) = self.page_dialog.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("page_setup")).show(ctx, |ui| {
            ui.set_width(420.);
            ui.heading("Параметры страницы и колонтитулы");
            ui.add_space(6.);
            ui.horizontal(|ui| {
                ui.label("Верхний колонтитул:");
                ui.add(
                    TextEdit::singleline(&mut settings.header)
                        .desired_width(240.)
                        .hint_text("По умолчанию — название листа"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Нижний колонтитул:");
                ui.add(
                    TextEdit::singleline(&mut settings.footer)
                        .desired_width(240.)
                        .hint_text("Например: Инженерный расчёт"),
                );
            });
            ui.checkbox(&mut settings.show_page_numbers, "Показывать номера страниц");
            ui.horizontal(|ui| {
                ui.label("Высота страницы (px):");
                ui.add(
                    egui::DragValue::new(&mut settings.page_height)
                        .range(360.0..=2000.0)
                        .speed(10.0),
                );
            });
            ui.add_space(8.);
            ui.horizontal(|ui| {
                apply = ui.button("Применить").clicked();
                cancel = ui.button("Отмена").clicked();
            });
        });
        if apply {
            let before = self.document.snapshot();
            match self.document.set_page_settings(&settings) {
                Ok(()) => {
                    self.record_undo(before);
                    self.error = None;
                    self.status = "Параметры страницы сохранены".into();
                    ctx.request_repaint();
                    return;
                }
                Err(error) => self.error = Some(error),
            }
        } else if cancel || response.should_close() {
            return;
        }
        self.page_dialog = Some(settings);
    }

    fn edit_plot3d(&mut self, index: usize) {
        if let Some(graph) = self.document.plots3d.get(&index) {
            self.insertion = Some(Insertion {
                at: self.position_for_insert(),
                draft: DraftRegion::Plot3d(Box::new(plot3d_view::Draft3d::from_spec(&graph.spec))),
                error: None,
                focus: false,
                replace_chart: Some(index),
                replace_formula: None,
            });
        }
    }

    fn insert_image_from_picker(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Изображение", &["png", "bmp", "jpg", "jpeg"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(format!("Не удалось прочитать файл: {e}"));
                return;
            }
        };
        if bytes.len() > 4 * 1024 * 1024 {
            self.error = Some("Размер изображения превышает 4 МБ".into());
            return;
        }
        let (mime, w, h) = detect_image_meta(&bytes, &path);
        let spec = editing::ImageSpec {
            name: file_name(&path),
            mime,
            width: w,
            height: h,
            data_base64: editing::encode_base64(&bytes),
        };
        let before = self.document.snapshot();
        let at = self.position_for_insert();
        match self.document.insert(at, editing::NewRegion::Image(&spec)) {
            Ok(index) => {
                self.record_undo(before);
                self.select(index);
                self.jump = Some(index);
                self.error = None;
                self.status = format!("Вставлено изображение · {}", spec.name);
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn area_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.area_dialog.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("area_dialog")).show(ctx, |ui| {
            ui.set_width(380.);
            ui.heading("Сворачиваемая область");
            ui.add_space(6.);
            ui.horizontal(|ui| {
                ui.label("Название:");
                ui.add(
                    TextEdit::singleline(&mut dlg.title)
                        .desired_width(240.)
                        .hint_text("Например: Исходные данные"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Начало Y:");
                ui.add(egui::DragValue::new(&mut dlg.top).range(0.0..=50000.0));
                ui.label("Высота:");
                ui.add(egui::DragValue::new(&mut dlg.height).range(30.0..=5000.0));
            });
            if let Some(err) = &dlg.error {
                ui.label(RichText::new(err).color(ERROR_DARK));
            }
            ui.add_space(6.);
            ui.horizontal(|ui| {
                apply = ui.button("Создать").clicked();
                cancel = ui.button("Отмена").clicked();
            });
        });
        if apply {
            let before = self.document.snapshot();
            match self
                .document
                .insert_area(&dlg.title, dlg.top, dlg.top + dlg.height)
            {
                Ok(_) => {
                    self.record_undo(before);
                    self.error = None;
                    self.status = format!("Добавлена область «{}»", dlg.title.trim());
                    ctx.request_repaint();
                    return;
                }
                Err(error) => dlg.error = Some(error),
            }
        } else if cancel || response.should_close() {
            return;
        }
        self.area_dialog = Some(dlg);
    }

    fn ensure_image_texture(
        &mut self,
        ctx: &egui::Context,
        region_id: &str,
        spec: &editing::ImageSpec,
    ) -> Option<egui::TextureId> {
        if !self.textures.contains_key(region_id) {
            let handle = editing::decode_base64(&spec.data_base64)
                .and_then(|bytes| decode_image_rgba(&bytes))
                .map(|(w, h, rgba)| {
                    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    ctx.load_texture(
                        format!("img_{}_{}", self.generation, region_id),
                        img,
                        egui::TextureOptions::LINEAR,
                    )
                });
            self.textures.insert(region_id.to_owned(), handle);
        }
        self.textures
            .get(region_id)
            .and_then(|opt| opt.as_ref())
            .map(egui::TextureHandle::id)
    }

    fn open_animation(&mut self, input_index: usize) {
        let current = self.document.sheet.input_value(input_index).unwrap_or(1.0);
        let to = if current.abs() < 1e-6 {
            10.0
        } else {
            current * 2.0
        };
        self.animation = Some(AnimationState {
            input_index,
            from: display_number(current),
            to: display_number(to),
            steps: 24,
            fps: 8.0,
            frame: 0,
            playing: false,
            last_tick: Instant::now(),
            error: None,
        });
    }

    fn apply_animation_frame(
        &mut self,
        input_index: usize,
        from: f64,
        to: f64,
        steps: usize,
        frame: usize,
    ) -> Calc<()> {
        let denom = steps.max(1) as f64;
        let t = (frame.min(steps) as f64) / denom;
        let val = from + (to - from) * t;
        self.document.update_input(input_index, Some(val))?;
        if self.selected == Some(input_index) {
            self.entry = val.to_string();
            self.refresh_details(input_index);
        }
        Ok(())
    }

    fn animation_window(&mut self, ctx: &egui::Context) {
        let Some(mut anim) = self.animation.take() else {
            return;
        };
        if !self.document.sheet.editable(anim.input_index) {
            return;
        }
        let mut keep_open = true;
        let label = self.document.sheet.regions[anim.input_index].label.clone();

        if anim.playing {
            let interval = std::time::Duration::from_secs_f32(1.0 / anim.fps.clamp(1.0, 30.0));
            if anim.last_tick.elapsed() >= interval {
                anim.last_tick = Instant::now();
                anim.frame = (anim.frame + 1) % (anim.steps + 1);
                match (number(anim.from.trim()), number(anim.to.trim())) {
                    (Ok(from), Ok(to)) => {
                        if let Err(e) = self.apply_animation_frame(
                            anim.input_index,
                            from,
                            to,
                            anim.steps,
                            anim.frame,
                        ) {
                            anim.error = Some(e);
                            anim.playing = false;
                        } else {
                            anim.error = None;
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        anim.error = Some(e);
                        anim.playing = false;
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        let mut step_to: Option<usize> = None;
        egui::Window::new(format!("Анимация · {label}"))
            .open(&mut keep_open)
            .resizable(false)
            .default_width(320.)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("От:");
                    ui.add(TextEdit::singleline(&mut anim.from).desired_width(68.));
                    ui.label("До:");
                    ui.add(TextEdit::singleline(&mut anim.to).desired_width(68.));
                    ui.label("Кадров:");
                    ui.add(egui::DragValue::new(&mut anim.steps).range(2..=200));
                });
                ui.horizontal(|ui| {
                    ui.label("Кадр:");
                    let prev = anim.frame;
                    ui.add(egui::Slider::new(&mut anim.frame, 0..=anim.steps));
                    if anim.frame != prev {
                        step_to = Some(anim.frame);
                    }
                });
                ui.horizontal(|ui| {
                    let btn = if anim.playing {
                        "Пауза"
                    } else {
                        "▶ Старт"
                    };
                    if ui.button(btn).clicked() {
                        anim.playing = !anim.playing;
                        anim.last_tick = Instant::now();
                    }
                    if ui.button("Сброс").clicked() {
                        anim.playing = false;
                        anim.frame = 0;
                        step_to = Some(0);
                    }
                    ui.label("FPS:");
                    ui.add(
                        egui::DragValue::new(&mut anim.fps)
                            .range(1.0..=24.0)
                            .speed(0.5),
                    );
                });
                if let Some(err) = &anim.error {
                    ui.label(RichText::new(err).color(ERROR_DARK).size(12.));
                }
            });

        if let Some(frame) = step_to {
            match (number(anim.from.trim()), number(anim.to.trim())) {
                (Ok(from), Ok(to)) => {
                    if let Err(e) =
                        self.apply_animation_frame(anim.input_index, from, to, anim.steps, frame)
                    {
                        anim.error = Some(e);
                    } else {
                        anim.error = None;
                    }
                }
                (Err(e), _) | (_, Err(e)) => anim.error = Some(e),
            }
        }
        if keep_open {
            self.animation = Some(anim);
        }
    }

    fn edit_chart(&mut self, index: usize) {
        if let Some(graph) = self.document.plots.get(&index) {
            self.insertion = Some(Insertion {
                at: self.position_for_insert(),
                draft: DraftRegion::Chart(Box::new(chart_view::Draft::from_spec(
                    &graph.spec,
                    &graph.options,
                ))),
                error: None,
                focus: false,
                replace_chart: Some(index),
                replace_formula: None,
            });
        }
    }

    fn commit_insert(&mut self, insert: &Insertion, size: Option<egui::Vec2>) -> Calc<()> {
        let before = self.document.snapshot();
        let index = match &insert.draft {
            DraftRegion::Formula {
                math,
                text,
                text_mode,
            } => {
                let source = if *text_mode {
                    text.clone()
                } else {
                    math.to_source()
                };
                if let Some(existing) = insert.replace_formula {
                    self.document.apply_formula_sized(existing, &source, size)?;
                    existing
                } else {
                    self.document.insert_sized(
                        insert.at,
                        editing::NewRegion::Formula(&source),
                        size,
                    )?
                }
            }
            DraftRegion::Text {
                body,
                style,
                replace_region,
                rich,
                rich_active,
                ..
            } => {
                let rich = rich.as_ref().filter(|_| *rich_active);
                if let Some(existing) = *replace_region {
                    if let Some(rich) = rich {
                        let region = &self.document.sheet.regions[existing];
                        let xml = editing::replace_rich_text_region(
                            &self.document.xml,
                            &region.source_id,
                            rich,
                            *style,
                        )?;
                        self.document = self.document.rebuild(&xml, None)?;
                    } else {
                        self.document.replace_text(existing, body, *style)?;
                    }
                    existing
                } else if let Some(rich) = rich {
                    let (xml, id) = editing::insert_region(
                        &self.document.xml,
                        insert.at.x,
                        insert.at.y,
                        editing::NewRegion::StyledText(body, *style),
                    )?;
                    let xml = editing::replace_rich_text_region(&xml, &id, rich, *style)?;
                    let candidate = self.document.rebuild(&xml, None)?;
                    let index = candidate.by_id(&id)?;
                    self.document = candidate;
                    index
                } else {
                    self.document
                        .insert(insert.at, editing::NewRegion::StyledText(body, *style))?
                }
            }
            DraftRegion::Chart(value) => {
                let spec = value.spec()?;
                let options = value.options()?;
                if let Some(index) = insert.replace_chart {
                    self.document.change_chart(index, &spec, &options)?;
                    index
                } else {
                    self.document.insert(
                        insert.at,
                        editing::NewRegion::ChartConfigured(&spec, &options),
                    )?
                }
            }
            DraftRegion::Plot3d(value) => {
                let spec = value.spec()?;
                if let Some(index) = insert.replace_chart {
                    self.document.change_plot3d(index, &spec)?;
                    index
                } else {
                    self.document
                        .insert(insert.at, editing::NewRegion::Plot3d(&spec))?
                }
            }
        };
        self.record_undo(before);
        self.selected = None;
        self.selected_ids.clear();
        if insert.replace_formula.is_some() {
            self.select(index);
        } else if matches!(&insert.draft, DraftRegion::Formula { .. }) {
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
        } else if insert.replace_formula.is_some() {
            format!("Формула #{} обновлена", index + 1)
        } else {
            format!("Добавлен регион #{}", index + 1)
        };
        Ok(())
    }

    fn insert_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut insert) = self.insertion.take() else {
            return;
        };
        if matches!(&insert.draft, DraftRegion::Formula { .. }) {
            self.insertion = Some(insert);
            return;
        }
        let response = egui::Modal::new(egui::Id::new("add_region")).show(ctx, |ui| {
            ui.set_width(435.);
            match &mut insert.draft {
                DraftRegion::Text {
                    body,
                    style,
                    rich,
                    rich_active,
                    selection,
                    ..
                } => {
                    ui.heading("Текст на листе");
                    let previous = *style;
                    ui.horizontal(|ui| {
                        ui.label("Стиль:");
                        ui.selectable_value(style, editing::TextStyle::Normal, "Обычный");
                        ui.selectable_value(style, editing::TextStyle::Heading1, "Заголовок 1");
                        ui.selectable_value(style, editing::TextStyle::Heading2, "Заголовок 2");
                    });
                    if previous != *style && !*rich_active {
                        *rich = Some(StyledText::plain(
                            body,
                            TextFormat::for_heading(match style {
                                editing::TextStyle::Normal => 0,
                                editing::TextStyle::Heading1 => 1,
                                editing::TextStyle::Heading2 => 2,
                            }),
                        ));
                    }
                    ui.add_space(4.);
                    let mut layout = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, width: f32| {
                        let color = ui.visuals().text_color();
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = width;
                        if let Some(rich) = rich.as_ref().filter(|_| *rich_active) {
                            if rich.body == buffer.as_str() {
                                append_rich_text(&mut job, rich, color, 1.2);
                            } else {
                                let mut preview = rich.clone();
                                preview.sync(buffer.as_str());
                                append_rich_text(&mut job, &preview, color, 1.2);
                            }
                        } else {
                            job.append(
                                buffer.as_str(),
                                0.,
                                egui::TextFormat::simple(FontId::proportional(14.), color),
                            );
                        }
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                    let mut field = TextEdit::multiline(body)
                        .desired_width(435.)
                        .desired_rows(5)
                        .char_limit(24_000)
                        .hint_text("Введите текст")
                        .layouter(&mut layout)
                        .show(ui);
                    if insert.focus {
                        field.response.request_focus();
                        insert.focus = false;
                    }
                    if field.response.has_focus() {
                        *selection = field.cursor_range.filter(|range| !range.is_empty());
                    }
                    if let Some(rich) = rich {
                        rich.sync(body);
                        let selected = selection.map(|range| range.as_sorted_char_range());
                        ui.add_enabled_ui(selected.is_some(), |ui| {
                            if let Some(range) = selected {
                                let before = rich.format_at(range.start);
                                let mut format = before;
                                ui.horizontal(|ui| fragment_format_controls(ui, &mut format));
                                if before != format {
                                    rich.apply(range, |value| {
                                        if before.font != format.font {
                                            value.font = format.font;
                                        }
                                        if before.size != format.size {
                                            value.size = format.size;
                                        }
                                        if before.bold != format.bold {
                                            value.bold = format.bold;
                                        }
                                        if before.italic != format.italic {
                                            value.italic = format.italic;
                                        }
                                        if before.underline != format.underline {
                                            value.underline = format.underline;
                                        }
                                    });
                                    *rich_active = true;
                                    field.state.cursor.set_char_range(*selection);
                                    field.state.store(ui.ctx(), field.response.id);
                                    field.response.request_focus();
                                }
                            } else {
                                ui.label("Выделите фрагмент текста для форматирования");
                            }
                        });
                    }
                }
                DraftRegion::Chart(draft) => {
                    ui.heading("График X–Y");
                    draft.fields(ui);
                }
                DraftRegion::Plot3d(draft) => {
                    ui.heading("3D-поверхность / Контурный график");
                    draft.fields(ui);
                }
                DraftRegion::Formula { .. } => unreachable!(),
            }
            if let Some(error) = &insert.error {
                ui.label(RichText::new(error).color(ERROR_DARK));
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
            match self.commit_insert(&insert, None) {
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

    fn start_formula_inline(&mut self, index: usize) {
        let Some(region) = self.document.sheet.regions.get(index) else {
            return;
        };
        let Some(edit) = region.visual.editor.as_ref() else {
            return;
        };
        if edit.native_symbolic_commands {
            self.error =
                Some("Правка импортированных символьных команд пока не поддерживается".into());
            return;
        }
        let source = if self.document.sheet.edited(index) {
            let label = &region.label;
            let unit = region
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
        let math = MathEditor::from_source(&source);
        let at = Pos2::new(region.visual.left, region.visual.top);
        self.insertion = Some(Insertion {
            at,
            draft: DraftRegion::Formula {
                math,
                text: source,
                text_mode: false,
            },
            error: None,
            focus: true,
            replace_chart: None,
            replace_formula: Some(index),
        });
    }

    fn start_formula(&mut self, index: usize) {
        let Some(edit) = self.document.sheet.regions[index].visual.editor.as_ref() else {
            return;
        };
        if edit.native_symbolic_commands {
            self.error =
                Some("Правка импортированных символьных команд пока не поддерживается".into());
            return;
        }
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
        let math = MathEditor::from_source(&draft);
        let synced_draft = draft.clone();
        self.editor = Some(FormulaEditor {
            index,
            draft,
            synced_draft,
            math,
            text_mode: false,
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

    fn open_table_for_region(&mut self, index: usize) {
        let Some((rows, cols, values)) = self.document.sheet.region_real_matrix(index) else {
            return;
        };
        let name = self.document.sheet.regions[index].label.clone();
        let table = data_io::CsvTable {
            headers: Vec::new(),
            rows,
            cols,
            values,
        };
        self.table_editor = Some(TableEditor::from_csv(name, table, Some(index)));
    }

    fn import_csv_from_picker(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Таблица CSV", &["csv", "txt", "tsv"])
            .pick_file()
        else {
            return;
        };
        match data_io::read_csv_file(&path) {
            Ok(table) => {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Data".into());
                let clean: String = stem
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let name = if clean
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    clean
                } else {
                    "Data".into()
                };
                self.table_editor = Some(TableEditor::from_csv(name, table, None));
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn export_selected_csv(&mut self) {
        let Some(index) = self.selected else {
            self.error =
                Some("Выберите регион с матрицей, диапазоном или числом для экспорта".into());
            return;
        };
        let csv = match self
            .document
            .sheet
            .export_csv(index, ',', self.document.precision)
        {
            Ok(csv) => csv,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let default_name = {
            let lbl = &self.document.sheet.regions[index].label;
            let clean: String = lbl
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if clean.is_empty() {
                "result.csv".into()
            } else {
                format!("{clean}.csv")
            }
        };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Таблица CSV", &["csv"])
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("csv");
        }
        match data_io::write_csv_file(&path, &csv) {
            Ok(()) => {
                self.error = None;
                self.status = format!("Экспортировано в CSV · {}", file_name(&path));
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn commit_table(&mut self, table: &TableEditor) -> Calc<()> {
        let formula = table.to_formula()?;
        let before = self.document.snapshot();
        let index = if let Some(existing) = table.replace_region {
            self.document.apply_formula(existing, &formula)?;
            existing
        } else {
            let at = self.position_for_insert();
            self.document
                .insert(at, editing::NewRegion::Formula(&formula))?
        };
        self.record_undo(before);
        self.selected = None;
        self.select(index);
        self.jump = Some(index);
        self.error = None;
        self.status = format!(
            "Таблица {} ({}×{}) применена",
            table.name.trim(),
            table.rows,
            table.cols
        );
        Ok(())
    }

    fn table_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut table) = self.table_editor.take() else {
            return;
        };
        let mut load_csv = false;
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("table_editor")).show(ctx, |ui| {
            ui.set_width(540.);
            ui.horizontal(|ui| {
                ui.heading("Таблица данных");
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Загрузить CSV…").clicked() {
                        load_csv = true;
                    }
                });
            });
            ui.add_space(4.);
            ui.horizontal(|ui| {
                ui.label("Имя переменной:");
                ui.add(TextEdit::singleline(&mut table.name).desired_width(120.));
                ui.separator();
                let mut rows = table.rows;
                let mut cols = table.cols;
                ui.label("Строк:");
                ui.add(egui::DragValue::new(&mut rows).range(1..=64));
                ui.label("Столб.:");
                ui.add(egui::DragValue::new(&mut cols).range(1..=32));
                if rows != table.rows || cols != table.cols {
                    table.resize(rows, cols);
                }
            });
            ui.label(
                RichText::new(
                    "Используйте col(Имя, 0), col(Имя, 1) в формулах и графиках (нумерация с 0).",
                )
                .size(11.5)
                .color(tool_ribbon::TEXT_MUTED),
            );
            ui.add_space(6.);
            egui::ScrollArea::both()
                .max_height(260.)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new("editable_data_table")
                        .striped(true)
                        .spacing(vec2(4., 4.))
                        .show(ui, |ui| {
                            ui.label(RichText::new("#").color(tool_ribbon::TEXT_MUTED).size(11.));
                            for c in 0..table.cols {
                                let hdr = table
                                    .headers
                                    .get(c)
                                    .filter(|h| !h.is_empty())
                                    .cloned()
                                    .unwrap_or_else(|| format!("col {c}"));
                                ui.label(
                                    RichText::new(hdr)
                                        .strong()
                                        .size(11.5)
                                        .color(tool_ribbon::BLUE_ACCENT_BORDER),
                                );
                            }
                            ui.end_row();
                            for r in 0..table.rows {
                                ui.label(
                                    RichText::new(format!("{r}"))
                                        .color(tool_ribbon::TEXT_MUTED)
                                        .size(11.),
                                );
                                for c in 0..table.cols {
                                    let idx = r * table.cols + c;
                                    ui.add(
                                        TextEdit::singleline(&mut table.cells[idx])
                                            .desired_width(68.)
                                            .font(FontId::monospace(12.)),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
            if let Some(error) = &table.error {
                ui.add_space(4.);
                ui.label(RichText::new(error).color(ERROR_DARK));
            }
            ui.add_space(6.);
            ui.horizontal(|ui| {
                apply = ui.button("Применить").clicked();
                cancel = ui.button("Отмена").clicked();
            });
        });
        if load_csv {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Таблица CSV", &["csv", "txt", "tsv"])
                .pick_file()
            {
                match data_io::read_csv_file(&path) {
                    Ok(loaded) => {
                        table =
                            TableEditor::from_csv(table.name.clone(), loaded, table.replace_region);
                    }
                    Err(error) => table.error = Some(error),
                }
            }
            self.table_editor = Some(table);
            return;
        }
        if apply {
            match self.commit_table(&table) {
                Ok(()) => {
                    ctx.request_repaint();
                    return;
                }
                Err(error) => table.error = Some(error),
            }
        } else if cancel || response.should_close() {
            return;
        }
        self.table_editor = Some(table);
    }

    fn formula_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.editor.take() else {
            return;
        };
        if editor.draft != editor.synced_draft {
            editor.math = MathEditor::from_source(&editor.draft);
            editor.synced_draft = editor.draft.clone();
        }
        let result = egui::Modal::new(egui::Id::new("formula_editor")).show(ctx, |ui| {
            ui.set_width(520.);
            ui.horizontal(|ui| {
                ui.heading(format!("Формула #{}", editor.index + 1));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.selectable_label(editor.text_mode, "Текст").clicked() {
                        editor.text_mode = true;
                    }
                    if ui
                        .selectable_label(!editor.text_mode, "2D-редактор")
                        .clicked()
                    {
                        editor.text_mode = false;
                        editor.math = MathEditor::from_source(&editor.draft);
                        editor.synced_draft = editor.draft.clone();
                    }
                });
            });
            ui.add_space(4.);
            if !editor.text_mode {
                if math_view::palette_buttons(ui, &mut editor.math) {
                    editor.draft = editor.math.to_source();
                    editor.synced_draft = editor.draft.clone();
                }
                ui.add_space(4.);
                let style = math_view::Style {
                    font_size: 16.0,
                    ink: INK,
                    accent: ACCENT,
                    muted: MUTED,
                    index_color: INDEX_INK,
                    placeholder_bg: PALE,
                };
                let metrics =
                    math_view::measure_slot(ui.painter(), &editor.math.root, style.font_size, true);
                let box_height = (metrics.height() + 22.0).max(58.0);
                let (box_rect, response) =
                    ui.allocate_exact_size(vec2(520.0, box_height), Sense::click());
                ui.painter().rect(
                    box_rect,
                    5.0,
                    PAPER,
                    Stroke::new(1.2_f32, ACCENT),
                    StrokeKind::Inside,
                );
                let click_pos = response
                    .clicked()
                    .then(|| response.interact_pointer_pos())
                    .flatten();
                let origin = Pos2::new(
                    box_rect.left() + 10.0,
                    box_rect.center().y - metrics.height() * 0.5,
                );
                math_view::paint_editor(
                    &ui.painter().with_clip_rect(box_rect.shrink(3.0)),
                    origin,
                    &mut editor.math,
                    style,
                    click_pos,
                    true,
                );
                if editor.focus {
                    editor.focus = false;
                } else if !self.tool_search.is_blocking_sheet(ctx)
                    && math_view::handle_keyboard(ctx, &mut editor.math)
                {
                    editor.draft = editor.math.to_source();
                    editor.synced_draft = editor.draft.clone();
                }
                ui.add_space(4.);
                ui.label(
                    RichText::new(format!("Запись: {}", editor.draft))
                        .monospace()
                        .size(11.5)
                        .color(tool_ribbon::TEXT_MUTED),
                );
            } else {
                ui.label(
                    RichText::new("p := F / S  или  sqrt(a^2 + b^2) =")
                        .color(tool_ribbon::TEXT_MUTED),
                );
                ui.add_space(4.);
                let response = ui.add(
                    TextEdit::multiline(&mut editor.draft)
                        .code_editor()
                        .desired_width(520.)
                        .desired_rows(4)
                        .char_limit(16_384)
                        .hint_text("Введите формулу целиком"),
                );
                if editor.focus {
                    response.request_focus();
                    editor.focus = false;
                }
                if response.changed() {
                    editor.math = MathEditor::from_source(&editor.draft);
                    editor.synced_draft = editor.draft.clone();
                }
            }
            if let Some(error) = &editor.error {
                ui.label(RichText::new(error).color(ERROR_DARK));
            }
            ui.add_space(4.);
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
            let before = self.document.snapshot();
            match self.document.apply_formula(editor.index, &editor.draft) {
                Ok(()) => {
                    self.record_undo(before);
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
        let before = self.document.snapshot();
        let start = Instant::now();
        match self.document.update_input(index, value) {
            Ok(regions) => {
                self.record_undo(before);
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

    fn insert_reference_from_picker(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Лист Mathcad", &["xmcd", "xmcdz"]);
        if let Some(parent) = self.document.path.as_deref().and_then(Path::parent) {
            dialog = dialog.set_directory(parent);
        }
        let Some(picked) = dialog.pick_file() else {
            return;
        };
        let href = if let Some(parent) = self.document.path.as_deref().and_then(Path::parent) {
            picked
                .strip_prefix(parent)
                .map(|rel| rel.to_string_lossy().into_owned())
                .unwrap_or_else(|_| picked.to_string_lossy().into_owned())
        } else {
            picked.to_string_lossy().into_owned()
        };
        let before = self.document.snapshot();
        let at = self.position_for_insert();
        match self
            .document
            .insert(at, editing::NewRegion::Reference(&href))
        {
            Ok(index) => {
                self.record_undo(before);
                self.select(index);
                self.jump = Some(index);
                self.error = None;
                self.status = format!("Подключён лист · {href}");
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn reload_references(&mut self) {
        let snap = self.document.snapshot();
        match self.document.restore(snap) {
            Ok(()) => {
                self.sync_selection_from_ids();
                self.error = None;
                self.status = format!(
                    "Подключения обновлены · рассчитано формул {}",
                    self.document.formulas - self.document.errors
                );
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn apply_display_unit(&mut self, index: usize, unit: Option<&str>) {
        let before = self.document.snapshot();
        match self.document.set_display_unit(index, unit) {
            Ok(()) => {
                self.record_undo(before);
                self.unit_entry = self
                    .document
                    .sheet
                    .display_unit_label(index)
                    .unwrap_or_default()
                    .to_owned();
                self.refresh_details(index);
                self.error = None;
                self.status = match unit.map(str::trim).filter(|s| !s.is_empty()) {
                    Some(u) => format!("Единица вывода: {u}"),
                    None => "Единица вывода сброшена".into(),
                };
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn apply_precision(&mut self, precision: Option<usize>) {
        if self.document.precision == precision {
            return;
        }
        let before = self.document.snapshot();
        match self.document.set_precision(precision) {
            Ok(()) => {
                self.record_undo(before);
                if let Some(index) = self.selected {
                    self.refresh_details(index);
                }
                self.error = None;
                self.status = match precision {
                    Some(p) => format!("Точность вывода: {p} знач. цифр"),
                    None => "Точность вывода: авто (12 цифр)".into(),
                };
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn apply_palette_action(&mut self, action: math_view::PaletteAction) {
        if let Some(editor) = &mut self.editor {
            if editor.text_mode {
                editor.math = MathEditor::from_source(&editor.draft);
                editor.text_mode = false;
            }
            action.apply(&mut editor.math);
            editor.draft = editor.math.to_source();
            editor.synced_draft = editor.draft.clone();
            return;
        }
        if let Some(insert) = &mut self.insertion {
            if let DraftRegion::Formula {
                math,
                text,
                text_mode,
            } = &mut insert.draft
            {
                if *text_mode {
                    *math = MathEditor::from_source(text);
                    *text_mode = false;
                }
                action.apply(math);
                *text = math.to_source();
                return;
            }
        }
        let mut math = MathEditor::empty();
        action.apply(&mut math);
        let text = math.to_source();
        self.start_insert(DraftRegion::Formula {
            math,
            text,
            text_mode: false,
        });
    }

    fn apply_tool_action(&mut self, action: tool_ribbon::ToolAction) {
        match action {
            tool_ribbon::ToolAction::Math(act) => self.apply_palette_action(act),
            tool_ribbon::ToolAction::CustomMatrix => {
                let (rows, cols) = (self.matrix_rows, self.matrix_cols);
                self.apply_palette_action(math_view::PaletteAction::Matrix(rows, cols));
            }
            tool_ribbon::ToolAction::ChartXY => {
                self.start_insert(DraftRegion::Chart(Box::default()));
            }
            tool_ribbon::ToolAction::ChartVectors => {
                self.start_insert(DraftRegion::Chart(Box::new(
                    chart_view::Draft::vectors_preset(),
                )));
            }
            tool_ribbon::ToolAction::ChartParametric => {
                self.start_insert(DraftRegion::Chart(Box::new(
                    chart_view::Draft::parametric_preset(),
                )));
            }
            tool_ribbon::ToolAction::ChartPolar => {
                self.start_insert(DraftRegion::Chart(Box::new(
                    chart_view::Draft::polar_preset(),
                )));
            }
            tool_ribbon::ToolAction::Plot3dSurface => {
                self.start_insert(DraftRegion::Plot3d(Box::new(
                    plot3d_view::Draft3d::default_surface(),
                )));
            }
            tool_ribbon::ToolAction::Plot3dContour => {
                self.start_insert(DraftRegion::Plot3d(Box::new(
                    plot3d_view::Draft3d::default_contour(),
                )));
            }
            tool_ribbon::ToolAction::Table => {
                self.table_editor = Some(TableEditor::empty("Data", 3, 2));
            }
        }
    }

    fn tool_ribbon_panel(&mut self, ctx: &egui::Context) {
        if !self.show_palette {
            return;
        }
        if let Some(action) = tool_ribbon::show_ribbon(
            ctx,
            &mut self.palette_tab,
            &mut self.ribbon_expanded,
            &mut self.matrix_rows,
            &mut self.matrix_cols,
            &mut self.tool_search,
        ) {
            self.apply_tool_action(action);
            ctx.request_repaint();
        }
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar")
            .exact_height(26.)
            .frame(
                Frame::new()
                    .fill(MENU_BG)
                    .stroke(Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK))
                    .inner_margin(Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button("Файл", |ui| {
                        if ui.button("Новый лист (Ctrl+N)").clicked() {
                            self.request_open(Open::Blank, ctx);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.load.is_none(),
                                egui::Button::new("Открыть… (Ctrl+O)"),
                            )
                            .clicked()
                        {
                            self.request_open(Open::Picker, ctx);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.load.is_none(),
                                egui::Button::new("Пример с графиком"),
                            )
                            .clicked()
                        {
                            self.request_open(Open::Demo, ctx);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                self.load.is_none(),
                                egui::Button::new("Сохранить копию… (Ctrl+S)"),
                            )
                            .clicked()
                        {
                            self.save_copy();
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                self.document.path.is_some() && self.load.is_none(),
                                egui::Button::new("Перезагрузить с диска (Ctrl+R)"),
                            )
                            .clicked()
                        {
                            self.reload_from_disk(ctx);
                            ui.close();
                        }
                        if ui.button("Обновить подключения (F5)").clicked() {
                            self.reload_references();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Импорт таблицы CSV…").clicked() {
                            self.import_csv_from_picker();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.selected.is_some(),
                                egui::Button::new("Экспорт результата в CSV…"),
                            )
                            .clicked()
                        {
                            self.export_selected_csv();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Параметры страницы и колонтитулы…").clicked()
                        {
                            self.page_dialog = Some(self.document.page_settings.clone());
                            ui.close();
                        }
                        if ui.button("Экспорт отчёта (HTML / Печать)…").clicked()
                        {
                            self.export_html_report_picker();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.pdf_export.is_none(),
                                egui::Button::new("Экспорт в PDF…"),
                            )
                            .clicked()
                        {
                            self.export_pdf_report_picker(ctx);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Выход").clicked() {
                            self.request_open(Open::Close, ctx);
                            ui.close();
                        }
                    });
                    ui.menu_button("Правка", |ui| {
                        if ui
                            .add_enabled(
                                !self.undo_stack.is_empty(),
                                egui::Button::new("Отменить (Ctrl+Z)"),
                            )
                            .clicked()
                        {
                            self.undo();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.redo_stack.is_empty(),
                                egui::Button::new("Повторить (Ctrl+Y)"),
                            )
                            .clicked()
                        {
                            self.redo();
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                !self.selected_ids.is_empty(),
                                egui::Button::new("Вырезать (Ctrl+X)"),
                            )
                            .clicked()
                        {
                            self.remove_selected(true);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.selected_ids.is_empty(),
                                egui::Button::new("Копировать (Ctrl+C)"),
                            )
                            .clicked()
                        {
                            self.copy_selected();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.clipboard.is_empty(),
                                egui::Button::new("Вставить (Ctrl+V)"),
                            )
                            .clicked()
                        {
                            self.paste_clipboard();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.selected_ids.is_empty(),
                                egui::Button::new("Удалить (Del)"),
                            )
                            .clicked()
                        {
                            self.remove_selected(false);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                self.selected.is_some(),
                                egui::Button::new("Править формулу… (F2)"),
                            )
                            .clicked()
                        {
                            if let Some(idx) = self.selected {
                                self.start_formula(idx);
                            }
                            ui.close();
                        }
                    });
                    ui.menu_button("Вид", |ui| {
                        ui.checkbox(&mut self.show_grid, "Сетка листа");
                        ui.checkbox(&mut self.show_regions, "Границы регионов");
                        ui.checkbox(&mut self.print_preview, "Предпросмотр печати (страницы)");
                        ui.checkbox(&mut self.auto_reload, "Автообновление файла с диска");
                        if ui
                            .add_enabled(
                                !self.document.input_indices.is_empty(),
                                egui::Button::new("Анимация параметра…"),
                            )
                            .clicked()
                        {
                            let idx = self
                                .selected
                                .filter(|&i| self.document.sheet.editable(i))
                                .unwrap_or(self.document.input_indices[0]);
                            self.open_animation(idx);
                            ui.close();
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_palette, "Панель инструментов");
                        ui.checkbox(&mut self.show_inputs, "Панель входов и инспектор");
                        ui.separator();
                        if ui.button("Масштаб 100 %").clicked() {
                            self.fit = false;
                            self.zoom = NORMAL_ZOOM;
                            ui.close();
                        }
                        if ui.checkbox(&mut self.fit, "По ширине листа").clicked() {
                            ui.close();
                        }
                    });
                    ui.menu_button("Вставка", |ui| {
                        if ui.button("Формула").clicked() {
                            self.start_insert(formula_draft(String::new()));
                            ui.close();
                        }
                        if ui.button("Текстовый регион").clicked() {
                            self.start_insert(new_text_draft());
                            ui.close();
                        }
                        if ui.button("Таблица данных / CSV…").clicked() {
                            self.table_editor = Some(TableEditor::empty("Data", 3, 2));
                            ui.close();
                        }
                        if ui.button("Подключить лист XMCD…").clicked() {
                            self.insert_reference_from_picker();
                            ui.close();
                        }
                        if ui.button("Разрыв страницы").clicked() {
                            self.insert_page_break();
                            ui.close();
                        }
                        if ui.button("Сворачиваемая область…").clicked() {
                            let at = self.position_for_insert();
                            self.area_dialog = Some(AreaDialog {
                                title: "Область расчёта".into(),
                                top: at.y,
                                height: 180.0,
                                error: None,
                            });
                            ui.close();
                        }
                        if ui.button("Изображение (PNG / BMP / JPEG)…").clicked() {
                            self.insert_image_from_picker();
                            ui.close();
                        }
                        if ui.button("График X–Y").clicked() {
                            self.start_insert(DraftRegion::Chart(Box::default()));
                            ui.close();
                        }
                        if ui.button("Параметрический график").clicked() {
                            self.start_insert(DraftRegion::Chart(Box::new(
                                chart_view::Draft::parametric_preset(),
                            )));
                            ui.close();
                        }
                        if ui.button("Полярный график").clicked() {
                            self.start_insert(DraftRegion::Chart(Box::new(
                                chart_view::Draft::polar_preset(),
                            )));
                            ui.close();
                        }
                        if ui.button("3D-поверхность z(x, y)").clicked() {
                            self.start_insert(DraftRegion::Plot3d(Box::new(
                                plot3d_view::Draft3d::default_surface(),
                            )));
                            ui.close();
                        }
                        if ui.button("Контурный график (матрица / z(x, y))").clicked()
                        {
                            self.start_insert(DraftRegion::Plot3d(Box::new(
                                plot3d_view::Draft3d::default_contour(),
                            )));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Квадратный корень √x").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Sqrt);
                            ui.close();
                        }
                        if ui.button("Матрица 2×2").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Matrix(2, 2));
                            ui.close();
                        }
                        if ui.button("Вектор 3×1").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Matrix(3, 1));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Производная diff(f, x0)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Derivative);
                            ui.close();
                        }
                        if ui.button("Интеграл integ(f, a, b)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::BigOp(
                                list_rs::math_editor::BigOpKind::Integral,
                            ));
                            ui.close();
                        }
                        if ui.button("Сумма isum(f, k0, k1)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::BigOp(
                                list_rs::math_editor::BigOpKind::Sum,
                            ));
                            ui.close();
                        }
                        if ui.button("Произведение iprod(f, k0, k1)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::BigOp(
                                list_rs::math_editor::BigOpKind::Product,
                            ));
                            ui.close();
                        }
                        if ui.button("Поиск корня root(f, x0)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("root"));
                            ui.close();
                        }
                        if ui.button("Корни полинома polyroots(v)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("polyroots"));
                            ui.close();
                        }
                        if ui.button("Система уравнений nsolve(F, x0)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("nsolve"));
                            ui.close();
                        }
                        if ui.button("Минимизация minimize(f, x0)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("minimize"));
                            ui.close();
                        }
                        if ui.button("Максимизация maximize(f, x0)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("maximize"));
                            ui.close();
                        }
                        if ui.button("Блок решения Given … Find").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Raw("Given"));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Программный блок prog(…)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Program(2));
                            ui.close();
                        }
                        if ui.button("Решатель ОДУ odesolve(f, x0, y0, x1)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("odesolve"));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Символьное упрощение simplify(…) ->").clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("simplify"));
                            ui.close();
                        }
                        if ui.button("Символьная производная sdiff(f, x) ->").clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("sdiff"));
                            ui.close();
                        }
                        if ui.button("Символьное решение ssolve(eq, x) ->").clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("ssolve"));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Собственные значения eigenvals(M)").clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("eigenvals"));
                            ui.close();
                        }
                        if ui.button("QR-разложение qr(M)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("qr"));
                            ui.close();
                        }
                        if ui.button("БПФ сигнала fft(v) / ifft(v)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("fft"));
                            ui.close();
                        }
                        if ui.button("Кубический сплайн cspline(vx, vy)").clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("cspline"));
                            ui.close();
                        }
                        if ui
                            .button("Полиномиальная регрессия regress(vx, vy, k)")
                            .clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("regress"));
                            ui.close();
                        }
                        if ui
                            .button("Нормальное распределение pnorm(x, μ, σ)")
                            .clicked()
                        {
                            self.apply_palette_action(math_view::PaletteAction::Call("pnorm"));
                            ui.close();
                        }
                        if ui.button("Температура Цельсия C(t)").clicked() {
                            self.apply_palette_action(math_view::PaletteAction::Call("C"));
                            ui.close();
                        }
                    });
                    ui.menu_button("Формат", |ui| {
                        if ui.button("Параметры страницы и колонтитулы…").clicked()
                        {
                            self.page_dialog = Some(self.document.page_settings.clone());
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.selected_text().is_some(),
                                egui::Button::new("Текст и стиль…"),
                            )
                            .clicked()
                        {
                            if let Some((index, _)) = self.selected_text() {
                                self.edit_text(index);
                            }
                            ui.close();
                        }
                        ui.menu_button("Точность результата", |ui| {
                            self.precision_options(ui)
                        });
                    });
                    ui.menu_button("Инструменты", |ui| {
                        if ui.button("Поиск инструментов (Ctrl+K)").clicked() {
                            self.show_palette = true;
                            self.tool_search.request_focus(ctx);
                            ui.close();
                        }
                        ui.separator();
                        for group in tool_ribbon::GROUPS {
                            if ui
                                .button(group.title)
                                .on_hover_text(group.tooltip)
                                .clicked()
                            {
                                self.show_palette = true;
                                self.palette_tab = group.tab;
                                self.ribbon_expanded = true;
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Символьные операции", |ui| {
                        for item in tool_ribbon::tools_in_group(math_view::PaletteTab::Symbolic) {
                            if item.id.starts_with("sym.")
                                && ui.button(item.name).on_hover_text(item.tooltip).clicked()
                            {
                                self.apply_tool_action(item.action);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Справка", |ui| {
                        ui.label(format!("Лист {}", env!("CARGO_PKG_VERSION")));
                        ui.separator();
                        ui.label("Ctrl+K — поиск инструмента");
                        ui.label("F2 — правка выбранной формулы");
                        ui.label("Tab и Shift+Tab — переход между полями формулы");
                        if ui.button("Открыть пример с графиком").clicked() {
                            self.request_open(Open::Demo, ctx);
                            ui.close();
                        }
                    });
                });
            });
    }

    fn selected_text(&self) -> Option<(usize, editing::TextStyle)> {
        let index = self.selected?;
        let region = self.document.sheet.regions.get(index)?;
        if region.imported
            || region.visual.graphical
            || region.is_page_break()
            || region.label == "Ссылка"
        {
            return None;
        }
        region.text.as_ref()?;
        Some((
            index,
            editing::TextStyle::from_heading_level(region.visual.heading),
        ))
    }

    fn apply_text_format(&mut self, index: usize, format: TextFormat) -> Calc<()> {
        let region = &self.document.sheet.regions[index];
        if region.visual.text_format == Some(format) {
            return Ok(());
        }
        let xml = editing::format_text_region(&self.document.xml, &region.source_id, format)?;
        let candidate = self.document.rebuild(&xml, None)?;
        self.record_undo(self.document.snapshot());
        self.document = candidate;
        self.sync_selection_from_ids();
        self.error = None;
        self.status = "Формат текста обновлён".into();
        Ok(())
    }

    fn precision_options(&mut self, ui: &mut egui::Ui) {
        for value in RESULT_PRECISIONS {
            let label = value.map_or_else(
                || "Авто (12 цифр)".to_owned(),
                |digits| format!("{digits} значащих цифр"),
            );
            if ui
                .selectable_label(self.document.precision == value, label)
                .clicked()
            {
                self.apply_precision(value);
                ui.close();
            }
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        use command_icons::{button, Icon};

        egui::TopBottomPanel::top("toolbar")
            .frame(
                Frame::new()
                    .fill(tool_ribbon::PANEL_DARK)
                    .inner_margin(Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.;
                    let ready = self.load.is_none();
                    if button(ui, Icon::New, "Новый лист (Ctrl+N)", ready) {
                        self.request_open(Open::Blank, ctx);
                    }
                    if button(ui, Icon::Open, "Открыть лист (Ctrl+O)", ready) {
                        self.request_open(Open::Picker, ctx);
                    }
                    if button(ui, Icon::Save, "Сохранить копию XMCD (Ctrl+S)", ready)
                    {
                        self.save_copy();
                    }
                    if button(
                        ui,
                        Icon::ExportPdf,
                        "Экспорт в PDF",
                        ready && self.pdf_export.is_none(),
                    ) {
                        self.export_pdf_report_picker(ctx);
                    }
                    ui.separator();
                    let selected = !self.selected_ids.is_empty();
                    if button(ui, Icon::Cut, "Вырезать регионы (Ctrl+X)", selected) {
                        self.remove_selected(true);
                    }
                    if button(ui, Icon::Copy, "Копировать регионы (Ctrl+C)", selected)
                    {
                        self.copy_selected();
                    }
                    if button(
                        ui,
                        Icon::Paste,
                        "Вставить регионы (Ctrl+V)",
                        !self.clipboard.is_empty(),
                    ) {
                        self.paste_clipboard();
                    }
                    ui.separator();
                    if button(
                        ui,
                        Icon::Undo,
                        "Отменить (Ctrl+Z)",
                        !self.undo_stack.is_empty(),
                    ) {
                        self.undo();
                    }
                    if button(
                        ui,
                        Icon::Redo,
                        "Повторить (Ctrl+Y)",
                        !self.redo_stack.is_empty(),
                    ) {
                        self.redo();
                    }
                    ui.separator();
                    if button(ui, Icon::Formula, "Вставить формулу", ready) {
                        self.start_insert(formula_draft(String::new()));
                    }
                    if button(ui, Icon::Text, "Вставить текстовый регион", ready)
                    {
                        self.start_insert(new_text_draft());
                    }
                    if button(ui, Icon::Graph, "Вставить график X–Y", ready) {
                        self.apply_tool_action(tool_ribbon::ToolAction::ChartXY);
                    }
                    ui.separator();
                    let zoom = self.effective_zoom(self.sheet_width);
                    let caption = if self.fit {
                        "По ширине".to_owned()
                    } else {
                        format!("{:.0} %", zoom / NORMAL_ZOOM * 100.)
                    };
                    egui::ComboBox::from_id_salt("worksheet_zoom")
                        .width(98.)
                        .selected_text(caption)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(self.fit, "По ширине").clicked() {
                                self.fit = true;
                            }
                            ui.separator();
                            for percent in [50, 75, 100, 125, 150, 200] {
                                let value = NORMAL_ZOOM * (percent as f32 / 100.);
                                if ui
                                    .selectable_label(
                                        !self.fit && self.zoom == value,
                                        format!("{percent} %"),
                                    )
                                    .clicked()
                                {
                                    self.fit = false;
                                    self.zoom = value;
                                }
                            }
                            ui.separator();
                            for (label, value) in [
                                ("Уменьшить", (zoom / 1.17).max(0.50)),
                                ("Увеличить", (zoom * 1.17).min(2.8)),
                            ] {
                                if ui.button(label).clicked() {
                                    self.fit = false;
                                    self.zoom = value;
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text("Масштаб листа");
                    ui.separator();
                    ui.add(
                        egui::Label::new(
                            RichText::new(&self.document.name).color(tool_ribbon::TEXT_MUTED),
                        )
                        .truncate(),
                    )
                    .on_hover_text(self.document.path.as_deref().map_or_else(
                        || self.document.name.clone(),
                        |path| path.display().to_string(),
                    ));
                });
            });
    }

    fn format_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("format_bar")
            .frame(
                Frame::new()
                    .fill(tool_ribbon::PANEL_SURFACE)
                    .stroke(Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK))
                    .inner_margin(Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                let show_view_toggles = ui.available_width() >= 780.;
                ui.horizontal(|ui| {
                    let text = self.selected_text();
                    let label = match text.map(|(_, style)| style) {
                        Some(editing::TextStyle::Normal) => "Обычный",
                        Some(editing::TextStyle::Heading1) => "Заголовок 1",
                        Some(editing::TextStyle::Heading2) => "Заголовок 2",
                        None => "Текст и стиль…",
                    };
                    if ui
                        .add_enabled(text.is_some(), egui::Button::new(label))
                        .on_hover_text("Изменить стиль и текст выбранного региона")
                        .on_disabled_hover_text("Выберите текстовый регион на листе")
                        .clicked()
                    {
                        if let Some((index, _)) = text {
                            self.edit_text(index);
                        }
                    }
                    ui.separator();
                    let target = text.and_then(|(index, _)| {
                        self.document.sheet.regions[index]
                            .visual
                            .text_format
                            .map(|format| (index, format))
                    });
                    let mut format = target.map(|(_, format)| format);
                    ui.add_enabled_ui(format.is_some(), |ui| {
                        let font_label = format.map_or("Шрифт", |value| value.font.name());
                        egui::ComboBox::from_id_salt("region_text_font")
                            .width(128.)
                            .selected_text(font_label)
                            .show_ui(ui, |ui| {
                                if let Some(format) = &mut format {
                                    for font in TextFont::OPTIONS {
                                        ui.selectable_value(&mut format.font, font, font.name());
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Шрифт выбранного текста");
                        egui::ComboBox::from_id_salt("region_text_size")
                            .width(48.)
                            .selected_text(
                                format.map_or_else(
                                    || "пт".to_owned(),
                                    |value| value.size.to_string(),
                                ),
                            )
                            .show_ui(ui, |ui| {
                                if let Some(format) = &mut format {
                                    for size in
                                        [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 72]
                                    {
                                        ui.selectable_value(
                                            &mut format.size,
                                            size,
                                            format!("{size} пт"),
                                        );
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Размер шрифта");
                        for (index, label, tip) in [
                            (0, RichText::new("B").strong(), "Полужирный"),
                            (1, RichText::new("I").italics(), "Курсив"),
                            (2, RichText::new("U").underline(), "Подчёркивание"),
                        ] {
                            let selected = format.is_some_and(|value| {
                                [value.bold, value.italic, value.underline][index]
                            });
                            if ui
                                .selectable_label(selected, label)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                if let Some(value) = &mut format {
                                    let field = match index {
                                        0 => &mut value.bold,
                                        1 => &mut value.italic,
                                        _ => &mut value.underline,
                                    };
                                    *field = !*field;
                                }
                            }
                        }
                        ui.separator();
                        for (alignment, label, tip) in [
                            (
                                TextAlignment::Left,
                                "Влево",
                                "Выровнять текст по левому краю",
                            ),
                            (TextAlignment::Center, "Центр", "Выровнять текст по центру"),
                            (
                                TextAlignment::Right,
                                "Вправо",
                                "Выровнять текст по правому краю",
                            ),
                        ] {
                            if ui
                                .selectable_label(
                                    format.is_some_and(|value| value.alignment == alignment),
                                    label,
                                )
                                .on_hover_text(tip)
                                .clicked()
                            {
                                if let Some(value) = &mut format {
                                    value.alignment = alignment;
                                }
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text("Выберите текстовый регион с однородным оформлением");
                    if let (Some((index, previous)), Some(format)) = (target, format) {
                        if format != previous {
                            if let Err(error) = self.apply_text_format(index, format) {
                                self.error = Some(error);
                            }
                        }
                    }
                    ui.separator();
                    ui.label("Точность");
                    egui::ComboBox::from_id_salt("result_precision")
                        .width(80.)
                        .selected_text(
                            self.document.precision.map_or_else(
                                || "Авто".to_owned(),
                                |digits| format!("{digits} цифр"),
                            ),
                        )
                        .show_ui(ui, |ui| self.precision_options(ui))
                        .response
                        .on_hover_text("Число значащих цифр в результатах");
                    if show_view_toggles {
                        ui.separator();
                        ui.toggle_value(&mut self.show_grid, "Сетка")
                            .on_hover_text("Показать сетку листа");
                        ui.toggle_value(&mut self.show_regions, "Регионы")
                            .on_hover_text("Показать границы регионов");
                    }
                });
            });
    }

    fn footer(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                Frame::new()
                    .fill(MENU_BG)
                    .stroke(Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK))
                    .inner_margin(Margin::symmetric(14, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(error) = &self.error {
                        ui.add(
                            egui::Label::new(RichText::new(error).color(ERROR_DARK).size(12.))
                                .truncate(),
                        )
                        .on_hover_text(error);
                    } else {
                        ui.label(
                            RichText::new(&self.status)
                                .color(tool_ribbon::TEXT_MUTED)
                                .size(12.),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let summary = if self.document.skipped_visual > 0 {
                            format!(
                                "Авто · {} формул · {} входов · {} ошибок · {} пропущ.",
                                self.document.formulas,
                                self.document.input_indices.len(),
                                self.document.errors,
                                self.document.skipped_visual
                            )
                        } else {
                            format!(
                                "Авто · {} формул · {} входов · {} ошибок",
                                self.document.formulas,
                                self.document.input_indices.len(),
                                self.document.errors
                            )
                        };
                        ui.label(
                            RichText::new(summary)
                                .color(tool_ribbon::TEXT_MUTED)
                                .size(12.),
                        );
                        if self.document.unsaved {
                            ui.separator();
                            ui.label(
                                RichText::new("Есть изменения")
                                    .color(tool_ribbon::BLUE_ACCENT_BORDER)
                                    .size(12.),
                            );
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
            .default_width(270.)
            .min_width(215.)
            .frame(
                Frame::new()
                    .fill(tool_ribbon::PANEL_DARK)
                    .stroke(Stroke::new(1.0_f32, tool_ribbon::BORDER_DARK))
                    .inner_margin(Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.heading(RichText::new("Входные значения").size(16.5));
                ui.add_space(5.);
                if self.document.input_indices.is_empty() {
                    ui.label(
                        RichText::new("В листе нет доступных для изменения входов.")
                            .color(tool_ribbon::TEXT_MUTED)
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
                            .color(tool_ribbon::TEXT_MUTED),
                    );
                    return;
                };
                let region = &self.document.sheet.regions[index];
                ui.label(
                    RichText::new(format!("#{} · XMCD {}", index + 1, region.source_id))
                        .size(11.)
                        .color(tool_ribbon::TEXT_MUTED),
                );
                let input = self.document.sheet.editable(index);
                if !region.visual.formula.is_empty() && !input {
                    ui.add_space(7.);
                    ui.label(RichText::new(&region.visual.formula).size(14.));
                }
                let has_formula_editor = region.visual.editor.is_some();
                let can_set_unit = has_formula_editor && !region.visual.function && !input;
                let can_edit_text = region.text.is_some()
                    && !region.visual.graphical
                    && !region.is_page_break()
                    && region.label != "Ссылка";
                if has_formula_editor && ui.button("Править формулу…").on_hover_text("F2").clicked()
                {
                    self.start_formula(index);
                }
                if can_edit_text && ui.button("Править текст…").clicked() {
                    self.edit_text(index);
                }
                if has_formula_editor
                    && self.document.sheet.region_real_matrix(index).is_some()
                    && ui.button("Редактировать таблицу…").clicked()
                {
                    self.open_table_for_region(index);
                }
                if self
                    .document
                    .sheet
                    .export_csv(index, ',', self.document.precision)
                    .is_ok()
                    && ui.button("Экспорт в CSV…").clicked()
                {
                    self.export_selected_csv();
                }
                if can_set_unit {
                    ui.add_space(6.);
                    let mut set_unit_action: Option<Option<String>> = None;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Ед.:")
                                .size(12.)
                                .color(tool_ribbon::TEXT_MUTED),
                        );
                        let resp = ui.add(
                            TextEdit::singleline(&mut self.unit_entry)
                                .desired_width(82.)
                                .hint_text("kPa, mm, deg"),
                        );
                        if (resp.has_focus() || resp.lost_focus())
                            && ui.input(|i| i.key_pressed(Key::Enter))
                        {
                            let v = self.unit_entry.trim().to_owned();
                            set_unit_action = Some((!v.is_empty()).then_some(v));
                        }
                        if ui
                            .small_button("✓")
                            .on_hover_text("Задать единицу вывода")
                            .clicked()
                        {
                            let v = self.unit_entry.trim().to_owned();
                            set_unit_action = Some((!v.is_empty()).then_some(v));
                        }
                        if ui
                            .add_enabled(
                                self.document.sheet.display_unit_label(index).is_some(),
                                egui::Button::new("×").small(),
                            )
                            .on_hover_text("Сбросить единицу вывода")
                            .clicked()
                        {
                            set_unit_action = Some(None);
                        }
                    });
                    if let Some(unit) = set_unit_action {
                        self.apply_display_unit(index, unit.as_deref());
                    }
                }
                if self.document.plots.contains_key(&index) {
                    if ui.button("Настроить график…").clicked() {
                        self.edit_chart(index);
                    }
                    if let Some(plot) = self.document.plots.get_mut(&index) {
                        ui.add_space(4.);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Приближение:")
                                    .size(12.)
                                    .color(tool_ribbon::TEXT_MUTED),
                            );
                            if ui.small_button("−").clicked() {
                                plot.zoom = (plot.zoom / 1.4).max(0.25);
                            }
                            if ui.small_button("+").clicked() {
                                plot.zoom = (plot.zoom * 1.4).min(16.0);
                            }
                            if ui
                                .small_button("1:1")
                                .on_hover_text("Сбросить приближение")
                                .clicked()
                            {
                                plot.zoom = 1.0;
                            }
                        });
                    }
                }
                if self.document.plots3d.contains_key(&index) {
                    if ui.button("Настроить 3D-график…").clicked() {
                        self.edit_plot3d(index);
                    }
                    let mut new_angles: Option<(f32, f32)> = None;
                    if let Some(plot3d) = self.document.plots3d.get(&index) {
                        if plot3d.spec.mode == editing::Plot3dMode::Surface {
                            let (yaw, pitch) = (plot3d.spec.yaw_deg, plot3d.spec.pitch_deg);
                            ui.add_space(4.);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Ракурс:")
                                        .size(12.)
                                        .color(tool_ribbon::TEXT_MUTED),
                                );
                                if ui.small_button("◀").clicked() {
                                    new_angles = Some((yaw - 15.0, pitch));
                                }
                                if ui.small_button("▶").clicked() {
                                    new_angles = Some((yaw + 15.0, pitch));
                                }
                                if ui.small_button("▲").clicked() {
                                    new_angles = Some((yaw, (pitch + 10.0).min(80.0)));
                                }
                                if ui.small_button("▼").clicked() {
                                    new_angles = Some((yaw, (pitch - 10.0).max(-80.0)));
                                }
                                if ui.small_button("Сброс").clicked() {
                                    new_angles = Some((38.0, 30.0));
                                }
                            });
                        }
                    }
                    if let Some((yaw, pitch)) = new_angles {
                        let _ = self.document.save_plot3d_angle(index, yaw, pitch);
                    }
                }
                if input {
                    ui.add_space(10.);
                    ui.label(
                        RichText::new("Enter в поле на листе применяет число.")
                            .size(12.)
                            .color(tool_ribbon::TEXT_MUTED),
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
                    if ui.button("Анимировать параметр…").clicked() {
                        self.open_animation(index);
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
            .frame(Frame::new().fill(tool_ribbon::SHELL_BG))
            .show(ctx, |ui| {
                if self.document.errors > 0 || self.document.skipped_visual > 0 {
                    let (banner_bg, banner_border, banner_ink) = if self.document.errors > 0 {
                        (
                            Color32::from_rgb(76, 28, 36),
                            ERROR_DARK,
                            Color32::from_rgb(255, 214, 210),
                        )
                    } else {
                        (
                            Color32::from_rgb(66, 52, 22),
                            WARN_DARK,
                            Color32::from_rgb(255, 236, 179),
                        )
                    };
                    let mut parts = Vec::new();
                    if self.document.errors > 0 {
                        parts.push(format!(
                            "Ошибки формул: {} (результаты зависимых расчётов могут быть недостоверны)",
                            self.document.errors
                        ));
                    }
                    if self.document.skipped_visual > 0 {
                        parts.push(format!(
                            "Пропущено визуальных блоков: {} (не влияют на расчёт)",
                            self.document.skipped_visual
                        ));
                    }
                    Frame::new()
                        .fill(banner_bg)
                        .stroke(Stroke::new(1.0_f32, banner_border))
                        .inner_margin(Margin::symmetric(14, 5))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(parts.join(" · "))
                                    .size(12.)
                                    .color(banner_ink),
                            );
                        });
                }
                self.sheet_width = ui.available_width();
                let zoom = self.effective_zoom(self.sheet_width);
                egui::ScrollArea::both()
                    .id_salt(("sheet_view", self.generation))
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, viewport| self.paper(ui, zoom, viewport.size()));
            });
    }

    fn effective_zoom(&self, width: f32) -> f32 {
        if !self.fit {
            return self.zoom;
        }
        (width / self.document.extent.x).clamp(0.50, NORMAL_ZOOM)
    }

    fn inline_formula_size(ui: &egui::Ui, draft: &DraftRegion, zoom: f32) -> Option<egui::Vec2> {
        let DraftRegion::Formula {
            math,
            text_mode,
            text,
        } = draft
        else {
            return None;
        };
        if *text_mode {
            let font = FontId::monospace(15.);
            let width = ui
                .painter()
                .layout_no_wrap(text.clone(), font, INK)
                .rect
                .width();
            Some(vec2((width + 32.).clamp(240., 540.), 27.))
        } else {
            let metrics =
                math_view::measure_slot(ui.painter(), &math.root, inline_font_size(zoom), true);
            Some(vec2(
                (metrics.width + 26.).max(190.),
                (metrics.height() + 14.).max(30.),
            ))
        }
    }

    fn paper_size(&self, ui: &egui::Ui, zoom: f32, viewport: egui::Vec2) -> egui::Vec2 {
        let potential = self.insertion.as_ref().map(|draft| draft.at).or(self.caret);
        let mut page_size = (self.document.extent * zoom).max(viewport);
        if let Some(point) = potential {
            let offset = (point - self.document.origin + vec2(25., 26.)) * zoom;
            if let Some(size) = self
                .insertion
                .as_ref()
                .and_then(|entry| Self::inline_formula_size(ui, &entry.draft, zoom))
            {
                page_size = page_size.max(offset + (size + vec2(48., 64.)).max(vec2(415., 0.)));
            } else {
                page_size = page_size.max(offset + vec2(28., 100.));
            }
        }
        page_size
    }

    fn paper(&mut self, ui: &mut egui::Ui, zoom: f32, viewport: egui::Vec2) {
        let (page, _) = ui.allocate_exact_size(self.paper_size(ui, zoom, viewport), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(page, 0., PAPER);
        if self.show_grid {
            let step = (24. * zoom).max(12.);
            let grid_clip = page.intersect(ui.clip_rect());
            if grid_clip.is_positive() {
                let start_x = page.left()
                    + (((grid_clip.left() - page.left()) / step).floor().max(1.) * step);
                let start_y =
                    page.top() + (((grid_clip.top() - page.top()) / step).floor().max(1.) * step);
                let mut y = start_y;
                while y < grid_clip.bottom() - 4. {
                    let mut x = start_x;
                    while x < grid_clip.right() - 4. {
                        painter.circle_filled(Pos2::new(x, y), 0.9, GRID_DOT);
                        x += step;
                    }
                    y += step;
                }
            }
        }
        let sheet_response = ui.interact(
            page,
            egui::Id::new(("paper", self.generation)),
            Sense::click_and_drag(),
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
            let stroke = Stroke::new(1.4_f32, CROSSHAIR);
            ui.painter()
                .line_segment([pixel - vec2(6., 0.), pixel + vec2(7., 0.)], stroke);
            ui.painter()
                .line_segment([pixel - vec2(0., 6.), pixel + vec2(0., 7.)], stroke);
        }
        if let Some(drag) = &mut self.drag {
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                let delta = (pos - drag.start_pointer) / zoom;
                if delta.length() > 2.0 {
                    drag.moved = true;
                }
                drag.delta_sheet = delta;
            }
        }
        let mut next: Option<(usize, bool)> = None;
        let mut submit = None;
        let mut edit_region: Option<(usize, u8)> = None;
        let mut drag_start: Option<(usize, Pos2)> = None;
        let mut screen_rects: Vec<(usize, Rect)> = Vec::with_capacity(self.document.rects.len());
        let clip = ui.clip_rect();
        let ctrl_held = ui.input(|i| i.modifiers.ctrl);
        // Draw collapsible area bars and collect toggle/delete actions
        let mut toggle_area_id: Option<String> = None;
        let mut delete_area_id: Option<String> = None;
        for area in &self.document.areas {
            let y_top = page.min.y + ((area.top - self.document.origin.y) + 20.) * zoom;
            let y_bot = page.min.y + ((area.bottom - self.document.origin.y) + 26.) * zoom;
            let bar = Rect::from_min_max(
                Pos2::new(page.left() + 10., y_top),
                Pos2::new(page.right() - 10., y_top + 20. * zoom),
            );
            if !area.collapsed && y_bot > bar.bottom() {
                let body_rect = Rect::from_min_max(
                    Pos2::new(bar.left(), bar.bottom()),
                    Pos2::new(bar.right(), y_bot),
                );
                ui.painter().rect_stroke(
                    body_rect,
                    3.,
                    Stroke::new(1.0_f32, Color32::from_rgb(186, 208, 236)),
                    StrokeKind::Inside,
                );
            }
            ui.painter().rect(
                bar,
                4.,
                Color32::from_rgb(232, 241, 253),
                Stroke::new(1.0_f32, ACCENT),
                StrokeKind::Inside,
            );
            let arrow = if area.collapsed { "▸" } else { "▾" };
            let label = if area.collapsed {
                format!("{arrow}  {} (свёрнуто)", area.title)
            } else {
                format!("{arrow}  {}", area.title)
            };
            ui.painter().text(
                Pos2::new(bar.left() + 8., bar.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.5 * zoom),
                ACCENT,
            );
            let del_rect = Rect::from_min_max(
                Pos2::new(bar.right() - 22. * zoom, bar.top() + 2.),
                Pos2::new(bar.right() - 4., bar.bottom() - 2.),
            );
            let toggle_rect =
                Rect::from_min_max(bar.min, Pos2::new(del_rect.left() - 2., bar.bottom()));
            let t_resp = ui.interact(
                toggle_rect,
                egui::Id::new(("area_toggle", self.generation, &area.id)),
                Sense::click(),
            );
            if t_resp.clicked() {
                toggle_area_id = Some(area.id.clone());
            }
            if ui
                .put(del_rect, egui::Button::new("×").small())
                .on_hover_text("Удалить границу области (регионы сохранятся)")
                .clicked()
            {
                delete_area_id = Some(area.id.clone());
            }
        }

        let inline_editing_idx = self.insertion.as_ref().and_then(|ins| ins.replace_formula);
        for index in 0..self.document.rects.len() {
            if self.document.sheet.regions[index].imported || inline_editing_idx == Some(index) {
                continue;
            }
            let r_top = self.document.sheet.regions[index].visual.top;
            let hidden_by_area = self
                .document
                .areas
                .iter()
                .any(|a| a.collapsed && r_top >= a.top && r_top <= a.bottom);
            if hidden_by_area {
                continue;
            }
            let original = self.document.rects[index];
            let drag_offset = self
                .drag
                .as_ref()
                .filter(|d| {
                    d.moved
                        && d.ids
                            .iter()
                            .any(|id| id == &self.document.sheet.regions[index].source_id)
                })
                .map_or(egui::Vec2::ZERO, |d| d.delta_sheet * zoom);
            let rect = Rect::from_min_size(
                page.min + original.min.to_vec2() * zoom + drag_offset,
                original.size() * zoom,
            );
            screen_rects.push((index, rect));
            if self.jump == Some(index) {
                ui.scroll_to_rect(rect, Some(Align::Center));
                self.jump = None;
            }
            if !rect.intersects(clip) {
                continue;
            }
            let region = &self.document.sheet.regions[index];
            let selected = self.is_selected_index(index);
            if self.show_regions {
                ui.painter().rect_stroke(
                    rect.expand(1.),
                    2.,
                    Stroke::new(0.8_f32, Color32::from_rgb(196, 210, 218)),
                    StrokeKind::Inside,
                );
            }
            if let Some(plot) = self.document.plots.get(&index) {
                let response = ui.interact(
                    rect,
                    egui::Id::new((self.generation, index)),
                    Sense::click_and_drag(),
                );
                chart_view::draw(
                    ui.painter(),
                    rect,
                    &plot.curves,
                    &plot.options,
                    plot.spec.is_polar(),
                    plot.zoom,
                    response.hover_pos(),
                    selected,
                );
                if response.double_clicked() {
                    edit_region = Some((index, 1));
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        drag_start = Some((index, pos));
                    }
                }
                if response.clicked() {
                    next = Some((index, ctrl_held));
                }
                continue;
            }
            if self.document.plots3d.contains_key(&index) {
                let response = ui.interact(
                    rect,
                    egui::Id::new((self.generation, index)),
                    Sense::click_and_drag(),
                );
                if response.dragged_by(egui::PointerButton::Secondary) {
                    let delta = response.drag_delta();
                    if let Some(plot3d) = self.document.plots3d.get_mut(&index) {
                        if plot3d.spec.mode == editing::Plot3dMode::Surface {
                            plot3d.spec.yaw_deg =
                                (plot3d.spec.yaw_deg + delta.x * 0.6).clamp(-360.0, 360.0);
                            plot3d.spec.pitch_deg =
                                (plot3d.spec.pitch_deg + delta.y * 0.5).clamp(-80.0, 80.0);
                        }
                    }
                }
                if response.drag_stopped_by(egui::PointerButton::Secondary) {
                    if let Some(plot3d) = self.document.plots3d.get(&index) {
                        let (yaw, pitch) = (plot3d.spec.yaw_deg, plot3d.spec.pitch_deg);
                        let _ = self.document.save_plot3d_angle(index, yaw, pitch);
                    }
                }
                if let Some(plot3d) = self.document.plots3d.get(&index) {
                    plot3d_view::draw(ui.painter(), rect, &plot3d.grid, &plot3d.spec, selected);
                }
                if response.double_clicked() {
                    edit_region = Some((index, 3));
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        drag_start = Some((index, pos));
                    }
                }
                if response.clicked() {
                    next = Some((index, ctrl_held));
                }
                continue;
            }
            if let Some(img_spec) = self.document.images.get(&index).cloned() {
                let response = ui.interact(
                    rect,
                    egui::Id::new((self.generation, index)),
                    Sense::click_and_drag(),
                );
                let border = if selected {
                    Stroke::new(1.5_f32, ACCENT)
                } else {
                    Stroke::new(1.0_f32, LINE)
                };
                ui.painter()
                    .rect(rect, 5., PAPER, border, StrokeKind::Inside);
                let rid = self.document.sheet.regions[index].source_id.clone();
                if let Some(tex_id) = self.ensure_image_texture(ui.ctx(), &rid, &img_spec) {
                    ui.painter().image(
                        tex_id,
                        rect.shrink(3.),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                        Color32::WHITE,
                    );
                } else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!(
                            "🖼 {} ({}×{})",
                            img_spec.name, img_spec.width, img_spec.height
                        ),
                        FontId::proportional(12. * zoom),
                        MUTED,
                    );
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        drag_start = Some((index, pos));
                    }
                }
                if response.clicked() {
                    next = Some((index, ctrl_held));
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
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        if let Some(pos) = response.interact_pointer_pos() {
                            drag_start = Some((index, pos));
                        }
                    }
                    if response.clicked() {
                        next = Some((index, ctrl_held));
                    }
                    continue;
                }
                let label_min = prefix.rect.width() + 17.;
                chip.max.x = chip.max.x.max(chip.min.x + label_min + 22.);
                if !selected || self.selected_ids.len() > 1 {
                    let value = self
                        .document
                        .sheet
                        .input_value(index)
                        .map(display_number)
                        .unwrap_or_else(|| "—".into());
                    let unit = region.visual.input_unit.as_deref().unwrap_or_default();
                    let caption = format!("{} := {value} {unit}", region.label);
                    ui.painter().rect_filled(chip, 5., PALE);
                    if selected {
                        ui.painter().rect_stroke(
                            chip,
                            5.,
                            Stroke::new(1_f32, ACCENT),
                            StrokeKind::Inside,
                        );
                    }
                    let response = draw_line(
                        ui,
                        chip.shrink2(vec2(6., 1.)),
                        &caption,
                        font,
                        ACCENT,
                        false,
                        (self.generation, index),
                    );
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        if let Some(pos) = response.interact_pointer_pos() {
                            drag_start = Some((index, pos));
                        }
                    }
                    if response.clicked() {
                        next = Some((index, ctrl_held));
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
                    let response = ui
                        .scope(|ui| {
                            paper_edit_visuals(ui);
                            ui.put(
                                edit_rect,
                                TextEdit::singleline(&mut self.entry)
                                    .id(id)
                                    .char_limit(80)
                                    .font(font.clone())
                                    .desired_width(edit_width)
                                    .margin(Margin::symmetric(4, 1)),
                            )
                        })
                        .inner;
                    if !unit.is_empty() {
                        ui.painter().text(
                            Pos2::new(edit_rect.right() + 4., chip.center().y),
                            egui::Align2::LEFT_CENTER,
                            unit,
                            FontId::proportional(11. * zoom),
                            MUTED,
                        );
                    }
                    if !self.tool_search.enter_consumed
                        && (response.has_focus() || response.lost_focus())
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
                        PAPER_CHIP_BG,
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
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        if let Some(pos) = response.interact_pointer_pos() {
                            drag_start = Some((index, pos));
                        }
                    }
                    if response.clicked() {
                        next = Some((index, ctrl_held));
                    }
                } else if region.is_page_break() {
                    let y = rect.center().y;
                    let stroke = Stroke::new(1.2_f32, if selected { ACCENT } else { MUTED });
                    let mut x = page.left() + 14.;
                    while x < page.right() - 18. {
                        let x2 = (x + 8.).min(page.right() - 14.);
                        ui.painter()
                            .line_segment([Pos2::new(x, y), Pos2::new(x2, y)], stroke);
                        x += 14.;
                    }
                    let label_rect = Rect::from_center_size(
                        Pos2::new(page.center().x, y),
                        vec2(130. * zoom, 16. * zoom),
                    );
                    ui.painter()
                        .rect_filled(label_rect, 3., if selected { PALE } else { PAPER });
                    ui.painter().text(
                        label_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Разрыв страницы",
                        FontId::proportional(10.5 * zoom),
                        if selected { ACCENT } else { MUTED },
                    );
                    let response = ui.interact(
                        Rect::from_min_max(
                            Pos2::new(page.left() + 12., y - 8.),
                            Pos2::new(page.right() - 12., y + 8.),
                        ),
                        egui::Id::new((self.generation, index)),
                        Sense::click_and_drag(),
                    );
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        if let Some(pos) = response.interact_pointer_pos() {
                            drag_start = Some((index, pos));
                        }
                    }
                    if response.clicked() {
                        next = Some((index, ctrl_held));
                    }
                } else {
                    let mut anchor = rect.min;
                    let alignment = region
                        .visual
                        .rich_text
                        .as_ref()
                        .map(|rich| rich.alignment)
                        .or_else(|| region.visual.text_format.map(|format| format.alignment));
                    let galley = if let Some(alignment) = alignment {
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = rect.width().max(1.);
                        job.halign = match alignment {
                            TextAlignment::Left => Align::Min,
                            TextAlignment::Center => {
                                anchor.x = rect.center().x;
                                Align::Center
                            }
                            TextAlignment::Right => {
                                anchor.x = rect.right();
                                Align::Max
                            }
                        };
                        if let Some(rich) = &region.visual.rich_text {
                            append_rich_text(&mut job, rich, color, zoom);
                        } else if let Some(format) = region.visual.text_format {
                            job.append(
                                text,
                                0.,
                                egui::TextFormat {
                                    font_id: FontId::new(
                                        f32::from(format.size) * zoom,
                                        text_font_family(format.font, format.bold, format.italic),
                                    ),
                                    color,
                                    underline: if format.underline {
                                        Stroke::new(zoom, color)
                                    } else {
                                        Stroke::NONE
                                    },
                                    ..Default::default()
                                },
                            );
                        }
                        ui.painter().layout_job(job)
                    } else {
                        ui.painter().layout(
                            text.clone(),
                            FontId::proportional(size * zoom),
                            color,
                            rect.width().max(60.),
                        )
                    };
                    let text_height = if alignment.is_some() {
                        galley.rect.height()
                    } else {
                        galley.rect.height().min(40. * zoom)
                    };
                    let area = Rect::from_min_size(
                        rect.min,
                        vec2(rect.width(), rect.height().max(text_height)),
                    );
                    if selected {
                        ui.painter().rect_filled(area, 4., PALE);
                    }
                    ui.painter()
                        .with_clip_rect(area)
                        .galley(anchor, galley, color);
                    let response = ui.interact(
                        area,
                        egui::Id::new((self.generation, index)),
                        Sense::click_and_drag(),
                    );
                    if response.double_clicked() && region.label != "Ссылка" {
                        edit_region = Some((index, 2));
                    }
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        if let Some(pos) = response.interact_pointer_pos() {
                            drag_start = Some((index, pos));
                        }
                    }
                    if response.clicked() {
                        next = Some((index, ctrl_held));
                    }
                }
                continue;
            }
            let formula = &region.visual.formula;
            let result = self
                .document
                .sheet
                .display_with_precision(index, self.document.precision);
            let (suffix, color) = match &result {
                Ok(_) if region.visual.function => (String::new(), ACCENT),
                Ok(v) if formula.ends_with('→') => (format!("  {v}"), ACCENT),
                Ok(v) => (format!("  =  {v}"), ACCENT),
                Err(_) => ("  · ошибка".into(), ERROR),
            };
            let parsed_2d = region
                .visual
                .editor
                .as_ref()
                .filter(|edit| !edit.native_symbolic_commands)
                .and_then(|e| e.source.as_deref())
                .map(parse_source_to_slot)
                .filter(|slot| slot.has_2d_structures());
            let hit = if let Some(slot) = parsed_2d {
                let base_font = 14. * zoom;
                let raw = math_view::measure_slot(ui.painter(), &slot, base_font, false);
                let scale = if raw.height() > rect.height() && raw.height() > 1.0 {
                    (rect.height() / raw.height()).clamp(0.72, 1.0)
                } else {
                    1.0
                };
                let font_size = (base_font * scale).max(9.0);
                let m = math_view::measure_slot(ui.painter(), &slot, font_size, false);
                let suffix_galley = ui.painter().layout_no_wrap(
                    suffix.clone(),
                    FontId::proportional(font_size),
                    color,
                );
                let total_w = (m.width + suffix_galley.rect.width() + 8.).min(rect.width());
                let total_h = rect.height().max(m.height() + 2.);
                let hit = Rect::from_min_size(rect.min, vec2(total_w, total_h));
                if selected {
                    ui.painter().rect_filled(hit.expand(3.), 5., PALE);
                }
                let style = math_view::Style {
                    font_size,
                    ink: INK,
                    accent: ACCENT,
                    muted: MUTED,
                    index_color: INDEX_INK,
                    placeholder_bg: PALE,
                };
                let origin = Pos2::new(
                    rect.left(),
                    rect.top() + ((total_h - m.height()) * 0.5).max(0.0),
                );
                let clipped = ui.painter().with_clip_rect(hit.expand(1.));
                math_view::paint_static(&clipped, origin, &slot, style);
                if !suffix.is_empty() {
                    let baseline = origin.y + m.ascent;
                    clipped.galley(
                        Pos2::new(origin.x + m.width + 2., baseline - font_size * 0.80),
                        suffix_galley,
                        color,
                    );
                }
                hit
            } else {
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
                hit
            };
            let response = ui.interact(
                hit,
                egui::Id::new((self.generation, index)),
                Sense::click_and_drag(),
            );
            let response = response.on_hover_text(format!(
                "{formula}{suffix}\n{}",
                result.err().unwrap_or_default()
            ));
            if response.double_clicked() {
                edit_region = Some((index, 0));
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                if let Some(pos) = response.interact_pointer_pos() {
                    drag_start = Some((index, pos));
                }
            }
            if response.clicked() {
                next = Some((index, ctrl_held));
            }
        }
        if let Some((index, start_pos)) = drag_start {
            if !self.is_selected_index(index) {
                self.select(index);
            }
            let origins = self
                .selected_ids
                .iter()
                .filter_map(|id| {
                    let idx = self.document.by_id(id).ok()?;
                    let v = &self.document.sheet.regions[idx].visual;
                    Some((id.clone(), v.left, v.top))
                })
                .collect();
            self.drag = Some(DragState {
                ids: self.selected_ids.clone(),
                origins,
                start_pointer: start_pos,
                delta_sheet: egui::Vec2::ZERO,
                moved: false,
            });
        }
        if self.drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
            if let Some(drag) = self.drag.take() {
                if drag.moved {
                    let moves: Vec<_> = drag
                        .origins
                        .into_iter()
                        .map(|(id, left, top)| {
                            (
                                id,
                                (left + drag.delta_sheet.x).max(0.),
                                (top + drag.delta_sheet.y).max(0.),
                            )
                        })
                        .collect();
                    self.move_selected_to(&moves);
                    ui.ctx().request_repaint();
                }
            }
        }
        if self.drag.is_none()
            && self.insertion.is_none()
            && sheet_response.drag_started_by(egui::PointerButton::Primary)
        {
            if let Some(pos) = sheet_response.interact_pointer_pos() {
                self.marquee = Some(MarqueeState {
                    start: pos,
                    current: pos,
                });
            }
        }
        if let Some(marquee) = &mut self.marquee {
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                marquee.current = pos;
            }
            let sel_rect = Rect::from_two_pos(marquee.start, marquee.current);
            ui.painter().rect(
                sel_rect,
                2.,
                Color32::from_rgba_unmultiplied(37, 112, 218, 34),
                Stroke::new(1.0_f32, ACCENT),
                StrokeKind::Inside,
            );
            if !ui.input(|i| i.pointer.primary_down()) {
                let ids: Vec<String> = screen_rects
                    .iter()
                    .filter(|(_, r)| r.intersects(sel_rect))
                    .map(|&(idx, _)| self.document.sheet.regions[idx].source_id.clone())
                    .collect();
                self.marquee = None;
                self.selected_ids = ids;
                self.sync_selection_from_ids();
                if !self.selected_ids.is_empty() {
                    self.caret = None;
                }
                ui.ctx().request_repaint();
            }
        }
        if let Some(index) = submit {
            self.apply(index, false);
            ui.ctx().request_repaint();
        }
        if let Some((index, ctrl)) = next {
            if ctrl {
                self.toggle_select(index);
            } else {
                self.select(index);
                if self.document.sheet.editable(index) {
                    ui.memory_mut(|memory| {
                        memory.request_focus(egui::Id::new(("entry", self.generation, index)))
                    });
                }
            }
            ui.ctx().request_repaint();
        } else if self.insertion.is_none() && self.drag.is_none() && sheet_response.clicked() {
            if let Some(click) = sheet_response.interact_pointer_pos() {
                let relative = (click - page.min) / zoom - vec2(25., 26.);
                self.caret = Some(Pos2::new(
                    (relative.x + self.document.origin.x).max(0.),
                    (relative.y + self.document.origin.y).max(0.),
                ));
                self.selected = None;
                self.selected_ids.clear();
                self.details.clear();
                ui.memory_mut(|memory| {
                    if let Some(focused) = memory.focused() {
                        memory.surrender_focus(focused);
                    }
                });
                ui.ctx().request_repaint();
            }
        }
        if self.print_preview {
            let pages = report::compute_pages(
                &self.document.sheet,
                self.document.page_settings.page_height,
            );
            let total = pages.len();
            let header = if self.document.page_settings.header.trim().is_empty() {
                self.document.name.as_str()
            } else {
                self.document.page_settings.header.trim()
            };
            for slice in &pages {
                let top_px = page.min.y + ((slice.top_y - self.document.origin.y) + 20.) * zoom;
                let bot_px = page.min.y + ((slice.bottom_y - self.document.origin.y) + 26.) * zoom;
                if slice.number > 1 && top_px >= page.top() && top_px <= page.bottom() {
                    ui.painter().line_segment(
                        [
                            Pos2::new(page.left() + 8., top_px),
                            Pos2::new(page.right() - 8., top_px),
                        ],
                        Stroke::new(1.5_f32, ACCENT),
                    );
                }
                if top_px >= page.top() - 10. && top_px <= page.bottom() {
                    ui.painter().text(
                        Pos2::new(page.left() + 14., top_px + 3.),
                        egui::Align2::LEFT_TOP,
                        header,
                        FontId::proportional(10. * zoom),
                        MUTED,
                    );
                    if self.document.page_settings.show_page_numbers {
                        ui.painter().text(
                            Pos2::new(page.right() - 14., top_px + 3.),
                            egui::Align2::RIGHT_TOP,
                            format!("Стр. {} из {total}", slice.number),
                            FontId::proportional(10. * zoom),
                            ACCENT,
                        );
                    }
                }
                if !self.document.page_settings.footer.trim().is_empty()
                    && bot_px >= page.top()
                    && bot_px <= page.bottom()
                {
                    ui.painter().text(
                        Pos2::new(page.left() + 14., bot_px - 14. * zoom),
                        egui::Align2::LEFT_TOP,
                        self.document.page_settings.footer.trim(),
                        FontId::proportional(9.5 * zoom),
                        MUTED,
                    );
                }
            }
        }
        if let Some((index, kind_code)) = edit_region {
            match kind_code {
                1 => self.edit_chart(index),
                2 => self.edit_text(index),
                3 => self.edit_plot3d(index),
                _ => self.start_formula_inline(index),
            }
        }
        if let Some(id) = toggle_area_id {
            let before = self.document.snapshot();
            if self.document.toggle_area(&id).is_ok() {
                self.record_undo(before);
                ui.ctx().request_repaint();
            }
        }
        if let Some(id) = delete_area_id {
            let before = self.document.snapshot();
            if self.document.delete_area(&id).is_ok() {
                self.record_undo(before);
                ui.ctx().request_repaint();
            }
        }
        self.inline_formula(ui, page, zoom);
    }

    fn inline_formula(&mut self, ui: &mut egui::Ui, page: Rect, zoom: f32) {
        let Some(mut insert) = self.insertion.take() else {
            self.last_inline_caret = None;
            return;
        };
        let size = Self::inline_formula_size(ui, &insert.draft, zoom);
        let DraftRegion::Formula {
            math,
            text,
            text_mode,
        } = &mut insert.draft
        else {
            self.last_inline_caret = None;
            self.insertion = Some(insert);
            return;
        };
        let point = page.min + ((insert.at - self.document.origin) + vec2(25., 26.)) * zoom;
        let bar_y = (point.y - 24.).max(page.top() + 4.);
        let mut bx = point.x;
        let btn_h = 20.;
        if !*text_mode {
            for (label, tip, action) in [
                ("a/b", "Вертикальная дробь (/)", 0u8),
                ("xⁿ", "Степень (^)", 1),
                ("√x", "Квадратный корень", 4),
                ("|x|", "Модуль |x|", 6),
                ("∫", "Интеграл ∫_a^b f", 7),
                ("∑", "Сумма ∑_a^b f", 8),
                ("prog", "Программный блок (Shift+Enter: +строка)", 9),
                ("xₐ", "Индекс имени (. после имени)", 2),
                ("A[i]", "Индекс массива ([)", 3),
                ("[2×2]", "Матрица 2×2", 5),
            ] {
                let w = if label.chars().count() > 3 { 38. } else { 26. };
                let r = Rect::from_min_size(Pos2::new(bx, bar_y), vec2(w, btn_h));
                if ui
                    .put(r, egui::Button::new(label).small())
                    .on_hover_text(tip)
                    .clicked()
                {
                    match action {
                        0 => math.insert_fraction(),
                        1 => math.insert_power(),
                        2 => math.insert_subscript(list_rs::math_editor::SubscriptKind::Literal),
                        3 => math.insert_subscript(list_rs::math_editor::SubscriptKind::Index),
                        4 => math.insert_sqrt(),
                        5 => math.insert_matrix(2, 2),
                        6 => math.insert_abs(),
                        7 => math.insert_big_op(list_rs::math_editor::BigOpKind::Integral),
                        8 => math.insert_big_op(list_rs::math_editor::BigOpKind::Sum),
                        _ => math.insert_program(2),
                    }
                    *text = math.to_source();
                }
                bx += w + 3.;
            }
        }
        let mode_rect = Rect::from_min_size(Pos2::new(bx, bar_y), vec2(46., btn_h));
        let mode_label = if *text_mode { "2D" } else { "Текст" };
        if ui
            .put(mode_rect, egui::Button::new(mode_label).small())
            .on_hover_text("Переключить 2D и текстовый ввод")
            .clicked()
        {
            if *text_mode {
                *math = MathEditor::from_source(text);
                *text_mode = false;
            } else {
                *text = math.to_source();
                *text_mode = true;
            }
            insert.focus = true;
        }

        let search_blocks = self.tool_search.is_blocking_sheet(ui.ctx());
        let (field, enter) = if *text_mode {
            self.last_inline_caret = None;
            let field = Rect::from_min_size(point, size.expect("formula size"));
            let response = ui
                .scope(|ui| {
                    paper_edit_visuals(ui);
                    ui.put(
                        field,
                        TextEdit::singleline(text)
                            .font(FontId::monospace(15.))
                            .char_limit(4096)
                            .desired_width(field.width())
                            .hint_text("Формула  x := 2  или  x^2 ="),
                    )
                })
                .inner;
            if insert.focus {
                ui.scroll_to_rect(field, Some(Align::Center));
                response.request_focus();
                insert.focus = false;
            }
            let enter = !search_blocks
                && (response.has_focus() || response.lost_focus())
                && ui.input(|input| input.key_pressed(Key::Enter));
            (field, enter)
        } else {
            let style = math_view::Style {
                font_size: inline_font_size(zoom),
                ink: INK,
                accent: ACCENT,
                muted: MUTED,
                index_color: INDEX_INK,
                placeholder_bg: PALE,
            };
            let metrics = math_view::measure_slot(ui.painter(), &math.root, style.font_size, true);
            let field = Rect::from_min_size(
                point,
                vec2(
                    (metrics.width + 26.).max(190.),
                    (metrics.height() + 14.).max(30.),
                ),
            );
            ui.painter().rect(
                field,
                5.,
                PAPER,
                Stroke::new(1.3_f32, ACCENT),
                StrokeKind::Inside,
            );
            let response = ui.interact(
                field,
                egui::Id::new(("inline_math", self.generation)),
                Sense::click(),
            );
            let click_pos = response
                .clicked()
                .then(|| response.interact_pointer_pos())
                .flatten();
            let origin = Pos2::new(field.left() + 8., field.center().y - metrics.height() * 0.5);
            if math.is_empty() {
                ui.painter().text(
                    Pos2::new(field.left() + 14., field.center().y),
                    egui::Align2::LEFT_CENTER,
                    "x := 2  или  (a + b) / c =",
                    FontId::proportional(12.5),
                    MUTED,
                );
            }
            let caret = math_view::paint_editor(
                &ui.painter().with_clip_rect(field.shrink(2.)),
                origin,
                math,
                style,
                click_pos,
                !search_blocks,
            );
            let position = (math.cursor.clone(), field.size());
            if !search_blocks
                && (insert.focus || self.last_inline_caret.as_ref() != Some(&position))
            {
                let controls = field.union(mode_rect).expand(12.);
                let visible = ui.clip_rect().size();
                let target = if controls.width() <= visible.x && controls.height() <= visible.y {
                    controls
                } else {
                    caret.unwrap_or(field).expand(16.)
                };
                ui.scroll_to_rect(target, None);
            }
            self.last_inline_caret = Some(position);
            if insert.focus {
                insert.focus = false;
            } else if !search_blocks {
                let cursor = math.cursor.clone();
                if math_view::handle_keyboard(ui.ctx(), math) {
                    *text = math.to_source();
                    insert.error = None;
                    ui.ctx().request_repaint();
                } else if math.cursor != cursor {
                    ui.ctx().request_repaint();
                }
            }
            let enter = !search_blocks && ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift)
                })
            });
            (field, enter)
        };

        let ok = ui
            .put(
                Rect::from_min_size(field.right_top() + vec2(5., 0.), vec2(26., 26.)),
                egui::Button::new("✓").small(),
            )
            .on_hover_text("Применить формулу (Enter)");
        if ok.clicked() || enter {
            let size = (field.size() + vec2(165., 8.)) / zoom;
            match self.commit_insert(&insert, Some(size)) {
                Ok(()) => {
                    ui.ctx().request_repaint();
                    return;
                }
                Err(error) => {
                    insert.error = Some(error);
                }
            }
        }
        if !search_blocks && ui.input(|input| input.key_pressed(Key::Escape)) {
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

fn inline_font_size(zoom: f32) -> f32 {
    (15. * zoom).clamp(13., 24.)
}

fn detect_image_meta(bytes: &[u8], path: &Path) -> (String, u32, u32) {
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return ("image/png".into(), w.max(1), h.max(1));
    }
    if bytes.len() >= 26 && &bytes[0..2] == b"BM" {
        let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).unsigned_abs();
        let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).unsigned_abs();
        return ("image/bmp".into(), w.max(1), h.max(1));
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    (mime.into(), 320, 220)
}

fn decode_image_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        return decode_png_rgba(bytes);
    }
    if bytes.len() >= 54 && &bytes[0..2] == b"BM" {
        return decode_bmp_rgba(bytes);
    }
    None
}

fn decode_bmp_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let h_signed = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
    let comp = u32::from_le_bytes([bytes[30], bytes[31], bytes[32], bytes[33]]);
    if w <= 0 || h_signed == 0 || comp != 0 || (bpp != 24 && bpp != 32) {
        return None;
    }
    let width = w as usize;
    let height = h_signed.unsigned_abs() as usize;
    if width > 4096 || height > 4096 {
        return None;
    }
    let bytes_per_px = (bpp / 8) as usize;
    let row_stride = (width * bytes_per_px).div_ceil(4) * 4;
    if offset + row_stride * height > bytes.len() {
        return None;
    }
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        let src_y = if h_signed > 0 { height - 1 - y } else { y };
        let row = &bytes[offset + src_y * row_stride..];
        for x in 0..width {
            let s = x * bytes_per_px;
            let d = (y * width + x) * 4;
            rgba[d] = row[s + 2];
            rgba[d + 1] = row[s + 1];
            rgba[d + 2] = row[s];
            rgba[d + 3] = if bytes_per_px == 4 { row[s + 3] } else { 255 };
        }
    }
    Some((width, height, rgba))
}

fn decode_png_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    use std::io::Read;
    let mut pos = 8usize;
    let mut width = 0usize;
    let mut height = 0usize;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut idat = Vec::new();
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start.checked_add(len)?;
        if data_end + 4 > bytes.len() {
            break;
        }
        let chunk = &bytes[data_start..data_end];
        match kind {
            b"IHDR" if chunk.len() >= 13 => {
                width = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
                height = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize;
                bit_depth = chunk[8];
                color_type = chunk[9];
                interlace = chunk[12];
            }
            b"IDAT" => idat.extend_from_slice(chunk),
            b"IEND" => break,
            _ => {}
        }
        pos = data_end + 4;
    }
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || bit_depth != 8
        || interlace != 0
    {
        return None;
    }
    let bpp: usize = match color_type {
        0 => 1, // Grayscale
        2 => 3, // RGB
        4 => 2, // Grayscale + Alpha
        6 => 4, // RGBA
        _ => return None,
    };
    let stride = width * bpp;
    let expected = (stride + 1) * height;
    let mut raw = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .take((expected + 1024) as u64)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() < expected {
        return None;
    }
    let paeth = |a: u8, b: u8, c: u8| -> u8 {
        let (a, b, c) = (i32::from(a), i32::from(b), i32::from(c));
        let p = a + b - c;
        let pa = (p - a).abs();
        let pb = (p - b).abs();
        let pc = (p - c).abs();
        if pa <= pb && pa <= pc {
            a as u8
        } else if pb <= pc {
            b as u8
        } else {
            c as u8
        }
    };
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row_in = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = row_in[0];
        for i in 0..stride {
            let x = row_in[i + 1];
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            cur[i] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                _ => return None,
            };
        }
        for x in 0..width {
            let p = x * bpp;
            match color_type {
                0 => rgba.extend_from_slice(&[cur[p], cur[p], cur[p], 255]),
                2 => rgba.extend_from_slice(&[cur[p], cur[p + 1], cur[p + 2], 255]),
                4 => rgba.extend_from_slice(&[cur[p], cur[p], cur[p], cur[p + 1]]),
                6 => rgba.extend_from_slice(&[cur[p], cur[p + 1], cur[p + 2], cur[p + 3]]),
                _ => unreachable!(),
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Some((width, height, rgba))
}

fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
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
    ui.interact(rect, egui::Id::new(id), Sense::click_and_drag())
        .on_hover_text(text)
}

impl eframe::App for Workbench {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        tool_ribbon::SHELL_BG.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tool_search.begin_frame(ctx);
        if let Some(receiver) = &self.pdf_export {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Не удалось завершить экспорт PDF".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.pdf_export = None;
                match result {
                    Ok(path) => self.status = format!("PDF сохранён · {}", file_name(&path)),
                    Err(error) => {
                        self.status = "Экспорт PDF не завершён".into();
                        self.error = Some(error);
                    }
                }
            }
        }
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
        let no_modal = self.pending.is_none()
            && self.editor.is_none()
            && self.table_editor.is_none()
            && self.page_dialog.is_none()
            && self.area_dialog.is_none()
            && self.load.is_none();
        if no_modal {
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::K)) {
                self.show_palette = true;
                self.tool_search.request_focus(ctx);
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::O)) {
                self.request_open(Open::Picker, ctx);
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::S)) {
                self.save_copy();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::N)) {
                self.request_open(Open::Blank, ctx);
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::R)) {
                self.reload_from_disk(ctx);
            }
        }
        self.check_external_file_change(ctx);
        self.menu_bar(ctx);
        self.toolbar(ctx);
        self.format_bar(ctx);
        self.tool_ribbon_panel(ctx);
        if no_modal {
            self.route_sheet_typing(ctx);
            if self.insertion.is_none()
                && !ctx.wants_keyboard_input()
                && !self.tool_search.is_blocking_sheet(ctx)
            {
                if ctx.input(|i| i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(Key::Z)) {
                    self.undo();
                } else if ctx.input(|i| {
                    i.modifiers.ctrl
                        && (i.key_pressed(Key::Y) || (i.modifiers.shift && i.key_pressed(Key::Z)))
                }) {
                    self.redo();
                } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::X)) {
                    self.remove_selected(true);
                } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::C)) {
                    self.copy_selected();
                } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::V)) {
                    self.paste_clipboard();
                } else if ctx.input(|i| i.key_pressed(Key::Delete)) {
                    self.remove_selected(false);
                }
            }
            if !self.tool_search.is_blocking_sheet(ctx) && ctx.input(|i| i.key_pressed(Key::F2)) {
                if let Some(index) = self.selected {
                    self.start_formula_inline(index);
                }
            }
            if ctx.input(|i| i.key_pressed(Key::F5)) {
                self.reload_references();
            }
            if let Some(path) =
                ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()))
            {
                self.request_open(Open::File(path), ctx);
            }
        }
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
            self.table_dialog(ctx);
            self.page_setup_dialog(ctx);
            self.insert_dialog(ctx);
            self.area_setup_dialog(ctx);
            self.animation_window(ctx);
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
    fn sheet_fills_the_viewport_and_grows_for_a_large_draft() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let viewport = vec2(1200., 920.);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert_eq!(app.paper_size(ui, NORMAL_ZOOM, viewport), viewport);
                app.caret = Some(Pos2::new(970., 730.));
                app.start_insert(formula_draft(String::new()));
                let draft = &mut app.insertion.as_mut().unwrap().draft;
                let DraftRegion::Formula { math, .. } = draft else {
                    unreachable!()
                };
                math.insert_program(2);
                for _ in 0..35 {
                    math.add_program_line();
                }
                let size = Workbench::inline_formula_size(ui, draft, NORMAL_ZOOM).unwrap();
                let paper = app.paper_size(ui, NORMAL_ZOOM, viewport);
                assert!(paper.x > viewport.x);
                assert!(paper.y >= (730. + 26.) * NORMAL_ZOOM + size.y);
            });
        });
        let mut insert = app.insertion.take().unwrap();
        insert.draft = formula_draft("2 + 3 =".into());
        app.commit_insert(&insert, Some(vec2(512., 480.))).unwrap();
        let copy = app.document.export().unwrap();
        let reopened = Worksheet::load(&copy, "Копия".into(), None).unwrap();
        assert_eq!(reopened.sheet.regions[0].visual.height, 480.);
        assert_eq!(reopened.sheet.regions[0].visual.width, 512.);
    }

    #[test]
    fn formats_only_the_selected_unicode_text_and_preserves_runs_on_reopen() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        app.start_insert(new_text_draft());
        let mut insert = app.insertion.take().unwrap();
        let DraftRegion::Text {
            body,
            rich,
            rich_active,
            ..
        } = &mut insert.draft
        else {
            unreachable!();
        };
        *body = "До & после\nВторая строка".into();
        let rich = rich.as_mut().unwrap();
        rich.sync(body);
        rich.apply(5..10, |format| {
            format.font = TextFont::Times;
            format.size = 24;
            format.bold = true;
            format.italic = true;
            format.underline = true;
        });
        *rich_active = true;
        app.commit_insert(&insert, None).unwrap();
        let copy = app.document.export().unwrap();
        assert!(copy.contains("<b><i><u>после</u></i></b>"));
        let reopened = Worksheet::load(&copy, "Копия".into(), None).unwrap();
        let rich = reopened.sheet.regions[0].visual.rich_text.as_ref().unwrap();
        assert_eq!(rich.body, "До & после\nВторая строка");
        assert_eq!(rich.format_at(4).size, 10);
        assert_eq!(rich.format_at(5).size, 24);
        assert_eq!(rich.format_at(10).size, 10);
        let html = reopened.build_html_report();
        assert!(html.contains("text-decoration:underline\">после</span>"));
        app.undo();
        assert!(app.document.sheet.regions.is_empty());
    }

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
    fn mathcad_calculus_recalculates_and_survives_saving_a_copy() {
        let original = include_str!("../fixtures/mathcad15-calculus.xmcd");
        let mut doc = Worksheet::load(original, "Mathcad 15".into(), None).unwrap();
        let report = doc.sheet.verify_snapshots_report();
        assert!(report.ok, "{report:?}");
        assert_eq!(report.matched, 4);

        doc.update_input(doc.by_id("1").unwrap(), Some(4.)).unwrap();
        let copy = doc.export().unwrap();
        for operator in ["derivative", "integral", "summation", "product"] {
            assert!(copy.contains(&format!("<ml:{operator}")));
        }
        let xml = roxmltree::Document::parse(&copy).unwrap();
        assert!(!xml
            .descendants()
            .any(|node| node.has_tag_name(("http://schemas.mathsoft.com/math30", "result"))));
        let reopened = Worksheet::load(&copy, "Копия".into(), None).unwrap();
        for (id, result) in [
            ("7", "8 m"),
            ("8", "21.333333 m^3"),
            ("9", "30"),
            ("10", "120"),
        ] {
            assert_eq!(
                reopened
                    .sheet
                    .display_with_precision(reopened.by_id(id).unwrap(), Some(8))
                    .unwrap(),
                result
            );
        }
    }

    #[test]
    fn mathcad_symbolic_commands_recalculate_and_survive_saving_a_copy() {
        let original = include_str!("../fixtures/mathcad15-symbolic.xmcd");
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        app.replace(Worksheet::load(original, "Mathcad 15".into(), None).unwrap());
        let substitute = app.document.by_id("4").unwrap();
        let initial = app.document.sheet.display(substitute).unwrap();
        assert!(initial.contains("y") && initial.contains("9"), "{initial}");
        assert!(app.document.sheet.regions[substitute]
            .visual
            .formula
            .contains("substitute"));
        app.start_formula_inline(substitute);
        assert!(app.insertion.is_none());
        assert!(app.document.apply_formula(substitute, "x ->").is_err());

        app.document
            .update_input(app.document.by_id("1").unwrap(), Some(4.))
            .unwrap();
        let copy = app.document.export().unwrap();
        let original_xml = roxmltree::Document::parse(original).unwrap();
        let copy_xml = roxmltree::Document::parse(&copy).unwrap();
        let command_count = |xml: &roxmltree::Document<'_>| {
            xml.descendants()
                .filter(|node| node.has_tag_name(("http://schemas.mathsoft.com/math30", "command")))
                .count()
        };
        assert_eq!(command_count(&original_xml), 7);
        assert_eq!(command_count(&copy_xml), 7);
        assert!(!copy_xml
            .descendants()
            .any(|node| node.has_tag_name(("http://schemas.mathsoft.com/math30", "symResult"))));

        let reopened = Worksheet::load(&copy, "Копия".into(), None).unwrap();
        assert_eq!(
            reopened
                .sheet
                .display(reopened.by_id("2").unwrap())
                .unwrap(),
            "4"
        );
        let updated = reopened
            .sheet
            .display(reopened.by_id("4").unwrap())
            .unwrap();
        assert!(updated.contains("y") && updated.contains("16"), "{updated}");
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

    #[test]
    fn inline_2d_editor_builds_fraction_power_and_subscripts_and_evaluates() {
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
        let key = |k: Key| egui::Event::Key {
            key: k,
            physical_key: Some(k),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
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
        // Define x.1 := 3
        tick(vec![egui::Event::Text("x.1".into())]);
        tick(vec![key(Key::ArrowRight)]);
        tick(vec![egui::Event::Text(" := 3".into())]);
        tick(vec![key(Key::Enter)]);
        // Define (x.1^2 + 1) / (1 + 1/2) =
        tick(vec![egui::Event::Text("(x.1".into())]);
        tick(vec![key(Key::ArrowRight)]);
        tick(vec![egui::Event::Text("^2".into())]);
        tick(vec![key(Key::ArrowRight)]);
        tick(vec![egui::Event::Text(" + 1)/1 + 1/2".into())]);
        tick(vec![key(Key::ArrowRight), key(Key::ArrowRight)]);
        tick(vec![egui::Event::Text(" =".into())]);
        tick(vec![key(Key::Enter)]);
        let result: f64 = app.document.sheet.display(1).unwrap().parse().unwrap();
        assert!((result - (10.0 / 1.5)).abs() < 1e-9);
    }

    #[test]
    fn moving_definition_past_dependent_recalculates_and_undo_restores_original() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        // Create: a := 2 (top=20), b := a + 5 = (top=80), note (left=240, top=40)
        app.document
            .insert(Pos2::new(12., 20.), editing::NewRegion::Formula("a := 2"))
            .unwrap();
        app.document
            .insert(
                Pos2::new(12., 80.),
                editing::NewRegion::Formula("b := a + 5 ="),
            )
            .unwrap();
        app.document
            .insert(
                Pos2::new(240., 40.),
                editing::NewRegion::Text("Постоянная заметка"),
            )
            .unwrap();
        let id_a = app.document.sheet.regions[0].source_id.clone();
        let id_note = app.document.sheet.regions[1].source_id.clone();
        let id_b = app.document.sheet.regions[2].source_id.clone();
        assert_eq!(app.document.sheet.display(2).unwrap(), "7");

        // Move `a := 2` below `b := a + 5 =` (top = 150)
        app.move_selected_to(&[(id_a.clone(), 12., 150.)]);
        let idx_b = app.document.by_id(&id_b).unwrap();
        let idx_note = app.document.by_id(&id_note).unwrap();
        assert!(app.document.sheet.display(idx_b).is_err());
        // Other regions' coordinates stay intact
        assert_eq!(app.document.sheet.regions[idx_note].visual.left, 240.);
        assert_eq!(app.document.sheet.regions[idx_note].visual.top, 40.);
        assert_eq!(app.document.sheet.regions[idx_b].visual.top, 80.);

        // Undo restores original order and result `7`
        app.undo();
        let idx_b = app.document.by_id(&id_b).unwrap();
        let idx_a = app.document.by_id(&id_a).unwrap();
        assert_eq!(app.document.sheet.regions[idx_a].visual.top, 20.);
        assert_eq!(app.document.sheet.display(idx_b).unwrap(), "7");

        // Redo re-applies the move
        app.redo();
        let idx_b = app.document.by_id(&id_b).unwrap();
        assert!(app.document.sheet.display(idx_b).is_err());
    }

    #[test]
    fn copy_paste_and_delete_regions_support_undo_redo() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let idx = app
            .document
            .insert(Pos2::new(12., 20.), editing::NewRegion::Formula("x := 10"))
            .unwrap();
        app.select(idx);
        app.copy_selected();
        app.paste_clipboard();
        assert_eq!(app.document.sheet.regions.len(), 2);

        app.remove_selected(false);
        assert_eq!(app.document.sheet.regions.len(), 1);

        app.undo();
        assert_eq!(app.document.sheet.regions.len(), 2);
    }

    #[test]
    fn palette_inserts_sqrt_and_greek_and_undo_works_without_overflow() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        app.apply_palette_action(math_view::PaletteAction::Raw("α := "));
        app.apply_palette_action(math_view::PaletteAction::Sqrt);
        app.apply_palette_action(math_view::PaletteAction::Raw("16"));
        let insert = app.insertion.take().unwrap();
        app.commit_insert(&insert, None).unwrap();
        assert_eq!(app.document.sheet.display(0).unwrap(), "4");
        assert_eq!(app.undo_stack.len(), 1);
        app.undo();
        assert!(app.document.sheet.regions.is_empty());
    }

    #[test]
    fn imports_mathcad_xy_plots_and_preserves_binary_content_on_export() {
        let xml = include_str!("../fixtures/Complex_numbers_variant_24_calculations.xmcd");
        let doc = Worksheet::load(xml, "Вариант 24".into(), None).unwrap();
        assert_eq!(doc.errors, 0);
        assert_eq!(doc.plots.len(), 3);
        assert_eq!(doc.skipped_visual, 0);
        for plot in doc.plots.values() {
            assert!(plot.curves.is_ok());
        }
        let exported = doc.export().unwrap();
        assert!(exported.contains("<binaryContent>"));
        let reopened = Worksheet::load(&exported, "Копия".into(), None).unwrap();
        assert_eq!(reopened.plots.len(), 3);
        assert_eq!(reopened.skipped_visual, 0);
    }

    #[test]
    fn parametric_circle_updates_on_parameter_change_and_survives_reopen() {
        let mut doc = Worksheet::blank().unwrap();
        let r_idx = doc
            .insert(Pos2::new(12., 20.), editing::NewRegion::Formula("R := 2"))
            .unwrap();
        let spec = editing::ChartSpec::Parametric {
            x_expr: "R * cos(t)".into(),
            y_expr: "R * sin(t)".into(),
            from: 0.,
            to: std::f64::consts::TAU,
        };
        let options = editing::ChartOptions {
            equal_scale: true,
            markers: true,
            show_legend: true,
            x_label: "X".into(),
            y_label: "Y".into(),
            ..editing::ChartOptions::default()
        };
        let plot_idx = doc
            .insert(
                Pos2::new(12., 70.),
                editing::NewRegion::ChartConfigured(&spec, &options),
            )
            .unwrap();
        let first_pt = doc.plots[&plot_idx].curves.as_ref().unwrap()[0].1[0].unwrap();
        assert!((first_pt[0] - 2.0).abs() < 1e-9);
        assert!(first_pt[1].abs() < 1e-9);

        // Change R from 2 to 5 and check parametric circle radius updates
        doc.update_input(r_idx, Some(5.)).unwrap();
        let updated_pt = doc.plots[&plot_idx].curves.as_ref().unwrap()[0].1[0].unwrap();
        assert!((updated_pt[0] - 5.0).abs() < 1e-9);

        // Export and reopen: radius and all chart options survive round-trip
        let exported = doc.export().unwrap();
        let reopened = Worksheet::load(&exported, "Копия".into(), None).unwrap();
        let reopened_plot = &reopened.plots[&plot_idx];
        let reopened_pt = reopened_plot.curves.as_ref().unwrap()[0].1[0].unwrap();
        assert!((reopened_pt[0] - 5.0).abs() < 1e-9);
        assert_eq!(reopened_plot.options, options);
    }

    #[test]
    fn polar_chart_evaluates_and_survives_roundtrip() {
        let mut doc = Worksheet::blank().unwrap();
        let spec = editing::ChartSpec::Polar {
            expression: "2".into(),
            from: 0.,
            to: std::f64::consts::TAU,
        };
        let plot_idx = doc
            .insert(Pos2::new(12., 20.), editing::NewRegion::Chart(&spec))
            .unwrap();
        let pts = &doc.plots[&plot_idx].curves.as_ref().unwrap()[0].1;
        let p0 = pts[0].unwrap();
        assert!((p0[0] - 2.0).abs() < 1e-9);
        assert!(p0[1].abs() < 1e-9);
        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert!(reopened.plots[&plot_idx].spec.is_polar());
    }

    #[test]
    fn display_unit_override_and_precision_survive_xmcd_roundtrip() {
        let mut doc = Worksheet::demo().unwrap();
        // Region 5 is `p =` -> initially `1 MPa`
        assert_eq!(
            doc.sheet.display_with_precision(5, doc.precision).unwrap(),
            "1 MPa"
        );
        // Switch display unit to kPa -> `1000 kPa`
        doc.set_display_unit(5, Some("kPa")).unwrap();
        assert_eq!(
            doc.sheet.display_with_precision(5, doc.precision).unwrap(),
            "1000 kPa"
        );
        // Incompatible unit fails and preserves `1000 kPa`
        assert!(doc.set_display_unit(5, Some("mm")).is_err());
        assert_eq!(
            doc.sheet.display_with_precision(5, doc.precision).unwrap(),
            "1000 kPa"
        );
        // Set precision to 3 significant digits
        doc.set_precision(Some(3)).unwrap();
        assert_eq!(doc.precision, Some(3));
        // Export and reopen preserves both display unit and precision
        let xml = doc.export().unwrap();
        let reopened = Worksheet::load(&xml, "Копия".into(), None).unwrap();
        assert_eq!(reopened.precision, Some(3));
        assert_eq!(
            reopened
                .sheet
                .display_with_precision(5, reopened.precision)
                .unwrap(),
            "1000 kPa"
        );
    }

    #[test]
    fn linterp_and_statistics_evaluate_on_worksheet() {
        let mut doc = Worksheet::blank().unwrap();
        doc.insert(
            Pos2::new(12., 10.),
            editing::NewRegion::Formula("vx := [0; 2; 4]"),
        )
        .unwrap();
        doc.insert(
            Pos2::new(12., 45.),
            editing::NewRegion::Formula("vy := [0; 10; 30]"),
        )
        .unwrap();
        let interp = doc
            .insert(
                Pos2::new(12., 80.),
                editing::NewRegion::Formula("linterp(vx, vy, 3) ="),
            )
            .unwrap();
        assert_eq!(doc.sheet.display(interp).unwrap(), "20");
        let med = doc
            .insert(
                Pos2::new(12., 115.),
                editing::NewRegion::Formula("median(vy) ="),
            )
            .unwrap();
        assert_eq!(doc.sheet.display(med).unwrap(), "10");
    }

    #[test]
    fn csv_table_import_column_formula_chart_and_csv_export_pipeline() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let table = data_io::parse_csv("t;y\n0;0\n1;2\n2;8\n").unwrap();
        let editor = TableEditor::from_csv("Data".into(), table, None);
        app.commit_table(&editor).unwrap();

        // Use column 0 and column 1 in formulas and in a chart
        app.document
            .insert(
                Pos2::new(12., 80.),
                editing::NewRegion::Formula("X := col(Data, 0)"),
            )
            .unwrap();
        let y_idx = app
            .document
            .insert(
                Pos2::new(12., 120.),
                editing::NewRegion::Formula("Y := col(Data, 1) * 2"),
            )
            .unwrap();
        let spec = editing::ChartSpec::Vectors {
            x: "X".into(),
            y: "Y".into(),
        };
        let plot_idx = app
            .document
            .insert(Pos2::new(12., 160.), editing::NewRegion::Chart(&spec))
            .unwrap();
        assert_eq!(
            app.document.plots[&plot_idx].curves.as_ref().unwrap()[0].1,
            vec![Some([0., 0.]), Some([1., 4.]), Some([2., 16.])]
        );

        // Export computed column Y to CSV with chosen precision
        let csv = app.document.sheet.export_csv(y_idx, ';', Some(4)).unwrap();
        assert_eq!(csv, "0\n4\n16\n");
    }

    #[test]
    fn global_definition_syntax_evaluates_above_and_survives_export() {
        let mut doc = Worksheet::blank().unwrap();
        let eval_idx = doc
            .insert(
                Pos2::new(12., 20.),
                editing::NewRegion::Formula("g === 9.81"),
            )
            .unwrap();
        assert_eq!(eval_idx, 0);
        // Insert a formula ABOVE g === 9.81 (at top=5) that uses g
        let above = doc
            .insert(
                Pos2::new(12., 5.),
                editing::NewRegion::Formula("w := 2 * g ="),
            )
            .unwrap();
        assert_eq!(doc.sheet.display(above).unwrap(), "19.62");

        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(reopened.sheet.display(0).unwrap(), "19.62");
    }

    #[test]
    fn text_toolbar_format_survives_editing_export_and_undo() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let text = app
            .document
            .insert(
                Pos2::new(18., 24.),
                editing::NewRegion::StyledText(
                    "Расчёт & проверка\nИтог",
                    editing::TextStyle::Heading1,
                ),
            )
            .unwrap();
        let input = app
            .document
            .insert(Pos2::new(18., 200.), editing::NewRegion::Formula("x := 2"))
            .unwrap();
        app.document.update_input(input, Some(4.)).unwrap();
        app.select(text);
        let initial = app.document.sheet.regions[text].visual.text_format.unwrap();
        let format = TextFormat {
            font: TextFont::Times,
            size: 24,
            bold: false,
            italic: true,
            underline: true,
            alignment: TextAlignment::Center,
        };
        app.apply_text_format(text, format).unwrap();
        assert_eq!(
            app.document.sheet.regions[text].visual.text_format,
            Some(format)
        );
        assert_eq!(app.document.sheet.input_value(input), Some(4.));
        app.undo();
        assert_eq!(
            app.document.sheet.regions[text].visual.text_format,
            Some(initial)
        );
        app.redo();
        assert_eq!(
            app.document.sheet.regions[text].visual.text_format,
            Some(format)
        );
        app.document
            .replace_text(
                text,
                "Новый заголовок\nПараметры",
                editing::TextStyle::Heading1,
            )
            .unwrap();
        let copy = app.document.export().unwrap();
        let reopened = Worksheet::load(&copy, "Копия".into(), None).unwrap();
        assert_eq!(
            reopened.sheet.regions[text].visual.text_format,
            Some(format)
        );
        assert_eq!(reopened.sheet.regions[text].visual.heading, 1);
        assert_eq!(
            reopened.sheet.regions[text].text.as_deref(),
            Some("Новый заголовок\nПараметры")
        );
        assert_eq!(reopened.sheet.input_value(input), Some(4.));
        let html = reopened.build_html_report();
        assert!(html.contains("font-family:'Times New Roman';font-size:24pt;font-weight:normal;font-style:italic;text-decoration:underline;text-align:center"));
        let id = reopened.sheet.regions[text].source_id.clone();
        let copied = editing::copy_regions(&copy, &[id]).unwrap();
        let (pasted, _) = editing::paste_regions(list_rs::BLANK, &copied, 18., 24.).unwrap();
        let pasted = Worksheet::load(&pasted, "Вставка".into(), None).unwrap();
        assert_eq!(pasted.sheet.regions[0].visual.text_format, Some(format));
    }

    #[test]
    fn styled_text_page_break_settings_and_multipage_report_roundtrip() {
        let mut doc = Worksheet::blank().unwrap();
        let t_idx = doc
            .insert(
                Pos2::new(12., 10.),
                editing::NewRegion::StyledText("Введение", editing::TextStyle::Normal),
            )
            .unwrap();
        doc.replace_text(t_idx, "Отчёт по механике", editing::TextStyle::Heading1)
            .unwrap();
        doc.insert(
            Pos2::new(12., 60.),
            editing::NewRegion::Formula("m := 5 * kg"),
        )
        .unwrap();
        doc.insert(Pos2::new(12., 110.), editing::NewRegion::PageBreak)
            .unwrap();
        let spec = editing::ChartSpec::Function {
            expression: "x^2".into(),
            from: -2.,
            to: 2.,
        };
        doc.insert(Pos2::new(12., 150.), editing::NewRegion::Chart(&spec))
            .unwrap();
        let settings = editing::PageSettings {
            header: "Кафедра механики".into(),
            footer: "Лист · отчёт".into(),
            show_page_numbers: true,
            page_height: 800.0,
        };
        doc.set_page_settings(&settings).unwrap();

        let reopened = Worksheet::load(&doc.export().unwrap(), "Отчёт".into(), None).unwrap();
        assert_eq!(reopened.page_settings, settings);
        assert_eq!(reopened.sheet.regions[0].visual.heading, 1);
        let html = reopened.build_html_report();
        assert!(html.contains("Отчёт по механике"));
        assert!(html.contains("Кафедра механики"));
        assert!(html.contains("Стр. 1 из 2"));
        assert!(html.contains("Стр. 2 из 2"));
        assert!(html.contains("<svg class=\"chart-svg\""));
    }

    #[test]
    fn program_piecewise_accumulation_and_ode_cauchy_survive_xmcd_roundtrip() {
        let mut doc = Worksheet::blank().unwrap();
        // Piecewise function with accumulation in a program block:
        // f(n) := prog(s <- 0; for(k, 1..n, if(k > 2, s <- s + k, s <- s + 1)); return(s))
        doc.insert(
            Pos2::new(12., 10.),
            editing::NewRegion::Formula(
                "f(n) := prog(s <- 0; for(k, 1..n, if(k > 2, s <- s + k, s <- s + 1)); return(s))",
            ),
        )
        .unwrap();
        let n_idx = doc
            .insert(Pos2::new(12., 50.), editing::NewRegion::Formula("N := 5"))
            .unwrap();
        let res_idx = doc
            .insert(Pos2::new(12., 90.), editing::NewRegion::Formula("f(N) ="))
            .unwrap();
        // 1 + 1 + 3 + 4 + 5 = 14
        assert_eq!(doc.sheet.display(res_idx).unwrap(), "14");

        // Update N -> 6: 14 + 6 = 20
        doc.update_input(n_idx, Some(6.)).unwrap();
        assert_eq!(doc.sheet.display(res_idx).unwrap(), "20");

        // Cauchy ODE: y'(x) = -2*x*y, y(0) = 1 -> analytical y(1) = exp(-1)
        doc.insert(
            Pos2::new(12., 140.),
            editing::NewRegion::Formula("g(x, y) := -2 * x * y"),
        )
        .unwrap();
        let ode_idx = doc
            .insert(
                Pos2::new(12., 180.),
                editing::NewRegion::Formula("odesolve(g, 0, 1, 1) ="),
            )
            .unwrap();
        let val: f64 = doc.sheet.display(ode_idx).unwrap().parse().unwrap();
        assert!((val - (-1.0_f64).exp()).abs() < 1e-7);

        // Export and reopen: both program and ODE survive XMCD round-trip
        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(reopened.sheet.display(res_idx).unwrap(), "20");
        let val2: f64 = reopened.sheet.display(ode_idx).unwrap().parse().unwrap();
        assert!((val2 - (-1.0_f64).exp()).abs() < 1e-7);
    }

    #[test]
    fn symbolic_eval_updates_on_input_change_preserves_domain_and_roundtrips() {
        let mut doc = Worksheet::blank().unwrap();
        let a_idx = doc
            .insert(Pos2::new(12., 10.), editing::NewRegion::Formula("a := 2"))
            .unwrap();
        let sym_idx = doc
            .insert(
                Pos2::new(12., 50.),
                editing::NewRegion::Formula("a * x + 3 * x ->"),
            )
            .unwrap();
        assert_eq!(doc.sheet.display(sym_idx).unwrap(), "5·x");

        // Update upstream input a := 5 -> symbolic result updates to 8·x
        doc.update_input(a_idx, Some(5.)).unwrap();
        assert_eq!(doc.sheet.display(sym_idx).unwrap(), "8·x");

        // Domain preservation: (x / x) -> 1 [x ≠ 0]
        let dom_idx = doc
            .insert(Pos2::new(12., 90.), editing::NewRegion::Formula("x / x ->"))
            .unwrap();
        let dom_res = doc.sheet.display(dom_idx).unwrap();
        assert!(dom_res.starts_with("1"));
        assert!(dom_res.contains("x ≠ 0"));

        // Symbolic derivative: sdiff(x^3 + a*x, x) -> 3·x^2 + 5
        let diff_idx = doc
            .insert(
                Pos2::new(12., 130.),
                editing::NewRegion::Formula("sdiff(x^3 + a*x, x) ->"),
            )
            .unwrap();
        assert_eq!(doc.sheet.display(diff_idx).unwrap(), "3·x^2 + 5");

        // Export and reopen preserves all symbolic results
        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(reopened.sheet.display(sym_idx).unwrap(), "8·x");
        assert_eq!(reopened.sheet.display(diff_idx).unwrap(), "3·x^2 + 5");
    }

    #[test]
    fn given_find_block_and_optimization_survive_xmcd_roundtrip() {
        let mut doc = Worksheet::blank().unwrap();
        doc.insert(Pos2::new(12., 10.), editing::NewRegion::Formula("x := 3"))
            .unwrap();
        doc.insert(Pos2::new(12., 40.), editing::NewRegion::Formula("y := 2"))
            .unwrap();
        doc.insert(Pos2::new(12., 70.), editing::NewRegion::Formula("Given"))
            .unwrap();
        doc.insert(
            Pos2::new(12., 100.),
            editing::NewRegion::Formula("x^2 + y^2 == 25"),
        )
        .unwrap();
        doc.insert(
            Pos2::new(12., 130.),
            editing::NewRegion::Formula("x - y == 1"),
        )
        .unwrap();
        doc.insert(
            Pos2::new(12., 160.),
            editing::NewRegion::Formula("sol := Find(x, y)"),
        )
        .unwrap();
        let sx = doc
            .insert(
                Pos2::new(12., 195.),
                editing::NewRegion::Formula("sol[0] ="),
            )
            .unwrap();
        let sy = doc
            .insert(
                Pos2::new(12., 225.),
                editing::NewRegion::Formula("sol[1] ="),
            )
            .unwrap();
        assert_eq!(doc.errors, 0);
        assert!((doc.sheet.display(sx).unwrap().parse::<f64>().unwrap() - 4.0).abs() < 1e-6);
        assert!((doc.sheet.display(sy).unwrap().parse::<f64>().unwrap() - 3.0).abs() < 1e-6);

        // Box-constrained maximize: -(x - 5)^2 on [0, 3] -> x = 3
        doc.insert(
            Pos2::new(12., 260.),
            editing::NewRegion::Formula("h(t) := -(t - 5)^2"),
        )
        .unwrap();
        let opt = doc
            .insert(
                Pos2::new(12., 295.),
                editing::NewRegion::Formula("maximize(h, 1, 0, 3) ="),
            )
            .unwrap();
        assert!((doc.sheet.display(opt).unwrap().parse::<f64>().unwrap() - 3.0).abs() < 1e-5);

        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(reopened.errors, 0);
        assert!((reopened.sheet.display(sx).unwrap().parse::<f64>().unwrap() - 4.0).abs() < 1e-6);
        assert!((reopened.sheet.display(opt).unwrap().parse::<f64>().unwrap() - 3.0).abs() < 1e-5);
    }

    #[test]
    fn surface_3d_rebuilds_from_input_and_preserves_view_angle_on_reopen() {
        let mut doc = Worksheet::blank().unwrap();
        let a_idx = doc
            .insert(Pos2::new(12., 10.), editing::NewRegion::Formula("A := 2"))
            .unwrap();
        let mut spec = editing::Plot3dSpec::default_surface();
        spec.source = editing::Plot3dSource::Function {
            expression: "A * (x^2 + y^2)".into(),
            x_from: -1.0,
            x_to: 1.0,
            y_from: -1.0,
            y_to: 1.0,
        };
        spec.grid = 7;
        let plot_idx = doc
            .insert(Pos2::new(12., 50.), editing::NewRegion::Plot3d(&spec))
            .unwrap();
        let zmax0 = doc.plots3d[&plot_idx].grid.as_ref().unwrap().z_max;
        assert!((zmax0 - 4.0).abs() < 1e-9);

        // Update input A -> 5 and check surface rebuilds (z_max = 5 * (1 + 1) = 10)
        doc.update_input(a_idx, Some(5.)).unwrap();
        let zmax1 = doc.plots3d[&plot_idx].grid.as_ref().unwrap().z_max;
        assert!((zmax1 - 10.0).abs() < 1e-9);

        // Rotate surface and save angle into XMCD
        doc.save_plot3d_angle(plot_idx, 64.0, 22.0).unwrap();

        // Also add contour plot from a 2x2 matrix
        doc.insert(
            Pos2::new(12., 420.),
            editing::NewRegion::Formula("M := [1, 2; 3, 4]"),
        )
        .unwrap();
        let contour_spec = editing::Plot3dSpec {
            mode: editing::Plot3dMode::Contour,
            source: editing::Plot3dSource::Matrix { name: "M".into() },
            yaw_deg: 38.0,
            pitch_deg: 30.0,
            grid: 10,
        };
        let c_idx = doc
            .insert(
                Pos2::new(12., 460.),
                editing::NewRegion::Plot3d(&contour_spec),
            )
            .unwrap();
        assert_eq!(doc.plots3d[&c_idx].grid.as_ref().unwrap().z_max, 4.0);

        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(reopened.plots3d.len(), 2);
        assert_eq!(reopened.skipped_visual, 0);
        assert_eq!(reopened.plots3d[&plot_idx].spec.yaw_deg, 64.0);
        assert_eq!(reopened.plots3d[&plot_idx].spec.pitch_deg, 22.0);
        assert!((reopened.plots3d[&plot_idx].grid.as_ref().unwrap().z_max - 10.0).abs() < 1e-9);
    }

    #[test]
    fn inline_formula_edit_and_program_local_indexed_array_survive_roundtrip() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        // Program building a local vector via v[k] <- k^2
        app.document
            .insert(
                Pos2::new(12., 10.),
                editing::NewRegion::Formula(
                    "squares(n) := prog(for(k, 0..n, v[k] <- k^2); return(v))",
                ),
            )
            .unwrap();
        let call_idx = app
            .document
            .insert(
                Pos2::new(12., 60.),
                editing::NewRegion::Formula("squares(3) ="),
            )
            .unwrap();
        assert_eq!(
            app.document.sheet.display(call_idx).unwrap(),
            "4×1 [0; 1; 4; 9]"
        );

        // Edit existing formula in-place on the sheet (without modal dialog)
        app.start_formula_inline(call_idx);
        assert!(app.editor.is_none());
        let mut ins = app.insertion.take().unwrap();
        assert_eq!(ins.replace_formula, Some(call_idx));
        if let DraftRegion::Formula {
            math,
            text,
            text_mode,
        } = &mut ins.draft
        {
            *math = MathEditor::from_source("squares(4) =");
            *text = math.to_source();
            *text_mode = false;
        }
        app.commit_insert(&ins, None).unwrap();
        assert_eq!(
            app.document.sheet.display(call_idx).unwrap(),
            "5×1 [0; 1; 4; 9; 16]"
        );

        // Undo restores squares(3) =
        app.undo();
        assert_eq!(
            app.document.sheet.display(call_idx).unwrap(),
            "4×1 [0; 1; 4; 9]"
        );

        // Redo and verify XMCD roundtrip
        app.redo();
        let reopened =
            Worksheet::load(&app.document.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(
            reopened.sheet.display(call_idx).unwrap(),
            "5×1 [0; 1; 4; 9; 16]"
        );
    }

    #[test]
    fn engineering_library_collapsible_areas_and_images_survive_roundtrip() {
        let mut doc = Worksheet::blank().unwrap();
        // Engineering functions: eigenvals, fft/ifft, cspline/interp, regress, pnorm, temperature C/°C/°F
        doc.insert(
            Pos2::new(12., 10.),
            editing::NewRegion::Formula("A := [2, 1; 1, 2]"),
        )
        .unwrap();
        let eig = doc
            .insert(
                Pos2::new(12., 45.),
                editing::NewRegion::Formula("eigenvals(A) ="),
            )
            .unwrap();
        assert!(doc.sheet.display(eig).unwrap().contains("3"));

        doc.insert(
            Pos2::new(12., 80.),
            editing::NewRegion::Formula("T := C(100)"),
        )
        .unwrap();
        let tc = doc
            .insert(Pos2::new(12., 115.), editing::NewRegion::Formula("T ="))
            .unwrap();
        doc.set_display_unit(tc, Some("°C")).unwrap();
        assert_eq!(doc.sheet.display(tc).unwrap(), "100 °C");
        doc.set_display_unit(tc, Some("°F")).unwrap();
        assert_eq!(doc.sheet.display(tc).unwrap(), "212 °F");

        // Collapsible area covering top 0..95 (hides A and T visually when collapsed, keeps calculation live)
        let area_id = doc.insert_area("Исходные данные", 0., 95.).unwrap();
        doc.toggle_area(&area_id).unwrap();
        assert!(doc.areas[0].collapsed);
        assert_eq!(doc.sheet.display(tc).unwrap(), "212 °F");

        // Embedded image survives XMCD roundtrip & appears in HTML report
        let img = editing::ImageSpec {
            name: "diagram.png".into(),
            mime: "image/png".into(),
            width: 120,
            height: 80,
            data_base64: editing::encode_base64(&[137, 80, 78, 71, 13, 10, 26, 10]),
        };
        let img_idx = doc
            .insert(Pos2::new(12., 160.), editing::NewRegion::Image(&img))
            .unwrap();
        assert_eq!(doc.images.get(&img_idx), Some(&img));
        assert_eq!(doc.skipped_visual, 0);

        let reopened = Worksheet::load(&doc.export().unwrap(), "Копия".into(), None).unwrap();
        assert_eq!(reopened.areas.len(), 1);
        assert!(reopened.areas[0].collapsed);
        assert_eq!(reopened.images.len(), 1);
        assert_eq!(reopened.skipped_visual, 0);
        assert_eq!(reopened.sheet.display(tc).unwrap(), "212 °F");
        let html = reopened.build_html_report();
        assert!(html.contains("data:image/png;base64,"));
    }

    #[test]
    fn auto_reload_picks_up_external_xmcd_updates_and_respects_unsaved_edits() {
        let path = std::env::temp_dir().join(format!(
            "list-rs-autoreload-test-{}.xmcd",
            std::process::id()
        ));
        let blank = include_str!("../examples/blank.xmcd");
        let (xml1, _) =
            editing::insert_region(blank, 12., 10., editing::NewRegion::Formula("a := 2")).unwrap();
        let (xml1, _) =
            editing::insert_region(&xml1, 12., 45., editing::NewRegion::Formula("a^2 =")).unwrap();
        std::fs::write(&path, &xml1).unwrap();

        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, Some(path.clone())).unwrap();
        assert_eq!(app.document.sheet.display(1).unwrap(), "4");

        // Simulate external MCP edit: rewrite file with a := 5 (different length triggers stamp change)
        let (xml2, _) =
            editing::insert_region(blank, 12., 10., editing::NewRegion::Formula("a := 50"))
                .unwrap();
        let (xml2, _) =
            editing::insert_region(&xml2, 12., 45., editing::NewRegion::Formula("a^2 =")).unwrap();
        std::fs::write(&path, &xml2).unwrap();

        assert!(app.reload_from_disk_if_changed());
        assert_eq!(app.document.sheet.display(1).unwrap(), "2500");

        // Local unsaved edit must NOT be overwritten by auto-reload
        app.document.update_input(0, Some(3.)).unwrap();
        assert!(app.document.unsaved);
        let (xml3, _) =
            editing::insert_region(blank, 12., 10., editing::NewRegion::Formula("a := 999"))
                .unwrap();
        std::fs::write(&path, &xml3).unwrap();
        assert!(!app.reload_from_disk_if_changed());
        assert_eq!(app.document.sheet.display(1).unwrap(), "9");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tool_search_typing_and_enter_reveal_group_without_mutating_sheet() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        app.caret = Some(Pos2::new(20., 20.));
        app.tool_search.request_focus(&ctx);

        let tick = |app: &mut Workbench, events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1080., 740.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.tool_search.begin_frame(ctx);
                    app.tool_ribbon_panel(ctx);
                    app.route_sheet_typing(ctx);
                    app.workspace(ctx);
                },
            );
        };

        // Frame 1: focus lands in search input
        tick(&mut app, Vec::new());
        // Frame 2: type "  МАТР  " into search input -> must NOT create a formula on sheet
        tick(&mut app, vec![egui::Event::Text("  МАТР  ".into())]);
        assert!(app.insertion.is_none());
        assert!(app.document.sheet.regions.is_empty());
        assert!(app.tool_search.open);

        // Frame 3: press Enter -> opens "Вектор и матрица" and highlights it without inserting
        tick(
            &mut app,
            vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: Some(Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.palette_tab, math_view::PaletteTab::Matrix);
        assert!(app.ribbon_expanded);
        assert_eq!(
            app.tool_search.highlight,
            Some(tool_ribbon::SearchHighlight::Group(
                math_view::PaletteTab::Matrix
            ))
        );
        assert!(app.insertion.is_none());
        assert!(app.document.sheet.regions.is_empty());

        // Escape while search popup is open closes it without mutating the sheet
        app.tool_search.request_focus(&ctx);
        tick(&mut app, Vec::new());
        tick(
            &mut app,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.tool_search.open);
        assert!(app.document.sheet.regions.is_empty());
    }

    #[test]
    fn cut_regions_keeps_a_pasteable_copy_and_can_be_undone() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let index = app
            .document
            .insert(Pos2::new(12., 12.), editing::NewRegion::Formula("x := 2"))
            .unwrap();
        app.select(index);
        app.remove_selected(true);
        assert!(app.document.sheet.regions.is_empty());
        assert_eq!(app.clipboard.len(), 1);

        app.paste_clipboard();
        assert_eq!(app.document.sheet.input_value(0), Some(2.));
        app.undo();
        assert!(app.document.sheet.regions.is_empty());
        app.undo();
        assert_eq!(app.document.sheet.input_value(0), Some(2.));
    }

    #[test]
    fn tool_ribbon_renders_cleanly_at_narrow_width_and_inserts_via_actions() {
        let ctx = egui::Context::default();
        let mut app = Workbench::new(&ctx, None).unwrap();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(780., 560.))),
                ..Default::default()
            },
            |ctx| {
                app.tool_search.begin_frame(ctx);
                app.menu_bar(ctx);
                app.toolbar(ctx);
                app.format_bar(ctx);
                app.tool_ribbon_panel(ctx);
                app.footer(ctx);
                app.sidebar(ctx);
                app.workspace(ctx);
            },
        );
        app.apply_tool_action(tool_ribbon::ToolAction::CustomMatrix);
        assert!(app.insertion.is_some());
        app.insertion = None;
        app.apply_tool_action(tool_ribbon::ToolAction::ChartXY);
        assert!(matches!(
            app.insertion.as_ref().map(|i| &i.draft),
            Some(DraftRegion::Chart(_))
        ));
    }
}
