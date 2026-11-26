use crate::editing::{
    ChartLine, ChartMarker, ChartOptions, ChartTraceStyle, LegendPlacement, PageSettings,
};
use crate::sheet::Sheet;
use crate::value::Calc;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub type CurveData = Vec<(String, Vec<Option<[f64; 2]>>)>;

pub struct ChartReport<'a> {
    pub curves: &'a Calc<CurveData>,
    pub options: &'a ChartOptions,
    pub is_polar: bool,
    pub title: &'a str,
}

pub struct Plot3dReport<'a> {
    pub grid: &'a Calc<crate::sheet::SurfaceGrid>,
    pub spec: &'a crate::editing::Plot3dSpec,
}

#[derive(Clone, Debug)]
pub struct PageSlice {
    pub number: usize,
    pub top_y: f32,
    pub bottom_y: f32,
    pub region_indices: Vec<usize>,
}

pub fn is_page_break_region(sheet: &Sheet, index: usize) -> bool {
    sheet.regions[index].is_page_break()
}

/// Splits visible regions of the sheet into printable pages.
/// Used identically by both the desktop Print Preview and the HTML/PDF exporter.
pub fn compute_pages(sheet: &Sheet, page_height: f32) -> Vec<PageSlice> {
    let page_height = page_height.clamp(300.0, 2400.0);
    let mut visible: Vec<(usize, f32, f32)> = sheet
        .regions
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.imported)
        .map(|(idx, r)| (idx, r.visual.top, r.visual.top + r.visual.height))
        .collect();
    visible.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));

    let mut pages = Vec::new();
    let mut current_top = 0.0f32;
    let mut current_indices = Vec::new();

    for (idx, top, bottom) in visible {
        if is_page_break_region(sheet, idx) {
            let cut = top.max(current_top + 1.0);
            pages.push(PageSlice {
                number: pages.len() + 1,
                top_y: current_top,
                bottom_y: cut,
                region_indices: std::mem::take(&mut current_indices),
            });
            current_top = bottom;
            continue;
        }
        while bottom - current_top > page_height && !current_indices.is_empty() {
            let cut = (current_top + page_height).min(top.max(current_top + 1.0));
            pages.push(PageSlice {
                number: pages.len() + 1,
                top_y: current_top,
                bottom_y: cut,
                region_indices: std::mem::take(&mut current_indices),
            });
            current_top = cut;
        }
        current_indices.push(idx);
    }

    if !current_indices.is_empty() || pages.is_empty() {
        let last_bottom = current_indices
            .iter()
            .map(|&idx| sheet.regions[idx].visual.top + sheet.regions[idx].visual.height)
            .fold(current_top + page_height, f32::max);
        pages.push(PageSlice {
            number: pages.len() + 1,
            top_y: current_top,
            bottom_y: last_bottom,
            region_indices: current_indices,
        });
    }
    pages
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn tick_str(n: f64) -> String {
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

fn write_chart_marker(
    svg: &mut String,
    marker: ChartMarker,
    (x, y): (f64, f64),
    radius: f64,
    color: &str,
) {
    let left = x - radius;
    let right = x + radius;
    let top = y - radius;
    let bottom = y + radius;
    match marker {
        ChartMarker::Circle => {
            let _ = write!(
                svg,
                r#"<circle cx="{x:.1}" cy="{y:.1}" r="{radius:.1}" fill="{color}"/>"#
            );
        }
        ChartMarker::Square => {
            let size = radius * 2.;
            let _ = write!(
                svg,
                r#"<rect x="{left:.1}" y="{top:.1}" width="{size:.1}" height="{size:.1}" fill="{color}"/>"#
            );
        }
        ChartMarker::Diamond => {
            let _ = write!(
                svg,
                r#"<polygon points="{x:.1},{top:.1} {right:.1},{y:.1} {x:.1},{bottom:.1} {left:.1},{y:.1}" fill="{color}"/>"#
            );
        }
        ChartMarker::Triangle => {
            let _ = write!(
                svg,
                r#"<polygon points="{x:.1},{top:.1} {right:.1},{bottom:.1} {left:.1},{bottom:.1}" fill="{color}"/>"#
            );
        }
        ChartMarker::Cross => {
            let _ = write!(
                svg,
                r#"<path d="M{left:.1},{top:.1} L{right:.1},{bottom:.1} M{right:.1},{top:.1} L{left:.1},{bottom:.1}" fill="none" stroke="{color}" stroke-width="1.5"/>"#
            );
        }
        ChartMarker::Auto | ChartMarker::None => {}
    }
}

fn render_chart_svg(chart: &ChartReport<'_>) -> String {
    const COLORS: [&str; 4] = ["#006f69", "#9c411f", "#4a4ea3", "#803974"];
    let w = 640.0f64;
    let h = 300.0f64;
    let Ok(curves) = chart.curves else {
        let err = chart.curves.as_ref().unwrap_err();
        return format!(
            "<div class=\"chart-err\">График ({}): {}</div>",
            escape_html(chart.title),
            escape_html(err)
        );
    };

    let options = chart.options;
    let format = &options.appearance;
    let title = format.title.trim();
    let top_title = if !title.is_empty() && !format.title_below {
        20.
    } else {
        0.
    };
    let bottom_title = if !title.is_empty() && format.title_below {
        20.
    } else {
        0.
    };
    let log_x = !chart.is_polar && options.log_x;
    let log_y = !chart.is_polar && options.log_y;
    let left = 62.0;
    let right = w - 20.0;
    let placement = options.show_legend.then_some(format.legend_placement);
    let top = if placement == Some(LegendPlacement::Above) {
        32.
    } else {
        16.
    } + top_title;
    let bottom_legend = if placement == Some(LegendPlacement::Below) {
        20.
    } else {
        0.
    };
    let bottom = if options.x_label.is_empty() {
        h - 32.0
    } else {
        h - 44.0
    } - bottom_title
        - bottom_legend;
    let gw = right - left;
    let gh = bottom - top;

    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for pt in curves.iter().flat_map(|(_, pts)| pts.iter().flatten()) {
        if let (Some(mx), Some(my)) = (map_coord(pt[0], log_x), map_coord(pt[1], log_y)) {
            bounds[0][0] = bounds[0][0].min(mx);
            bounds[0][1] = bounds[0][1].max(mx);
            bounds[1][0] = bounds[1][0].min(my);
            bounds[1][1] = bounds[1][1].max(my);
        }
    }
    if !bounds[0][0].is_finite() || !bounds[1][0].is_finite() {
        return "<div class=\"chart-err\">Нет точек для отображения графика</div>".into();
    }
    for (axis, (man_min, man_max, is_log)) in bounds.iter_mut().zip([
        (options.x_min, options.x_max, log_x),
        (options.y_min, options.y_max, log_y),
    ]) {
        let step = if (axis[1] - axis[0]).abs() < 1e-12 {
            if is_log {
                0.5
            } else {
                (axis[0].abs() * 0.1).max(1.0)
            }
        } else {
            (axis[1] - axis[0]) * 0.05
        };
        axis[0] -= step;
        axis[1] += step;
        if let Some(v) = man_min.and_then(|v| map_coord(v, is_log)) {
            axis[0] = v;
        }
        if let Some(v) = man_max.and_then(|v| map_coord(v, is_log)) {
            axis[1] = v;
        }
        if axis[0] >= axis[1] {
            axis[1] = axis[0] + 1.0;
        }
    }
    if (options.equal_scale || chart.is_polar) && !log_x && !log_y {
        let cx = 0.5 * (bounds[0][0] + bounds[0][1]);
        let cy = 0.5 * (bounds[1][0] + bounds[1][1]);
        let sx = gw / (bounds[0][1] - bounds[0][0]);
        let sy = gh / (bounds[1][1] - bounds[1][0]);
        let s = sx.min(sy);
        let hx = gw / (2.0 * s);
        let hy = gh / (2.0 * s);
        bounds[0] = [cx - hx, cx + hx];
        bounds[1] = [cy - hy, cy + hy];
    }

    let [[xmin, xmax], [ymin, ymax]] = bounds;
    let project = |[x, y]: [f64; 2]| -> Option<(f64, f64)> {
        let mx = map_coord(x, log_x)?;
        let my = map_coord(y, log_y)?;
        let px = left + ((mx - xmin) / (xmax - xmin)) * gw;
        let py = bottom - ((my - ymin) / (ymax - ymin)) * gh;
        Some((px, py))
    };

    let mut svg = String::with_capacity(4096);
    let _ = write!(
        svg,
        "<svg class=\"chart-svg\" viewBox=\"0 0 {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">"
    );
    let _ = write!(
        svg,
        "<rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"6\" fill=\"#ffffff\" stroke=\"#dae4e8\"/>",
        w - 1.0,
        h - 1.0
    );
    for axis in 0..2 {
        let count = format.divisions[axis].clamp(1, 20);
        for i in 0..=count {
            let r = f64::from(i) / f64::from(count);
            let (x, y, align, value) = if axis == 0 {
                let x = left + r * gw;
                if format.grid[axis] {
                    let _ = write!(svg, "<line x1=\"{x:.1}\" y1=\"{top:.1}\" x2=\"{x:.1}\" y2=\"{bottom:.1}\" stroke=\"#e5edf0\"/>");
                }
                (
                    x,
                    bottom + 14.,
                    "middle",
                    unmap_coord(xmin + (xmax - xmin) * r, log_x),
                )
            } else {
                let y = bottom - r * gh;
                if format.grid[axis] {
                    let _ = write!(svg, "<line x1=\"{left:.1}\" y1=\"{y:.1}\" x2=\"{right:.1}\" y2=\"{y:.1}\" stroke=\"#e5edf0\"/>");
                }
                (
                    left - 6.,
                    y + 3.5,
                    "end",
                    unmap_coord(ymin + (ymax - ymin) * r, log_y),
                )
            };
            if format.numbers[axis] {
                let _ = write!(svg, "<text x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"{align}\" font-size=\"10\" fill=\"#2f4651\">{}</text>", escape_html(&format.tick(value)));
            }
        }
    }
    if !title.is_empty() {
        let y = if format.title_below { h - 6. } else { 15. };
        let _ = write!(svg, "<text x=\"{}\" y=\"{y}\" text-anchor=\"middle\" font-size=\"13\" fill=\"#2f4651\">{}</text>", w / 2., escape_html(title));
    }
    let [at_x, at_y] = format.axes.positions(bounds, [log_x, log_y]);
    if let Some(x) = at_x {
        let x = left + x * gw;
        let _ = write!(
            svg,
            "<line x1=\"{x:.1}\" y1=\"{top:.1}\" x2=\"{x:.1}\" y2=\"{bottom:.1}\" stroke=\"#90a2aa\"/>"
        );
    }
    if let Some(y) = at_y {
        let y = bottom - y * gh;
        let _ = write!(
            svg,
            "<line x1=\"{left:.1}\" y1=\"{y:.1}\" x2=\"{right:.1}\" y2=\"{y:.1}\" stroke=\"#90a2aa\"/>"
        );
    }
    if !options.x_label.is_empty() {
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#2f4651\">{}</text>",
            left + gw * 0.5,
            h - 6.0 - bottom_title - bottom_legend,
            escape_html(&options.x_label)
        );
    }
    if !options.y_label.is_empty() {
        let _ = write!(
            svg,
            "<text x=\"8\" y=\"{:.1}\" font-size=\"11\" fill=\"#2f4651\">{}</text>",
            top - 4.0,
            escape_html(&options.y_label)
        );
    }

    let step_x = gw / curves.len().max(1) as f64;
    let corner = placement.and_then(|p| p.corner_box([left, top, right, bottom], curves.len()));
    let mut legend = String::new();
    if let Some(panel) = &corner {
        let _ = write!(legend,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"3\" fill=\"#fff\" fill-opacity=\"0.96\" stroke=\"#e5edf0\"/>",
            panel.x, panel.y, panel.width, panel.height
        );
    }
    let default_style = ChartTraceStyle::default();
    for (s_idx, (name, pts)) in curves.iter().enumerate() {
        let style = format.traces.get(s_idx).unwrap_or(&default_style);
        let marker = style.marker_shape(options.markers);
        let diameter = f64::from(style.marker_diameter(4.8));
        let legacy =
            style.marker == ChartMarker::Auto && style.marker_every == 0 && style.marker_size == 0;
        let custom = style
            .color
            .map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"));
        let color = custom.as_deref().unwrap_or(COLORS[s_idx % COLORS.len()]);
        let label = if style.label.trim().is_empty() {
            name
        } else {
            &style.label
        };
        let dash = if style.line == ChartLine::Dashed {
            r#" stroke-dasharray="7 4""#
        } else {
            ""
        };
        let legend_row = if let Some(panel) = &corner {
            Some((
                panel.x + 5.,
                panel.row_y(s_idx, curves.len()),
                panel.width - 10.,
            ))
        } else {
            match placement {
                Some(LegendPlacement::Above) => {
                    Some((left + s_idx as f64 * step_x, 10. + top_title, step_x))
                }
                Some(LegendPlacement::Below) => {
                    Some((left + s_idx as f64 * step_x, h - bottom_title - 17., step_x))
                }
                _ => None,
            }
        };
        if let Some((lx, ly, width)) = legend_row {
            let swatch = ly + 6.;
            if style.line != ChartLine::Points {
                let width = style.line_width(2.);
                let _ = write!(legend,
                    "<line x1=\"{lx:.1}\" y1=\"{swatch}\" x2=\"{:.1}\" y2=\"{swatch}\" stroke=\"{color}\" stroke-width=\"{width}\"{dash}/>",
                    lx + 14.
                );
            }
            if let Some(marker) = marker {
                write_chart_marker(
                    &mut legend,
                    marker,
                    (lx + 7., swatch),
                    diameter.min(12.) / 2.,
                    color,
                );
            }
            let _ = write!(legend,
                "<svg x=\"{:.1}\" y=\"{ly:.1}\" width=\"{:.1}\" height=\"15\" overflow=\"hidden\"><text x=\"0\" y=\"10\" font-size=\"11\" fill=\"{color}\">{}</text></svg>",
                lx + 18.,
                (width - 18.).max(0.),
                escape_html(label)
            );
        }
        let mut run = Vec::new();
        let mut point_index = 0;
        let auto_stride = if marker.is_some() && !legacy && style.marker_every == 0 {
            pts.iter().flatten().count() / 60
        } else {
            1
        };
        let stride = style.marker_stride(auto_stride);
        for pt in pts.iter().copied().chain(std::iter::once(None)) {
            if let Some(val) = pt.and_then(project) {
                run.push(val);
                continue;
            }
            if run.len() > 1 && style.line != ChartLine::Points {
                let mut poly = String::new();
                for (i, &(x, y)) in run.iter().enumerate() {
                    if i > 0 {
                        poly.push(' ');
                    }
                    let _ = write!(poly, "{x:.1},{y:.1}");
                }
                let _ = write!(
                    svg,
                    "<polyline fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\"{dash} points=\"{poly}\"/>", style.line_width(1.8)
                );
            }
            if legacy && run.len() == 1 {
                write_chart_marker(&mut svg, ChartMarker::Circle, run[0], 3., color);
            } else if let Some(marker) = marker {
                let (skip, step) = if legacy {
                    (0, style.marker_stride(run.len() / 40))
                } else {
                    ((stride - point_index % stride) % stride, stride)
                };
                for &point in run.iter().skip(skip).step_by(step) {
                    write_chart_marker(&mut svg, marker, point, diameter / 2., color);
                }
            }
            point_index += run.len();
            run.clear();
        }
    }
    svg.push_str(&legend);
    svg.push_str("</svg>");
    svg
}

