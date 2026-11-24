use crate::math_view::{PaletteAction, PaletteTab};
use eframe::egui::{
    self, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Margin, Painter, Pos2,
    Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit,
};
use list_rs::math_editor::{BigOpKind, ProgramCommand};

pub const SHELL_BG: Color32 = Color32::from_rgb(15, 25, 42);
pub const PANEL_DARK: Color32 = Color32::from_rgb(20, 33, 56);
pub const PANEL_SURFACE: Color32 = Color32::from_rgb(26, 42, 68);
pub const PANEL_ELEVATED: Color32 = Color32::from_rgb(35, 56, 89);
pub const PANEL_HOVER: Color32 = Color32::from_rgb(46, 73, 114);
pub const INPUT_BG: Color32 = Color32::from_rgb(14, 24, 41);
pub const BORDER_DARK: Color32 = Color32::from_rgb(56, 82, 120);

pub const TEXT_WHITE: Color32 = Color32::from_rgb(245, 248, 255);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(176, 196, 224);

pub const BLUE_ACCENT: Color32 = Color32::from_rgb(56, 139, 253);
pub const BLUE_ACCENT_SOFT: Color32 = Color32::from_rgb(33, 86, 173);
pub const BLUE_ACCENT_BORDER: Color32 = Color32::from_rgb(112, 178, 255);

