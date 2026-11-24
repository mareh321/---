use crate::data_io;
use crate::editing::{
    self, ChartOptions, ChartSpec, NewRegion, PageSettings, Plot3dMode, Plot3dSource, Plot3dSpec,
    TextStyle,
};
use crate::mcp_json::{parse_json, Json};
use crate::report::{self, ChartReport, Plot3dReport};
use crate::sheet::Sheet;
use crate::{read_xml, Calc, BLANK, DEMO};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SUPPORTED_PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const SERVER_INSTRUCTIONS: &str = "Сервер MCP для инженерной расчётной среды «Лист» (list-rs), совместимой с листами Mathcad 15 (XMCD/XMCDZ). Поддерживаются численные и символьные расчёты (->), единицы СИ и температуры (°C/°F), матрицы, программы prog(...), блоки Given…Find/Minimize, ОДУ odesolve, 2D/3D-графики, CSV и HTML/PDF-отчёты. Номера регионов (index) начинаются с 1; также можно указывать стабильный region_id из XMCD.";

struct ActiveSheet {
    sheet: Sheet,
    xml: String,
    name: String,
    path: Option<PathBuf>,
    disk_fingerprint: Option<u64>,
    precision: Option<usize>,
    page_settings: PageSettings,
    formula_changed: bool,
    unsaved: bool,
}

impl ActiveSheet {
    fn load(xml: &str, name: String, path: Option<PathBuf>) -> Calc<Self> {
        let mut sheet = Sheet::import_with_base(xml, path.as_deref().and_then(Path::parent))?;
        sheet.full();
        let precision = editing::read_sheet_precision(xml)?;
        let page_settings = editing::read_page_settings(xml)?;
        let disk_fingerprint = path.as_ref().map(|_| fingerprint(xml));
        Ok(Self {
            sheet,
            xml: xml.into(),
            name,
            path,
            disk_fingerprint,
            precision,
            page_settings,
            formula_changed: false,
            unsaved: false,
        })
    }

    fn blank(name: Option<&str>) -> Calc<Self> {
        Self::load(BLANK, name.unwrap_or("Новый лист").into(), None)
    }

    fn demo() -> Calc<Self> {
        Self::load(DEMO, "Давление под опорой".into(), None)
    }

    fn from_path(path: PathBuf) -> Calc<Self> {
        let xml = read_xml(&path)?;
        let name = path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        Self::load(&xml, name, Some(path))
    }

    fn by_id(&self, id: &str) -> Calc<usize> {
        self.sheet
            .regions
            .iter()
            .enumerate()
            .find(|(_, r)| !r.imported && r.source_id == id)
            .map(|(idx, _)| idx)
            .ok_or_else(|| format!("регион с region_id={id} не найден"))
    }

    fn resolve_region(&self, args: &Json) -> Calc<usize> {
        if let Some(id) = args.get("region_id").and_then(Json::as_str) {
            return self.by_id(id);
        }
        if let Some(num) = args.get("index").and_then(Json::as_usize) {
            let idx = num
                .checked_sub(1)
                .ok_or("номер региона index начинается с 1")?;
            if idx >= self.sheet.regions.len() || self.sheet.regions[idx].imported {
                return Err(format!("регион #{num} не найден"));
            }
            return Ok(idx);
        }
        Err("укажите index (начиная с 1) или region_id".into())
    }

    fn next_auto_pos(&self) -> (f32, f32) {
        let bottom = self
            .sheet
            .regions
            .iter()
            .filter(|r| !r.imported)
            .map(|r| r.visual.top + r.visual.height)
            .fold(0.0f32, f32::max);
        (12.0, bottom + 14.0)
    }

    fn rebuild(&self, xml: &str, replacing: Option<usize>) -> Calc<Self> {
        let mut candidate = Self::load(xml, self.name.clone(), self.path.clone())?;
        candidate.disk_fingerprint = self.disk_fingerprint;
        for old in 0..self.sheet.regions.len() {
            if Some(old) == replacing || !self.sheet.edited(old) {
                continue;
            }
            let id = &self.sheet.regions[old].source_id;
            if let Ok(other) = candidate.by_id(id) {
                if let Some(value) = self.sheet.input_value(old) {
                    if candidate.sheet.editable(other) {
                        candidate.sheet.set(other, Some(value), None)?;
                    }
                }
            }
        }
        candidate.sheet.incremental();
        candidate.formula_changed = true;
        candidate.unsaved = true;
        Ok(candidate)
    }