fn height_color_hex(t: f32) -> String {
    let t = t.clamp(0.0, 1.0);
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
            return format!("#{r:02x}{g:02x}{b:02x}");
        }
    }
    "#ba3a2c".into()
}

fn render_plot3d_svg(plot: &Plot3dReport<'_>) -> String {
    use crate::editing::Plot3dMode;
    let title = plot.spec.title();
    let Ok(grid) = plot.grid else {
        let err = plot.grid.as_ref().unwrap_err();
        return format!(
            "<div class=\"chart-err\">3D-график ({}): {}</div>",
            escape_html(&title),
            escape_html(err)
        );
    };
    let w = 640.0f32;
    let h = 320.0f32;
    let z_span = (grid.z_max - grid.z_min).max(1e-12);
    let norm_z = |z: f64| ((z - grid.z_min) / z_span) as f32;

    let mut svg = String::with_capacity(8192);
    let _ = write!(
        svg,
        "<svg class=\"chart-svg\" viewBox=\"0 0 {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"6\" fill=\"#ffffff\" stroke=\"#dae4e8\"/>\
         <text x=\"12\" y=\"19\" font-size=\"12\" fill=\"#1a2a36\">{}</text>",
        w - 1.0,
        h - 1.0,
        escape_html(&title)
    );

    let rows = grid.rows;
    let cols = grid.cols;
    match plot.spec.mode {
        Plot3dMode::Surface => {
            let yaw = plot.spec.yaw_deg.to_radians();
            let pitch = plot.spec.pitch_deg.to_radians();
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let cx = w * 0.5;
            let cy_pos = h * 0.52;
            let scale = h * 0.34;
            let project = |x: f32, y: f32, z: f32| -> ((f32, f32), f32) {
                let rx = x * cy - y * sy;
                let ry = x * sy + y * cy;
                let py = ry * sp + z * cp;
                let depth = ry * cp - z * sp;
                ((cx + rx * scale, cy_pos - py * scale), depth)
            };
            let mut projected = vec![None; rows * cols];
            for r in 0..rows {
                let ny = -1.0 + 2.0 * (r as f32 / (rows - 1) as f32);
                for c in 0..cols {
                    let nx = -1.0 + 2.0 * (c as f32 / (cols - 1) as f32);
                    if let Some(z) = grid.values[r * cols + c] {
                        let tz = norm_z(z);
                        let nz = -0.7 + 1.4 * tz;
                        let (pt, depth) = project(nx, ny, nz);
                        projected[r * cols + c] = Some((pt, depth, tz));
                    }
                }
            }
            let mut quads: Vec<(f32, [(f32, f32); 4], f32)> = Vec::new();
            for r in 0..rows - 1 {
                for c in 0..cols - 1 {
                    let (
                        Some((p00, d00, t00)),
                        Some((p10, d10, t10)),
                        Some((p11, d11, t11)),
                        Some((p01, d01, t01)),
                    ) = (
                        projected[r * cols + c],
                        projected[r * cols + c + 1],
                        projected[(r + 1) * cols + c + 1],
                        projected[(r + 1) * cols + c],
                    )
                    else {
                        continue;
                    };
                    quads.push((
                        0.25 * (d00 + d10 + d11 + d01),
                        [p00, p10, p11, p01],
                        0.25 * (t00 + t10 + t11 + t01),
                    ));
                }
            }
            quads.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, pts, tz) in quads {
                let fill = height_color_hex(tz);
                let _ = write!(
                    svg,
                    "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{fill}\" stroke=\"#1a2a36\" stroke-opacity=\"0.35\" stroke-width=\"0.6\"/>",
                    pts[0].0, pts[0].1, pts[1].0, pts[1].1, pts[2].0, pts[2].1, pts[3].0, pts[3].1
                );
            }
        }
        Plot3dMode::Contour => {
            let left = 44.0f32;
            let top = 30.0f32;
            let pw = w - 84.0;
            let ph = h - 58.0;
            let cw = pw / (cols - 1) as f32;
            let ch = ph / (rows - 1) as f32;
            for r in 0..rows - 1 {
                let y_bot = top + ph - r as f32 * ch;
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
                    let fill = height_color_hex(norm_z(0.25 * (z00 + z10 + z11 + z01)));
                    let x_left = left + c as f32 * cw;
                    let _ = write!(
                        svg,
                        "<rect x=\"{x_left:.1}\" y=\"{y_top:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{fill}\"/>",
                        cw + 0.5,
                        ch + 0.5
                    );
                }
            }
            let _ = write!(
                svg,
                "<rect x=\"{left:.1}\" y=\"{top:.1}\" width=\"{pw:.1}\" height=\"{ph:.1}\" fill=\"none\" stroke=\"#4f6571\"/>"
            );
        }
    }
    let _ = write!(
        svg,
        "<text x=\"12\" y=\"{:.1}\" font-size=\"10.5\" fill=\"#4f6571\">z ∈ [{}; {}]</text></svg>",
        h - 8.0,
        escape_html(&tick_str(grid.z_min)),
        escape_html(&tick_str(grid.z_max))
    );
    svg
}