pub const SEARCH_HIT_BG: Color32 = Color32::from_rgb(78, 56, 16);
pub const SEARCH_HIT_BORDER: Color32 = Color32::from_rgb(255, 196, 61);
pub const SEARCH_HIT_BADGE: Color32 = Color32::from_rgb(255, 214, 102);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolIcon {
    Fraction,
    Power,
    Sqrt,
    Abs,
    LiteralSub,
    IndexSub,
    Integral,
    Derivative,
    Sum,
    Product,
    MatrixGrid(u8, u8),
    VectorCol,
    TableGrid,
    PlotXY,
    PlotVectors,
    PlotParametric,
    PlotPolar,
    Plot3d,
    PlotContour,
    SymArrow,
    ProgramBar,
    AddProgramLine,
    GreekBadge,
    LogicBadge,
    Search,
    Glyph(&'static str),
    Badge(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub enum ToolAction {
    Math(PaletteAction),
    CustomMatrix,
    ChartXY,
    ChartZoom,
    ChartTrace,
    ChartVectors,
    ChartParametric,
    ChartPolar,
    Plot3dSurface,
    Plot3dContour,
    Table,
}

#[derive(Clone, Copy, Debug)]
pub struct GroupMeta {
    pub tab: PaletteTab,
    pub id: &'static str,
    pub title: &'static str,
    pub tab_label: &'static str,
    pub tooltip: &'static str,
    pub icon: ToolIcon,
    pub synonyms: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct ToolItem {
    pub id: &'static str,
    pub group: PaletteTab,
    pub name: &'static str,
    pub short_label: &'static str,
    pub tooltip: &'static str,
    pub shortcut: Option<&'static str>,
    pub icon: ToolIcon,
    pub synonyms: &'static [&'static str],
    pub action: ToolAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchHighlight {
    Group(PaletteTab),
    Item {
        group: PaletteTab,
        item_id: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchTarget {
    Group(PaletteTab),
    Item(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub target: SearchTarget,
    pub group: PaletteTab,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub icon: ToolIcon,
    score: u8,
}

impl SearchHit {
    pub fn highlight(self) -> SearchHighlight {
        match self.target {
            SearchTarget::Group(tab) => SearchHighlight::Group(tab),
            SearchTarget::Item(item_id) => SearchHighlight::Item {
                group: self.group,
                item_id,
            },
        }
    }
}

pub struct SearchState {
    pub query: String,
    pub open: bool,
    pub selected_idx: usize,
    pub highlight: Option<SearchHighlight>,
    pub scroll_to_highlight: bool,
    pub focus_requested: bool,
    pub prev_focus: Option<egui::Id>,
    pub has_focus: bool,
    pub enter_consumed: bool,
    pub escape_consumed: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            open: false,
            selected_idx: 0,
            highlight: None,
            scroll_to_highlight: false,
            focus_requested: false,
            prev_focus: None,
            has_focus: false,
            enter_consumed: false,
            escape_consumed: false,
        }
    }
}

impl SearchState {
    pub fn input_id() -> egui::Id {
        egui::Id::new("tool_ribbon_search_input")
    }

    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.enter_consumed = false;
        self.escape_consumed = false;
        let search_id = Self::input_id();
        let focused = ctx.memory(|m| m.focused());
        self.has_focus = focused == Some(search_id);
        if let Some(id) = focused {
            if id != search_id {
                self.prev_focus = Some(id);
            }
        }
    }

    pub fn request_focus(&mut self, ctx: &egui::Context) {
        let search_id = Self::input_id();
        let current = ctx.memory(|m| m.focused());
        if current != Some(search_id) {
            self.prev_focus = current;
        }
        self.focus_requested = true;
        if !self.query.trim().is_empty() {
            self.open = true;
        }
    }

    pub fn is_blocking_sheet(&self, ctx: &egui::Context) -> bool {
        self.has_focus
            || self.focus_requested
            || self.enter_consumed
            || self.escape_consumed
            || ctx.memory(|m| m.has_focus(Self::input_id()))
    }

    #[cfg(test)]
    pub fn set_query(&mut self, text: impl Into<String>) {
        self.query = text.into();
        self.selected_idx = 0;
        if self.query.trim().is_empty() {
            self.highlight = None;
            self.open = false;
        } else {
            self.open = true;
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.selected_idx = 0;
        self.highlight = None;
        self.open = false;
        self.scroll_to_highlight = false;
    }

    pub fn reveal_hit(
        &mut self,
        hit: SearchHit,
        active_tab: &mut PaletteTab,
        ribbon_expanded: &mut bool,
    ) {
        *active_tab = hit.group;
        *ribbon_expanded = true;
        self.highlight = Some(hit.highlight());
        self.scroll_to_highlight = true;
        self.open = false;
    }

    pub fn close_and_restore_focus(&mut self, ctx: &egui::Context) {
        let search_id = Self::input_id();
        self.open = false;
        self.focus_requested = false;
        self.escape_consumed = true;
        let prev = self.prev_focus.take();
        ctx.memory_mut(|m| {
            m.surrender_focus(search_id);
            if let Some(id) = prev {
                if id != search_id {
                    m.request_focus(id);
                }
            }
        });
    }
}

pub const GROUPS: &[GroupMeta] = &[
    GroupMeta {
        tab: PaletteTab::Operators,
        id: "group.operators",
        title: "Операторы",
        tab_label: "Операторы",
        tooltip: "Операторы, дроби, степени, корни, индексы и константы",
        icon: ToolIcon::Fraction,
        synonyms: &[
            "операторы",
            "арифметика",
            "формулы",
            "константы",
            "единицы",
            "температура",
            "operators",
        ],
    },
    GroupMeta {
        tab: PaletteTab::Plots,
        id: "group.plots",
        title: "Графики",
        tab_label: "Графики",
        tooltip: "Графики X–Y, параметрические, полярные, 3D-поверхности и контуры",
        icon: ToolIcon::PlotXY,
        synonyms: &[
            "графики",
            "график",
            "диаграмма",
            "кривые",
            "поверхность",
            "контуры",
            "plots",
            "charts",
        ],
    },
    GroupMeta {
        tab: PaletteTab::Matrix,
        id: "group.matrix",
        title: "Вектор и матрица",
        tab_label: "Вектор и матрица",
        tooltip: "Векторы, матрицы, определитель, СЛАУ, разложения и таблицы CSV",
        icon: ToolIcon::MatrixGrid(2, 2),
        synonyms: &[
            "вектор и матрица",
            "матрицы",
            "матрица",
            "вектор",
            "векторы",
            "массив",
            "таблица",
            "линейная алгебра",
            "matrix",
            "vector",
        ],
    },
    GroupMeta {
        tab: PaletteTab::Evaluation,
        id: "group.evaluation",
        title: "Вычисление",
        tab_label: "Вычисление",
        tooltip: "Определения, численный и символьный вывод",
        icon: ToolIcon::Glyph("x="),
        synonyms: &["вычисление", "присваивание", "результат", "evaluation"],
    },
    GroupMeta {
        tab: PaletteTab::Calculus,
        id: "group.calculus",
        title: "Анализ",
        tab_label: "Анализ",
        tooltip: "Производная, интеграл, сумма, решатели, оптимизация, БПФ и сплайны",
        icon: ToolIcon::Integral,
        synonyms: &[
            "анализ",
            "матанализ",
            "интеграл",
            "производная",
            "сумма",
            "решатели",
            "оптимизация",
            "calculus",
        ],
    },
    GroupMeta {
        tab: PaletteTab::Logic,
        id: "group.logic",
        title: "Логика",
        tab_label: "Логика",
        tooltip: "Булевы сравнения, условия и базовая статистика",
        icon: ToolIcon::LogicBadge,
        synonyms: &[
            "логика",
            "сравнения",
            "булевы",
            "условия",
            "статистика",
            "logic",
        ],
    },
    GroupMeta {
        tab: PaletteTab::Program,
        id: "group.program",
        title: "Программы",
        tab_label: "Программы",
        tooltip: "Программные блоки, локальные переменные, циклы и решатель ОДУ",
        icon: ToolIcon::ProgramBar,
        synonyms: &[
            "программы",
            "программирование",
            "цикл",
            "ветвление",
            "оду",
            "program",
            "ode",
        ],
    },
    GroupMeta {
        tab: PaletteTab::Greek,
        id: "group.greek",
        title: "Греческие буквы",
        tab_label: "Греческие",
        tooltip: "Греческие буквы и символы (α, β, γ, …, Ω)",
        icon: ToolIcon::GreekBadge,
        synonyms: &["греческие буквы", "греческий", "алфавит", "αβγ", "greek"],
    },
    GroupMeta {
        tab: PaletteTab::Symbolic,
        id: "group.symbolic",
        title: "Символьные",
        tab_label: "Символьные",
        tooltip: "Символьный вывод →, упрощение, раскрытие скобок и аналитические решения",
        icon: ToolIcon::SymArrow,
        synonyms: &[
            "символьные",
            "символика",
            "алгебра",
            "упрощение",
            "symbolic",
        ],
    },
];

pub const TOOLS: &[ToolItem] = &[
    // --- Операторы ---
    ToolItem {
        id: "op.fraction",
        group: PaletteTab::Operators,
        name: "Вертикальная дробь",
        short_label: "",
        tooltip: "Вертикальная дробь (/)\nДеление в виде числителя и знаменателя",
        shortcut: Some("/"),
        icon: ToolIcon::Fraction,
        synonyms: &["дробь", "деление", "числитель", "знаменатель", "a/b", "/"],
        action: ToolAction::Math(PaletteAction::Fraction),
    },
    ToolItem {
        id: "op.power",
        group: PaletteTab::Operators,
        name: "Степень",
        short_label: "",
        tooltip: "Степень (^)\nВозведение основания в степень xⁿ",
        shortcut: Some("^"),
        icon: ToolIcon::Power,
        synonyms: &["степень", "возведение", "показатель", "x^n", "xⁿ", "^"],
        action: ToolAction::Math(PaletteAction::Power),
    },
    ToolItem {
        id: "op.sqrt",
        group: PaletteTab::Operators,
        name: "Квадратный корень",
        short_label: "",
        tooltip: "Квадратный корень √x\nВставка радикала sqrt(x)",
        shortcut: None,
        icon: ToolIcon::Sqrt,
        synonyms: &["корень", "квадратный корень", "радикал", "sqrt", "√"],
        action: ToolAction::Math(PaletteAction::Sqrt),
    },
    ToolItem {
        id: "op.abs",
        group: PaletteTab::Operators,
        name: "Модуль |x|",
        short_label: "",
        tooltip: "Модуль |x|\nАбсолютное значение abs(x)",
        shortcut: None,
        icon: ToolIcon::Abs,
        synonyms: &["модуль", "абсолютное значение", "abs", "|x|"],
        action: ToolAction::Math(PaletteAction::Abs),
    },
    ToolItem {
        id: "op.literal_sub",
        group: PaletteTab::Operators,
        name: "Индекс — часть имени",
        short_label: "",
        tooltip: "Индекс — часть имени (.)\nЛитеральный нижний индекс xₐ",
        shortcut: Some("."),
        icon: ToolIcon::LiteralSub,
        synonyms: &["индекс", "нижний индекс", "часть имени", "подстрочный", "."],
        action: ToolAction::Math(PaletteAction::LiteralSub),
    },
    ToolItem {
        id: "op.index_sub",
        group: PaletteTab::Operators,
        name: "Индекс элемента массива",
        short_label: "",
        tooltip: "Индекс элемента массива ([)\nОбращение A[i] к вектору или матрице",
        shortcut: Some("["),
        icon: ToolIcon::IndexSub,
        synonyms: &["индекс", "массив", "элемент", "вектор", "A[i]", "["],
        action: ToolAction::Math(PaletteAction::IndexSub),
    },
    ToolItem {
        id: "op.define",
        group: PaletteTab::Evaluation,
        name: "Определение :=",
        short_label: "",
        tooltip: "Определение :=\nПрисваивание переменной или функции",
        shortcut: None,
        icon: ToolIcon::Glyph(":="),
        synonyms: &["определение", "присваивание", "переменная", ":="],
        action: ToolAction::Math(PaletteAction::Raw(" := ")),
    },
    ToolItem {
        id: "op.global_define",
        group: PaletteTab::Evaluation,
        name: "Глобальное определение ≡",
        short_label: "",
        tooltip: "Глобальное определение ≡ (===)\nДействует по всему листу",
        shortcut: None,
        icon: ToolIcon::Glyph("≡"),
        synonyms: &["глобальное определение", "глобальная", "===", "≡"],
        action: ToolAction::Math(PaletteAction::Raw(" === ")),
    },
    ToolItem {
        id: "op.eval",
        group: PaletteTab::Evaluation,
        name: "Вывод результата =",
        short_label: "",
        tooltip: "Вывод результата =\nЧисленное вычисление выражения",
        shortcut: None,
        icon: ToolIcon::Glyph("="),
        synonyms: &["вывод результата", "равно", "вычислить", "="],
        action: ToolAction::Math(PaletteAction::Raw(" =")),
    },
    ToolItem {
        id: "op.range",
        group: PaletteTab::Operators,
        name: "Диапазон ..",
        short_label: "",
        tooltip: "Диапазон ..\nРанжированная переменная, например 0..10",
        shortcut: None,
        icon: ToolIcon::Glyph(".."),
        synonyms: &["диапазон", "интервал", "ранжированная", "range", ".."],
        action: ToolAction::Math(PaletteAction::Raw("..")),
    },
    ToolItem {
        id: "op.parens",
        group: PaletteTab::Operators,
        name: "Скобки ( )",
        short_label: "",
        tooltip: "Скобки ( )\nГруппировка подвыражения",
        shortcut: None,
        icon: ToolIcon::Glyph("( )"),
        synonyms: &["скобки", "группировка", "()"],
        action: ToolAction::Math(PaletteAction::Raw("()")),
    },
    ToolItem {
        id: "op.pi",
        group: PaletteTab::Operators,
        name: "Константа π",
        short_label: "",
        tooltip: "Константа π\nЧисло пи (3.14159…)",
        shortcut: None,
        icon: ToolIcon::Glyph("π"),
        synonyms: &["пи", "pi", "константа", "π"],
        action: ToolAction::Math(PaletteAction::Raw("π")),
    },
    ToolItem {
        id: "op.euler",
        group: PaletteTab::Operators,
        name: "Число Эйлера e",
        short_label: "",
        tooltip: "Число Эйлера e\nОснование натурального логарифма",
        shortcut: None,
        icon: ToolIcon::Glyph("e"),
        synonyms: &["эйлер", "экспонента", "константа", "e"],
        action: ToolAction::Math(PaletteAction::Raw("e")),
    },
    ToolItem {
        id: "op.imag",
        group: PaletteTab::Operators,
        name: "Мнимая единица 1i",
        short_label: "",
        tooltip: "Мнимая единица 1i\nКомплексные числа",
        shortcut: None,
        icon: ToolIcon::Glyph("1i"),
        synonyms: &["мнимая единица", "комплексное", "1i", "i"],
        action: ToolAction::Math(PaletteAction::Raw("1i")),
    },
    ToolItem {
        id: "op.deg",
        group: PaletteTab::Operators,
        name: "Градусы deg",
        short_label: "",
        tooltip: "Градусы (* deg)\nПеревод углов из градусов",
        shortcut: None,
        icon: ToolIcon::Badge("deg"),
        synonyms: &["градусы", "угол", "deg", "°"],
        action: ToolAction::Math(PaletteAction::Raw(" * deg")),
    },
    ToolItem {
        id: "op.celsius",
        group: PaletteTab::Operators,
        name: "Температура Цельсия C(t)",
        short_label: "",
        tooltip: "Температура Цельсия C(t)\nПеревод °C в кельвины",
        shortcut: None,
        icon: ToolIcon::Badge("°C"),
        synonyms: &["температура", "цельсий", "C(t)", "°C", "degC"],
        action: ToolAction::Math(PaletteAction::Call("C")),
    },
    // --- Графики ---
    ToolItem {
        id: "plot.xy",
        group: PaletteTab::Plots,
        name: "График X–Y",
        short_label: "X–Y",
        tooltip: "График X–Y\nПостроение функций y(x) на декартовой плоскости",
        shortcut: None,
        icon: ToolIcon::PlotXY,
        synonyms: &["график", "xy", "x-y", "функция", "y(x)", "plot", "chart"],
        action: ToolAction::ChartXY,
    },
    ToolItem {
        id: "plot.zoom",
        group: PaletteTab::Plots,
        name: "Масштаб графика",
        short_label: "Масштаб",
        tooltip: "Масштаб выбранного 2D-графика",
        shortcut: None,
        icon: ToolIcon::Search,
        synonyms: &["масштаб графика", "приближение", "zoom"],
        action: ToolAction::ChartZoom,
    },
    ToolItem {
        id: "plot.trace",
        group: PaletteTab::Plots,
        name: "Трассировка графика",
        short_label: "Трассир.",
        tooltip: "Трассировка координат выбранного 2D-графика",
        shortcut: None,
        icon: ToolIcon::Glyph("⊕"),
        synonyms: &["трассировка", "координаты", "точка", "trace"],
        action: ToolAction::ChartTrace,
    },
    ToolItem {
        id: "plot.polar",
        group: PaletteTab::Plots,
        name: "Полярный график",
        short_label: "r(θ)",
        tooltip: "Полярный график\nКривая r(θ) в полярных координатах",
        shortcut: None,
        icon: ToolIcon::PlotPolar,
        synonyms: &["график", "полярный", "r(θ)", "угол", "polar"],
        action: ToolAction::ChartPolar,
    },
    ToolItem {
        id: "plot.surface3d",
        group: PaletteTab::Plots,
        name: "3D-поверхность z(x, y)",
        short_label: "3D",
        tooltip: "3D-поверхность z(x, y)\nТрёхмерный график функции или матрицы",
        shortcut: None,
        icon: ToolIcon::Plot3d,
        synonyms: &["график", "3d", "поверхность", "z(x,y)", "surface"],
        action: ToolAction::Plot3dSurface,
    },
    ToolItem {
        id: "plot.contour",
        group: PaletteTab::Plots,
        name: "Контурный график",
        short_label: "Контур",
        tooltip: "Контурный график (теплокарта / изолинии)\nПо матрице или функции z(x, y)",
        shortcut: None,
        icon: ToolIcon::PlotContour,
        synonyms: &["график", "контурный", "изолинии", "теплокарта", "contour"],
        action: ToolAction::Plot3dContour,
    },
    ToolItem {
        id: "plot.vectors",
        group: PaletteTab::Plots,
        name: "График по векторам X, Y",
        short_label: "X, Y",
        tooltip: "График по векторам X, Y\nПостроение кривой по парам массивов данных",
        shortcut: None,
        icon: ToolIcon::PlotVectors,
        synonyms: &["график", "данные", "векторы", "массивы", "точки", "xy"],
        action: ToolAction::ChartVectors,
    },
    ToolItem {
        id: "plot.parametric",
        group: PaletteTab::Plots,
        name: "Параметрический график",
        short_label: "x(t), y(t)",
        tooltip: "Параметрический график\nКривая x(t), y(t) на интервале параметра",
        shortcut: None,
        icon: ToolIcon::PlotParametric,
        synonyms: &["график", "параметрический", "x(t)", "y(t)", "parametric"],
        action: ToolAction::ChartParametric,
    },
    // --- Вектор и матрица ---
    ToolItem {
        id: "mat.custom",
        group: PaletteTab::Matrix,
        name: "Матрица заданного размера",
        short_label: "m×n",
        tooltip: "Вставить матрицу выбранного размера [строк × столбцов]",
        shortcut: None,
        icon: ToolIcon::MatrixGrid(3, 3),
        synonyms: &[
            "матрица",
            "размер матрицы",
            "сетка",
            "вектор",
            "вставить матрицу",
            "matrix",
        ],
        action: ToolAction::CustomMatrix,
    },
    ToolItem {
        id: "mat.2x2",
        group: PaletteTab::Matrix,
        name: "Матрица 2×2",
        short_label: "2×2",
        tooltip: "Матрица 2×2\nБыстрая вставка квадратной матрицы 2×2",
        shortcut: None,
        icon: ToolIcon::MatrixGrid(2, 2),
        synonyms: &["матрица", "2x2", "2×2", "квадратная"],
        action: ToolAction::Math(PaletteAction::Matrix(2, 2)),
    },
    ToolItem {
        id: "mat.3x3",
        group: PaletteTab::Matrix,
        name: "Матрица 3×3",
        short_label: "3×3",
        tooltip: "Матрица 3×3\nБыстрая вставка квадратной матрицы 3×3",
        shortcut: None,
        icon: ToolIcon::MatrixGrid(3, 3),
        synonyms: &["матрица", "3x3", "3×3", "квадратная"],
        action: ToolAction::Math(PaletteAction::Matrix(3, 3)),
    },
    ToolItem {
        id: "mat.3x1",
        group: PaletteTab::Matrix,
        name: "Вектор 3×1",
        short_label: "3×1",
        tooltip: "Вектор-столбец 3×1\nВставка вектора из трёх элементов",
        shortcut: None,
        icon: ToolIcon::VectorCol,
        synonyms: &["вектор", "столбец", "3x1", "3×1", "матрица"],
        action: ToolAction::Math(PaletteAction::Matrix(3, 1)),
    },
    ToolItem {
        id: "mat.det",
        group: PaletteTab::Matrix,
        name: "Определитель det(M)",
        short_label: "",
        tooltip: "Определитель det(M)\nДетерминант квадратной матрицы",
        shortcut: None,
        icon: ToolIcon::Badge("det"),
        synonyms: &["определитель", "детерминант", "det", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("det")),
    },
    ToolItem {
        id: "mat.transpose",
        group: PaletteTab::Matrix,
        name: "Транспонирование Mᵀ",
        short_label: "",
        tooltip: "Транспонирование transpose(M)\nЗамена строк и столбцов матрицы",
        shortcut: None,
        icon: ToolIcon::Glyph("Mᵀ"),
        synonyms: &["транспонирование", "transpose", "M^T", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("transpose")),
    },
    ToolItem {
        id: "mat.lsolve",
        group: PaletteTab::Matrix,
        name: "Решение СЛАУ lsolve(A, b)",
        short_label: "",
        tooltip: "Решение СЛАУ lsolve(A, b)\nЛинейная система A·x = b",
        shortcut: None,
        icon: ToolIcon::Badge("lsolve"),
        synonyms: &["слау", "линейная система", "lsolve", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("lsolve")),
    },
    ToolItem {
        id: "mat.col",
        group: PaletteTab::Matrix,
        name: "Столбец col(M, j)",
        short_label: "",
        tooltip: "Столбец матрицы col(M, j)\nВыбор столбца по индексу (с 0)",
        shortcut: None,
        icon: ToolIcon::Badge("col"),
        synonyms: &["столбец", "колонка", "col", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("col")),
    },
    ToolItem {
        id: "mat.row",
        group: PaletteTab::Matrix,
        name: "Строка row(M, i)",
        short_label: "",
        tooltip: "Строка матрицы row(M, i)\nВыбор строки по индексу (с 0)",
        shortcut: None,
        icon: ToolIcon::Badge("row"),
        synonyms: &["строка", "row", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("row")),
    },
    ToolItem {
        id: "mat.augment",
        group: PaletteTab::Matrix,
        name: "Объединить augment(A, B)",
        short_label: "",
        tooltip: "Объединить матрицы по столбцам augment(A, B)",
        shortcut: None,
        icon: ToolIcon::Badge("aug"),
        synonyms: &["объединить", "склеить", "augment", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("augment")),
    },
    ToolItem {
        id: "mat.stack",
        group: PaletteTab::Matrix,
        name: "Объединить по строкам stack(A, B)",
        short_label: "",
        tooltip: "Объединить матрицы по строкам stack(A, B)",
        shortcut: None,
        icon: ToolIcon::Badge("stack"),
        synonyms: &["объединить", "стек", "stack", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("stack")),
    },
    ToolItem {
        id: "mat.eigenvals",
        group: PaletteTab::Matrix,
        name: "Собственные значения eigenvals(M)",
        short_label: "",
        tooltip: "Собственные значения eigenvals(M)\nСпектр квадратной матрицы",
        shortcut: None,
        icon: ToolIcon::Badge("eig"),
        synonyms: &["собственные значения", "спектр", "eigenvals", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("eigenvals")),
    },
    ToolItem {
        id: "mat.qr",
        group: PaletteTab::Matrix,
        name: "QR-разложение qr(M)",
        short_label: "",
        tooltip: "QR-разложение матрицы qr(M)",
        shortcut: None,
        icon: ToolIcon::Badge("QR"),
        synonyms: &["разложение", "qr", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("qr")),
    },
    ToolItem {
        id: "mat.rank",
        group: PaletteTab::Matrix,
        name: "Ранг матрицы rank(M)",
        short_label: "",
        tooltip: "Ранг матрицы rank(M)",
        shortcut: None,
        icon: ToolIcon::Badge("rank"),
        synonyms: &["ранг", "rank", "матрица"],
        action: ToolAction::Math(PaletteAction::Call("rank")),
    },
    ToolItem {
        id: "mat.table",
        group: PaletteTab::Matrix,
        name: "Таблица данных / CSV",
        short_label: "CSV",
        tooltip: "Таблица данных / CSV\nСоздать или импортировать числовую таблицу",
        shortcut: None,
        icon: ToolIcon::TableGrid,
        synonyms: &["таблица", "csv", "данные", "импорт", "матрица"],
        action: ToolAction::Table,
    },
    // --- Анализ ---
    ToolItem {
        id: "calc.derivative",
        group: PaletteTab::Calculus,
        name: "Численная производная d/dx",
        short_label: "",
        tooltip: "Численная производная d/dx f|x0\nОператор diff(f, x0)",
        shortcut: None,
        icon: ToolIcon::Derivative,
        synonyms: &["производная", "дифференцирование", "diff", "d/dx"],
        action: ToolAction::Math(PaletteAction::Derivative),
    },
    ToolItem {
        id: "calc.integral",
        group: PaletteTab::Calculus,
        name: "Определённый интеграл ∫",
        short_label: "",
        tooltip: "Определённый интеграл ∫_a^b f\nОператор integ(f, a, b)",
        shortcut: None,
        icon: ToolIcon::Integral,
        synonyms: &["интеграл", "интегрирование", "integ", "∫"],
        action: ToolAction::Math(PaletteAction::BigOp(BigOpKind::Integral)),
    },
    ToolItem {
        id: "calc.sum",
        group: PaletteTab::Calculus,
        name: "Сумма по индексу ∑",
        short_label: "",
        tooltip: "Сумма по индексу ∑_a^b f\nОператор isum(f, k0, k1)",
        shortcut: None,
        icon: ToolIcon::Sum,
        synonyms: &["сумма", "суммирование", "ряд", "isum", "Σ", "∑"],
        action: ToolAction::Math(PaletteAction::BigOp(BigOpKind::Sum)),
    },
    ToolItem {
        id: "calc.product",
        group: PaletteTab::Calculus,
        name: "Произведение по индексу ∏",
        short_label: "",
        tooltip: "Произведение по индексу ∏_a^b f\nОператор iprod(f, k0, k1)",
        shortcut: None,
        icon: ToolIcon::Product,
        synonyms: &["произведение", "iprod", "Π", "∏"],
        action: ToolAction::Math(PaletteAction::BigOp(BigOpKind::Product)),
    },
    ToolItem {
        id: "calc.root",
        group: PaletteTab::Calculus,
        name: "Поиск корня root(f, x0)",
        short_label: "",
        tooltip: "Корень уравнения root(f, x0) или root(f, a, b)",
        shortcut: None,
        icon: ToolIcon::Badge("root"),
        synonyms: &["корень уравнения", "уравнение", "root", "решатель"],
        action: ToolAction::Math(PaletteAction::Call("root")),
    },
    ToolItem {
        id: "calc.polyroots",
        group: PaletteTab::Calculus,
        name: "Корни полинома polyroots(v)",
        short_label: "",
        tooltip: "Корни полинома polyroots(v)\nВектор коэффициентов по возрастанию степеней",
        shortcut: None,
        icon: ToolIcon::Badge("poly"),
        synonyms: &["полином", "многочлен", "корни", "polyroots"],
        action: ToolAction::Math(PaletteAction::Call("polyroots")),
    },
    ToolItem {
        id: "calc.nsolve",
        group: PaletteTab::Calculus,
        name: "Система уравнений nsolve(F, x0)",
        short_label: "",
        tooltip: "Система нелинейных уравнений nsolve(F, x0)",
        shortcut: None,
        icon: ToolIcon::Badge("nslv"),
        synonyms: &["система уравнений", "нелинейная", "nsolve", "решатель"],
        action: ToolAction::Math(PaletteAction::Call("nsolve")),
    },
    ToolItem {
        id: "calc.minimize",
        group: PaletteTab::Calculus,
        name: "Минимизация minimize(f, x0)",
        short_label: "",
        tooltip: "Минимизация minimize(f, x0)\nПоиск минимума функции",
        shortcut: None,
        icon: ToolIcon::Badge("min"),
        synonyms: &["минимум", "минимизация", "оптимизация", "minimize"],
        action: ToolAction::Math(PaletteAction::Call("minimize")),
    },
    ToolItem {
        id: "calc.maximize",
        group: PaletteTab::Calculus,
        name: "Максимизация maximize(f, x0)",
        short_label: "",
        tooltip: "Максимизация maximize(f, x0)\nПоиск максимума функции",
        shortcut: None,
        icon: ToolIcon::Badge("max"),
        synonyms: &["максимум", "максимизация", "оптимизация", "maximize"],
        action: ToolAction::Math(PaletteAction::Call("maximize")),
    },
    ToolItem {
        id: "calc.minerr",
        group: PaletteTab::Calculus,
        name: "МНК-решение minerr(F, x0)",
        short_label: "",
        tooltip: "МНК-решение minerr(F, x0)\nМинимизация невязки системы",
        shortcut: None,
        icon: ToolIcon::Badge("merr"),
        synonyms: &["мнк", "наименьших квадратов", "minerr", "невязка"],
        action: ToolAction::Math(PaletteAction::Call("minerr")),
    },
    ToolItem {
        id: "calc.given",
        group: PaletteTab::Calculus,
        name: "Блок решения Given",
        short_label: "",
        tooltip: "Начало блока решения Given\nДалее введите уравнения и Find(…)",
        shortcut: None,
        icon: ToolIcon::Badge("Given"),
        synonyms: &["блок решения", "given", "дано", "ограничения"],
        action: ToolAction::Math(PaletteAction::Raw("Given")),
    },
    ToolItem {
        id: "calc.find",
        group: PaletteTab::Calculus,
        name: "Решение блока Find(x, y)",
        short_label: "",
        tooltip: "Конец блока решения Find(x, y)",
        shortcut: None,
        icon: ToolIcon::Badge("Find"),
        synonyms: &["блок решения", "find", "найти"],
        action: ToolAction::Math(PaletteAction::Call("Find")),
    },
    ToolItem {
        id: "calc.fft",
        group: PaletteTab::Calculus,
        name: "БПФ сигнала fft(v)",
        short_label: "",
        tooltip: "Быстрое преобразование Фурье fft(v) / ifft(v)",
        shortcut: None,
        icon: ToolIcon::Badge("fft"),
        synonyms: &["бпф", "фурье", "спектр", "сигнал", "fft", "ifft"],
        action: ToolAction::Math(PaletteAction::Call("fft")),
    },
    ToolItem {
        id: "calc.cspline",
        group: PaletteTab::Calculus,
        name: "Кубический сплайн cspline(vx, vy)",
        short_label: "",
        tooltip: "Кубический сплайн cspline(vx, vy)\nДля интерполяции interp(vs, vx, vy, x)",
        shortcut: None,
        icon: ToolIcon::Badge("spl"),
        synonyms: &["сплайн", "интерполяция", "cspline", "interp"],
        action: ToolAction::Math(PaletteAction::Call("cspline")),
    },
    ToolItem {
        id: "calc.regress",
        group: PaletteTab::Calculus,
        name: "Полиномиальная регрессия regress",
        short_label: "",
        tooltip: "Полиномиальная регрессия regress(vx, vy, k)",
        shortcut: None,
        icon: ToolIcon::Badge("reg"),
        synonyms: &["регрессия", "аппроксимация", "regress"],
        action: ToolAction::Math(PaletteAction::Call("regress")),
    },
    ToolItem {
        id: "calc.pnorm",
        group: PaletteTab::Calculus,
        name: "Нормальное распределение pnorm",
        short_label: "",
        tooltip: "Нормальное распределение pnorm(x, μ, σ)",
        shortcut: None,
        icon: ToolIcon::Badge("pnorm"),
        synonyms: &[
            "распределение",
            "нормальное",
            "гаусс",
            "вероятность",
            "pnorm",
        ],
        action: ToolAction::Math(PaletteAction::Call("pnorm")),
    },
    // --- Символьные ---
    ToolItem {
        id: "op.sym_eval",
        group: PaletteTab::Evaluation,
        name: "Символьное вычисление →",
        short_label: "",
        tooltip: "Символьное вычисление expr ->\nАналитическое преобразование",
        shortcut: None,
        icon: ToolIcon::SymArrow,
        synonyms: &["символьное", "стрелка", "->", "→", "аналитически"],
        action: ToolAction::Math(PaletteAction::Raw(" ->")),
    },
    ToolItem {
        id: "sym.arrow",
        group: PaletteTab::Symbolic,
        name: "Символьное вычисление →",
        short_label: "",
        tooltip: "Символьное вычисление expr ->\nАналитическое преобразование",
        shortcut: None,
        icon: ToolIcon::SymArrow,
        synonyms: &["символьное", "стрелка", "->", "→", "аналитически"],
        action: ToolAction::Math(PaletteAction::Raw(" ->")),
    },
    ToolItem {
        id: "sym.simplify",
        group: PaletteTab::Symbolic,
        name: "Упростить simplify(expr)",
        short_label: "",
        tooltip: "Символьное упрощение simplify(expr)",
        shortcut: None,
        icon: ToolIcon::Badge("simp"),
        synonyms: &["упростить", "упрощение", "simplify"],
        action: ToolAction::Math(PaletteAction::Call("simplify")),
    },
    ToolItem {
        id: "sym.expand",
        group: PaletteTab::Symbolic,
        name: "Раскрыть скобки expand(expr)",
        short_label: "",
        tooltip: "Раскрыть скобки expand(expr)",
        shortcut: None,
        icon: ToolIcon::Badge("expd"),
        synonyms: &["раскрыть скобки", "разложить", "expand"],
        action: ToolAction::Math(PaletteAction::Call("expand")),
    },
    ToolItem {
        id: "sym.sdiff",
        group: PaletteTab::Symbolic,
        name: "Символьная производная sdiff",
        short_label: "sdiff",
        tooltip: "Символьная производная sdiff(expr, x)",
        shortcut: None,
        icon: ToolIcon::Derivative,
        synonyms: &["символьная производная", "производная", "sdiff", "d/dx"],
        action: ToolAction::Math(PaletteAction::Call("sdiff")),
    },
    ToolItem {
        id: "sym.ssolve",
        group: PaletteTab::Symbolic,
        name: "Символьное решение ssolve",
        short_label: "",
        tooltip: "Символьное решение уравнения ssolve(expr, x)",
        shortcut: None,
        icon: ToolIcon::Badge("solv"),
        synonyms: &["символьное решение", "уравнение", "решить", "ssolve"],
        action: ToolAction::Math(PaletteAction::Call("ssolve")),
    },
    ToolItem {
        id: "sym.subst",
        group: PaletteTab::Symbolic,
        name: "Подстановка subst(expr, x, val)",
        short_label: "",
        tooltip: "Символьная подстановка subst(expr, x, val)",
        shortcut: None,
        icon: ToolIcon::Badge("sub"),
        synonyms: &["подстановка", "замена", "subst"],
        action: ToolAction::Math(PaletteAction::Call("subst")),
    },
    ToolItem {
        id: "sym.sin",
        group: PaletteTab::Symbolic,
        name: "Синус sin(x)",
        short_label: "",
        tooltip: "Синус sin(x)",
        shortcut: None,
        icon: ToolIcon::Badge("sin"),
        synonyms: &["синус", "тригонометрия", "sin"],
        action: ToolAction::Math(PaletteAction::Call("sin")),
    },
    ToolItem {
        id: "sym.cos",
        group: PaletteTab::Symbolic,
        name: "Косинус cos(x)",
        short_label: "",
        tooltip: "Косинус cos(x)",
        shortcut: None,
        icon: ToolIcon::Badge("cos"),
        synonyms: &["косинус", "тригонометрия", "cos"],
        action: ToolAction::Math(PaletteAction::Call("cos")),
    },
    ToolItem {
        id: "sym.ln",
        group: PaletteTab::Symbolic,
        name: "Логарифм ln(x)",
        short_label: "",
        tooltip: "Натуральный логарифм ln(x)",
        shortcut: None,
        icon: ToolIcon::Badge("ln"),
        synonyms: &["логарифм", "натуральный логарифм", "ln", "log"],
        action: ToolAction::Math(PaletteAction::Call("ln")),
    },
    // --- Программы ---
    ToolItem {
        id: "prog.block",
        group: PaletteTab::Program,
        name: "Программный блок prog",
        short_label: "prog",
        tooltip: "Программный блок (Shift+Enter: добавить строку)",
        shortcut: Some("Shift+Enter"),
        icon: ToolIcon::ProgramBar,
        synonyms: &["программа", "программный блок", "prog", "алгоритм"],
        action: ToolAction::Math(PaletteAction::Program(2)),
    },
    ToolItem {
        id: "prog.assign",
        group: PaletteTab::Program,
        name: "Локальное присваивание ←",
        short_label: "",
        tooltip: "Локальное присваивание <- внутри программы",
        shortcut: None,
        icon: ToolIcon::Glyph("←"),
        synonyms: &["локальное присваивание", "переменная", "<-", "←"],
        action: ToolAction::Math(PaletteAction::Raw(" <- ")),
    },
    ToolItem {
        id: "prog.if",
        group: PaletteTab::Program,
        name: "Условие if",
        short_label: "",
        tooltip: "Условие и действие при его выполнении. Tab — переход к действию.",
        shortcut: None,
        icon: ToolIcon::Badge("if"),
        synonyms: &["ветвление", "условие", "если", "if"],
        action: ToolAction::Math(PaletteAction::ProgramCommand(ProgramCommand::If)),
    },
    ToolItem {
        id: "prog.if_else",
        group: PaletteTab::Program,
        name: "Ветвление if / otherwise",
        short_label: "",
        tooltip: "Условие, действие при «да», действие при «нет». Tab — переход между полями.",
        shortcut: None,
        icon: ToolIcon::Badge("if/else"),
        synonyms: &["ветвление", "иначе", "otherwise", "else"],
        action: ToolAction::Math(PaletteAction::ProgramCommand(ProgramCommand::IfElse)),
    },
    ToolItem {
        id: "prog.for",
        group: PaletteTab::Program,
        name: "Цикл for(k, 1..n, body)",
        short_label: "",
        tooltip: "Цикл for(k, 1..n, body)",
        shortcut: None,
        icon: ToolIcon::Badge("for"),
        synonyms: &["цикл", "for", "перебор"],
        action: ToolAction::Math(PaletteAction::ProgramCommand(ProgramCommand::For)),
    },
    ToolItem {
        id: "prog.while",
        group: PaletteTab::Program,
        name: "Цикл while(cond, body)",
        short_label: "",
        tooltip: "Цикл while(cond, body)",
        shortcut: None,
        icon: ToolIcon::Badge("while"),
        synonyms: &["цикл", "пока", "while"],
        action: ToolAction::Math(PaletteAction::ProgramCommand(ProgramCommand::While)),
    },
    ToolItem {
        id: "prog.return",
        group: PaletteTab::Program,
        name: "Возврат return(x)",
        short_label: "",
        tooltip: "Возврат значения из программы return(x)",
        shortcut: None,
        icon: ToolIcon::Badge("ret"),
        synonyms: &["возврат", "вернуть", "return"],
        action: ToolAction::Math(PaletteAction::ProgramCommand(ProgramCommand::Return)),
    },
    ToolItem {
        id: "prog.break",
        group: PaletteTab::Program,
        name: "Выйти из цикла",
        short_label: "",
        tooltip: "Завершить ближайший цикл",
        shortcut: None,
        icon: ToolIcon::Badge("break"),
        synonyms: &["прервать цикл", "выход из цикла", "break"],
        action: ToolAction::Math(PaletteAction::Raw("break()")),
    },
    ToolItem {
        id: "prog.continue",
        group: PaletteTab::Program,
        name: "Перейти к следующей итерации",
        short_label: "",
        tooltip: "Пропустить остаток текущей итерации цикла",
        shortcut: None,
        icon: ToolIcon::Badge("next"),
        synonyms: &["итерация", "продолжить цикл", "continue"],
        action: ToolAction::Math(PaletteAction::Raw("continue()")),
    },
    ToolItem {
        id: "prog.odesolve",
        group: PaletteTab::Program,
        name: "Решатель ОДУ odesolve",
        short_label: "",
        tooltip: "Решатель ОДУ odesolve(f, x0, y0, x1)\nЗадача Коши y' = f(x, y)",
        shortcut: None,
        icon: ToolIcon::Badge("ode"),
        synonyms: &["оду", "дифференциальное уравнение", "коши", "odesolve"],
        action: ToolAction::Math(PaletteAction::Call("odesolve")),
    },
    ToolItem {
        id: "prog.odesolve_table",
        group: PaletteTab::Program,
        name: "Таблица ОДУ odesolve_table",
        short_label: "",
        tooltip: "Таблица решения ОДУ odesolve_table(f, x0, y0, x1, n)",
        shortcut: None,
        icon: ToolIcon::Badge("ode[]"),
        synonyms: &["оду", "таблица оду", "odesolve_table"],
        action: ToolAction::Math(PaletteAction::Call("odesolve_table")),
    },
    ToolItem {
        id: "prog.add_line",
        group: PaletteTab::Program,
        name: "Добавить строку программы",
        short_label: "+стр",
        tooltip: "Добавить строку в программный блок (Shift+Enter)",
        shortcut: Some("Shift+Enter"),
        icon: ToolIcon::AddProgramLine,
        synonyms: &["добавить строку", "новая строка", "программа"],
        action: ToolAction::Math(PaletteAction::AddProgramLine),
    },
    // --- Греческие буквы ---
    ToolItem {
        id: "greek.alpha",
        group: PaletteTab::Greek,
        name: "Альфа α",
        short_label: "",
        tooltip: "Греческая буква α (альфа)",
        shortcut: None,
        icon: ToolIcon::Glyph("α"),
        synonyms: &["альфа", "alpha", "α"],
        action: ToolAction::Math(PaletteAction::Raw("α")),
    },
    ToolItem {
        id: "greek.beta",
        group: PaletteTab::Greek,
        name: "Бета β",
        short_label: "",
        tooltip: "Греческая буква β (бета)",
        shortcut: None,
        icon: ToolIcon::Glyph("β"),
        synonyms: &["бета", "beta", "β"],
        action: ToolAction::Math(PaletteAction::Raw("β")),
    },
    ToolItem {
        id: "greek.gamma",
        group: PaletteTab::Greek,
        name: "Гамма γ",
        short_label: "",
        tooltip: "Греческая буква γ (гамма)",
        shortcut: None,
        icon: ToolIcon::Glyph("γ"),
        synonyms: &["гамма", "gamma", "γ"],
        action: ToolAction::Math(PaletteAction::Raw("γ")),
    },
    ToolItem {
        id: "greek.delta",
        group: PaletteTab::Greek,
        name: "Дельта δ",
        short_label: "",
        tooltip: "Греческая буква δ (дельта)",
        shortcut: None,
        icon: ToolIcon::Glyph("δ"),
        synonyms: &["дельта", "delta", "δ"],
        action: ToolAction::Math(PaletteAction::Raw("δ")),
    },
    ToolItem {
        id: "greek.epsilon",
        group: PaletteTab::Greek,
        name: "Эпсилон ε",
        short_label: "",
        tooltip: "Греческая буква ε (эпсилон)",
        shortcut: None,
        icon: ToolIcon::Glyph("ε"),
        synonyms: &["эпсилон", "epsilon", "ε"],
        action: ToolAction::Math(PaletteAction::Raw("ε")),
    },
    ToolItem {
        id: "greek.zeta",
        group: PaletteTab::Greek,
        name: "Дзета ζ",
        short_label: "",
        tooltip: "Греческая буква ζ (дзета)",
        shortcut: None,
        icon: ToolIcon::Glyph("ζ"),
        synonyms: &["дзета", "zeta", "ζ"],
        action: ToolAction::Math(PaletteAction::Raw("ζ")),
    },
    ToolItem {
        id: "greek.eta",
        group: PaletteTab::Greek,
        name: "Эта η",
        short_label: "",
        tooltip: "Греческая буква η (эта, КПД)",
        shortcut: None,
        icon: ToolIcon::Glyph("η"),
        synonyms: &["эта", "кпд", "eta", "η"],
        action: ToolAction::Math(PaletteAction::Raw("η")),
    },
    ToolItem {
        id: "greek.theta",
        group: PaletteTab::Greek,
        name: "Тета θ",
        short_label: "",
        tooltip: "Греческая буква θ (тета)",
        shortcut: None,
        icon: ToolIcon::Glyph("θ"),
        synonyms: &["тета", "угол", "theta", "θ"],
        action: ToolAction::Math(PaletteAction::Raw("θ")),
    },
    ToolItem {
        id: "greek.lambda",
        group: PaletteTab::Greek,
        name: "Лямбда λ",
        short_label: "",
        tooltip: "Греческая буква λ (лямбда)",
        shortcut: None,
        icon: ToolIcon::Glyph("λ"),
        synonyms: &["лямбда", "lambda", "λ"],
        action: ToolAction::Math(PaletteAction::Raw("λ")),
    },
    ToolItem {
        id: "greek.mu",
        group: PaletteTab::Greek,
        name: "Мю μ",
        short_label: "",
        tooltip: "Греческая буква μ (мю)",
        shortcut: None,
        icon: ToolIcon::Glyph("μ"),
        synonyms: &["мю", "микро", "mu", "μ"],
        action: ToolAction::Math(PaletteAction::Raw("μ")),
    },
    ToolItem {
        id: "greek.nu",
        group: PaletteTab::Greek,
        name: "Ню ν",
        short_label: "",
        tooltip: "Греческая буква ν (ню)",
        shortcut: None,
        icon: ToolIcon::Glyph("ν"),
        synonyms: &["ню", "частота", "nu", "ν"],
        action: ToolAction::Math(PaletteAction::Raw("ν")),
    },
    ToolItem {
        id: "greek.xi",
        group: PaletteTab::Greek,
        name: "Кси ξ",
        short_label: "",
        tooltip: "Греческая буква ξ (кси)",
        shortcut: None,
        icon: ToolIcon::Glyph("ξ"),
        synonyms: &["кси", "xi", "ξ"],
        action: ToolAction::Math(PaletteAction::Raw("ξ")),
    },
    ToolItem {
        id: "greek.pi",
        group: PaletteTab::Greek,
        name: "Пи π",
        short_label: "",
        tooltip: "Греческая буква π (пи)",
        shortcut: None,
        icon: ToolIcon::Glyph("π"),
        synonyms: &["пи", "pi", "π"],
        action: ToolAction::Math(PaletteAction::Raw("π")),
    },
    ToolItem {
        id: "greek.rho",
        group: PaletteTab::Greek,
        name: "Ро ρ",
        short_label: "",
        tooltip: "Греческая буква ρ (ро, плотность)",
        shortcut: None,
        icon: ToolIcon::Glyph("ρ"),
        synonyms: &["ро", "плотность", "rho", "ρ"],
        action: ToolAction::Math(PaletteAction::Raw("ρ")),
    },
    ToolItem {
        id: "greek.sigma",
        group: PaletteTab::Greek,
        name: "Сигма σ",
        short_label: "",
        tooltip: "Греческая буква σ (сигма, напряжение)",
        shortcut: None,
        icon: ToolIcon::Glyph("σ"),
        synonyms: &["сигма", "напряжение", "sigma", "σ"],
        action: ToolAction::Math(PaletteAction::Raw("σ")),
    },
    ToolItem {
        id: "greek.tau",
        group: PaletteTab::Greek,
        name: "Тау τ",
        short_label: "",
        tooltip: "Греческая буква τ (тау)",
        shortcut: None,
        icon: ToolIcon::Glyph("τ"),
        synonyms: &["тау", "tau", "τ"],
        action: ToolAction::Math(PaletteAction::Raw("τ")),
    },
    ToolItem {
        id: "greek.phi",
        group: PaletteTab::Greek,
        name: "Фи φ",
        short_label: "",
        tooltip: "Греческая буква φ (фи)",
        shortcut: None,
        icon: ToolIcon::Glyph("φ"),
        synonyms: &["фи", "фаза", "phi", "φ"],
        action: ToolAction::Math(PaletteAction::Raw("φ")),
    },
    ToolItem {
        id: "greek.psi",
        group: PaletteTab::Greek,
        name: "Пси ψ",
        short_label: "",
        tooltip: "Греческая буква ψ (пси)",
        shortcut: None,
        icon: ToolIcon::Glyph("ψ"),
        synonyms: &["пси", "psi", "ψ"],
        action: ToolAction::Math(PaletteAction::Raw("ψ")),
    },
    ToolItem {
        id: "greek.omega",
        group: PaletteTab::Greek,
        name: "Омега ω",
        short_label: "",
        tooltip: "Греческая буква ω (омега)",
        shortcut: None,
        icon: ToolIcon::Glyph("ω"),
        synonyms: &["омега", "угловая частота", "omega", "ω"],
        action: ToolAction::Math(PaletteAction::Raw("ω")),
    },
    ToolItem {
        id: "greek.cap_delta",
        group: PaletteTab::Greek,
        name: "Дельта заглавная Δ",
        short_label: "",
        tooltip: "Греческая буква Δ (дельта заглавная)",
        shortcut: None,
        icon: ToolIcon::Glyph("Δ"),
        synonyms: &["дельта", "приращение", "delta", "Δ"],
        action: ToolAction::Math(PaletteAction::Raw("Δ")),
    },
    ToolItem {
        id: "greek.cap_theta",
        group: PaletteTab::Greek,
        name: "Тета заглавная Θ",
        short_label: "",
        tooltip: "Греческая буква Θ (тета заглавная)",
        shortcut: None,
        icon: ToolIcon::Glyph("Θ"),
        synonyms: &["тета", "theta", "Θ"],
        action: ToolAction::Math(PaletteAction::Raw("Θ")),
    },
    ToolItem {
        id: "greek.cap_lambda",
        group: PaletteTab::Greek,
        name: "Лямбда заглавная Λ",
        short_label: "",
        tooltip: "Греческая буква Λ (лямбда заглавная)",
        shortcut: None,
        icon: ToolIcon::Glyph("Λ"),
        synonyms: &["лямбда", "lambda", "Λ"],
        action: ToolAction::Math(PaletteAction::Raw("Λ")),
    },
    ToolItem {
        id: "greek.cap_sigma",
        group: PaletteTab::Greek,
        name: "Сигма заглавная Σ",
        short_label: "",
        tooltip: "Греческая буква Σ (сигма заглавная)",
        shortcut: None,
        icon: ToolIcon::Glyph("Σ"),
        synonyms: &["сигма", "sigma", "Σ"],
        action: ToolAction::Math(PaletteAction::Raw("Σ")),
    },
    ToolItem {
        id: "greek.cap_omega",
        group: PaletteTab::Greek,
        name: "Омега заглавная Ω",
        short_label: "",
        tooltip: "Греческая буква Ω (омега заглавная, Ом)",
        shortcut: None,
        icon: ToolIcon::Glyph("Ω"),
        synonyms: &["омега", "ом", "сопротивление", "omega", "Ω"],
        action: ToolAction::Math(PaletteAction::Raw("Ω")),
    },
    // --- Логика ---
    ToolItem {
        id: "logic.eq",
        group: PaletteTab::Logic,
        name: "Равно (булево) ==",
        short_label: "",
        tooltip: "Булево равенство ==\nСравнение и уравнения в блоке Given",
        shortcut: None,
        icon: ToolIcon::Glyph("=="),
        synonyms: &["равно", "равенство", "булево", "=="],
        action: ToolAction::Math(PaletteAction::Raw(" == ")),
    },
    ToolItem {
        id: "logic.neq",
        group: PaletteTab::Logic,
        name: "Не равно ≠",
        short_label: "",
        tooltip: "Не равно (!=)",
        shortcut: None,
        icon: ToolIcon::Glyph("≠"),
        synonyms: &["не равно", "неравенство", "!=", "≠"],
        action: ToolAction::Math(PaletteAction::Raw(" != ")),
    },
    ToolItem {
        id: "logic.lt",
        group: PaletteTab::Logic,
        name: "Меньше <",
        short_label: "",
        tooltip: "Меньше (<)",
        shortcut: None,
        icon: ToolIcon::Glyph("<"),
        synonyms: &["меньше", "сравнение", "<"],
        action: ToolAction::Math(PaletteAction::Raw(" < ")),
    },
    ToolItem {
        id: "logic.le",
        group: PaletteTab::Logic,
        name: "Меньше или равно ≤",
        short_label: "",
        tooltip: "Меньше или равно (<=)",
        shortcut: None,
        icon: ToolIcon::Glyph("≤"),
        synonyms: &["меньше или равно", "<=", "≤"],
        action: ToolAction::Math(PaletteAction::Raw(" <= ")),
    },
    ToolItem {
        id: "logic.gt",
        group: PaletteTab::Logic,
        name: "Больше >",
        short_label: "",
        tooltip: "Больше (>)",
        shortcut: None,
        icon: ToolIcon::Glyph(">"),
        synonyms: &["больше", "сравнение", ">"],
        action: ToolAction::Math(PaletteAction::Raw(" > ")),
    },
    ToolItem {
        id: "logic.ge",
        group: PaletteTab::Logic,
        name: "Больше или равно ≥",
        short_label: "",
        tooltip: "Больше или равно (>=)",
        shortcut: None,
        icon: ToolIcon::Glyph("≥"),
        synonyms: &["больше или равно", ">=", "≥"],
        action: ToolAction::Math(PaletteAction::Raw(" >= ")),
    },
    ToolItem {
        id: "logic.if",
        group: PaletteTab::Logic,
        name: "Условие if(cond, a, b)",
        short_label: "",
        tooltip: "Условное выражение if(cond, a, b)",
        shortcut: None,
        icon: ToolIcon::Badge("if"),
        synonyms: &["условие", "если", "if"],
        action: ToolAction::Math(PaletteAction::Call("if")),
    },
    ToolItem {
        id: "logic.sum",
        group: PaletteTab::Logic,
        name: "Сумма элементов sum(v)",
        short_label: "",
        tooltip: "Сумма элементов массива или диапазона sum(v)",
        shortcut: None,
        icon: ToolIcon::Badge("sum"),
        synonyms: &["сумма элементов", "сумма", "sum"],
        action: ToolAction::Math(PaletteAction::Call("sum")),
    },
    ToolItem {
        id: "logic.mean",
        group: PaletteTab::Logic,
        name: "Среднее значение mean(v)",
        short_label: "",
        tooltip: "Среднее арифметическое mean(v)",
        shortcut: None,
        icon: ToolIcon::Badge("mean"),
        synonyms: &["среднее", "арифметическое", "mean"],
        action: ToolAction::Math(PaletteAction::Call("mean")),
    },
    ToolItem {
        id: "logic.median",
        group: PaletteTab::Logic,
        name: "Медиана median(v)",
        short_label: "",
        tooltip: "Медиана элементов массива median(v)",
        shortcut: None,
        icon: ToolIcon::Badge("med"),
        synonyms: &["медиана", "статистика", "median"],
        action: ToolAction::Math(PaletteAction::Call("median")),
    },
];

pub fn group_meta(tab: PaletteTab) -> &'static GroupMeta {
    GROUPS.iter().find(|g| g.tab == tab).unwrap_or(&GROUPS[0])
}

pub fn tools_in_group(tab: PaletteTab) -> impl Iterator<Item = &'static ToolItem> {
    TOOLS.iter().filter(move |item| item.group == tab)
}

fn match_rank(haystack: &str, needle: &str, prefix_score: u8, sub_score: u8) -> Option<u8> {
    let lower = haystack.to_lowercase();
    if lower == needle {
        Some(prefix_score.saturating_sub(1))
    } else if lower.starts_with(needle) {
        Some(prefix_score)
    } else if lower.contains(needle) {
        Some(sub_score)
    } else {
        None
    }
}

pub fn search_catalog(query: &str) -> Vec<SearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();

    for group in GROUPS {
        let mut best: Option<u8> = None;
        let mut consider = |score: Option<u8>| {
            if let Some(s) = score {
                best = Some(best.map_or(s, |cur| cur.min(s)));
            }
        };
        consider(match_rank(group.title, &needle, 1, 3));
        consider(match_rank(group.tab_label, &needle, 2, 4));
        for syn in group.synonyms {
            consider(match_rank(syn, &needle, 1, 6));
        }
        if let Some(score) = best {
            hits.push(SearchHit {
                target: SearchTarget::Group(group.tab),
                group: group.tab,
                title: group.title,
                subtitle: "Группа инструментов",
                icon: group.icon,
                score,
            });
        }
    }

    for item in TOOLS {
        let group = group_meta(item.group);
        let mut best: Option<u8> = None;
        let mut consider = |score: Option<u8>| {
            if let Some(s) = score {
                best = Some(best.map_or(s, |cur| cur.min(s)));
            }
        };
        consider(match_rank(item.name, &needle, 2, 5));
        if !item.short_label.is_empty() {
            consider(match_rank(item.short_label, &needle, 4, 7));
        }
        for syn in item.synonyms {
            consider(match_rank(syn, &needle, 4, 7));
        }
        consider(match_rank(item.tooltip, &needle, 9, 10));
        consider(match_rank(group.title, &needle, 11, 12));
        for syn in group.synonyms {
            consider(match_rank(syn, &needle, 12, 13));
        }
        if let Some(score) = best {
            hits.push(SearchHit {
                target: SearchTarget::Item(item.id),
                group: item.group,
                title: item.name,
                subtitle: group.title,
                icon: item.icon,
                score,
            });
        }
    }

    hits.sort_by_key(|h| h.score);
    hits
}

pub fn paint_icon(painter: &Painter, rect: Rect, icon: ToolIcon, fg: Color32, accent: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new(1.35_f32, fg);
    let thin = Stroke::new(1.1_f32, fg);
    let acc_stroke = Stroke::new(1.4_f32, accent);
    match icon {
        ToolIcon::Fraction => {
            let box_w = (s * 0.36).max(5.0);
            let box_h = (s * 0.28).max(4.0);
            let top_r = Rect::from_center_size(Pos2::new(c.x, c.y - s * 0.28), vec2(box_w, box_h));
            let bot_r = Rect::from_center_size(Pos2::new(c.x, c.y + s * 0.28), vec2(box_w, box_h));
            painter.rect_stroke(top_r, 1.0, thin, StrokeKind::Inside);
            painter.rect_stroke(bot_r, 1.0, thin, StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(c.x - s * 0.40, c.y),
                    Pos2::new(c.x + s * 0.40, c.y),
                ],
                acc_stroke,
            );
        }
        ToolIcon::Power => {
            painter.text(
                Pos2::new(c.x - s * 0.16, c.y + s * 0.10),
                Align2::CENTER_CENTER,
                "x",
                FontId::proportional((s * 0.72).max(10.0)),
                fg,
            );
            let exp_r = Rect::from_center_size(
                Pos2::new(c.x + s * 0.24, c.y - s * 0.22),
                vec2((s * 0.30).max(4.5), (s * 0.30).max(4.5)),
            );
            painter.rect_stroke(exp_r, 1.0, acc_stroke, StrokeKind::Inside);
        }
        ToolIcon::Sqrt => {
            let pts = vec![
                Pos2::new(c.x - s * 0.42, c.y + s * 0.05),
                Pos2::new(c.x - s * 0.28, c.y + s * 0.02),
                Pos2::new(c.x - s * 0.14, c.y + s * 0.34),
                Pos2::new(c.x + s * 0.02, c.y - s * 0.32),
                Pos2::new(c.x + s * 0.42, c.y - s * 0.32),
            ];
            painter.add(Shape::line(pts, acc_stroke));
            painter.text(
                Pos2::new(c.x + s * 0.22, c.y + s * 0.08),
                Align2::CENTER_CENTER,
                "x",
                FontId::proportional((s * 0.56).max(9.0)),
                fg,
            );
        }
        ToolIcon::Abs => {
            let x1 = c.x - s * 0.30;
            let x2 = c.x + s * 0.30;
            painter.line_segment(
                [Pos2::new(x1, c.y - s * 0.36), Pos2::new(x1, c.y + s * 0.36)],
                acc_stroke,
            );
            painter.line_segment(
                [Pos2::new(x2, c.y - s * 0.36), Pos2::new(x2, c.y + s * 0.36)],
                acc_stroke,
            );
            painter.text(
                c,
                Align2::CENTER_CENTER,
                "x",
                FontId::proportional((s * 0.62).max(9.0)),
                fg,
            );
        }
        ToolIcon::LiteralSub => {
            painter.text(
                Pos2::new(c.x - s * 0.16, c.y - s * 0.04),
                Align2::CENTER_CENTER,
                "x",
                FontId::proportional((s * 0.72).max(10.0)),
                fg,
            );
            let sub_r = Rect::from_center_size(
                Pos2::new(c.x + s * 0.24, c.y + s * 0.24),
                vec2((s * 0.28).max(4.5), (s * 0.28).max(4.5)),
            );
            painter.rect_stroke(sub_r, 1.0, thin, StrokeKind::Inside);
        }
        ToolIcon::IndexSub => {
            painter.text(
                Pos2::new(c.x - s * 0.18, c.y - s * 0.04),
                Align2::CENTER_CENTER,
                "A",
                FontId::proportional((s * 0.66).max(9.5)),
                fg,
            );
            let sub_r = Rect::from_center_size(
                Pos2::new(c.x + s * 0.24, c.y + s * 0.22),
                vec2((s * 0.30).max(5.0), (s * 0.30).max(5.0)),
            );
            painter.rect_filled(sub_r, 1.0, accent);
        }
        ToolIcon::Integral => {
            painter.text(
                Pos2::new(c.x - s * 0.12, c.y),
                Align2::CENTER_CENTER,
                "∫",
                FontId::proportional((s * 0.96).max(13.0)),
                accent,
            );
            let lim = (s * 0.22).max(3.5);
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x + s * 0.24, c.y - s * 0.28), vec2(lim, lim)),
                0.5,
                thin,
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x + s * 0.20, c.y + s * 0.28), vec2(lim, lim)),
                0.5,
                thin,
                StrokeKind::Inside,
            );
        }
        ToolIcon::Derivative => {
            painter.text(
                Pos2::new(c.x, c.y - s * 0.26),
                Align2::CENTER_CENTER,
                "d",
                FontId::proportional((s * 0.48).max(8.5)),
                fg,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - s * 0.36, c.y),
                    Pos2::new(c.x + s * 0.36, c.y),
                ],
                acc_stroke,
            );
            painter.text(
                Pos2::new(c.x, c.y + s * 0.28),
                Align2::CENTER_CENTER,
                "dx",
                FontId::proportional((s * 0.44).max(8.0)),
                fg,
            );
        }
        ToolIcon::Sum => {
            let pts = vec![
                Pos2::new(c.x + s * 0.32, c.y - s * 0.34),
                Pos2::new(c.x - s * 0.32, c.y - s * 0.34),
                Pos2::new(c.x + s * 0.04, c.y),
                Pos2::new(c.x - s * 0.32, c.y + s * 0.34),
                Pos2::new(c.x + s * 0.32, c.y + s * 0.34),
            ];
            painter.add(Shape::line(pts, acc_stroke));
        }
        ToolIcon::Product => {
            let x1 = c.x - s * 0.24;
            let x2 = c.x + s * 0.24;
            let y1 = c.y - s * 0.32;
            let y2 = c.y + s * 0.34;
            painter.line_segment(
                [Pos2::new(x1 - 2.0, y1), Pos2::new(x2 + 2.0, y1)],
                acc_stroke,
            );
            painter.line_segment([Pos2::new(x1, y1), Pos2::new(x1, y2)], acc_stroke);
            painter.line_segment([Pos2::new(x2, y1), Pos2::new(x2, y2)], acc_stroke);
        }
        ToolIcon::MatrixGrid(rows, cols) => {
            let lx = c.x - s * 0.38;
            let rx = c.x + s * 0.38;
            let ty = c.y - s * 0.34;
            let by = c.y + s * 0.34;
            let hook = (s * 0.14).max(2.5);
            painter.add(Shape::line(
                vec![
                    Pos2::new(lx + hook, ty),
                    Pos2::new(lx, ty),
                    Pos2::new(lx, by),
                    Pos2::new(lx + hook, by),
                ],
                stroke,
            ));
            painter.add(Shape::line(
                vec![
                    Pos2::new(rx - hook, ty),
                    Pos2::new(rx, ty),
                    Pos2::new(rx, by),
                    Pos2::new(rx - hook, by),
                ],
                stroke,
            ));
            let r_cnt = rows.clamp(2, 3);
            let c_cnt = cols.clamp(2, 3);
            let inner_w = (rx - lx) - 6.0;
            let inner_h = (by - ty) - 4.0;
            for r in 0..r_cnt {
                let fy = if r_cnt == 1 {
                    0.5
                } else {
                    r as f32 / (r_cnt - 1) as f32
                };
                for col in 0..c_cnt {
                    let fx = if c_cnt == 1 {
                        0.5
                    } else {
                        col as f32 / (c_cnt - 1) as f32
                    };
                    let pt = Pos2::new(lx + 3.0 + fx * inner_w, ty + 2.0 + fy * inner_h);
                    painter.circle_filled(pt, if r_cnt == 3 { 1.25 } else { 1.6 }, accent);
                }
            }
        }
        ToolIcon::VectorCol => {
            let lx = c.x - s * 0.24;
            let rx = c.x + s * 0.24;
            let ty = c.y - s * 0.36;
            let by = c.y + s * 0.36;
            let hook = 2.5;
            painter.add(Shape::line(
                vec![
                    Pos2::new(lx + hook, ty),
                    Pos2::new(lx, ty),
                    Pos2::new(lx, by),
                    Pos2::new(lx + hook, by),
                ],
                stroke,
            ));
            painter.add(Shape::line(
                vec![
                    Pos2::new(rx - hook, ty),
                    Pos2::new(rx, ty),
                    Pos2::new(rx, by),
                    Pos2::new(rx - hook, by),
                ],
                stroke,
            ));
            for dy in [-0.20_f32, 0.0, 0.20] {
                painter.circle_filled(Pos2::new(c.x, c.y + s * dy), 1.5, accent);
            }
        }
        ToolIcon::TableGrid => {
            let r = Rect::from_center_size(c, vec2(s * 0.78, s * 0.68));
            painter.rect_stroke(r, 1.5, stroke, StrokeKind::Inside);
            let h_y = r.top() + r.height() * 0.34;
            painter.line_segment(
                [Pos2::new(r.left(), h_y), Pos2::new(r.right(), h_y)],
                acc_stroke,
            );
            let mid_x = r.center().x;
            painter.line_segment([Pos2::new(mid_x, h_y), Pos2::new(mid_x, r.bottom())], thin);
            let r2_y = r.top() + r.height() * 0.67;
            painter.line_segment(
                [Pos2::new(r.left(), r2_y), Pos2::new(r.right(), r2_y)],
                thin,
            );
        }
        ToolIcon::PlotXY => {
            let l = c.x - s * 0.36;
            let r = c.x + s * 0.38;
            let t = c.y - s * 0.34;
            let b = c.y + s * 0.34;
            painter.line_segment([Pos2::new(l, t), Pos2::new(l, b)], thin);
            painter.line_segment([Pos2::new(l, b), Pos2::new(r, b)], thin);
            let curve = vec![
                Pos2::new(l + 1.5, b - s * 0.16),
                Pos2::new(l + s * 0.20, t + s * 0.12),
                Pos2::new(l + s * 0.42, b - s * 0.10),
                Pos2::new(r - 1.0, t + s * 0.14),
            ];
            painter.add(Shape::line(curve, acc_stroke));
        }
        ToolIcon::PlotVectors => {
            let l = c.x - s * 0.36;
            let r = c.x + s * 0.38;
            let t = c.y - s * 0.34;
            let b = c.y + s * 0.34;
            painter.line_segment([Pos2::new(l, t), Pos2::new(l, b)], thin);
            painter.line_segment([Pos2::new(l, b), Pos2::new(r, b)], thin);
            let pts = [
                Pos2::new(l + s * 0.14, b - s * 0.14),
                Pos2::new(l + s * 0.34, c.y - s * 0.06),
                Pos2::new(l + s * 0.56, t + s * 0.14),
            ];
            painter.add(Shape::line(pts.to_vec(), thin));
            for p in pts {
                painter.circle_filled(p, 2.0, accent);
            }
        }
        ToolIcon::PlotParametric => {
            let l = c.x - s * 0.36;
            let r = c.x + s * 0.38;
            let t = c.y - s * 0.34;
            let b = c.y + s * 0.34;
            painter.line_segment([Pos2::new(l, c.y), Pos2::new(r, c.y)], thin);
            painter.line_segment([Pos2::new(c.x, t), Pos2::new(c.x, b)], thin);
            painter.circle_stroke(c, s * 0.26, acc_stroke);
        }
        ToolIcon::PlotPolar => {
            painter.circle_stroke(c, s * 0.34, thin);
            painter.circle_stroke(c, s * 0.17, thin);
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y),
                    Pos2::new(c.x + s * 0.28, c.y - s * 0.22),
                ],
                acc_stroke,
            );
            painter.circle_filled(Pos2::new(c.x + s * 0.26, c.y - s * 0.20), 1.8, accent);
        }
        ToolIcon::Plot3d => {
            let p_top = Pos2::new(c.x, c.y - s * 0.34);
            let p_right = Pos2::new(c.x + s * 0.38, c.y - s * 0.04);
            let p_bot = Pos2::new(c.x, c.y + s * 0.34);
            let p_left = Pos2::new(c.x - s * 0.38, c.y - s * 0.04);
            painter.add(Shape::line(
                vec![p_top, p_right, p_bot, p_left, p_top],
                acc_stroke,
            ));
            painter.line_segment([p_left, p_right], thin);
            painter.line_segment([p_top, p_bot], thin);
        }
        ToolIcon::PlotContour => {
            let r = Rect::from_center_size(c, vec2(s * 0.76, s * 0.68));
            painter.rect_stroke(r, 1.5, thin, StrokeKind::Inside);
            painter.circle_stroke(c, s * 0.24, acc_stroke);
            painter.circle_stroke(c, s * 0.11, stroke);
        }
        ToolIcon::SymArrow => {
            let y = c.y;
            let x1 = c.x - s * 0.38;
            let x2 = c.x + s * 0.36;
            painter.line_segment([Pos2::new(x1, y), Pos2::new(x2, y)], acc_stroke);
            painter.add(Shape::line(
                vec![
                    Pos2::new(x2 - s * 0.22, y - s * 0.20),
                    Pos2::new(x2, y),
                    Pos2::new(x2 - s * 0.22, y + s * 0.20),
                ],
                acc_stroke,
            ));
        }
        ToolIcon::ProgramBar => {
            let bx = c.x - s * 0.30;
            painter.line_segment(
                [Pos2::new(bx, c.y - s * 0.36), Pos2::new(bx, c.y + s * 0.36)],
                Stroke::new(2.1_f32, accent),
            );
            for (dy, len) in [(-0.22_f32, 0.56_f32), (0.0, 0.42), (0.22, 0.50)] {
                let y = c.y + s * dy;
                painter.line_segment([Pos2::new(bx + 3.5, y), Pos2::new(bx + s * len, y)], thin);
            }
        }
        ToolIcon::AddProgramLine => {
            let bx = c.x - s * 0.30;
            painter.line_segment(
                [Pos2::new(bx, c.y - s * 0.34), Pos2::new(bx, c.y + s * 0.34)],
                Stroke::new(2.0_f32, fg),
            );
            let px = c.x + s * 0.12;
            let arm = s * 0.22;
            painter.line_segment(
                [Pos2::new(px - arm, c.y), Pos2::new(px + arm, c.y)],
                acc_stroke,
            );
            painter.line_segment(
                [Pos2::new(px, c.y - arm), Pos2::new(px, c.y + arm)],
                acc_stroke,
            );
        }
        ToolIcon::GreekBadge => {
            painter.text(
                c,
                Align2::CENTER_CENTER,
                "αβγ",
                FontId::proportional((s * 0.54).max(9.5)),
                accent,
            );
        }
        ToolIcon::LogicBadge => {
            painter.text(
                c,
                Align2::CENTER_CENTER,
                "≤≠",
                FontId::proportional((s * 0.58).max(10.0)),
                accent,
            );
        }
        ToolIcon::Search => {
            let lens_c = Pos2::new(c.x - s * 0.10, c.y - s * 0.10);
            let radius = (s * 0.27).max(3.8);
            painter.circle_stroke(lens_c, radius, Stroke::new(1.5_f32, fg));
            let offset = radius * 0.72;
            painter.line_segment(
                [
                    Pos2::new(lens_c.x + offset, lens_c.y + offset),
                    Pos2::new(c.x + s * 0.36, c.y + s * 0.36),
                ],
                Stroke::new(1.7_f32, accent),
            );
        }
        ToolIcon::Glyph(glyph) => {
            let scale = if glyph.chars().count() > 2 {
                0.58
            } else {
                0.74
            };
            painter.text(
                c,
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional((s * scale).max(10.0)),
                fg,
            );
        }
        ToolIcon::Badge(label) => {
            let scale = match label.chars().count() {
                0..=2 => 0.62,
                3..=4 => 0.52,
                _ => 0.45,
            };
            painter.text(
                c,
                Align2::CENTER_CENTER,
                label,
                FontId::proportional((s * scale).max(9.0)),
                fg,
            );
        }
    }
}

