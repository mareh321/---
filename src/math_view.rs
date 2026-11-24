use eframe::egui::{
    self, vec2, Color32, Event, FontId, Key, Painter, Pos2, Rect, Stroke, StrokeKind,
};
use list_rs::math_editor::{
    BigOpKind, Cursor, MathEditor, MathItem, MathSlot, SlotPart, SubscriptKind,
};

#[derive(Clone, Copy)]
pub struct Style {
    pub font_size: f32,
    pub ink: Color32,
    pub accent: Color32,
    pub muted: Color32,
    pub index_color: Color32,
    pub placeholder_bg: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

impl Metrics {
    pub fn height(self) -> f32 {
        self.ascent + self.descent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteTab {
    Operators,
    Plots,
    Matrix,
    Calculus,
    Symbolic,
    Program,
    Greek,
    Logic,
}

#[derive(Clone, Copy, Debug)]
pub enum PaletteAction {
    Fraction,
    Power,
    LiteralSub,
    IndexSub,
    Sqrt,
    Abs,
    BigOp(BigOpKind),
    Derivative,
    Program(usize),
    AddProgramLine,
    Matrix(usize, usize),
    Call(&'static str),
    Raw(&'static str),
}

impl PaletteAction {
    pub fn apply(&self, editor: &mut MathEditor) {
        match *self {
            Self::Fraction => editor.insert_fraction(),
            Self::Power => editor.insert_power(),
            Self::LiteralSub => editor.insert_subscript(SubscriptKind::Literal),
            Self::IndexSub => editor.insert_subscript(SubscriptKind::Index),
            Self::Sqrt => editor.insert_sqrt(),
            Self::Abs => editor.insert_abs(),
            Self::BigOp(k) => editor.insert_big_op(k),
            Self::Derivative => editor.insert_derivative(),
            Self::Program(n) => editor.insert_program(n),
            Self::AddProgramLine => editor.add_program_line(),
            Self::Matrix(r, c) => editor.insert_matrix(r, c),
            Self::Call(name) => editor.insert_call_template(name),
            Self::Raw(text) => editor.insert_raw(text),
        }
    }
}

struct HitTarget {
    path: Vec<(usize, SlotPart)>,
    offset: usize,
    caret_x: f32,
    rect: Rect,
}

fn sub_font(size: f32, scale: f32) -> f32 {
    (size * scale).max(9.0)
}

fn char_display(c: char) -> char {
    match c {
        '*' => '·',
        '-' => '−',
        _ => c,
    }
}

struct MatrixLayout {
    col_widths: Vec<f32>,
    row_ascents: Vec<f32>,
    row_descents: Vec<f32>,
    total_width: f32,
    total_height: f32,
}

fn measure_matrix(
    painter: &Painter,
    rows: usize,
    cols: usize,
    cells: &[MathSlot],
    font_size: f32,
    interactive: bool,
) -> MatrixLayout {
    let inner = sub_font(font_size, 0.90);
    let mut col_widths = vec![(inner * 0.9).max(12.0); cols];
    let mut row_ascents = vec![inner * 0.78; rows];
    let mut row_descents = vec![inner * 0.24; rows];
    for r in 0..rows {
        for c in 0..cols {
            if let Some(cell) = cells.get(r * cols + c) {
                let m = measure_slot(painter, cell, inner, interactive);
                col_widths[c] = col_widths[c].max(m.width);
                row_ascents[r] = row_ascents[r].max(m.ascent);
                row_descents[r] = row_descents[r].max(m.descent);
            }
        }
    }
    let col_gap = 10.0;
    let row_gap = 5.0;
    let bracket_pad = 12.0;
    let content_w: f32 = col_widths.iter().sum::<f32>() + col_gap * cols.saturating_sub(1) as f32;
    let content_h: f32 = row_ascents
        .iter()
        .zip(&row_descents)
        .map(|(a, d)| a + d)
        .sum::<f32>()
        + row_gap * rows.saturating_sub(1) as f32;
    MatrixLayout {
        col_widths,
        row_ascents,
        row_descents,
        total_width: content_w + bracket_pad,
        total_height: content_h + 6.0,
    }
}

pub fn measure_slot(
    painter: &Painter,
    slot: &MathSlot,
    font_size: f32,
    interactive: bool,
) -> Metrics {
    if slot.items.is_empty() {
        return Metrics {
            width: if interactive {
                (font_size * 0.7).max(9.0)
            } else {
                font_size * 0.35
            },
            ascent: font_size * 0.78,
            descent: font_size * 0.24,
        };
    }
    let mut width = 0.0f32;
    let mut ascent = font_size * 0.78;
    let mut descent = font_size * 0.24;
    for item in &slot.items {
        let m = measure_item(painter, item, font_size, interactive);
        width += m.width;
        ascent = ascent.max(m.ascent);
        descent = descent.max(m.descent);
    }
    Metrics {
        width,
        ascent,
        descent,
    }
}

fn measure_item(painter: &Painter, item: &MathItem, font_size: f32, interactive: bool) -> Metrics {
    match item {
        MathItem::Char(c) => {
            let s = char_display(*c).to_string();
            let galley = painter.layout_no_wrap(s, FontId::proportional(font_size), Color32::BLACK);
            Metrics {
                width: galley.rect.width().max(font_size * 0.28),
                ascent: font_size * 0.78,
                descent: font_size * 0.24,
            }
        }
        MathItem::Fraction { num, den } => {
            let inner = sub_font(font_size, 0.90);
            let nm = measure_slot(painter, num, inner, interactive);
            let dm = measure_slot(painter, den, inner, interactive);
            let axis = font_size * 0.32;
            let gap = 3.0;
            Metrics {
                width: nm.width.max(dm.width) + 8.0,
                ascent: (axis + gap + nm.height()).max(font_size * 0.78),
                descent: (dm.height() + gap - axis).max(font_size * 0.35),
            }
        }
        MathItem::Power { base, exp } => {
            let bm = measure_slot(painter, base, font_size, interactive);
            let em = measure_slot(painter, exp, sub_font(font_size, 0.72), interactive);
            let lift = bm.ascent * 0.52;
            Metrics {
                width: bm.width + em.width + 1.5,
                ascent: bm.ascent.max(lift + em.ascent),
                descent: bm.descent,
            }
        }
        MathItem::Subscript { base, sub, .. } => {
            let bm = measure_slot(painter, base, font_size, interactive);
            let sm = measure_slot(painter, sub, sub_font(font_size, 0.72), interactive);
            let drop = font_size * 0.28;
            Metrics {
                width: bm.width + sm.width + 1.5,
                ascent: bm.ascent,
                descent: bm.descent.max(drop + sm.descent),
            }
        }
        MathItem::Sqrt(inner) => {
            let im = measure_slot(painter, inner, font_size, interactive);
            let radical_w = (font_size * 0.62).max(9.0);
            Metrics {
                width: radical_w + im.width + 4.0,
                ascent: im.ascent + 3.5,
                descent: im.descent,
            }
        }
        MathItem::Abs(inner) => {
            let im = measure_slot(painter, inner, font_size, interactive);
            Metrics {
                width: im.width + 10.0,
                ascent: im.ascent + 1.5,
                descent: im.descent + 1.5,
            }
        }
        MathItem::BigOp {
            kind,
            lower,
            upper,
            func,
        } => {
            let lim_f = sub_font(font_size, 0.70);
            let lm = measure_slot(painter, lower, lim_f, interactive);
            let um = measure_slot(painter, upper, lim_f, interactive);
            let fm = measure_slot(painter, func, font_size, interactive);
            let sym_g = painter.layout_no_wrap(
                kind.symbol().to_owned(),
                FontId::proportional(font_size * 1.45),
                Color32::BLACK,
            );
            let op_w = sym_g.rect.width().max(lm.width).max(um.width) + 6.0;
            let op_asc = font_size * 0.85 + um.height() + 2.0;
            let op_desc = font_size * 0.45 + lm.height() + 2.0;
            Metrics {
                width: op_w + 4.0 + fm.width,
                ascent: op_asc.max(fm.ascent),
                descent: op_desc.max(fm.descent),
            }
        }
        MathItem::Derivative { func, point } => {
            let frac_f = sub_font(font_size, 0.82);
            let pt_f = sub_font(font_size, 0.72);
            let fm = measure_slot(painter, func, font_size, interactive);
            let pm = measure_slot(painter, point, pt_f, interactive);
            let op_w = frac_f * 1.55 + 6.0;
            let op_asc = font_size * 0.32 + 2.0 + frac_f;
            let op_desc = frac_f + 2.0 - font_size * 0.32;
            Metrics {
                width: op_w + 4.0 + fm.width + 7.0 + pm.width,
                ascent: op_asc.max(fm.ascent),
                descent: op_desc.max(fm.descent).max(font_size * 0.28 + pm.descent),
            }
        }
        MathItem::Program { lines } => {
            let inner = sub_font(font_size, 0.92);
            let row_gap = 4.0;
            let mut max_w = (inner * 2.0).max(24.0);
            let mut total_h = 0.0f32;
            for (i, line) in lines.iter().enumerate() {
                let lm = measure_slot(painter, line, inner, interactive);
                max_w = max_w.max(lm.width);
                if i > 0 {
                    total_h += row_gap;
                }
                total_h += lm.height();
            }
            total_h = (total_h + 6.0).max(font_size * 1.2);
            let half = total_h * 0.5;
            let axis = font_size * 0.30;
            Metrics {
                width: max_w + 14.0,
                ascent: (half + axis).max(font_size * 0.78),
                descent: (half - axis).max(font_size * 0.24),
            }
        }
        MathItem::Matrix { rows, cols, cells } => {
            let layout = measure_matrix(painter, *rows, *cols, cells, font_size, interactive);
            let half = layout.total_height * 0.5;
            let axis = font_size * 0.30;
            Metrics {
                width: layout.total_width,
                ascent: (half + axis).max(font_size * 0.78),
                descent: (half - axis).max(font_size * 0.24),
            }
        }
    }
}

pub fn paint_static(painter: &Painter, origin: Pos2, slot: &MathSlot, style: Style) -> Metrics {
    let m = measure_slot(painter, slot, style.font_size, false);
    let baseline = origin.y + m.ascent;
    let mut hits = Vec::new();
    let mut path = Vec::new();
    paint_slot(
        painter,
        origin.x,
        baseline,
        slot,
        style.font_size,
        style,
        None,
        &mut path,
        &mut hits,
        false,
    );
    m
}

pub fn paint_editor(
    painter: &Painter,
    origin: Pos2,
    editor: &mut MathEditor,
    style: Style,
    click_pos: Option<Pos2>,
    show_caret: bool,
) -> Option<Rect> {
    let m = measure_slot(painter, &editor.root, style.font_size, true);
    let baseline = origin.y + m.ascent;
    let mut hits = Vec::new();
    let mut path = Vec::new();
    let cursor = show_caret.then_some(&editor.cursor);
    paint_slot(
        painter,
        origin.x,
        baseline,
        &editor.root,
        style.font_size,
        style,
        cursor,
        &mut path,
        &mut hits,
        true,
    );
    if let Some(pos) = click_pos {
        let mut best: Option<(&HitTarget, usize, f32)> = None;
        for target in &hits {
            if target.rect.expand(4.0).contains(pos) {
                let dist = (pos.x - target.caret_x).abs();
                let depth = target.path.len();
                let better = match best {
                    None => true,
                    Some((_, best_depth, best_dist)) => {
                        depth > best_depth || (depth == best_depth && dist < best_dist)
                    }
                };
                if better {
                    best = Some((target, depth, dist));
                }
            }
        }
        if let Some((target, _, _)) = best {
            editor.set_cursor(target.path.clone(), target.offset);
        }
    }
    hits.iter()
        .find(|target| target.path == editor.cursor.path && target.offset == editor.cursor.offset)
        .map(|target| target.rect)
}

#[allow(clippy::too_many_arguments)]
fn paint_slot(
    painter: &Painter,
    x: f32,
    baseline: f32,
    slot: &MathSlot,
    font_size: f32,
    style: Style,
    cursor: Option<&Cursor>,
    path: &mut Vec<(usize, SlotPart)>,
    hits: &mut Vec<HitTarget>,
    interactive: bool,
) {
    let active_here = cursor.is_some_and(|c| c.path == *path);
    let slot_metrics = measure_slot(painter, slot, font_size, interactive);
    let slot_rect = Rect::from_min_size(
        Pos2::new(x, baseline - slot_metrics.ascent),
        vec2(slot_metrics.width, slot_metrics.height()),
    );

    if interactive && slot.items.is_empty() {
        let box_rect = Rect::from_min_size(
            Pos2::new(x + 1.0, baseline - font_size * 0.72),
            vec2((slot_metrics.width - 2.0).max(6.0), font_size * 0.88),
        );
        let stroke_color = if active_here {
            style.accent
        } else {
            style.muted
        };
        painter.rect(
            box_rect,
            2.0,
            style.placeholder_bg,
            Stroke::new(1.0_f32, stroke_color),
            StrokeKind::Inside,
        );
        hits.push(HitTarget {
            path: path.clone(),
            offset: 0,
            caret_x: box_rect.center().x,
            rect: box_rect,
        });
        if active_here {
            draw_caret(
                painter,
                box_rect.left() + 2.5,
                baseline,
                font_size,
                style.accent,
            );
        }
        return;
    }

    let mut cur_x = x;
    if interactive {
        hits.push(HitTarget {
            path: path.clone(),
            offset: 0,
            caret_x: cur_x,
            rect: Rect::from_min_size(
                Pos2::new(cur_x - 3.0, slot_rect.top()),
                vec2(6.0, slot_rect.height()),
            ),
        });
    }
    if active_here && cursor.is_some_and(|c| c.offset == 0) {
        draw_caret(painter, cur_x, baseline, font_size, style.accent);
    }

    for (index, item) in slot.items.iter().enumerate() {
        let m = measure_item(painter, item, font_size, interactive);
        match item {
            MathItem::Char(c) => {
                let s = char_display(*c).to_string();
                let galley = painter.layout_no_wrap(s, FontId::proportional(font_size), style.ink);
                let top = baseline - font_size * 0.80;
                painter.galley(Pos2::new(cur_x, top), galley, style.ink);
                if interactive {
                    let mid = cur_x + m.width * 0.5;
                    hits.push(HitTarget {
                        path: path.clone(),
                        offset: index,
                        caret_x: cur_x,
                        rect: Rect::from_min_size(
                            Pos2::new(cur_x, slot_rect.top()),
                            vec2((mid - cur_x).max(1.0), slot_rect.height()),
                        ),
                    });
                    hits.push(HitTarget {
                        path: path.clone(),
                        offset: index + 1,
                        caret_x: cur_x + m.width,
                        rect: Rect::from_min_size(
                            Pos2::new(mid, slot_rect.top()),
                            vec2((cur_x + m.width - mid).max(1.0), slot_rect.height()),
                        ),
                    });
                }
            }
            MathItem::Fraction { num, den } => {
                let inner = sub_font(font_size, 0.90);
                let nm = measure_slot(painter, num, inner, interactive);
                let dm = measure_slot(painter, den, inner, interactive);
                let axis_y = baseline - font_size * 0.32;
                let gap = 3.0;
                let num_x = cur_x + (m.width - nm.width) * 0.5;
                let num_base = axis_y - gap - nm.descent;
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    num_x,
                    num_base,
                    num,
                    inner,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();

                painter.line_segment(
                    [
                        Pos2::new(cur_x + 2.0, axis_y),
                        Pos2::new(cur_x + m.width - 2.0, axis_y),
                    ],
                    Stroke::new(1.2_f32, style.ink),
                );

                let den_x = cur_x + (m.width - dm.width) * 0.5;
                let den_base = axis_y + gap + dm.ascent;
                path.push((index, SlotPart::Second));
                paint_slot(
                    painter,
                    den_x,
                    den_base,
                    den,
                    inner,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
            }
            MathItem::Power { base, exp } => {
                let bm = measure_slot(painter, base, font_size, interactive);
                let exp_font = sub_font(font_size, 0.72);
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    cur_x,
                    baseline,
                    base,
                    font_size,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();

                let exp_x = cur_x + bm.width + 1.0;
                let exp_base = baseline - bm.ascent * 0.52;
                path.push((index, SlotPart::Second));
                paint_slot(
                    painter,
                    exp_x,
                    exp_base,
                    exp,
                    exp_font,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
            }
            MathItem::Subscript { base, sub, kind } => {
                let bm = measure_slot(painter, base, font_size, interactive);
                let sub_f = sub_font(font_size, 0.72);
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    cur_x,
                    baseline,
                    base,
                    font_size,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();

                let mut sub_style = style;
                if *kind == SubscriptKind::Index {
                    sub_style.ink = style.index_color;
                }
                let sub_x = cur_x + bm.width + 1.0;
                let sub_base = baseline + font_size * 0.28;
                path.push((index, SlotPart::Second));
                paint_slot(
                    painter,
                    sub_x,
                    sub_base,
                    sub,
                    sub_f,
                    sub_style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
            }
            MathItem::Sqrt(inner) => {
                let im = measure_slot(painter, inner, font_size, interactive);
                let radical_w = (font_size * 0.62).max(9.0);
                let top_y = baseline - im.ascent - 2.0;
                let bot_y = baseline + im.descent * 0.65;
                let mid_y = baseline - im.ascent * 0.25;
                let stroke = Stroke::new(1.25_f32, style.ink);
                let pts = vec![
                    Pos2::new(cur_x + 1.0, mid_y),
                    Pos2::new(cur_x + radical_w * 0.28, mid_y - 1.5),
                    Pos2::new(cur_x + radical_w * 0.52, bot_y),
                    Pos2::new(cur_x + radical_w - 0.5, top_y),
                    Pos2::new(cur_x + radical_w + im.width + 2.5, top_y),
                ];
                painter.add(egui::Shape::line(pts, stroke));
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    cur_x + radical_w + 1.0,
                    baseline,
                    inner,
                    font_size,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
            }
            MathItem::Abs(inner) => {
                let im = measure_slot(painter, inner, font_size, interactive);
                let top_y = baseline - im.ascent - 1.0;
                let bot_y = baseline + im.descent + 1.0;
                let stroke = Stroke::new(1.3_f32, style.ink);
                painter.line_segment(
                    [Pos2::new(cur_x + 2.5, top_y), Pos2::new(cur_x + 2.5, bot_y)],
                    stroke,
                );
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    cur_x + 5.0,
                    baseline,
                    inner,
                    font_size,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
                let rx = cur_x + 5.0 + im.width + 2.5;
                painter.line_segment([Pos2::new(rx, top_y), Pos2::new(rx, bot_y)], stroke);
            }
            MathItem::BigOp {
                kind,
                lower,
                upper,
                func,
            } => {
                let lim_f = sub_font(font_size, 0.70);
                let lm = measure_slot(painter, lower, lim_f, interactive);
                let um = measure_slot(painter, upper, lim_f, interactive);
                let sym_g = painter.layout_no_wrap(
                    kind.symbol().to_owned(),
                    FontId::proportional(font_size * 1.45),
                    style.ink,
                );
                let op_w = sym_g.rect.width().max(lm.width).max(um.width) + 6.0;
                let center_x = cur_x + op_w * 0.5;
                // Upper limit
                let upper_x = center_x - um.width * 0.5;
                let upper_base = baseline - font_size * 0.85 - um.descent;
                path.push((index, SlotPart::Second));
                paint_slot(
                    painter,
                    upper_x,
                    upper_base,
                    upper,
                    lim_f,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
                // Big operator glyph
                painter.galley(
                    Pos2::new(
                        center_x - sym_g.rect.width() * 0.5,
                        baseline - font_size * 0.92,
                    ),
                    sym_g,
                    style.ink,
                );
                // Lower limit
                let lower_x = center_x - lm.width * 0.5;
                let lower_base = baseline + font_size * 0.45 + lm.ascent;
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    lower_x,
                    lower_base,
                    lower,
                    lim_f,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
                // Integrand / summand
                path.push((index, SlotPart::Third));
                paint_slot(
                    painter,
                    cur_x + op_w + 4.0,
                    baseline,
                    func,
                    font_size,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
            }
            MathItem::Derivative { func, point } => {
                let frac_f = sub_font(font_size, 0.82);
                let pt_f = sub_font(font_size, 0.72);
                let fm = measure_slot(painter, func, font_size, interactive);
                let op_w = frac_f * 1.55 + 6.0;
                let axis_y = baseline - font_size * 0.32;
                let cx = cur_x + op_w * 0.5;
                painter.text(
                    Pos2::new(cx, axis_y - 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    "d",
                    FontId::proportional(frac_f),
                    style.ink,
                );
                painter.line_segment(
                    [
                        Pos2::new(cur_x + 2.0, axis_y),
                        Pos2::new(cur_x + op_w - 2.0, axis_y),
                    ],
                    Stroke::new(1.15_f32, style.ink),
                );
                painter.text(
                    Pos2::new(cx, axis_y + 2.0),
                    egui::Align2::CENTER_TOP,
                    "dx",
                    FontId::proportional(frac_f),
                    style.ink,
                );
                let fx = cur_x + op_w + 4.0;
                path.push((index, SlotPart::First));
                paint_slot(
                    painter,
                    fx,
                    baseline,
                    func,
                    font_size,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
                let bar_x = fx + fm.width + 3.0;
                painter.line_segment(
                    [
                        Pos2::new(bar_x, baseline - fm.ascent - 1.0),
                        Pos2::new(bar_x, baseline + fm.descent + 3.0),
                    ],
                    Stroke::new(1.2_f32, style.muted),
                );
                path.push((index, SlotPart::Second));
                paint_slot(
                    painter,
                    bar_x + 3.5,
                    baseline + font_size * 0.28,
                    point,
                    pt_f,
                    style,
                    cursor,
                    path,
                    hits,
                    interactive,
                );
                path.pop();
            }
            MathItem::Program { lines } => {
                let inner_f = sub_font(font_size, 0.92);
                let row_gap = 4.0;
                let top_y = baseline - m.ascent + 1.0;
                let bot_y = baseline + m.descent - 1.0;
                let bar_x = cur_x + 3.5;
                // Signature Mathcad 15 vertical program bar
                painter.line_segment(
                    [Pos2::new(bar_x, top_y), Pos2::new(bar_x, bot_y)],
                    Stroke::new(2.2_f32, style.ink),
                );
                let mut row_top = top_y + 3.0;
                for (line_idx, line) in lines.iter().enumerate() {
                    let lm = measure_slot(painter, line, inner_f, interactive);
                    let row_base = row_top + lm.ascent;
                    path.push((index, SlotPart::Cell(line_idx)));
                    paint_slot(
                        painter,
                        cur_x + 10.0,
                        row_base,
                        line,
                        inner_f,
                        style,
                        cursor,
                        path,
                        hits,
                        interactive,
                    );
                    path.pop();
                    row_top += lm.height() + row_gap;
                }
            }
            MathItem::Matrix { rows, cols, cells } => {
                let inner_f = sub_font(font_size, 0.90);
                let layout = measure_matrix(painter, *rows, *cols, cells, font_size, interactive);
                let top_y = baseline - m.ascent + 1.0;
                let bot_y = top_y + layout.total_height - 2.0;
                let left_x = cur_x + 2.0;
                let right_x = cur_x + layout.total_width - 2.0;
                let hook = 4.0;
                let stroke = Stroke::new(1.25_f32, style.ink);
                // Left square bracket
                painter.add(egui::Shape::line(
                    vec![
                        Pos2::new(left_x + hook, top_y),
                        Pos2::new(left_x, top_y),
                        Pos2::new(left_x, bot_y),
                        Pos2::new(left_x + hook, bot_y),
                    ],
                    stroke,
                ));
                // Right square bracket
                painter.add(egui::Shape::line(
                    vec![
                        Pos2::new(right_x - hook, top_y),
                        Pos2::new(right_x, top_y),
                        Pos2::new(right_x, bot_y),
                        Pos2::new(right_x - hook, bot_y),
                    ],
                    stroke,
                ));

                let col_gap = 10.0;
                let row_gap = 5.0;
                let mut row_top = top_y + 3.0;
                for r in 0..*rows {
                    let row_base = row_top + layout.row_ascents[r];
                    let mut cell_x = left_x + 4.0;
                    for c in 0..*cols {
                        let cell_idx = r * *cols + c;
                        if let Some(cell) = cells.get(cell_idx) {
                            let cm = measure_slot(painter, cell, inner_f, interactive);
                            let centered_x = cell_x + (layout.col_widths[c] - cm.width) * 0.5;
                            path.push((index, SlotPart::Cell(cell_idx)));
                            paint_slot(
                                painter,
                                centered_x,
                                row_base,
                                cell,
                                inner_f,
                                style,
                                cursor,
                                path,
                                hits,
                                interactive,
                            );
                            path.pop();
                        }
                        cell_x += layout.col_widths[c] + col_gap;
                    }
                    row_top += layout.row_ascents[r] + layout.row_descents[r] + row_gap;
                }
            }
        }
        cur_x += m.width;
        if interactive {
            hits.push(HitTarget {
                path: path.clone(),
                offset: index + 1,
                caret_x: cur_x,
                rect: Rect::from_min_size(
                    Pos2::new(cur_x - 3.0, slot_rect.top()),
                    vec2(6.0, slot_rect.height()),
                ),
            });
        }
        if active_here && cursor.is_some_and(|c| c.offset == index + 1) {
            draw_caret(painter, cur_x, baseline, font_size, style.accent);
        }
    }
}

fn draw_caret(painter: &Painter, x: f32, baseline: f32, font_size: f32, color: Color32) {
    painter.line_segment(
        [
            Pos2::new(x, baseline - font_size * 0.78),
            Pos2::new(x, baseline + font_size * 0.20),
        ],
        Stroke::new(1.6_f32, color),
    );
}

pub fn handle_keyboard(ctx: &egui::Context, editor: &mut MathEditor) -> bool {
    let events = ctx.input(|i| i.events.clone());
    let mut changed = false;
    for event in events {
        match event {
            Event::Text(text) | Event::Paste(text) => {
                if !text.is_empty() {
                    editor.insert_text(&text);
                    changed = true;
                }
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if modifiers.ctrl || modifiers.command {
                    continue;
                }
                match key {
                    Key::ArrowLeft => editor.move_left(),
                    Key::ArrowRight => editor.move_right(),
                    Key::ArrowUp => editor.move_up(),
                    Key::ArrowDown => editor.move_down(),
                    Key::Tab => editor.move_next_slot(modifiers.shift),
                    Key::Backspace => {
                        editor.backspace();
                        changed = true;
                    }
                    Key::Delete => {
                        editor.delete();
                        changed = true;
                    }
                    Key::Enter if modifiers.shift => {
                        editor.add_program_line();
                        changed = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    changed
}

pub fn palette_buttons(ui: &mut egui::Ui, editor: &mut MathEditor) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (label, name, tip, action) in [
            (
                "a/b",
                "Вертикальная дробь",
                "Вертикальная дробь (/)",
                PaletteAction::Fraction,
            ),
            ("xⁿ", "Степень", "Степень (^)", PaletteAction::Power),
            (
                "√x",
                "Квадратный корень",
                "Квадратный корень",
                PaletteAction::Sqrt,
            ),
            (
                "xₐ",
                "Индекс — часть имени",
                "Индекс — часть имени (.)",
                PaletteAction::LiteralSub,
            ),
            (
                "A[i]",
                "Индекс элемента массива",
                "Индекс элемента массива ([)",
                PaletteAction::IndexSub,
            ),
            (
                "[2×2]",
                "Матрица 2×2",
                "Матрица 2×2",
                PaletteAction::Matrix(2, 2),
            ),
            ("|x|", "Модуль", "Модуль |x|", PaletteAction::Abs),
            ("π", "Число пи", "Константа π", PaletteAction::Raw("π")),
        ] {
            let response = ui.small_button(label).on_hover_text(tip);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
            if response.clicked() {
                action.apply(editor);
                changed = true;
            }
        }
    });
    changed
}