pub fn render_html_report(
    title: &str,
    sheet: &Sheet,
    charts: &HashMap<usize, ChartReport<'_>>,
    settings: &PageSettings,
    precision: Option<usize>,
) -> String {
    let empty_images = HashMap::new();
    render_html_report_with_images(title, sheet, charts, &empty_images, settings, precision)
}

pub fn render_html_report_with_images(
    title: &str,
    sheet: &Sheet,
    charts: &HashMap<usize, ChartReport<'_>>,
    images: &HashMap<usize, crate::editing::ImageSpec>,
    settings: &PageSettings,
    precision: Option<usize>,
) -> String {
    let empty_3d = HashMap::new();
    render_html_report_full(title, sheet, charts, &empty_3d, images, settings, precision)
}

pub fn render_html_report_full(
    title: &str,
    sheet: &Sheet,
    charts: &HashMap<usize, ChartReport<'_>>,
    plots3d: &HashMap<usize, Plot3dReport<'_>>,
    images: &HashMap<usize, crate::editing::ImageSpec>,
    settings: &PageSettings,
    precision: Option<usize>,
) -> String {
    let pages = compute_pages(sheet, settings.page_height);
    let total_pages = pages.len();
    let mut html = String::with_capacity(8192);
    let _ = write!(
        html,
        r#"<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
@page {{ size: A4; margin: 14mm; }}
:root {{ font-family: "Segoe UI", system-ui, -apple-system, sans-serif; color: #1a2a36; background: #eaf0f2; }}
body {{ margin: 0; padding: 18px; }}
.toolbar {{ max-width: 794px; margin: 0 auto 14px; display: flex; justify-content: space-between; align-items: center; background: #fff; padding: 10px 16px; border: 1px solid #dae4e8; border-radius: 8px; }}
.toolbar button {{ background: #006861; color: #fff; border: none; border-radius: 5px; padding: 7px 14px; font-size: 13px; cursor: pointer; }}
.page {{ width: 794px; min-height: 1020px; margin: 0 auto 20px; background: #fff; border: 1px solid #dae4e8; border-radius: 6px; padding: 28px 38px; box-sizing: border-box; display: flex; flex-direction: column; justify-content: space-between; page-break-after: always; break-after: page; }}
.page:last-child {{ page-break-after: auto; break-after: auto; }}
.hdr, .ftr {{ font-size: 11px; color: #4f6571; display: flex; justify-content: space-between; border-color: #dae4e8; }}
.hdr {{ border-bottom: 1px solid #dae4e8; padding-bottom: 6px; margin-bottom: 14px; }}
.ftr {{ border-top: 1px solid #dae4e8; padding-top: 6px; margin-top: 14px; }}
.content {{ flex: 1; }}
h1 {{ font-size: 21px; margin: 8px 0 10px; color: #1a2a36; }}
h2 {{ font-size: 16px; margin: 8px 0 8px; color: #1a2a36; }}
p.txt {{ font-size: 13px; line-height: 1.45; margin: 6px 0; color: #364d59; white-space: pre-wrap; }}
.formula {{ font-family: "Cambria Math", "Segoe UI", serif; font-size: 14.5px; margin: 7px 0; padding: 4px 8px; background: #f8fbfb; border-left: 3px solid #006861; border-radius: 3px; }}
.formula .res {{ color: #006861; font-weight: 600; }}
.formula .err {{ color: #a12921; }}
.chart-wrap {{ margin: 12px 0; }}
.chart-svg {{ width: 100%; max-width: 660px; height: auto; display: block; }}
.chart-err {{ padding: 10px; border: 1px solid #e7b8b4; background: #fdeeed; color: #a12921; border-radius: 5px; font-size: 12px; }}
@media print {{
  body {{ background: #fff; padding: 0; }}
  .no-print {{ display: none !important; }}
  .page {{ width: auto; min-height: 0; margin: 0; border: none; border-radius: 0; padding: 0; }}
}}
</style>
</head>
<body>
<div class="toolbar no-print">
  <span><strong>{title}</strong> · страниц: {total_pages}</span>
  <button onclick="window.print()">Печать / Сохранить в PDF</button>
</div>
"#,
        title = escape_html(title)
    );

    let header_text = if settings.header.trim().is_empty() {
        title
    } else {
        settings.header.trim()
    };

    for page in &pages {
        let _ = write!(
            html,
            "<section class=\"page\" data-page=\"{}\">",
            page.number
        );
        let _ = write!(
            html,
            "<header class=\"hdr\"><span>{}</span><span></span></header><div class=\"content\">",
            escape_html(header_text)
        );
        for &idx in &page.region_indices {
            let region = &sheet.regions[idx];
            if let Some(chart) = charts.get(&idx) {
                let _ = write!(
                    html,
                    "<div class=\"chart-wrap\">{}</div>",
                    render_chart_svg(chart)
                );
                continue;
            }
            if let Some(plot3d) = plots3d.get(&idx) {
                let _ = write!(
                    html,
                    "<div class=\"chart-wrap\">{}</div>",
                    render_plot3d_svg(plot3d)
                );
                continue;
            }
            if let Some(img) = images.get(&idx) {
                let _ = write!(
                    html,
                    "<div class=\"chart-wrap\"><img src=\"data:{};base64,{}\" alt=\"{}\" style=\"max-width:100%;height:auto;border:1px solid #dae4e8;border-radius:5px;\"/></div>",
                    escape_html(&img.mime),
                    img.data_base64,
                    escape_html(&img.name)
                );
                continue;
            }
            if let Some(text) = &region.text {
                if region.visual.graphical {
                    let _ = write!(html, "<div class=\"chart-err\">{}</div>", escape_html(text));
                } else if let Some(rich) = &region.visual.rich_text {
                    let tag = match region.visual.heading {
                        1 => "h1",
                        2 => "h2",
                        _ => "p",
                    };
                    let _ = write!(
                        html,
                        "<{tag} class=\"txt\" style=\"white-space:pre-wrap;text-align:{}\">",
                        rich.alignment.name()
                    );
                    for (text, format) in rich.runs() {
                        let font = format.font.name();
                        let size = format.size;
                        let weight = if format.bold { "bold" } else { "normal" };
                        let slant = if format.italic { "italic" } else { "normal" };
                        let underline = if format.underline {
                            "underline"
                        } else {
                            "none"
                        };
                        let _ = write!(
                            html,
                            "<span style=\"font-family:'{font}';font-size:{size}pt;font-weight:{weight};font-style:{slant};text-decoration:{underline}\">{}</span>",
                            escape_html(text)
                        );
                    }
                    let _ = write!(html, "</{tag}>");
                } else if let Some(format) = region.visual.text_format {
                    let (tag, class) = match region.visual.heading {
                        1 => ("h1", ""),
                        2 => ("h2", ""),
                        _ => ("p", "txt"),
                    };
                    let font = format.font.name();
                    let size = format.size;
                    let weight = if format.bold { "bold" } else { "normal" };
                    let slant = if format.italic { "italic" } else { "normal" };
                    let underline = if format.underline {
                        "underline"
                    } else {
                        "none"
                    };
                    let alignment = format.alignment.name();
                    let _ = write!(
                        html,
                        "<{tag} class=\"{class}\" style=\"font-family:'{font}';font-size:{size}pt;font-weight:{weight};font-style:{slant};text-decoration:{underline};text-align:{alignment};white-space:pre-wrap\">{}</{tag}>",
                        escape_html(text)
                    );
                } else {
                    match region.visual.heading {
                        1 => {
                            let _ = write!(html, "<h1>{}</h1>", escape_html(text));
                        }
                        2 => {
                            let _ = write!(html, "<h2>{}</h2>", escape_html(text));
                        }
                        _ => {
                            let _ = write!(html, "<p class=\"txt\">{}</p>", escape_html(text));
                        }
                    }
                }
                continue;
            }
            let formula = escape_html(&region.visual.formula);
            match sheet.display_with_precision(idx, precision) {
                Ok(_) if region.visual.function => {
                    let _ = write!(html, "<div class=\"formula\">{formula}</div>");
                }
                Ok(val) if region.visual.formula.ends_with('→') => {
                    let _ = write!(
                        html,
                        "<div class=\"formula\">{formula} <span class=\"res\">{}</span></div>",
                        escape_html(&val)
                    );
                }
                Ok(val) => {
                    let _ = write!(
                        html,
                        "<div class=\"formula\">{formula} <span class=\"res\">= {}</span></div>",
                        escape_html(&val)
                    );
                }
                Err(err) => {
                    let _ = write!(
                        html,
                        "<div class=\"formula\">{formula} <span class=\"err\">· {}</span></div>",
                        escape_html(&err)
                    );
                }
            }
        }
        let page_num = if settings.show_page_numbers {
            format!("Стр. {} из {total_pages}", page.number)
        } else {
            String::new()
        };
        let _ = write!(
            html,
            "</div><footer class=\"ftr\"><span>{}</span><span>{}</span></footer></section>\n",
            escape_html(settings.footer.trim()),
            escape_html(&page_num)
        );
    }

    html.push_str("</body>\n</html>\n");
    html
}

pub fn write_html_report(path: &Path, html: &str) -> Calc<()> {
    std::fs::write(path, html.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn export_pdf_via_browser(html_content: &str, pdf_path: &Path) -> Calc<()> {
    static NEXT_JOB: AtomicU64 = AtomicU64::new(0);
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("list-rs-report-{}-{job}", std::process::id()));
    std::fs::create_dir(&dir)
        .map_err(|error| format!("Не удалось создать папку отчёта: {error}"))?;
    let result = print_pdf(html_content, pdf_path, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn print_pdf(html_content: &str, pdf_path: &Path, dir: &Path) -> Calc<()> {
    let html = dir.join("report.html");
    let output = dir.join("report.pdf");
    write_html_report(&html, html_content)?;
    let mut file_url = String::from(if cfg!(windows) { "file:///" } else { "file://" });
    for byte in html.to_string_lossy().replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"/:-._~".contains(&byte) {
            file_url.push(char::from(byte));
        } else {
            let _ = write!(file_url, "%{byte:02X}");
        }
    }
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "chrome",
        "msedge",
        "chromium",
    ];
    let mut browser_started = false;
    let mut timed_out = false;
    let deadline = Instant::now() + Duration::from_secs(45);
    for (index, bin) in candidates.into_iter().enumerate() {
        if Instant::now() >= deadline {
            break;
        }
        let _ = std::fs::remove_file(&output);
        let mut command = Command::new(bin);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let Ok(mut child) = command
            .args([
                "--headless",
                "--disable-gpu",
                "--disable-background-networking",
                "--disable-extensions",
                "--no-first-run",
                "--no-pdf-header-footer",
            ])
            .arg(format!(
                "--user-data-dir={}",
                dir.join(format!("browser-{index}")).display()
            ))
            .arg(format!("--print-to-pdf={}", output.display()))
            .arg(&file_url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        browser_started = true;
        let attempt_deadline = deadline.min(Instant::now() + Duration::from_secs(25));
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    if let Ok(bytes) = std::fs::read(&output) {
                        if bytes.starts_with(b"%PDF-") && bytes.trim_ascii_end().ends_with(b"%%EOF")
                        {
                            return std::fs::write(pdf_path, bytes).map_err(|error| {
                                format!("Не удалось сохранить {}: {error}", pdf_path.display())
                            });
                        }
                    }
                    break;
                }
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < attempt_deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Не удалось проверить завершение браузера: {error}"));
                }
            }
        }
    }
    if timed_out {
        Err("Браузер не завершил экспорт PDF вовремя. Сохраните HTML-отчёт и распечатайте его из браузера.".into())
    } else if browser_started {
        Err("Браузер не создал PDF. Сохраните HTML-отчёт и распечатайте его из браузера.".into())
    } else {
        Err("Не найден Edge или Chrome для прямой печати в PDF. Используйте «Экспорт отчёта (HTML / Печать)…» и сохраните PDF через окно печати.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::{insert_region, ChartSpec, NewRegion, TextStyle};

    #[test]
    fn chart_markers_use_per_trace_shape_size_and_phase_across_gaps() {
        let curves = Ok(vec![
            (
                "Измерения".into(),
                vec![
                    Some([0., 1.]),
                    Some([1., 2.]),
                    None,
                    Some([2., 0.]),
                    Some([3., 1.]),
                    Some([4., 2.]),
                ],
            ),
            ("Одиночная точка".into(), vec![Some([2., 1.])]),
        ]);
        let mut options = ChartOptions::default();
        options.markers = true;
        options.appearance.traces[0] = ChartTraceStyle {
            line: ChartLine::Points,
            marker: ChartMarker::Square,
            marker_size: 14,
            marker_every: 3,
            color: Some([17, 22, 30]),
            ..ChartTraceStyle::default()
        };
        options.appearance.traces[1].marker = ChartMarker::None;
        let svg = render_chart_svg(&ChartReport {
            curves: &curves,
            options: &options,
            is_polar: false,
            title: "Измерения",
        });
        let document = roxmltree::Document::parse(&svg).unwrap();
        let squares: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name("rect") && node.attribute("fill") == Some("#11161e"))
            .collect();
        assert_eq!(squares.len(), 3);
        assert_eq!(squares[0].attribute("width"), Some("14.0"));
        assert_eq!(squares[0].attribute("x"), Some("80.4")); // X = 0
        assert_eq!(squares[1].attribute("width"), Some("14.0"));
        assert_eq!(squares[1].attribute("x"), Some("460.8")); // X = 3
        assert_eq!(squares[2].attribute("width"), Some("12.0")); // legend swatch
        assert!(!svg.contains("<polyline"));
        assert!(!svg.contains("<circle"));
    }

    #[test]
    fn multipage_cyrillic_report_matches_preview_pages_and_embeds_svg_chart() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, _) = insert_region(
            blank,
            12.,
            20.,
            NewRegion::StyledText("Расчёт балки на изгиб", TextStyle::Heading1),
        )
        .unwrap();
        let (xml, _) = insert_region(&xml, 12., 70., NewRegion::Formula("F := 10 * kN")).unwrap();
        let (xml, _) = insert_region(&xml, 12., 120., NewRegion::PageBreak).unwrap();
        let spec = ChartSpec::Function {
            expression: "sin(x)".into(),
            from: 0.,
            to: 3.14,
        };
        let (xml, _) = insert_region(&xml, 12., 160., NewRegion::Chart(&spec)).unwrap();

        let mut sheet = Sheet::import(&xml).unwrap();
        sheet.full();
        let settings = PageSettings {
            header: "Отчёт по прочности".into(),
            footer: "Лист · 2026".into(),
            show_page_numbers: true,
            page_height: 840.0,
        };
        let pages = compute_pages(&sheet, settings.page_height);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].region_indices.len(), 2);
        assert_eq!(pages[1].region_indices.len(), 1);

        let plot_idx = pages[1].region_indices[0];
        let curves = sheet.chart_curves(plot_idx, &spec);
        let mut opts = ChartOptions {
            x_min: Some(-1000.),
            x_max: Some(1000.),
            ..ChartOptions::default()
        };
        opts.appearance.exponential_threshold = Some(3);
        let mut charts = HashMap::new();
        charts.insert(
            plot_idx,
            ChartReport {
                curves: &curves,
                options: &opts,
                is_polar: false,
                title: "sin(x)",
            },
        );
        let html = render_html_report("Балка", &sheet, &charts, &settings, Some(4));
        assert!(html.contains("Расчёт балки на изгиб"));
        assert!(html.contains("Отчёт по прочности"));
        assert!(html.contains("Стр. 1 из 2"));
        assert!(html.contains("Стр. 2 из 2"));
        assert!(html.contains("<svg class=\"chart-svg\""));
        assert!(html.contains("<polyline"));
        assert!(html.contains(">-1e3</text>"));
        assert!(html.contains(">1e3</text>"));
    }

    #[test]
    fn report_embeds_3d_surface_svg_and_formats_symbolic_without_equals() {
        let blank = include_str!("../examples/blank.xmcd");
        let (xml, _) =
            insert_region(blank, 12., 20., NewRegion::Formula("2 * x + 3 * x ->")).unwrap();
        let spec3d = crate::editing::Plot3dSpec::default_surface();
        let (xml, _) = insert_region(&xml, 12., 70., NewRegion::Plot3d(&spec3d)).unwrap();
        let mut sheet = Sheet::import(&xml).unwrap();
        sheet.full();
        let grid = sheet.surface_grid(1, &spec3d);
        let mut plots3d = HashMap::new();
        plots3d.insert(
            1,
            Plot3dReport {
                grid: &grid,
                spec: &spec3d,
            },
        );
        let html = render_html_report_full(
            "Тест 3D",
            &sheet,
            &HashMap::new(),
            &plots3d,
            &HashMap::new(),
            &PageSettings::default(),
            None,
        );
        assert!(html.contains("<polygon"));
        assert!(!html.contains("→ <span class=\"res\">= "));
        assert!(html.contains("5·x"));
    }
}