const GROUP_BUTTON_WIDTH: f32 = 34.0;
const GROUP_SPACING: f32 = 4.0;

fn paint_group_button(
    ui: &mut egui::Ui,
    group: &GroupMeta,
    active: bool,
    highlighted: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(GROUP_BUTTON_WIDTH, 25.0), Sense::click());
    let hovered = response.hovered();
    let (fill, border, icon_accent) = if highlighted {
        (
            SEARCH_HIT_BG,
            Stroke::new(2.0_f32, SEARCH_HIT_BORDER),
            SEARCH_HIT_BADGE,
        )
    } else if active {
        (
            BLUE_ACCENT_SOFT,
            Stroke::new(1.2_f32, BLUE_ACCENT_BORDER),
            TEXT_WHITE,
        )
    } else if hovered {
        (
            PANEL_HOVER,
            Stroke::new(1.0_f32, BORDER_DARK),
            BLUE_ACCENT_BORDER,
        )
    } else {
        (
            PANEL_SURFACE,
            Stroke::new(1.0_f32, Color32::TRANSPARENT),
            BLUE_ACCENT_BORDER,
        )
    };
    ui.painter()
        .rect(rect, 5.0, fill, border, StrokeKind::Inside);
    if active {
        let bar_color = if highlighted {
            BLUE_ACCENT_BORDER
        } else {
            BLUE_ACCENT
        };
        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + 6.0, rect.bottom() - 1.5),
                Pos2::new(rect.right() - 6.0, rect.bottom() - 1.5),
            ],
            Stroke::new(2.4_f32, bar_color),
        );
    }
    if highlighted {
        ui.painter().circle_filled(
            Pos2::new(rect.right() - 6.0, rect.top() + 6.0),
            3.0,
            SEARCH_HIT_BADGE,
        );
    }
    let icon_rect = Rect::from_center_size(rect.center(), vec2(17.0, 17.0));
    paint_icon(ui.painter(), icon_rect, group.icon, TEXT_WHITE, icon_accent);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, group.title));
    response.on_hover_text(format!("{}\n{}", group.title, group.tooltip))
}