    fn export_xml(&self) -> Calc<String> {
        let mut changes = Vec::new();
        let mut edited_inputs = 0usize;
        for index in 0..self.sheet.regions.len() {
            if !self.sheet.edited(index) {
                continue;
            }
            edited_inputs += 1;
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
        let xml = editing::export_xml(&xml, self.formula_changed || edited_inputs > 0)?;
        Sheet::import_with_base(&xml, self.path.as_deref().and_then(Path::parent))?;
        Ok(xml)
    }

    fn region_json(&self, index: usize, detailed: bool) -> Json {
        let region = &self.sheet.regions[index];
        let kind_str = if region.is_page_break() {
            "page_break"
        } else if region.label == "Ссылка" {
            "reference"
        } else if region.visual.graphical {
            "visual"
        } else if region.text.is_some() {
            "text"
        } else if region.visual.function {
            "function"
        } else if self.sheet.editable(index) {
            "input"
        } else {
            "formula"
        };
        let (value_json, error_json) = if region.text.is_some() || !region.has_formula() {
            (Json::Null, Json::Null)
        } else {
            let res = if detailed {
                self.sheet.details_with_precision(index, self.precision)
            } else {
                self.sheet.display_with_precision(index, self.precision)
            };
            match res {
                Ok(v) => (Json::s(v), Json::Null),
                Err(e) => (Json::Null, Json::s(e)),
            }
        };
        let mut fields = vec![
            ("index", Json::u(index + 1)),
            ("region_id", Json::s(&region.source_id)),
            ("kind", Json::s(kind_str)),
            ("label", Json::s(&region.label)),
            ("imported", Json::Bool(region.imported)),
            ("editable", Json::Bool(self.sheet.editable(index))),
            ("edited", Json::Bool(self.sheet.edited(index))),
            ("left", Json::f(f64::from(region.visual.left))),
            ("top", Json::f(f64::from(region.visual.top))),
        ];
        if let Some(text) = &region.text {
            fields.push(("text", Json::s(text)));
        }
        if !region.visual.formula.is_empty() {
            fields.push(("formula", Json::s(&region.visual.formula)));
        }
        if let Some(source) = region
            .visual
            .editor
            .as_ref()
            .and_then(|e| e.source.as_deref())
        {
            fields.push(("source", Json::s(source)));
        }
        if let Some(input_val) = self.sheet.input_value(index) {
            fields.push(("input_value", Json::f(input_val)));
        }
        if let Some(unit) = &region.visual.input_unit {
            fields.push(("input_unit", Json::s(unit)));
        }
        if let Some(unit) = self.sheet.display_unit_label(index) {
            fields.push(("display_unit", Json::s(unit)));
        }
        if value_json != Json::Null {
            fields.push(("value", value_json));
        }
        if error_json != Json::Null {
            fields.push(("error", error_json));
        }
        Json::obj(fields)
    }

    fn summary_json(&self) -> Json {
        let (inputs, formulas, errors) = self.sheet.counts();
        let visible_regions = self.sheet.regions.iter().filter(|r| !r.imported).count();
        let plots = editing::charts(&self.xml).map(|m| m.len()).unwrap_or(0);
        let plots3d = editing::plots3d(&self.xml).map(|m| m.len()).unwrap_or(0);
        let areas = editing::areas(&self.xml).map(|v| v.len()).unwrap_or(0);
        Json::obj(vec![
            ("name", Json::s(&self.name)),
            (
                "path",
                self.path
                    .as_ref()
                    .map(|p| Json::s(p.display().to_string()))
                    .unwrap_or(Json::Null),
            ),
            ("regions", Json::u(visible_regions)),
            ("formulas", Json::u(formulas)),
            ("inputs", Json::u(inputs)),
            ("errors", Json::u(errors)),
            ("plots_2d", Json::u(plots)),
            ("plots_3d", Json::u(plots3d)),
            ("areas", Json::u(areas)),
            (
                "precision",
                self.precision.map(Json::u).unwrap_or(Json::Null),
            ),
            ("unsaved", Json::Bool(self.unsaved)),
        ])
    }

    fn errors_json(&self) -> Json {
        let mut errs = Vec::new();
        for (idx, region) in self.sheet.regions.iter().enumerate() {
            if region.imported || !region.has_formula() {
                continue;
            }
            if let Err(err) = self.sheet.display_with_precision(idx, self.precision) {
                errs.push(Json::obj(vec![
                    ("index", Json::u(idx + 1)),
                    ("region_id", Json::s(&region.source_id)),
                    ("label", Json::s(&region.label)),
                    ("formula", Json::s(&region.visual.formula)),
                    ("error", Json::s(err)),
                ]));
            }
        }
        Json::arr(errs)
    }

    fn build_html_report(&self) -> Calc<String> {
        let specs = editing::charts(&self.xml)?;
        let opts = editing::chart_options(&self.xml)?;
        let specs3d = editing::plots3d(&self.xml)?;
        let img_specs = editing::images(&self.xml)?;

        let mut chart_data = Vec::new();
        for (idx, region) in self.sheet.regions.iter().enumerate() {
            if region.imported {
                continue;
            }
            if let Some(spec) = specs.get(&region.source_id) {
                let options = opts.get(&region.source_id).cloned().unwrap_or_default();
                let curves = self.sheet.chart_curves(idx, spec);
                chart_data.push((idx, spec.title(), spec.is_polar(), options, curves));
            }
        }
        let charts: HashMap<usize, ChartReport<'_>> = chart_data
            .iter()
            .map(|(idx, title, is_polar, options, curves)| {
                (
                    *idx,
                    ChartReport {
                        curves,
                        options,
                        is_polar: *is_polar,
                        title: title.as_str(),
                    },
                )
            })
            .collect();

        let mut p3d_data = Vec::new();
        for (idx, region) in self.sheet.regions.iter().enumerate() {
            if region.imported {
                continue;
            }
            if let Some(spec) = specs3d.get(&region.source_id) {
                let grid = self.sheet.surface_grid(idx, spec);
                p3d_data.push((idx, spec.clone(), grid));
            }
        }
        let plots3d: HashMap<usize, Plot3dReport<'_>> = p3d_data
            .iter()
            .map(|(idx, spec, grid)| (*idx, Plot3dReport { grid, spec }))
            .collect();

        let images: HashMap<usize, editing::ImageSpec> = self
            .sheet
            .regions
            .iter()
            .enumerate()
            .filter_map(|(idx, r)| {
                if r.imported {
                    return None;
                }
                img_specs.get(&r.source_id).cloned().map(|s| (idx, s))
            })
            .collect();

        Ok(report::render_html_report_full(
            &self.name,
            &self.sheet,
            &charts,
            &plots3d,
            &images,
            &self.page_settings,
            self.precision,
        ))
    }
}

pub struct McpServer {
    doc: ActiveSheet,
    phase: Phase,
}

#[derive(PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    Ready,
}

impl McpServer {
    pub fn new(initial_path: Option<&Path>) -> Calc<Self> {
        let doc = match initial_path {
            Some(path) => ActiveSheet::from_path(path.to_path_buf())?,
            None => ActiveSheet::blank(None)?,
        };
        Ok(Self {
            doc,
            phase: Phase::New,
        })
    }

    pub fn handle_message(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let req = match parse_json(trimmed) {
            Ok(v) => v,
            Err(err) => {
                return Some(
                    rpc_error(Json::Null, -32700, &format!("Parse error: {err}")).to_json_string(),
                );
            }
        };
        let id = req.get("id").cloned();
        let valid_id = id.as_ref().is_none_or(|id| {
            id.as_str().is_some()
                || id
                    .as_f64()
                    .is_some_and(|n| n.fract() == 0. && n.abs() <= 9_007_199_254_740_991.)
        });
        let method = req.get("method").and_then(Json::as_str);
        if req.get("jsonrpc").and_then(Json::as_str) != Some("2.0") || method.is_none() || !valid_id
        {
            return Some(rpc_error(Json::Null, -32600, "Invalid Request").to_json_string());
        }
        let method = method.unwrap();
        let params = req
            .get("params")
            .cloned()
            .unwrap_or_else(|| Json::obj(vec![]));

        // Notifications have no `id` and MUST NOT receive a response per JSON-RPC 2.0 / MCP spec
        let Some(id) = id else {
            if method == "notifications/initialized" && self.phase == Phase::Initializing {
                self.phase = Phase::Ready;
            }
            return None;
        };

        let response = match method {
            "initialize" => self.handle_initialize(id, &params),
            "ping" => rpc_result(id, Json::obj(vec![])),
            "server/discover" => rpc_error(id, -32601, "Use the MCP initialize handshake"),
            _ if self.phase != Phase::Ready => {
                rpc_error(id, -32002, "MCP initialization is incomplete")
            }
            "tools/list" => rpc_result(id, Json::obj(vec![("tools", tools_schema())])),
            "tools/call" => self.handle_tool_call(id, &params),
            "resources/list" => rpc_result(id, Json::obj(vec![("resources", resources_schema())])),
            "resources/read" => self.handle_resource_read(id, &params),
            other => rpc_error(id, -32601, &format!("Method not found: {other}")),
        };
        Some(response.to_json_string())
    }

