use eframe::egui::{self, vec2, Pos2, Rect, Stroke, StrokeKind};

#[derive(Clone, Copy)]
pub enum Icon {
    New,
    Open,
    Save,
    ExportPdf,
    Preview,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    AlignTop,
    AlignLeft,
    Formula,
    Text,
    Graph,
    Table,
}

pub fn button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, enabled: bool) -> bool {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.add_sized([28., 24.], egui::Button::new("").frame(false))
        })
        .inner
        .on_hover_text(tooltip)
        .on_disabled_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tooltip));
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() || response.is_pointer_button_down_on() {
        ui.painter().rect(
            response.rect,
            4.,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            StrokeKind::Inside,
        );
    }
    let ink = if enabled {
        visuals.fg_stroke.color
    } else {
        crate::tool_ribbon::TEXT_MUTED.gamma_multiply(0.60)
    };
    let accent = if enabled {
        crate::tool_ribbon::BLUE_ACCENT_BORDER
    } else {
        ink
    };
    let origin = response.rect.center() - vec2(9., 9.);
    let point = |x, y| origin + vec2(x, y);
    let painter = ui.painter();
    let stroke = Stroke::new(1.35_f32, ink);
    let blue = Stroke::new(1.35_f32, accent);
    let line = |coords: &[(f32, f32)], stroke| {
        let points: Vec<Pos2> = coords.iter().map(|&(x, y)| point(x, y)).collect();
        painter.add(egui::Shape::line(points, stroke));
    };
    let outline = |x1, y1, x2, y2, stroke| {
        painter.rect_stroke(
            Rect::from_min_max(point(x1, y1), point(x2, y2)),
            1.,
            stroke,
            StrokeKind::Inside,
        );
    };
    let page = |bottom_x| {
        line(
            &[
                (bottom_x, 16.),
                (3., 16.),
                (3., 2.),
                (10., 2.),
                (14., 6.),
                (14., 9.),
            ],
            stroke,
        );
        line(&[(10., 2.), (10., 6.), (14., 6.)], stroke);
    };
    match icon {
        Icon::New => {
            page(13.);
            line(&[(10., 12.), (17., 12.)], blue);
            line(&[(13.5, 8.5), (13.5, 15.5)], blue);
        }
        Icon::Open => {
            line(
                &[
                    (2., 15.),
                    (2., 4.),
                    (7., 4.),
                    (9., 6.),
                    (15., 6.),
                    (15., 8.),
                ],
                stroke,
            );
            line(
                &[(2., 15.), (5., 8.), (17., 8.), (14., 15.), (2., 15.)],
                blue,
            );
        }
        Icon::Save => {
            outline(2., 2., 16., 16., stroke);
            outline(5., 9., 13., 16., blue);
            line(&[(5., 2.), (5., 6.), (12., 6.), (12., 2.)], stroke);
        }
        Icon::ExportPdf => {
            page(8.);
            line(&[(13., 8.), (13., 15.)], blue);
            line(&[(10., 12.), (13., 15.), (16., 12.)], blue);
        }
        Icon::Preview => {
            page(9.);
            painter.circle_stroke(point(11.5, 10.5), 3.2, blue);
            line(&[(14., 13.), (17., 16.)], blue);
        }
        Icon::Cut => {
            painter.circle_stroke(point(5., 14.), 2.4, stroke);
            painter.circle_stroke(point(13., 14.), 2.4, stroke);
            line(&[(7., 12.), (14., 2.)], blue);
            line(&[(11., 12.), (4., 2.)], blue);
        }
        Icon::Copy => {
            line(&[(3., 12.), (3., 2.), (12., 2.), (12., 4.)], stroke);
            outline(6., 5., 15., 16., blue);
        }
        Icon::Paste => {
            outline(3., 4., 15., 16., stroke);
            outline(6., 2., 12., 6., blue);
            line(&[(6., 10.), (12., 10.)], stroke);
            line(&[(6., 13.), (12., 13.)], stroke);
        }
        Icon::Undo | Icon::Redo => {
            let x = |x| {
                if matches!(icon, Icon::Undo) {
                    x
                } else {
                    18. - x
                }
            };
            line(
                &[
                    (x(2.), 8.),
                    (x(8.), 8.),
                    (x(13.), 9.),
                    (x(16.), 12.),
                    (x(16.), 15.),
                ],
                stroke,
            );
            line(&[(x(6.), 4.), (x(2.), 8.), (x(6.), 12.)], blue);
        }
        Icon::AlignTop => {
            line(&[(2., 4.), (16., 4.)], blue);
            outline(4., 7., 7., 12., stroke);
            outline(10., 7., 13., 16., stroke);
        }
        Icon::AlignLeft => {
            line(&[(3., 2.), (3., 16.)], blue);
            outline(6., 4., 12., 7., stroke);
            outline(6., 10., 16., 13., stroke);
        }
        Icon::Table => {
            outline(2., 3., 16., 15., stroke);
            line(&[(2., 7.), (16., 7.)], blue);
            line(&[(2., 11.), (16., 11.)], stroke);
            line(&[(9., 7.), (9., 15.)], stroke);
        }
        Icon::Formula | Icon::Text => {
            let label = if matches!(icon, Icon::Formula) {
                "ƒx"
            } else {
                "T"
            };
            painter.text(
                point(9., 9.),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(16.),
                ink,
            );
        }
        Icon::Graph => {
            line(&[(3., 2.), (3., 15.), (17., 15.)], stroke);
            line(&[(4., 12.), (8., 7.), (11., 11.), (16., 4.)], blue);
        }
    }
    response.clicked()
}