fn paint_tool_button(
    ui: &mut egui::Ui,
    item: &ToolItem,
    highlighted: bool,
    scroll_to_me: bool,
    enabled: bool,
) -> egui::Response {
    let has_label = !item.short_label.is_empty();
    let width = if has_label {
        let chars = item.short_label.chars().count() as f32;
        (30.0 + chars * 7.0).clamp(52.0, 96.0)
    } else if matches!(item.icon, ToolIcon::Badge(lbl) if lbl.chars().count() >= 5) {
        50.0
    } else if item.group == PaletteTab::Greek {
        30.0
    } else {
        38.0
    };
    let height = 28.0;
    let (rect, response) = ui
        .add_enabled_ui(enabled, |ui| {
            ui.allocate_exact_size(vec2(width, height), Sense::click())
        })
        .inner;
    let ink = if enabled {
        TEXT_WHITE
    } else {
        TEXT_MUTED.gamma_multiply(0.65)
    };
    if scroll_to_me {
        response.scroll_to_me(Some(Align::Center));
    }
    let hovered = response.hovered();
    let (fill, border, icon_accent) = if highlighted {
        (
            SEARCH_HIT_BG,
            Stroke::new(2.2_f32, SEARCH_HIT_BORDER),
            SEARCH_HIT_BADGE,
        )
    } else if hovered {
        (
            PANEL_HOVER,
            Stroke::new(1.1_f32, BLUE_ACCENT_BORDER),
            BLUE_ACCENT_BORDER,
        )
    } else {
        (
            PANEL_ELEVATED,
            Stroke::new(1.0_f32, BORDER_DARK),
            BLUE_ACCENT_BORDER,
        )
    };
    ui.painter()
        .rect(rect, 5.0, fill, border, StrokeKind::Inside);
    if highlighted {
        ui.painter().circle_filled(
            Pos2::new(rect.right() - 5.0, rect.top() + 5.0),
            2.8,
            SEARCH_HIT_BADGE,
        );
    }
    if has_label {
        let icon_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 6.0, rect.top() + 5.0),
            vec2(18.0, 18.0),
        );
        paint_icon(ui.painter(), icon_rect, item.icon, ink, icon_accent);
        ui.painter().text(
            Pos2::new(icon_rect.right() + 4.0, rect.center().y),
            Align2::LEFT_CENTER,
            item.short_label,
            FontId::proportional(11.5),
            ink,
        );
    } else {
        let icon_rect = rect.shrink2(vec2(4.0, 3.0));
        paint_icon(ui.painter(), icon_rect, item.icon, ink, icon_accent);
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, item.name));
    let tip = match item.shortcut {
        Some(key) if !item.tooltip.contains(key) => format!("{} ({key})", item.tooltip),
        _ => item.tooltip.to_owned(),
    };
    response
        .on_disabled_hover_text(format!(
            "{tip}
Сначала выберите 2D-график"
        ))
        .on_hover_text(tip)
}