    fn handle_initialize(&mut self, id: Json, params: &Json) -> Json {
        if self.phase != Phase::New {
            return rpc_error(id, -32600, "MCP session is already initialized");
        }
        let Some(requested) = params.get("protocolVersion").and_then(Json::as_str) else {
            return rpc_error(id, -32602, "Missing protocolVersion");
        };
        let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
            requested
        } else {
            SUPPORTED_PROTOCOLS[0]
        };
        self.phase = Phase::Initializing;
        rpc_result(
            id,
            Json::obj(vec![
                ("protocolVersion", Json::s(version)),
                (
                    "capabilities",
                    Json::obj(vec![
                        ("tools", Json::obj(vec![("listChanged", Json::Bool(false))])),
                        (
                            "resources",
                            Json::obj(vec![
                                ("subscribe", Json::Bool(false)),
                                ("listChanged", Json::Bool(false)),
                            ]),
                        ),
                    ]),
                ),
                (
                    "serverInfo",
                    Json::obj(vec![
                        ("name", Json::s("list-rs")),
                        ("version", Json::s(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
                ("instructions", Json::s(SERVER_INSTRUCTIONS)),
            ]),
        )
    }

    fn handle_resource_read(&self, id: Json, params: &Json) -> Json {
        let Some(uri) = params.get("uri").and_then(Json::as_str) else {
            return rpc_error(id, -32602, "Missing required parameter: uri");
        };
        let (mime, text) = match uri {
            "sheet://current/summary" => {
                ("application/json", self.doc.summary_json().to_json_string())
            }
            "sheet://current/errors" => {
                ("application/json", self.doc.errors_json().to_json_string())
            }
            "sheet://current/xml" => match self.doc.export_xml() {
                Ok(xml) => ("application/xml", xml),
                Err(error) => return rpc_error(id, -32000, &error),
            },
            _ => return rpc_error(id, -32602, &format!("Unknown resource URI: {uri}")),
        };
        rpc_result(
            id,
            Json::obj(vec![(
                "contents",
                Json::arr(vec![Json::obj(vec![
                    ("uri", Json::s(uri)),
                    ("mimeType", Json::s(mime)),
                    ("text", Json::s(text)),
                ])]),
            )]),
        )
    }

    fn handle_tool_call(&mut self, id: Json, params: &Json) -> Json {
        let Some(name) = params.get("name").and_then(Json::as_str) else {
            return rpc_error(id, -32602, "Missing tool name");
        };
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Json::obj(vec![]));
        if !matches!(args, Json::Object(_)) {
            return rpc_error(id, -32602, "Tool arguments must be an object");
        }

        let outcome = match name {
            "new_sheet" => self.tool_new_sheet(&args),
            "open_sheet" => self.tool_open_sheet(&args),
            "list_regions" => self.tool_list_regions(&args),
            "get_region" => self.tool_get_region(&args),
            "set_input" => self.tool_set_input(&args),
            "reset_input" => self.tool_reset_input(&args),
            "insert_region" => self.tool_insert_region(&args),
            "edit_formula" => self.tool_edit_formula(&args),
            "move_region" => self.tool_move_region(&args),
            "delete_region" => self.tool_delete_region(&args),
            "set_display_unit" => self.tool_set_display_unit(&args),
            "set_precision" => self.tool_set_precision(&args),
            "eval_expr" => self.tool_eval_expr(&args),
            "verify_snapshots" => self.tool_verify_snapshots(&args),
            "import_csv" => self.tool_import_csv(&args),
            "export_csv" => self.tool_export_csv(&args),
            "export_xmcd" => self.tool_export_xmcd(&args),
            "export_report" => self.tool_export_report(&args),
            unknown => return rpc_error(id, -32602, &format!("Unknown tool: {unknown}")),
        };

        let result = match outcome {
            Ok(payload) => Json::obj(vec![
                (
                    "content",
                    Json::arr(vec![Json::obj(vec![
                        ("type", Json::s("text")),
                        ("text", Json::s(payload.to_json_string())),
                    ])]),
                ),
                ("isError", Json::Bool(false)),
            ]),
            Err(err) => Json::obj(vec![
                (
                    "content",
                    Json::arr(vec![Json::obj(vec![
                        ("type", Json::s("text")),
                        ("text", Json::s(format!("Ошибка: {err}"))),
                    ])]),
                ),
                ("isError", Json::Bool(true)),
            ]),
        };
        rpc_result(id, result)
    }

    fn tool_new_sheet(&mut self, args: &Json) -> Calc<Json> {
        self.require_saved_or_discard(args)?;
        let demo = args.get("demo").and_then(Json::as_bool).unwrap_or(false);
        let name = args.get("name").and_then(Json::as_str);
        self.doc = if demo {
            ActiveSheet::demo()?
        } else {
            ActiveSheet::blank(name)?
        };
        Ok(self.doc.summary_json())
    }

    fn tool_open_sheet(&mut self, args: &Json) -> Calc<Json> {
        self.require_saved_or_discard(args)?;
        let path = required_str(args, "path")?;
        self.doc = ActiveSheet::from_path(PathBuf::from(path))?;
        let regions: Vec<Json> = (0..self.doc.sheet.regions.len())
            .filter(|&i| !self.doc.sheet.regions[i].imported)
            .map(|i| self.doc.region_json(i, false))
            .collect();
        Ok(Json::obj(vec![
            ("summary", self.doc.summary_json()),
            ("regions", Json::arr(regions)),
        ]))
    }

    fn tool_list_regions(&self, args: &Json) -> Calc<Json> {
        let include_imported = args
            .get("include_imported")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        let items: Vec<Json> = (0..self.doc.sheet.regions.len())
            .filter(|&i| include_imported || !self.doc.sheet.regions[i].imported)
            .map(|i| self.doc.region_json(i, false))
            .collect();
        Ok(Json::obj(vec![
            ("summary", self.doc.summary_json()),
            ("regions", Json::arr(items)),
        ]))
    }

    fn tool_get_region(&self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        Ok(self.doc.region_json(idx, true))
    }

    fn tool_set_input(&mut self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        let value = args
            .get("value")
            .and_then(Json::as_f64)
            .ok_or("укажите числовое поле value")?;
        let unit = args
            .get("unit")
            .and_then(Json::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        self.doc.sheet.set(idx, Some(value), unit)?;
        let recalculated = self.doc.sheet.incremental();
        self.doc.unsaved |= self.doc.sheet.edited(idx);
        Ok(Json::obj(vec![
            ("recalculated", Json::u(recalculated)),
            ("region", self.doc.region_json(idx, true)),
            ("summary", self.doc.summary_json()),
            ("errors", self.doc.errors_json()),
        ]))
    }

    fn tool_reset_input(&mut self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        self.doc.sheet.set(idx, None, None)?;
        let recalculated = self.doc.sheet.incremental();
        Ok(Json::obj(vec![
            ("recalculated", Json::u(recalculated)),
            ("region", self.doc.region_json(idx, true)),
            ("summary", self.doc.summary_json()),
        ]))
    }

    fn tool_insert_region(&mut self, args: &Json) -> Calc<Json> {
        let kind = args.get("kind").and_then(Json::as_str).unwrap_or("formula");
        let (auto_left, auto_top) = self.doc.next_auto_pos();
        let left = args
            .get("left")
            .and_then(Json::as_f64)
            .map_or(auto_left, |v| v as f32);
        let top = args
            .get("top")
            .and_then(Json::as_f64)
            .map_or(auto_top, |v| v as f32);

        if kind == "area" {
            let title = required_str(args, "title")?;
            let bottom = args
                .get("bottom")
                .and_then(Json::as_f64)
                .map_or(top + 180.0, |v| v as f32);
            let (xml, area_id) = editing::insert_area(&self.doc.xml, title, top, bottom)?;
            self.doc = self.doc.rebuild(&xml, None)?;
            return Ok(Json::obj(vec![
                ("area_id", Json::s(area_id)),
                ("summary", self.doc.summary_json()),
            ]));
        }

        let (xml, region_id) = match kind {
            "formula" => {
                let formula = required_str(args, "formula")?;
                editing::insert_region(&self.doc.xml, left, top, NewRegion::Formula(formula))?
            }
            "text" => {
                let text = required_str(args, "text")?;
                let style = match args.get("style").and_then(Json::as_str) {
                    Some("h1" | "Heading 1") => TextStyle::Heading1,
                    Some("h2" | "Heading 2") => TextStyle::Heading2,
                    _ => TextStyle::Normal,
                };
                editing::insert_region(
                    &self.doc.xml,
                    left,
                    top,
                    NewRegion::StyledText(text, style),
                )?
            }
            "reference" => {
                let href = required_str(args, "href")?;
                editing::insert_region(&self.doc.xml, left, top, NewRegion::Reference(href))?
            }
            "page_break" => editing::insert_region(&self.doc.xml, left, top, NewRegion::PageBreak)?,
            "chart" => {
                let spec = parse_chart_spec(args)?;
                let options = parse_chart_options(args);
                editing::insert_region(
                    &self.doc.xml,
                    left,
                    top,
                    NewRegion::ChartConfigured(&spec, &options),
                )?
            }
            "plot3d" => {
                let spec = parse_plot3d_spec(args)?;
                editing::insert_region(&self.doc.xml, left, top, NewRegion::Plot3d(&spec))?
            }
            other => {
                return Err(format!(
                    "неизвестный kind={other}; допустимы formula, text, chart, plot3d, reference, page_break, area"
                ));
            }
        };

        let candidate = self.doc.rebuild(&xml, None)?;
        let idx = candidate.by_id(&region_id)?;
        let eval_note = if kind == "chart" {
            let specs = editing::charts(&candidate.xml)?;
            if let Some(spec) = specs.get(&region_id) {
                candidate.sheet.chart_curves(idx, spec)?;
            }
            None
        } else if kind == "plot3d" {
            let specs3d = editing::plots3d(&candidate.xml)?;
            if let Some(spec) = specs3d.get(&region_id) {
                let grid = candidate.sheet.surface_grid(idx, spec)?;
                Some(Json::obj(vec![
                    ("rows", Json::u(grid.rows)),
                    ("cols", Json::u(grid.cols)),
                    ("z_min", Json::f(grid.z_min)),
                    ("z_max", Json::f(grid.z_max)),
                ]))
            } else {
                None
            }
        } else {
            None
        };
        self.doc = candidate;
        let mut out = vec![
            ("region", self.doc.region_json(idx, true)),
            ("summary", self.doc.summary_json()),
        ];
        if let Some(meta) = eval_note {
            out.push(("plot_meta", meta));
        }
        Ok(Json::obj(out))
    }

    fn tool_edit_formula(&mut self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        let source = required_str(args, "formula")?;
        let edit = self.doc.sheet.regions[idx]
            .visual
            .editor
            .as_ref()
            .ok_or("выбранный регион не является редактируемой формулой")?;
        let region_id = self.doc.sheet.regions[idx].source_id.clone();
        let replacement = edit.replacement(&self.doc.xml, source)?;
        let xml = editing::patch(&self.doc.xml, vec![(edit.statement.clone(), replacement)])?;
        let candidate = self.doc.rebuild(&xml, Some(idx))?;
        let new_idx = candidate.by_id(&region_id)?;
        self.doc = candidate;
        Ok(Json::obj(vec![
            ("region", self.doc.region_json(new_idx, true)),
            ("summary", self.doc.summary_json()),
            ("errors", self.doc.errors_json()),
        ]))
    }

    fn tool_move_region(&mut self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        let left = args
            .get("left")
            .and_then(Json::as_f64)
            .ok_or("укажите числовое поле left")? as f32;
        let top = args
            .get("top")
            .and_then(Json::as_f64)
            .ok_or("укажите числовое поле top")? as f32;
        let region_id = self.doc.sheet.regions[idx].source_id.clone();
        let xml = editing::move_regions(&self.doc.xml, &[(region_id.clone(), left, top)])?;
        self.doc = self.doc.rebuild(&xml, None)?;
        let new_idx = self.doc.by_id(&region_id)?;
        Ok(Json::obj(vec![
            ("region", self.doc.region_json(new_idx, true)),
            ("summary", self.doc.summary_json()),
            ("errors", self.doc.errors_json()),
        ]))
    }

    fn tool_delete_region(&mut self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        let region_id = self.doc.sheet.regions[idx].source_id.clone();
        let xml = editing::delete_regions(&self.doc.xml, std::slice::from_ref(&region_id))?;
        self.doc = self.doc.rebuild(&xml, Some(idx))?;
        Ok(Json::obj(vec![
            ("deleted_region_id", Json::s(region_id)),
            ("summary", self.doc.summary_json()),
            ("errors", self.doc.errors_json()),
        ]))
    }

    fn tool_set_display_unit(&mut self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        let region_id = self.doc.sheet.regions[idx].source_id.clone();
        let unit = args
            .get("unit")
            .and_then(Json::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let xml = editing::set_display_unit(&self.doc.xml, &region_id, unit)?;
        let candidate = self.doc.rebuild(&xml, None)?;
        let new_idx = candidate.by_id(&region_id)?;
        candidate
            .sheet
            .display_with_precision(new_idx, candidate.precision)?;
        self.doc = candidate;
        Ok(Json::obj(vec![
            ("region", self.doc.region_json(new_idx, true)),
            ("summary", self.doc.summary_json()),
        ]))
    }

    fn tool_set_precision(&mut self, args: &Json) -> Calc<Json> {
        let precision = match args.get("precision") {
            None | Some(Json::Null) => None,
            Some(v) => {
                let p = v
                    .as_usize()
                    .ok_or("precision должен быть целым числом от 1 до 15 или null")?;
                if !(1..=15).contains(&p) {
                    return Err("precision должен быть в диапазоне 1..=15".into());
                }
                Some(p)
            }
        };
        let xml = editing::set_sheet_precision(&self.doc.xml, precision)?;
        self.doc = self.doc.rebuild(&xml, None)?;
        Ok(self.doc.summary_json())
    }

    fn tool_eval_expr(&self, args: &Json) -> Calc<Json> {
        let expression = required_str(args, "expression")?;
        let use_current = args
            .get("use_current_sheet")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        let mut xml = if use_current {
            self.doc.export_xml()?
        } else {
            BLANK.to_owned()
        };
        let mut top = if use_current {
            self.doc.next_auto_pos().1
        } else {
            10.0
        };
        if let Some(setup) = args.get("setup").and_then(Json::as_array) {
            for item in setup {
                let line = item
                    .as_str()
                    .ok_or("setup должен быть массивом строк-формул")?;
                let (next_xml, _) =
                    editing::insert_region(&xml, 12.0, top, NewRegion::Formula(line))?;
                xml = next_xml;
                top += 40.0;
            }
        }
        let (xml, id) = editing::insert_region(&xml, 12.0, top, NewRegion::Formula(expression))?;
        let base_dir = if use_current {
            self.doc.path.as_deref().and_then(Path::parent)
        } else {
            None
        };
        let mut sheet = Sheet::import_with_base(&xml, base_dir)?;
        sheet.full();
        let idx = sheet
            .regions
            .iter()
            .position(|r| !r.imported && r.source_id == id)
            .ok_or("не удалось найти вычисляемое выражение")?;
        let precision = args
            .get("precision")
            .and_then(Json::as_usize)
            .or(self.doc.precision);
        let value = sheet.details_with_precision(idx, precision)?;
        Ok(Json::obj(vec![
            ("expression", Json::s(expression)),
            ("formula", Json::s(&sheet.regions[idx].visual.formula)),
            ("value", Json::s(value)),
        ]))
    }

    fn tool_verify_snapshots(&self, args: &Json) -> Calc<Json> {
        let report = if let Some(path) = args.get("path").and_then(Json::as_str) {
            let mut sheet = Sheet::import_from_path(Path::new(path))?;
            sheet.full();
            sheet.verify_snapshots_report()
        } else {
            self.doc.sheet.verify_snapshots_report()
        };
        Ok(Json::obj(vec![
            ("ok", Json::Bool(report.ok)),
            ("matched", Json::u(report.matched)),
            ("failed", Json::u(report.failed)),
            ("skipped", Json::u(report.skipped)),
            (
                "mismatches",
                Json::arr(report.mismatches.into_iter().map(Json::s).collect()),
            ),
        ]))
    }

    fn tool_import_csv(&mut self, args: &Json) -> Calc<Json> {
        let name = required_str(args, "name")?;
        let table = if let Some(path) = args.get("path").and_then(Json::as_str) {
            data_io::read_csv_file(Path::new(path))?
        } else if let Some(csv_text) = args.get("csv_text").and_then(Json::as_str) {
            data_io::parse_csv(csv_text)?
        } else {
            return Err("укажите path или csv_text для импорта CSV".into());
        };
        let formula = table.to_formula(name)?;
        let (auto_left, auto_top) = self.doc.next_auto_pos();
        let left = args
            .get("left")
            .and_then(Json::as_f64)
            .map_or(auto_left, |v| v as f32);
        let top = args
            .get("top")
            .and_then(Json::as_f64)
            .map_or(auto_top, |v| v as f32);
        let (xml, id) =
            editing::insert_region(&self.doc.xml, left, top, NewRegion::Formula(&formula))?;
        let candidate = self.doc.rebuild(&xml, None)?;
        let idx = candidate.by_id(&id)?;
        self.doc = candidate;
        Ok(Json::obj(vec![
            ("rows", Json::u(table.rows)),
            ("cols", Json::u(table.cols)),
            (
                "headers",
                Json::arr(table.headers.into_iter().map(Json::s).collect()),
            ),
            ("region", self.doc.region_json(idx, false)),
            ("summary", self.doc.summary_json()),
        ]))
    }

    fn tool_export_csv(&self, args: &Json) -> Calc<Json> {
        let idx = self.doc.resolve_region(args)?;
        let delimiter = args
            .get("delimiter")
            .and_then(Json::as_str)
            .and_then(|s| s.chars().next())
            .unwrap_or(',');
        let csv = self
            .doc
            .sheet
            .export_csv(idx, delimiter, self.doc.precision)?;
        let written = if let Some(path) = args.get("path").and_then(Json::as_str) {
            data_io::write_csv_file(Path::new(path), &csv)?;
            Json::s(path)
        } else {
            Json::Null
        };
        Ok(Json::obj(vec![("csv", Json::s(csv)), ("path", written)]))
    }

    fn tool_export_xmcd(&mut self, args: &Json) -> Calc<Json> {
        let path_str = required_str(args, "path")?;
        let overwrite = args
            .get("overwrite")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        let path = PathBuf::from(path_str);
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xmcd"))
        {
            return Err("для экспорта укажите путь с расширением .xmcd".into());
        }
        let xml = self.doc.export_xml()?;
        let parent = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = std::path::absolute(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
        let path = parent.join(path.file_name().ok_or("укажите имя файла")?);
        if let Some(current) = self.doc.path.as_deref() {
            let previous_dir = current
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            if previous_dir.canonicalize().ok() != parent.canonicalize().ok()
                && has_relative_references(&xml)?
            {
                return Err(
                    "Лист с относительными подключениями нужно сохранить в папке исходного листа"
                        .into(),
                );
            }
        }
        if overwrite
            && self.doc.path.as_deref().is_some_and(|current| {
                match (current.canonicalize(), path.canonicalize()) {
                    (Ok(current), Ok(target)) => current == target,
                    _ => false,
                }
            })
            && self.doc.disk_fingerprint != Some(fingerprint(&read_xml(&path)?))
        {
            return Err(
                "Лист изменён на диске. Откройте его заново или сохраните правки в отдельную копию"
                    .into(),
            );
        }

        let name = path
            .file_stem()
            .ok_or("укажите имя файла")?
            .to_string_lossy()
            .into_owned();
        // Validate and rebase the saved worksheet before touching the destination.
        let candidate = ActiveSheet::load(&xml, name, Some(path.clone()))?;
        if overwrite {
            replace_xmcd(&parent, &path, &xml)?;
        } else {
            editing::write_copy(&path, &xml)?;
        }
        self.doc = candidate;
        Ok(Json::obj(vec![
            ("path", Json::s(path.display().to_string())),
            ("bytes", Json::u(xml.len())),
            ("overwrite", Json::Bool(overwrite)),
        ]))
    }

    fn require_saved_or_discard(&self, args: &Json) -> Calc<()> {
        if self.doc.unsaved && args.get("discard_unsaved").and_then(Json::as_bool) != Some(true) {
            return Err(
                "Есть несохранённые изменения. Сохраните лист или укажите discard_unsaved=true"
                    .into(),
            );
        }
        Ok(())
    }

    fn tool_export_report(&self, args: &Json) -> Calc<Json> {
        let path_str = required_str(args, "path")?;
        let path = Path::new(path_str);
        let format = args
            .get("format")
            .and_then(Json::as_str)
            .unwrap_or_else(|| {
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                {
                    "pdf"
                } else {
                    "html"
                }
            });
        let html = self.doc.build_html_report()?;
        match format {
            "html" => report::write_html_report(path, &html)?,
            "pdf" => report::export_pdf_via_browser(&html, path)?,
            other => {
                return Err(format!(
                    "неподдерживаемый формат отчёта {other}; используйте html или pdf"
                ))
            }
        }
        Ok(Json::obj(vec![
            ("path", Json::s(path.display().to_string())),
            ("format", Json::s(format)),
        ]))
    }
}

fn fingerprint(xml: &str) -> u64 {
    let mut hash = DefaultHasher::new();
    xml.hash(&mut hash);
    hash.finish()
}

fn has_relative_references(xml: &str) -> Calc<bool> {
    let document = roxmltree::Document::parse(xml).map_err(|error| error.to_string())?;
    Ok(document.descendants().any(|node| {
        node.tag_name().namespace() == Some("http://schemas.mathsoft.com/worksheet30")
            && matches!(node.tag_name().name(), "reference" | "include")
            && node
                .attribute("href")
                .or_else(|| node.attribute("file"))
                .or_else(|| node.attribute("path"))
                .is_some_and(|href| {
                    let href = href.trim();
                    let path = href
                        .strip_prefix("file:///")
                        .or_else(|| href.strip_prefix("file://"))
                        .unwrap_or(href);
                    !Path::new(path).is_absolute()
                })
    }))
}

fn replace_xmcd(parent: &Path, path: &Path, xml: &str) -> Calc<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".list-rs-{}-{nonce}.tmp", std::process::id()));
    editing::write_copy(&temp, xml)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(())
}

fn required_str<'a>(args: &'a Json, key: &str) -> Calc<&'a str> {
    args.get(key)
        .and_then(Json::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("отсутствует обязательный строковый параметр {key}"))
}

fn parse_chart_spec(args: &Json) -> Calc<ChartSpec> {
    let mode = args
        .get("mode")
        .and_then(Json::as_str)
        .unwrap_or("function");
    match mode {
        "function" => Ok(ChartSpec::Function {
            expression: required_str(args, "expression")?.into(),
            from: args.get("from").and_then(Json::as_f64).unwrap_or(-6.28),
            to: args.get("to").and_then(Json::as_f64).unwrap_or(6.28),
        }),
        "vectors" => Ok(ChartSpec::Vectors {
            x: required_str(args, "x")?.into(),
            y: required_str(args, "y")?.into(),
        }),
        "parametric" => Ok(ChartSpec::Parametric {
            x_expr: required_str(args, "x_expr")?.into(),
            y_expr: required_str(args, "y_expr")?.into(),
            from: args.get("from").and_then(Json::as_f64).unwrap_or(0.0),
            to: args
                .get("to")
                .and_then(Json::as_f64)
                .unwrap_or(std::f64::consts::TAU),
        }),
        "polar" => Ok(ChartSpec::Polar {
            expression: required_str(args, "expression")?.into(),
            from: args.get("from").and_then(Json::as_f64).unwrap_or(0.0),
            to: args
                .get("to")
                .and_then(Json::as_f64)
                .unwrap_or(std::f64::consts::TAU),
        }),
        other => Err(format!(
            "неизвестный режим графика {other}; допустимы function, vectors, parametric, polar"
        )),
    }
}

fn parse_chart_options(args: &Json) -> ChartOptions {
    ChartOptions {
        log_x: args.get("log_x").and_then(Json::as_bool).unwrap_or(false),
        log_y: args.get("log_y").and_then(Json::as_bool).unwrap_or(false),
        equal_scale: args
            .get("equal_scale")
            .and_then(Json::as_bool)
            .unwrap_or(false),
        markers: args.get("markers").and_then(Json::as_bool).unwrap_or(false),
        show_legend: args
            .get("show_legend")
            .and_then(Json::as_bool)
            .unwrap_or(true),
        x_min: args.get("x_min").and_then(Json::as_f64),
        x_max: args.get("x_max").and_then(Json::as_f64),
        y_min: args.get("y_min").and_then(Json::as_f64),
        y_max: args.get("y_max").and_then(Json::as_f64),
        x_label: args
            .get("x_label")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .into(),
        y_label: args
            .get("y_label")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .into(),
        appearance: Default::default(),
    }
}

fn parse_plot3d_spec(args: &Json) -> Calc<Plot3dSpec> {
    let mode = match args.get("mode").and_then(Json::as_str) {
        Some("contour") => Plot3dMode::Contour,
        _ => Plot3dMode::Surface,
    };
    let source = if let Some(matrix) = args.get("matrix").and_then(Json::as_str) {
        Plot3dSource::Matrix {
            name: matrix.trim().into(),
        }
    } else {
        Plot3dSource::Function {
            expression: required_str(args, "expression")?.into(),
            x_from: args.get("x_from").and_then(Json::as_f64).unwrap_or(-3.14),
            x_to: args.get("x_to").and_then(Json::as_f64).unwrap_or(3.14),
            y_from: args.get("y_from").and_then(Json::as_f64).unwrap_or(-3.14),
            y_to: args.get("y_to").and_then(Json::as_f64).unwrap_or(3.14),
        }
    };
    Ok(Plot3dSpec {
        mode,
        source,
        yaw_deg: args.get("yaw_deg").and_then(Json::as_f64).unwrap_or(38.0) as f32,
        pitch_deg: args.get("pitch_deg").and_then(Json::as_f64).unwrap_or(30.0) as f32,
        grid: args.get("grid").and_then(Json::as_usize).unwrap_or(18),
    })
}

fn rpc_result(id: Json, result: Json) -> Json {
    Json::obj(vec![
        ("jsonrpc", Json::s("2.0")),
        ("id", id),
        ("result", result),
    ])
}

fn rpc_error(id: Json, code: i64, message: &str) -> Json {
    Json::obj(vec![
        ("jsonrpc", Json::s("2.0")),
        ("id", id),
        (
            "error",
            Json::obj(vec![("code", Json::i(code)), ("message", Json::s(message))]),
        ),
    ])
}

fn prop(ty: &str, description: &str) -> Json {
    Json::obj(vec![
        ("type", Json::s(ty)),
        ("description", Json::s(description)),
    ])
}

fn tool_def(name: &str, description: &str, props: Vec<(&str, Json)>, required: &[&str]) -> Json {
    let mut schema_fields = vec![
        ("type", Json::s("object")),
        ("properties", Json::obj(props)),
    ];
    if !required.is_empty() {
        schema_fields.push((
            "required",
            Json::arr(required.iter().map(|&r| Json::s(r)).collect()),
        ));
    }
    Json::obj(vec![
        ("name", Json::s(name)),
        ("description", Json::s(description)),
        ("inputSchema", Json::obj(schema_fields)),
    ])
}

fn tools_schema() -> Json {
    Json::arr(vec![
        tool_def(
            "new_sheet",
            "Создать новый пустой расчётный лист (или встроенный демо-лист давления под опорой).",
            vec![
                ("name", prop("string", "Название листа")),
                ("demo", prop("boolean", "Если true — открыть демо-лист pressure.xmcd")),
                ("discard_unsaved", prop("boolean", "Разрешить сброс несохранённых правок текущего листа")),
            ],
            &[],
        ),
        tool_def(
            "open_sheet",
            "Открыть существующий лист Mathcad (.xmcd или .xmcdz), выполнить полный расчёт и вернуть список регионов.",
            vec![
                ("path", prop("string", "Путь к файлу .xmcd или .xmcdz")),
                ("discard_unsaved", prop("boolean", "Разрешить сброс несохранённых правок текущего листа")),
            ],
            &["path"],
        ),
        tool_def(
            "list_regions",
            "Показать все регионы текущего листа с формулами, вычисленными значениями, единицами и ошибками.",
            vec![(
                "include_imported",
                prop("boolean", "Включать скрытые регионы из подключённых <reference> листов"),
            )],
            &[],
        ),
        tool_def(
            "get_region",
            "Получить подробные сведения о регионе (полная матрица, исходная формула, координаты, единицы).",
            vec![
                ("index", prop("integer", "Номер региона на листе (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id из XMCD")),
            ],
            &[],
        ),
        tool_def(
            "set_input",
            "Изменить числовой вход (editable=true) и выполнить инкрементальный пересчёт зависимых формул.",
            vec![
                ("index", prop("integer", "Номер региона входа (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id входа")),
                ("value", prop("number", "Новое числовое значение")),
                ("unit", prop("string", "Опциональная единица измерения (например kN, mm, bar)")),
            ],
            &["value"],
        ),
        tool_def(
            "reset_input",
            "Сбросить переопределение числового входа к исходному значению из XMCD.",
            vec![
                ("index", prop("integer", "Номер региона (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id")),
            ],
            &[],
        ),
        tool_def(
            "insert_region",
            "Вставить новый регион на лист: формулу (включая :=, ===, ->, prog, Given/Find), текст, 2D/3D-график, ссылку на XMCD, разрыв страницы или сворачиваемую область.",
            vec![
                ("kind", prop("string", "Тип: formula (по умолчанию), text, chart, plot3d, reference, page_break, area")),
                ("formula", prop("string", "Текст формулы для kind=formula (например 'a := 2', 'f(x) := x^2', 'sdiff(x^3, x) ->')")),
                ("text", prop("string", "Содержимое текста для kind=text")),
                ("style", prop("string", "Стиль текста: normal, h1, h2")),
                ("href", prop("string", "Путь к подключаемому XMCD для kind=reference")),
                ("mode", prop("string", "Режим графика: function/vectors/parametric/polar для chart; surface/contour для plot3d")),
                ("expression", prop("string", "Выражение y(x), r(θ) или z(x, y)")),
                ("x", prop("string", "Вектор X для chart mode=vectors")),
                ("y", prop("string", "Вектор Y для chart mode=vectors")),
                ("x_expr", prop("string", "Выражение x(t) для chart mode=parametric")),
                ("y_expr", prop("string", "Выражение y(t) для chart mode=parametric")),
                ("matrix", prop("string", "Имя матрицы для plot3d")),
                ("from", prop("number", "Начало диапазона x/t/θ")),
                ("to", prop("number", "Конец диапазона x/t/θ")),
                ("title", prop("string", "Заголовок области для kind=area")),
                ("left", prop("number", "Координата X на листе (по умолчанию 12)")),
                ("top", prop("number", "Координата Y на листе (по умолчанию ниже последнего региона)")),
                ("bottom", prop("number", "Нижняя координата Y для kind=area")),
            ],
            &[],
        ),
        tool_def(
            "edit_formula",
            "Изменить существующую формулу на листе и пересчитать зависимые регионы.",
            vec![
                ("index", prop("integer", "Номер региона (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id")),
                ("formula", prop("string", "Новый текст формулы")),
            ],
            &["formula"],
        ),
        tool_def(
            "move_region",
            "Переместить регион в новые координаты (left, top) с пересчётом порядка вычислений.",
            vec![
                ("index", prop("integer", "Номер региона (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id")),
                ("left", prop("number", "Новая координата X")),
                ("top", prop("number", "Новая координата Y")),
            ],
            &["left", "top"],
        ),
        tool_def(
            "delete_region",
            "Удалить регион с листа и пересчитать лист.",
            vec![
                ("index", prop("integer", "Номер региона (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id")),
            ],
            &[],
        ),
        tool_def(
            "set_display_unit",
            "Задать или сбросить единицу вывода результата формулы (например kPa, mm, deg, °C, °F).",
            vec![
                ("index", prop("integer", "Номер региона (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id")),
                ("unit", prop("string", "Единица вывода; пусто или null — сбросить")),
            ],
            &[],
        ),
        tool_def(
            "set_precision",
            "Задать точность отображения чисел на листе (1..15 значащих цифр, null — авто 12).",
            vec![("precision", prop("integer", "Число значащих цифр (1..15) или null"))],
            &[],
        ),
        tool_def(
            "eval_expr",
            "Быстро вычислить выражение, символьную формулу (->), ОДУ, матрицу или блок Given…Find без изменения текущего листа.",
            vec![
                ("expression", prop("string", "Вычисляемое выражение (например 'sqrt(3^2 + 4^2) =' или 'sdiff(x^3, x) ->')")),
                (
                    "setup",
                    Json::obj(vec![
                        ("type", Json::s("array")),
                        ("items", Json::obj(vec![("type", Json::s("string"))])),
                        ("description", Json::s("Опциональный список предварительных определений/уравнений")),
                    ]),
                ),
                (
                    "use_current_sheet",
                    prop("boolean", "Если true — вычислять поверх переменных текущего открытого листа"),
                ),
                ("precision", prop("integer", "Точность вывода (1..15)")),
            ],
            &["expression"],
        ),
        tool_def(
            "verify_snapshots",
            "Сверить вычисленные результаты с сохранёнными эталонами Mathcad 15 (<ml:result>) для текущего листа или указанного файла.",
            vec![("path", prop("string", "Опциональный путь к .xmcd/.xmcdz для проверки без смены активного листа"))],
            &[],
        ),
        tool_def(
            "import_csv",
            "Импортировать числовую таблицу CSV (из файла или строки) как матрицу на текущий лист.",
            vec![
                ("name", prop("string", "Имя переменной матрицы (например Data)")),
                ("path", prop("string", "Путь к CSV-файлу")),
                ("csv_text", prop("string", "Содержимое CSV строкой (если не задан path)")),
                ("left", prop("number", "Координата X")),
                ("top", prop("number", "Координата Y")),
            ],
            &["name"],
        ),
        tool_def(
            "export_csv",
            "Экспортировать вычисленную матрицу, вектор, диапазон или скаляр в формат CSV.",
            vec![
                ("index", prop("integer", "Номер региона (начиная с 1)")),
                ("region_id", prop("string", "Идентификатор region-id")),
                ("delimiter", prop("string", "Разделитель (по умолчанию ',')")),
                ("path", prop("string", "Опциональный путь для сохранения CSV-файла")),
            ],
            &[],
        ),
        tool_def(
            "export_xmcd",
            "Сохранить текущий лист в файл .xmcd с очисткой устаревших кэшей. По умолчанию запрещает перезапись существующего файла (overwrite=false).",
            vec![
                ("path", prop("string", "Путь к выходному файлу .xmcd")),
                ("overwrite", prop("boolean", "Разрешить перезапись существующего файла (по умолчанию false)")),
            ],
            &["path"],
        ),
        tool_def(
            "export_report",
            "Сформировать многостраничный отчёт с формулами и SVG-графиками (2D/3D) в HTML или PDF.",
            vec![
                ("path", prop("string", "Путь к выходному файлу .html или .pdf")),
                ("format", prop("string", "Формат: html или pdf (по умолчанию определяется по расширению)")),
            ],
            &["path"],
        ),
    ])
}

fn resources_schema() -> Json {
    Json::arr(vec![
        Json::obj(vec![
            ("uri", Json::s("sheet://current/summary")),
            ("name", Json::s("Сводка текущего листа")),
            ("mimeType", Json::s("application/json")),
        ]),
        Json::obj(vec![
            ("uri", Json::s("sheet://current/errors")),
            ("name", Json::s("Ошибки формул текущего листа")),
            ("mimeType", Json::s("application/json")),
        ]),
        Json::obj(vec![
            ("uri", Json::s("sheet://current/xml")),
            ("name", Json::s("XML текущего листа (XMCD)")),
            ("mimeType", Json::s("application/xml")),
        ]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_server() -> McpServer {
        let mut server = McpServer::new(None).unwrap();
        assert!(server.handle_message(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#).is_some());
        assert!(server
            .handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
        server
    }

    fn call_tool(server: &mut McpServer, id: i64, name: &str, args: Json) -> Json {
        let req = Json::obj(vec![
            ("jsonrpc", Json::s("2.0")),
            ("id", Json::i(id)),
            ("method", Json::s("tools/call")),
            (
                "params",
                Json::obj(vec![("name", Json::s(name)), ("arguments", args)]),
            ),
        ]);
        let resp_str = server.handle_message(&req.to_json_string()).unwrap();
        let resp = parse_json(&resp_str).unwrap();
        let result = resp.get("result").expect("result").clone();
        assert_eq!(
            result.get("isError").and_then(Json::as_bool),
            Some(false),
            "{resp_str}"
        );
        let text = result.get("content").and_then(Json::as_array).unwrap()[0]
            .get("text")
            .and_then(Json::as_str)
            .unwrap();
        parse_json(text).unwrap()
    }

    #[test]
    fn mcp_handshake_tools_resources_and_notifications() {
        let mut server = McpServer::new(None).unwrap();
        let before_init = server
            .handle_message(r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#)
            .unwrap();
        assert!(before_init.contains("-32002"));
        assert!(server.handle_message("{}").unwrap().contains("-32600"));
        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"codex","version":"1.0"}}}"#;
        let init_resp = parse_json(&server.handle_message(init_req).unwrap()).unwrap();
        assert_eq!(
            init_resp
                .get("result")
                .and_then(|r| r.get("protocolVersion"))
                .and_then(Json::as_str),
            Some("2025-03-26")
        );

        // Notifications must not emit a response
        assert!(server
            .handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());

        let tools_resp = parse_json(
            &server
                .handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
                .unwrap(),
        )
        .unwrap();
        let tools = tools_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(Json::as_array)
            .unwrap();
        assert!(tools.len() >= 15);

        let res_resp = parse_json(
            &server
                .handle_message(
                    r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"sheet://current/summary"}}"#,
                )
                .unwrap(),
        )
        .unwrap();
        assert!(res_resp
            .get("result")
            .and_then(|r| r.get("contents"))
            .and_then(Json::as_array)
            .is_some_and(|c| !c.is_empty()));
    }

    #[test]
    fn mcp_full_worksheet_calculation_editing_units_and_export_flow() {
        let mut server = ready_server();

        // 1. Open built-in pressure demo and update input F = 60 kN -> p = 1.5 MPa
        call_tool(
            &mut server,
            1,
            "new_sheet",
            Json::obj(vec![("demo", Json::Bool(true))]),
        );
        let updated = call_tool(
            &mut server,
            2,
            "set_input",
            Json::obj(vec![("index", Json::i(2)), ("value", Json::f(60.0))]),
        );
        assert_eq!(
            updated.get("recalculated").and_then(Json::as_usize),
            Some(3)
        );
        let p_region = call_tool(
            &mut server,
            3,
            "get_region",
            Json::obj(vec![("index", Json::i(6))]),
        );
        assert_eq!(
            p_region.get("value").and_then(Json::as_str),
            Some("1.5 MPa")
        );

        // 2. Switch display unit to kPa -> 1500 kPa
        let unit_res = call_tool(
            &mut server,
            4,
            "set_display_unit",
            Json::obj(vec![("index", Json::i(6)), ("unit", Json::s("kPa"))]),
        );
        assert_eq!(
            unit_res
                .get("region")
                .and_then(|r| r.get("value"))
                .and_then(Json::as_str),
            Some("1500 kPa")
        );

        // 3. Stateless & contextual eval_expr (including symbolic diff and Given..Find)
        let sym = call_tool(
            &mut server,
            5,
            "eval_expr",
            Json::obj(vec![
                ("expression", Json::s("sdiff(x^3 + a*x, x) ->")),
                ("setup", Json::arr(vec![Json::s("a := 5")])),
            ]),
        );
        assert_eq!(sym.get("value").and_then(Json::as_str), Some("3·x^2 + 5"));

        let ctx_eval = call_tool(
            &mut server,
            6,
            "eval_expr",
            Json::obj(vec![
                ("expression", Json::s("p / MPa =")),
                ("use_current_sheet", Json::Bool(true)),
            ]),
        );
        assert_eq!(ctx_eval.get("value").and_then(Json::as_str), Some("1.5"));

        // 4. Insert formula with temperature unit °F and export copy to temp XMCD
        let inserted = call_tool(
            &mut server,
            7,
            "insert_region",
            Json::obj(vec![
                ("kind", Json::s("formula")),
                ("formula", Json::s("T := C(100) =")),
            ]),
        );
        let t_id = inserted
            .get("region")
            .and_then(|r| r.get("region_id"))
            .and_then(Json::as_str)
            .unwrap()
            .to_owned();
        let tf = call_tool(
            &mut server,
            8,
            "set_display_unit",
            Json::obj(vec![("region_id", Json::s(&t_id)), ("unit", Json::s("°F"))]),
        );
        assert_eq!(
            tf.get("region")
                .and_then(|r| r.get("value"))
                .and_then(Json::as_str),
            Some("212 °F")
        );

        let out_path =
            std::env::temp_dir().join(format!("list-rs-mcp-test-{}.xmcd", std::process::id()));
        let _ = std::fs::remove_file(&out_path);
        call_tool(
            &mut server,
            9,
            "export_xmcd",
            Json::obj(vec![("path", Json::s(out_path.display().to_string()))]),
        );
        assert!(out_path.exists());
        assert_eq!(server.doc.path.as_deref(), Some(out_path.as_path()));
        assert!(!server.doc.unsaved);
        assert!(!server.doc.sheet.edited(1));

        // Reopen exported XMCD via open_sheet
        let reopened = call_tool(
            &mut server,
            10,
            "open_sheet",
            Json::obj(vec![("path", Json::s(out_path.display().to_string()))]),
        );
        assert_eq!(
            reopened
                .get("summary")
                .and_then(|s| s.get("errors"))
                .and_then(Json::as_usize),
            Some(0)
        );
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn mcp_verifies_mathcad_fixture_snapshots() {
        let mut server = ready_server();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("Complex_numbers_variant_24_calculations.xmcd");
        let check = call_tool(
            &mut server,
            1,
            "verify_snapshots",
            Json::obj(vec![("path", Json::s(fixture.display().to_string()))]),
        );
        assert_eq!(check.get("ok").and_then(Json::as_bool), Some(true));
        assert_eq!(check.get("failed").and_then(Json::as_usize), Some(0));
    }
}