#[allow(clippy::too_many_arguments)]
pub fn show_ribbon(
    ctx: &egui::Context,
    active_tab: &mut PaletteTab,
    expanded: &mut bool,
    mat_rows: &mut usize,
    mat_cols: &mut usize,
    search: &mut SearchState,
    selected_chart: bool,
) -> Option<ToolAction> {
    let mut triggered: Option<ToolAction> = None;
    let mut search_bar_rect: Option<Rect> = None;
    let mut popup_rect: Option<Rect> = None;
    let mut hits = search_catalog(&search.query);
    let mut scroll_selection = false;
    if !hits.is_empty() && search.selected_idx >= hits.len() {
        search.selected_idx = 0;
    }

    let highlighted_group = match search.highlight {
        Some(SearchHighlight::Group(tab)) => Some(tab),
        Some(SearchHighlight::Item { group, .. }) => Some(group),
        None => None,
    };
    let highlighted_item_id = match search.highlight {
        Some(SearchHighlight::Item { item_id, .. }) => Some(item_id),
        _ => None,
    };

    let panel_rect = egui::TopBottomPanel::top("math_ribbon")
        .frame(
            Frame::new()
                .fill(PANEL_DARK)
                .stroke(Stroke::new(1.0_f32, BORDER_DARK))
                .inner_margin(Margin::symmetric(10, 3)),
        )
        .show(ctx, |ui| {
            // --- Row 1: Group tabs + overflow "Ещё" + search field ---
            let avail_w = ui.available_width();
            let search_w = if avail_w < 860.0 { 155.0 } else { 196.0 };
            let search_total_w = search_w + 56.0;
            let toggle_w = 26.0;
            let tabs_budget = (avail_w - search_total_w - toggle_w - 16.0).max(0.0);
            let group_step = GROUP_BUTTON_WIDTH + GROUP_SPACING;
            let all_groups_width = group_step * GROUPS.len() as f32 - GROUP_SPACING;
            let visible_count = if all_groups_width <= tabs_budget {
                GROUPS.len()
            } else {
                ((tabs_budget - 78.0) / group_step).max(0.0) as usize
            };

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GROUP_SPACING;
                for group in &GROUPS[..visible_count] {
                    let is_active = *active_tab == group.tab;
                    let is_hl = highlighted_group == Some(group.tab);
                    if paint_group_button(ui, group, is_active, is_hl).clicked() {
                        if *active_tab == group.tab {
                            *expanded = !*expanded;
                        } else {
                            *active_tab = group.tab;
                            *expanded = true;
                        }
                    }
                }

                if visible_count < GROUPS.len() {
                    let overflow_active =
                        GROUPS[visible_count..].iter().any(|g| g.tab == *active_tab);
                    let overflow_hl = GROUPS[visible_count..]
                        .iter()
                        .any(|g| highlighted_group == Some(g.tab));
                    let label_color = if overflow_hl {
                        SEARCH_HIT_BADGE
                    } else if overflow_active {
                        BLUE_ACCENT_BORDER
                    } else {
                        TEXT_WHITE
                    };
                    let more_text = if overflow_hl {
                        "Ещё • ▾"
                    } else {
                        "Ещё ▾"
                    };
                    ui.menu_button(
                        RichText::new(more_text)
                            .size(12.0)
                            .color(label_color)
                            .strong(),
                        |ui| {
                            for group in &GROUPS[visible_count..] {
                                let is_active = *active_tab == group.tab;
                                let is_hl = highlighted_group == Some(group.tab);
                                let prefix = if is_hl {
                                    "★ "
                                } else if is_active {
                                    "▸ "
                                } else {
                                    "  "
                                };
                                let color = if is_hl { SEARCH_HIT_BADGE } else { TEXT_WHITE };
                                let btn = egui::Button::selectable(
                                    is_active,
                                    RichText::new(format!("{prefix}{}", group.title)).color(color),
                                );
                                if ui.add(btn).on_hover_text(group.tooltip).clicked() {
                                    *active_tab = group.tab;
                                    *expanded = true;
                                    ui.close();
                                }
                            }
                        },
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    // Collapse/expand toggle for row 2
                    let arrow = if *expanded { "▴" } else { "▾" };
                    let toggle_tip = if *expanded {
                        "Свернуть панель инструментов"
                    } else {
                        "Развернуть панель инструментов"
                    };
                    let toggle_resp = ui
                        .add_sized([24.0, 24.0], egui::Button::new(arrow).small())
                        .on_hover_text(toggle_tip);
                    toggle_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, toggle_tip)
                    });
                    if toggle_resp.clicked() {
                        *expanded = !*expanded;
                    }

                    // Clear button when query is non-empty
                    if !search.query.is_empty() {
                        let clr = ui
                            .add_sized([22.0, 24.0], egui::Button::new("×").small())
                            .on_hover_text("Очистить поиск и сбросить подсветку");
                        clr.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Очистить поиск инструментов",
                            )
                        });
                        if clr.clicked() {
                            search.clear();
                        }
                    }

                    // Search text input
                    let search_id = SearchState::input_id();
                    let text_resp = ui.add_sized(
                        [search_w, 24.0],
                        TextEdit::singleline(&mut search.query)
                            .id(search_id)
                            .hint_text("Поиск инструментов")
                            .char_limit(80),
                    );

                    if search.focus_requested {
                        text_resp.request_focus();
                        search.focus_requested = false;
                    }
                    if text_resp.gained_focus() && !search.query.trim().is_empty() {
                        search.open = true;
                    }
                    if text_resp.changed() {
                        search.selected_idx = 0;
                        hits = search_catalog(&search.query);
                        scroll_selection = true;
                        if search.query.trim().is_empty() {
                            search.highlight = None;
                            search.open = false;
                        } else {
                            search.open = true;
                        }
                    }

                    // Magnifier button
                    let (lens_rect, lens_resp) =
                        ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
                    let lens_fill = if search.open || text_resp.has_focus() {
                        BLUE_ACCENT_SOFT
                    } else if lens_resp.hovered() {
                        PANEL_HOVER
                    } else {
                        PANEL_ELEVATED
                    };
                    ui.painter().rect(
                        lens_rect,
                        5.0,
                        lens_fill,
                        Stroke::new(1.0_f32, BORDER_DARK),
                        StrokeKind::Inside,
                    );
                    paint_icon(
                        ui.painter(),
                        lens_rect.shrink(4.0),
                        ToolIcon::Search,
                        TEXT_WHITE,
                        BLUE_ACCENT_BORDER,
                    );
                    lens_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Поиск инструментов",
                        )
                    });
                    if lens_resp
                        .on_hover_text("Поиск инструментов (Ctrl+K)")
                        .clicked()
                    {
                        search.request_focus(ctx);
                    }

                    search_bar_rect = Some(lens_rect.union(text_resp.rect));

                    // Keyboard handling inside search input
                    if text_resp.has_focus() {
                        search.has_focus = true;
                    }
                    let search_active = text_resp.has_focus() || text_resp.lost_focus();
                    if search_active {
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            search.close_and_restore_focus(ctx);
                        } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) && !hits.is_empty() {
                            search.open = true;
                            search.selected_idx = (search.selected_idx + 1) % hits.len();
                            scroll_selection = true;
                        } else if ui.input(|i| i.key_pressed(Key::ArrowUp)) && !hits.is_empty() {
                            search.open = true;
                            search.selected_idx =
                                (search.selected_idx + hits.len() - 1) % hits.len();
                            scroll_selection = true;
                        } else if ui.input(|i| i.key_pressed(Key::Enter)) {
                            search.enter_consumed = true;
                            if let Some(&hit) = hits.get(search.selected_idx) {
                                search.reveal_hit(hit, active_tab, expanded);
                                ctx.memory_mut(|m| m.surrender_focus(search_id));
                            } else if !search.query.trim().is_empty() {
                                search.open = true;
                            }
                        }
                    } else if search.open && ui.input(|i| i.key_pressed(Key::Escape)) {
                        search.close_and_restore_focus(ctx);
                    }
                });
            });

            // --- Row 2: Active group tool buttons ---
            if *expanded {
                ui.add_space(3.0);
                let active_meta = group_meta(*active_tab);
                Frame::new()
                    .fill(PANEL_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER_DARK))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt(("ribbon_tools_scroll", active_meta.id))
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    ui.label(
                                        RichText::new(active_meta.title)
                                            .size(11.5)
                                            .strong()
                                            .color(BLUE_ACCENT_BORDER),
                                    );
                                    ui.separator();

                                    // Matrix dimension controls directly inside "Вектор и матрица"
                                    if *active_tab == PaletteTab::Matrix {
                                        ui.label(
                                            RichText::new("Размер:").size(11.5).color(TEXT_MUTED),
                                        );
                                        ui.add(
                                            egui::DragValue::new(mat_rows)
                                                .range(1..=6)
                                                .prefix("стр "),
                                        );
                                        ui.label(RichText::new("×").size(12.0).color(TEXT_MUTED));
                                        ui.add(
                                            egui::DragValue::new(mat_cols)
                                                .range(1..=6)
                                                .prefix("стлб "),
                                        );
                                    }

                                    for item in tools_in_group(*active_tab) {
                                        let is_hl = highlighted_item_id == Some(item.id);
                                        let should_scroll = is_hl && search.scroll_to_highlight;
                                        let resp = if item.id == "mat.custom" {
                                            paint_custom_matrix_button(
                                                ui,
                                                item,
                                                *mat_rows,
                                                *mat_cols,
                                                is_hl,
                                                should_scroll,
                                            )
                                        } else {
                                            let enabled = selected_chart
                                                || !matches!(
                                                    item.action,
                                                    ToolAction::ChartZoom | ToolAction::ChartTrace
                                                );
                                            paint_tool_button(
                                                ui,
                                                item,
                                                is_hl,
                                                should_scroll,
                                                enabled,
                                            )
                                        };
                                        if should_scroll {
                                            search.scroll_to_highlight = false;
                                        }
                                        if resp.clicked() {
                                            triggered = Some(item.action);
                                        }
                                    }
                                });
                            });
                    });
            }
        })
        .response
        .rect;

    // --- Search dropdown overlay ---
    if search.open && !search.query.trim().is_empty() {
        if let Some(anchor) = search_bar_rect {
            let popup_w = 320.0_f32;
            let screen_w = panel_rect.right();
            let x = (anchor.right() - popup_w)
                .min(screen_w - popup_w - 8.0)
                .max(8.0);
            let y = anchor.bottom() + 4.0;
            let area_resp = egui::Area::new(egui::Id::new("tool_search_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(x, y))
                .show(ctx, |ui| {
                    Frame::new()
                        .fill(PANEL_ELEVATED)
                        .stroke(Stroke::new(1.2_f32, BLUE_ACCENT_BORDER))
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(Margin::same(6))
                        .show(ui, |ui| {
                            ui.set_width(popup_w - 12.0);
                            if hits.is_empty() {
                                ui.label(
                                    RichText::new("Ничего не найдено")
                                        .strong()
                                        .size(12.5)
                                        .color(TEXT_WHITE),
                                );
                                ui.label(
                                    RichText::new("Попробуйте: дроб, матр, интег, граф")
                                        .size(11.0)
                                        .color(TEXT_MUTED),
                                );
                                return;
                            }
                            ui.label(
                                RichText::new(
                                    "Нажмите Enter или выберите строку, чтобы показать инструмент",
                                )
                                .size(10.5)
                                .color(TEXT_MUTED),
                            );
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical()
                                .id_salt("tool_search_results_scroll")
                                .max_height(260.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for (idx, hit) in hits.iter().enumerate() {
                                        let is_sel = idx == search.selected_idx;
                                        let (row_rect, row_resp) = ui.allocate_exact_size(
                                            vec2(ui.available_width(), 28.0),
                                            Sense::click(),
                                        );
                                        if is_sel && scroll_selection {
                                            row_resp.scroll_to_me(Some(Align::Center));
                                        }
                                        let row_fill = if is_sel {
                                            SEARCH_HIT_BG
                                        } else if row_resp.hovered() {
                                            PANEL_HOVER
                                        } else {
                                            Color32::TRANSPARENT
                                        };
                                        let row_border = if is_sel {
                                            Stroke::new(1.2_f32, SEARCH_HIT_BORDER)
                                        } else {
                                            Stroke::NONE
                                        };
                                        ui.painter().rect(
                                            row_rect,
                                            5.0,
                                            row_fill,
                                            row_border,
                                            StrokeKind::Inside,
                                        );
                                        let ico = Rect::from_min_size(
                                            Pos2::new(row_rect.left() + 6.0, row_rect.top() + 5.0),
                                            vec2(18.0, 18.0),
                                        );
                                        paint_icon(
                                            ui.painter(),
                                            ico,
                                            hit.icon,
                                            TEXT_WHITE,
                                            if is_sel {
                                                SEARCH_HIT_BADGE
                                            } else {
                                                BLUE_ACCENT_BORDER
                                            },
                                        );
                                        ui.painter().text(
                                            Pos2::new(ico.right() + 6.0, row_rect.center().y),
                                            Align2::LEFT_CENTER,
                                            hit.title,
                                            FontId::proportional(12.0),
                                            TEXT_WHITE,
                                        );
                                        ui.painter().text(
                                            Pos2::new(row_rect.right() - 6.0, row_rect.center().y),
                                            Align2::RIGHT_CENTER,
                                            hit.subtitle,
                                            FontId::proportional(10.5),
                                            if matches!(hit.target, SearchTarget::Group(_)) {
                                                SEARCH_HIT_BADGE
                                            } else {
                                                TEXT_MUTED
                                            },
                                        );
                                        row_resp.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::WidgetType::Button,
                                                true,
                                                format!("{} — {}", hit.title, hit.subtitle),
                                            )
                                        });
                                        if row_resp.clicked() {
                                            search.reveal_hit(*hit, active_tab, expanded);
                                            ctx.memory_mut(|m| {
                                                m.surrender_focus(SearchState::input_id())
                                            });
                                        }
                                    }
                                });
                        });
                });
            popup_rect = Some(area_resp.response.rect);
        }

        // Close dropdown if the user clicked outside both search bar and popup
        if ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let in_bar = search_bar_rect.is_some_and(|r| r.contains(pos));
                let in_popup = popup_rect.is_some_and(|r| r.contains(pos));
                if !in_bar && !in_popup {
                    search.open = false;
                }
            }
        }
    }

    triggered
}

fn paint_custom_matrix_button(
    ui: &mut egui::Ui,
    item: &ToolItem,
    rows: usize,
    cols: usize,
    highlighted: bool,
    scroll_to_me: bool,
) -> egui::Response {
    let label = format!("Вставить [{rows}×{cols}]");
    let width = 112.0;
    let height = 28.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if scroll_to_me {
        response.scroll_to_me(Some(Align::Center));
    }
    let hovered = response.hovered();
    let (fill, border, icon_accent) = if highlighted {
        (
            SEARCH_HIT_BG,
            Stroke::new(2.2_f32, SEARCH_HIT_BORDER),
            SEARCH_HIT_BADGE,
        )
    } else if hovered {
        (
            BLUE_ACCENT,
            Stroke::new(1.2_f32, BLUE_ACCENT_BORDER),
            TEXT_WHITE,
        )
    } else {
        (
            BLUE_ACCENT_SOFT,
            Stroke::new(1.0_f32, BLUE_ACCENT_BORDER),
            TEXT_WHITE,
        )
    };
    ui.painter()
        .rect(rect, 5.0, fill, border, StrokeKind::Inside);
    if highlighted {
        ui.painter().circle_filled(
            Pos2::new(rect.right() - 5.0, rect.top() + 5.0),
            2.8,
            SEARCH_HIT_BADGE,
        );
    }
    let ico = Rect::from_min_size(
        Pos2::new(rect.left() + 6.0, rect.top() + 5.0),
        vec2(18.0, 18.0),
    );
    paint_icon(ui.painter(), ico, item.icon, TEXT_WHITE, icon_accent);
    ui.painter().text(
        Pos2::new(ico.right() + 5.0, rect.center().y),
        Align2::LEFT_CENTER,
        &label,
        FontId::proportional(11.5),
        TEXT_WHITE,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    let tip = match item.shortcut {
        Some(key) if !item.tooltip.contains(key) => format!("{} ({key})", item.tooltip),
        _ => item.tooltip.to_owned(),
    };
    response.on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_ids_are_unique_and_cover_all_nine_groups() {
        assert_eq!(GROUPS.len(), 9);
        let mut ids = HashSet::new();
        for item in TOOLS {
            assert!(ids.insert(item.id), "duplicate tool id: {}", item.id);
            assert!(!item.name.is_empty());
            assert!(!item.tooltip.is_empty());
        }
        for group in GROUPS {
            assert!(
                tools_in_group(group.tab).next().is_some(),
                "empty group: {}",
                group.title
            );
        }
    }

    #[test]
    fn search_matches_substring_case_cyrillic_and_trims_whitespace() {
        for query in ["матр", "МАТР", "  матр  "] {
            let hits = search_catalog(query);
            assert!(!hits.is_empty(), "expected hits for {query:?}");
            assert_eq!(hits[0].target, SearchTarget::Group(PaletteTab::Matrix));
            assert_eq!(hits[0].title, "Вектор и матрица");
            assert!(hits
                .iter()
                .any(|h| h.target == SearchTarget::Item("mat.2x2")));
        }

        let frac_hits = search_catalog("дроб");
        assert!(frac_hits
            .iter()
            .any(|h| h.target == SearchTarget::Item("op.fraction")));

        let int_hits = search_catalog("ИНТЕГ");
        assert!(int_hits
            .iter()
            .any(|h| h.target == SearchTarget::Item("calc.integral")));

        let plot_hits = search_catalog("  граф ");
        assert!(plot_hits
            .iter()
            .any(|h| h.target == SearchTarget::Group(PaletteTab::Plots)));
        assert!(plot_hits
            .iter()
            .any(|h| h.target == SearchTarget::Item("plot.xy")));

        let fn_hits = search_catalog("lsolve");
        assert!(fn_hits
            .iter()
            .any(|h| h.target == SearchTarget::Item("mat.lsolve")));

        let greek_hits = search_catalog("альфа");
        assert!(greek_hits
            .iter()
            .any(|h| h.target == SearchTarget::Item("greek.alpha")));
    }

    #[test]
    fn search_handles_empty_and_missing_queries_and_resets_highlight() {
        assert!(search_catalog("").is_empty());
        assert!(search_catalog("   \t  ").is_empty());
        assert!(search_catalog("несуществующая_команда_xyz").is_empty());

        let mut state = SearchState::default();
        let mut active = PaletteTab::Operators;
        let mut expanded = false;
        let hit = search_catalog("интег")[0];
        state.reveal_hit(hit, &mut active, &mut expanded);
        assert!(state.highlight.is_some());

        state.set_query("   ");
        assert!(state.highlight.is_none());
        assert!(!state.open);
    }

    #[test]
    fn selecting_result_navigates_to_expected_group_and_button() {
        let mut state = SearchState::default();
        let mut active = PaletteTab::Operators;
        let mut expanded = false;

        let mat_hit = search_catalog("матр")[0];
        state.reveal_hit(mat_hit, &mut active, &mut expanded);
        assert_eq!(active, PaletteTab::Matrix);
        assert!(expanded);
        assert_eq!(
            state.highlight,
            Some(SearchHighlight::Group(PaletteTab::Matrix))
        );

        let frac_hit = search_catalog("дроб")
            .into_iter()
            .find(|h| h.target == SearchTarget::Item("op.fraction"))
            .expect("fraction hit");
        state.reveal_hit(frac_hit, &mut active, &mut expanded);
        assert_eq!(active, PaletteTab::Operators);
        assert_eq!(
            state.highlight,
            Some(SearchHighlight::Item {
                group: PaletteTab::Operators,
                item_id: "op.fraction",
            })
        );
        assert!(state.scroll_to_highlight);
    }
}
