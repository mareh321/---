#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptKind {
    Literal,
    Index,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BigOpKind {
    Integral,
    Sum,
    Product,
}

impl BigOpKind {
    pub fn func_name(self) -> &'static str {
        match self {
            Self::Integral => "integ",
            Self::Sum => "isum",
            Self::Product => "iprod",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Integral => "∫",
            Self::Sum => "∑",
            Self::Product => "∏",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotPart {
    First,
    Second,
    Third,
    Cell(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathItem {
    Char(char),
    Fraction {
        num: MathSlot,
        den: MathSlot,
    },
    Power {
        base: MathSlot,
        exp: MathSlot,
    },
    Subscript {
        base: MathSlot,
        sub: MathSlot,
        kind: SubscriptKind,
    },
    Sqrt(MathSlot),
    Abs(MathSlot),
    BigOp {
        kind: BigOpKind,
        lower: MathSlot,
        upper: MathSlot,
        func: MathSlot,
    },
    Derivative {
        func: MathSlot,
        point: MathSlot,
    },
    Program {
        lines: Vec<MathSlot>,
    },
    Matrix {
        rows: usize,
        cols: usize,
        cells: Vec<MathSlot>,
    },
}

impl MathItem {
    pub fn child(&self, part: SlotPart) -> Option<&MathSlot> {
        match (self, part) {
            (Self::Fraction { num, .. }, SlotPart::First) => Some(num),
            (Self::Fraction { den, .. }, SlotPart::Second) => Some(den),
            (Self::Power { base, .. }, SlotPart::First) => Some(base),
            (Self::Power { exp, .. }, SlotPart::Second) => Some(exp),
            (Self::Subscript { base, .. }, SlotPart::First) => Some(base),
            (Self::Subscript { sub, .. }, SlotPart::Second) => Some(sub),
            (Self::Sqrt(inner) | Self::Abs(inner), SlotPart::First) => Some(inner),
            (Self::BigOp { lower, .. }, SlotPart::First) => Some(lower),
            (Self::BigOp { upper, .. }, SlotPart::Second) => Some(upper),
            (Self::BigOp { func, .. }, SlotPart::Third) => Some(func),
            (Self::Derivative { func, .. }, SlotPart::First) => Some(func),
            (Self::Derivative { point, .. }, SlotPart::Second) => Some(point),
            (Self::Program { lines }, SlotPart::Cell(idx)) => lines.get(idx),
            (Self::Matrix { cells, .. }, SlotPart::Cell(idx)) => cells.get(idx),
            _ => None,
        }
    }

    pub fn child_mut(&mut self, part: SlotPart) -> Option<&mut MathSlot> {
        match (self, part) {
            (Self::Fraction { num, .. }, SlotPart::First) => Some(num),
            (Self::Fraction { den, .. }, SlotPart::Second) => Some(den),
            (Self::Power { base, .. }, SlotPart::First) => Some(base),
            (Self::Power { exp, .. }, SlotPart::Second) => Some(exp),
            (Self::Subscript { base, .. }, SlotPart::First) => Some(base),
            (Self::Subscript { sub, .. }, SlotPart::Second) => Some(sub),
            (Self::Sqrt(inner) | Self::Abs(inner), SlotPart::First) => Some(inner),
            (Self::BigOp { lower, .. }, SlotPart::First) => Some(lower),
            (Self::BigOp { upper, .. }, SlotPart::Second) => Some(upper),
            (Self::BigOp { func, .. }, SlotPart::Third) => Some(func),
            (Self::Derivative { func, .. }, SlotPart::First) => Some(func),
            (Self::Derivative { point, .. }, SlotPart::Second) => Some(point),
            (Self::Program { lines }, SlotPart::Cell(idx)) => lines.get_mut(idx),
            (Self::Matrix { cells, .. }, SlotPart::Cell(idx)) => cells.get_mut(idx),
            _ => None,
        }
    }

    pub fn first_part(&self) -> Option<SlotPart> {
        match self {
            Self::Char(_) => None,
            Self::Matrix { cells, .. } if !cells.is_empty() => Some(SlotPart::Cell(0)),
            Self::Program { lines } if !lines.is_empty() => Some(SlotPart::Cell(0)),
            Self::Matrix { .. } | Self::Program { .. } => None,
            _ => Some(SlotPart::First),
        }
    }

    pub fn last_part(&self) -> Option<SlotPart> {
        match self {
            Self::Char(_) => None,
            Self::Sqrt(_) | Self::Abs(_) => Some(SlotPart::First),
            Self::BigOp { .. } => Some(SlotPart::Third),
            Self::Matrix { cells, .. } if !cells.is_empty() => {
                Some(SlotPart::Cell(cells.len() - 1))
            }
            Self::Program { lines } if !lines.is_empty() => Some(SlotPart::Cell(lines.len() - 1)),
            Self::Matrix { .. } | Self::Program { .. } => None,
            _ => Some(SlotPart::Second),
        }
    }

    pub fn next_part(&self, part: SlotPart) -> Option<SlotPart> {
        match (self, part) {
            (
                Self::Fraction { .. }
                | Self::Power { .. }
                | Self::Subscript { .. }
                | Self::Derivative { .. }
                | Self::BigOp { .. },
                SlotPart::First,
            ) => Some(SlotPart::Second),
            (Self::BigOp { .. }, SlotPart::Second) => Some(SlotPart::Third),
            (Self::Matrix { cells, .. }, SlotPart::Cell(idx)) if idx + 1 < cells.len() => {
                Some(SlotPart::Cell(idx + 1))
            }
            (Self::Program { lines }, SlotPart::Cell(idx)) if idx + 1 < lines.len() => {
                Some(SlotPart::Cell(idx + 1))
            }
            _ => None,
        }
    }

    pub fn prev_part(&self, part: SlotPart) -> Option<SlotPart> {
        match (self, part) {
            (
                Self::Fraction { .. }
                | Self::Power { .. }
                | Self::Subscript { .. }
                | Self::Derivative { .. }
                | Self::BigOp { .. },
                SlotPart::Second,
            ) => Some(SlotPart::First),
            (Self::BigOp { .. }, SlotPart::Third) => Some(SlotPart::Second),
            (Self::Matrix { .. } | Self::Program { .. }, SlotPart::Cell(idx)) if idx > 0 => {
                Some(SlotPart::Cell(idx - 1))
            }
            _ => None,
        }
    }

    pub fn is_structural(&self) -> bool {
        !matches!(self, Self::Char(_))
    }

    fn is_empty_structure(&self) -> bool {
        match self {
            Self::Char(_) => false,
            Self::Fraction { num, den } => num.items.is_empty() && den.items.is_empty(),
            Self::Power { base, exp } => base.items.is_empty() && exp.items.is_empty(),
            Self::Subscript { base, sub, .. } => base.items.is_empty() && sub.items.is_empty(),
            Self::Sqrt(inner) | Self::Abs(inner) => inner.items.is_empty(),
            Self::BigOp {
                lower, upper, func, ..
            } => lower.items.is_empty() && upper.items.is_empty() && func.items.is_empty(),
            Self::Derivative { func, point } => func.items.is_empty() && point.items.is_empty(),
            Self::Program { lines } => lines.iter().all(MathSlot::is_empty),
            Self::Matrix { cells, .. } => cells.iter().all(MathSlot::is_empty),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathSlot {
    pub items: Vec<MathItem>,
}

impl MathSlot {
    pub fn from_chars(text: &str) -> Self {
        Self {
            items: text.chars().map(MathItem::Char).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn to_source(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            match item {
                MathItem::Char(c) => out.push(*c),
                MathItem::Fraction { num, den } => {
                    let left = num.grouped_atom(false);
                    let right = den.grouped_atom(true);
                    out.push_str(&format!("{left} / {right}"));
                }
                MathItem::Power { base, exp } => {
                    let left = base.grouped_atom(true);
                    let right = exp.grouped_atom(true);
                    out.push_str(&format!("{left}^{right}"));
                }
                MathItem::Subscript {
                    base,
                    sub,
                    kind: SubscriptKind::Literal,
                } => {
                    let b = base.to_source();
                    let s = sub.to_source();
                    out.push_str(b.trim());
                    out.push('.');
                    out.push_str(s.trim());
                }
                MathItem::Subscript {
                    base,
                    sub,
                    kind: SubscriptKind::Index,
                } => {
                    let b = base.grouped_atom(true);
                    let s = sub.to_source();
                    out.push_str(&format!("{b}[{}]", s.trim()));
                }
                MathItem::Sqrt(inner) => {
                    let body = inner.to_source();
                    out.push_str(&format!("sqrt({})", body.trim()));
                }
                MathItem::Abs(inner) => {
                    let body = inner.to_source();
                    out.push_str(&format!("abs({})", body.trim()));
                }
                MathItem::BigOp {
                    kind,
                    lower,
                    upper,
                    func,
                } => {
                    out.push_str(&format!(
                        "{}({}, {}, {})",
                        kind.func_name(),
                        func.to_source().trim(),
                        lower.to_source().trim(),
                        upper.to_source().trim()
                    ));
                }
                MathItem::Derivative { func, point } => {
                    out.push_str(&format!(
                        "diff({}, {})",
                        func.to_source().trim(),
                        point.to_source().trim()
                    ));
                }
                MathItem::Program { lines } => {
                    let body = lines
                        .iter()
                        .map(|l| l.to_source())
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("; ");
                    out.push_str(&format!("prog({body})"));
                }
                MathItem::Matrix { rows, cols, cells } => {
                    out.push('[');
                    for r in 0..*rows {
                        if r > 0 {
                            out.push_str("; ");
                        }
                        for c in 0..*cols {
                            if c > 0 {
                                out.push_str(", ");
                            }
                            if let Some(cell) = cells.get(r * *cols + c) {
                                out.push_str(cell.to_source().trim());
                            }
                        }
                    }
                    out.push(']');
                }
            }
        }
        out
    }

    fn grouped_atom(&self, strict: bool) -> String {
        let raw = self.to_source();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if is_simple_atom(trimmed) || is_wrapped_in_parens(trimmed) {
            return trimmed.to_owned();
        }
        if !strict && self.items.len() == 1 {
            if matches!(
                &self.items[0],
                MathItem::Subscript { .. }
                    | MathItem::Sqrt(_)
                    | MathItem::Abs(_)
                    | MathItem::Matrix { .. }
            ) {
                return trimmed.to_owned();
            }
        }
        format!("({trimmed})")
    }

    pub fn has_2d_structures(&self) -> bool {
        self.items.iter().any(MathItem::is_structural)
    }
}

fn is_simple_atom(s: &str) -> bool {
    if s.is_empty() || s.contains("..") {
        return false;
    }
    s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

fn is_wrapped_in_parens(s: &str) -> bool {
    if !(s.starts_with('(') && s.ends_with(')')) {
        return false;
    }
    let mut depth = 0i32;
    for (idx, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && idx + 1 < s.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub path: Vec<(usize, SlotPart)>,
    pub offset: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathEditor {
    pub root: MathSlot,
    pub cursor: Cursor,
}

impl MathEditor {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_source(source: &str) -> Self {
        let root = parse_source_to_slot(source);
        let offset = root.items.len();
        Self {
            root,
            cursor: Cursor {
                path: Vec::new(),
                offset,
            },
        }
    }

    pub fn to_source(&self) -> String {
        self.root.to_source()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    pub fn slot_at(&self, path: &[(usize, SlotPart)]) -> Option<&MathSlot> {
        let mut current = &self.root;
        for &(index, part) in path {
            current = current.items.get(index)?.child(part)?;
        }
        Some(current)
    }

    fn slot_at_mut(&mut self, path: &[(usize, SlotPart)]) -> Option<&mut MathSlot> {
        let mut current = &mut self.root;
        for &(index, part) in path {
            current = current.items.get_mut(index)?.child_mut(part)?;
        }
        Some(current)
    }

    fn current_slot_mut(&mut self) -> &mut MathSlot {
        let path = self.cursor.path.clone();
        if self.slot_at(&path).is_none() {
            self.cursor.path.clear();
            self.cursor.offset = self.root.items.len();
            return &mut self.root;
        }
        self.slot_at_mut(&path).expect("validated cursor path")
    }

    fn clamp_cursor(&mut self) {
        let len = self
            .slot_at(&self.cursor.path)
            .map_or(0, |slot| slot.items.len());
        if self.slot_at(&self.cursor.path).is_none() {
            self.cursor.path.clear();
            self.cursor.offset = self.root.items.len();
        } else {
            self.cursor.offset = self.cursor.offset.min(len);
        }
    }

    pub fn set_cursor(&mut self, path: Vec<(usize, SlotPart)>, offset: usize) {
        if let Some(len) = self.slot_at(&path).map(|slot| slot.items.len()) {
            self.cursor.path = path;
            self.cursor.offset = offset.min(len);
        }
    }

    pub fn insert_text(&mut self, text: &str) {
        for c in text.chars() {
            if matches!(c, '\r' | '\n' | '\t') {
                continue;
            }
            self.insert_char(c);
        }
    }

    pub fn insert_raw(&mut self, text: &str) {
        self.clamp_cursor();
        let mut offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        for c in text.chars() {
            slot.items.insert(offset, MathItem::Char(c));
            offset += 1;
        }
        self.cursor.offset = offset;
    }

    pub fn insert_char(&mut self, c: char) {
        self.clamp_cursor();
        match c {
            '/' => {
                self.insert_fraction();
                return;
            }
            '^' => {
                self.insert_power();
                return;
            }
            '[' => {
                self.insert_subscript(SubscriptKind::Index);
                return;
            }
            '.' => {
                if self.try_convert_literal_sub_to_range() {
                    return;
                }
                if self.preceded_by_identifier_for_literal_sub() {
                    self.insert_subscript(SubscriptKind::Literal);
                    return;
                }
            }
            _ => {}
        }
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        slot.items.insert(offset, MathItem::Char(c));
        self.cursor.offset = offset + 1;
    }

    fn try_convert_literal_sub_to_range(&mut self) -> bool {
        let Some(&(parent_idx, SlotPart::Second)) = self.cursor.path.last() else {
            return false;
        };
        let parent_path = self.cursor.path[..self.cursor.path.len() - 1].to_vec();
        let is_empty_literal = self
            .slot_at(&parent_path)
            .and_then(|s| s.items.get(parent_idx))
            .is_some_and(|item| {
                matches!(
                    item,
                    MathItem::Subscript {
                        sub,
                        kind: SubscriptKind::Literal,
                        ..
                    } if sub.is_empty()
                )
            });
        if !is_empty_literal {
            return false;
        }
        let parent_slot = self.slot_at_mut(&parent_path).expect("parent exists");
        let removed = parent_slot.items.remove(parent_idx);
        let MathItem::Subscript { base, .. } = removed else {
            unreachable!();
        };
        let count = base.items.len();
        let mut insert_at = parent_idx;
        for item in base.items {
            parent_slot.items.insert(insert_at, item);
            insert_at += 1;
        }
        parent_slot.items.insert(insert_at, MathItem::Char('.'));
        parent_slot.items.insert(insert_at + 1, MathItem::Char('.'));
        self.cursor.path = parent_path;
        self.cursor.offset = parent_idx + count + 2;
        true
    }

    fn preceded_by_identifier_for_literal_sub(&self) -> bool {
        if self.cursor.offset == 0 {
            return false;
        }
        let Some(slot) = self.slot_at(&self.cursor.path) else {
            return false;
        };
        let mut i = self.cursor.offset;
        let mut saw_alpha = false;
        while i > 0 {
            match slot.items[i - 1] {
                MathItem::Char(ch) if ch.is_alphabetic() || ch == '_' => {
                    saw_alpha = true;
                    i -= 1;
                }
                MathItem::Char(ch) if ch.is_ascii_digit() => {
                    i -= 1;
                }
                _ => break,
            }
        }
        saw_alpha
    }

    pub fn insert_fraction(&mut self) {
        self.clamp_cursor();
        let mut offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        let num_items = take_preceding_atom(slot, &mut offset, true);
        let enter_part = if num_items.is_empty() {
            SlotPart::First
        } else {
            SlotPart::Second
        };
        slot.items.insert(
            offset,
            MathItem::Fraction {
                num: MathSlot { items: num_items },
                den: MathSlot::default(),
            },
        );
        self.cursor.path.push((offset, enter_part));
        self.cursor.offset = 0;
    }

    pub fn insert_power(&mut self) {
        self.clamp_cursor();
        let mut offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        let base_items = take_preceding_atom(slot, &mut offset, false);
        let enter_part = if base_items.is_empty() {
            SlotPart::First
        } else {
            SlotPart::Second
        };
        slot.items.insert(
            offset,
            MathItem::Power {
                base: MathSlot { items: base_items },
                exp: MathSlot::default(),
            },
        );
        self.cursor.path.push((offset, enter_part));
        self.cursor.offset = 0;
    }

    pub fn insert_subscript(&mut self, kind: SubscriptKind) {
        self.clamp_cursor();
        let mut offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        let base_items = take_preceding_atom(slot, &mut offset, false);
        let enter_part = if base_items.is_empty() {
            SlotPart::First
        } else {
            SlotPart::Second
        };
        slot.items.insert(
            offset,
            MathItem::Subscript {
                base: MathSlot { items: base_items },
                sub: MathSlot::default(),
                kind,
            },
        );
        self.cursor.path.push((offset, enter_part));
        self.cursor.offset = 0;
    }

    pub fn insert_sqrt(&mut self) {
        self.clamp_cursor();
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        slot.items
            .insert(offset, MathItem::Sqrt(MathSlot::default()));
        self.cursor.path.push((offset, SlotPart::First));
        self.cursor.offset = 0;
    }

    pub fn insert_matrix(&mut self, rows: usize, cols: usize) {
        let rows = rows.clamp(1, 8);
        let cols = cols.clamp(1, 8);
        self.clamp_cursor();
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        let cells = vec![MathSlot::default(); rows * cols];
        slot.items
            .insert(offset, MathItem::Matrix { rows, cols, cells });
        self.cursor.path.push((offset, SlotPart::Cell(0)));
        self.cursor.offset = 0;
    }

    pub fn insert_abs(&mut self) {
        self.clamp_cursor();
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        slot.items
            .insert(offset, MathItem::Abs(MathSlot::default()));
        self.cursor.path.push((offset, SlotPart::First));
        self.cursor.offset = 0;
    }

    pub fn insert_big_op(&mut self, kind: BigOpKind) {
        self.clamp_cursor();
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        slot.items.insert(
            offset,
            MathItem::BigOp {
                kind,
                lower: MathSlot::default(),
                upper: MathSlot::default(),
                func: MathSlot::default(),
            },
        );
        self.cursor.path.push((offset, SlotPart::First));
        self.cursor.offset = 0;
    }

    pub fn insert_derivative(&mut self) {
        self.clamp_cursor();
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        slot.items.insert(
            offset,
            MathItem::Derivative {
                func: MathSlot::default(),
                point: MathSlot::default(),
            },
        );
        self.cursor.path.push((offset, SlotPart::First));
        self.cursor.offset = 0;
    }

    pub fn insert_program(&mut self, line_count: usize) {
        self.clamp_cursor();
        let count = line_count.clamp(1, 32);
        let offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        slot.items.insert(
            offset,
            MathItem::Program {
                lines: vec![MathSlot::default(); count],
            },
        );
        self.cursor.path.push((offset, SlotPart::Cell(0)));
        self.cursor.offset = 0;
    }

    pub fn add_program_line(&mut self) {
        self.clamp_cursor();
        for depth in (0..self.cursor.path.len()).rev() {
            let (item_idx, part) = self.cursor.path[depth];
            let SlotPart::Cell(line_idx) = part else {
                continue;
            };
            let parent_path = self.cursor.path[..depth].to_vec();
            let is_prog = self
                .slot_at(&parent_path)
                .and_then(|s| s.items.get(item_idx))
                .is_some_and(|it| matches!(it, MathItem::Program { .. }));
            if is_prog {
                if let Some(MathItem::Program { lines }) = self
                    .slot_at_mut(&parent_path)
                    .and_then(|s| s.items.get_mut(item_idx))
                {
                    if lines.len() < 32 {
                        let next = (line_idx + 1).min(lines.len());
                        lines.insert(next, MathSlot::default());
                        self.cursor.path.truncate(depth);
                        self.cursor.path.push((item_idx, SlotPart::Cell(next)));
                        self.cursor.offset = 0;
                    }
                }
                return;
            }
        }
        self.insert_program(2);
    }

    pub fn insert_call_template(&mut self, name: &str) {
        self.clamp_cursor();
        let mut offset = self.cursor.offset;
        let slot = self.current_slot_mut();
        for c in name.chars() {
            slot.items.insert(offset, MathItem::Char(c));
            offset += 1;
        }
        slot.items.insert(offset, MathItem::Char('('));
        slot.items.insert(offset + 1, MathItem::Char(')'));
        self.cursor.offset = offset + 1;
    }

    pub fn move_left(&mut self) {
        self.clamp_cursor();
        if self.cursor.offset > 0 {
            let prev_idx = self.cursor.offset - 1;
            let last_part = self
                .slot_at(&self.cursor.path)
                .and_then(|s| s.items.get(prev_idx))
                .and_then(MathItem::last_part);
            if let Some(part) = last_part {
                self.cursor.path.push((prev_idx, part));
                self.cursor.offset = self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len());
            } else {
                self.cursor.offset -= 1;
            }
            return;
        }
        if let Some((parent_idx, part)) = self.cursor.path.pop() {
            let prev = self
                .slot_at(&self.cursor.path)
                .and_then(|s| s.items.get(parent_idx))
                .and_then(|item| item.prev_part(part));
            if let Some(prev_part) = prev {
                self.cursor.path.push((parent_idx, prev_part));
                self.cursor.offset = self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len());
            } else {
                self.cursor.offset = parent_idx;
            }
        }
    }

    pub fn move_right(&mut self) {
        self.clamp_cursor();
        let len = self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len());
        if self.cursor.offset < len {
            let idx = self.cursor.offset;
            let first_part = self
                .slot_at(&self.cursor.path)
                .and_then(|s| s.items.get(idx))
                .and_then(MathItem::first_part);
            if let Some(part) = first_part {
                self.cursor.path.push((idx, part));
                self.cursor.offset = 0;
            } else {
                self.cursor.offset += 1;
            }
            return;
        }
        if let Some((parent_idx, part)) = self.cursor.path.pop() {
            let next = self
                .slot_at(&self.cursor.path)
                .and_then(|s| s.items.get(parent_idx))
                .and_then(|item| item.next_part(part));
            if let Some(next_part) = next {
                self.cursor.path.push((parent_idx, next_part));
                self.cursor.offset = 0;
            } else {
                self.cursor.offset = parent_idx + 1;
            }
        }
    }

    pub fn move_up(&mut self) {
        self.clamp_cursor();
        self.move_vertical(true);
    }

    pub fn move_down(&mut self) {
        self.clamp_cursor();
        self.move_vertical(false);
    }

    fn move_vertical(&mut self, up: bool) {
        for depth in (0..self.cursor.path.len()).rev() {
            let (item_idx, part) = self.cursor.path[depth];
            let parent_path = &self.cursor.path[..depth];
            let Some(item) = self
                .slot_at(parent_path)
                .and_then(|s| s.items.get(item_idx))
            else {
                continue;
            };
            let target = match (item, part, up) {
                (MathItem::Fraction { .. }, SlotPart::Second, true) => Some(SlotPart::First),
                (MathItem::Fraction { .. }, SlotPart::First, false) => Some(SlotPart::Second),
                (MathItem::Power { .. }, SlotPart::First, true) => Some(SlotPart::Second),
                (MathItem::Power { .. }, SlotPart::Second, false) => Some(SlotPart::First),
                (MathItem::Subscript { .. }, SlotPart::First, false) => Some(SlotPart::Second),
                (MathItem::Subscript { .. }, SlotPart::Second, true) => Some(SlotPart::First),
                (MathItem::Matrix { cols, .. }, SlotPart::Cell(idx), true) => {
                    let row = idx / *cols;
                    let col = idx % *cols;
                    (row > 0).then(|| SlotPart::Cell((row - 1) * *cols + col))
                }
                (MathItem::Matrix { rows, cols, .. }, SlotPart::Cell(idx), false) => {
                    let row = idx / *cols;
                    let col = idx % *cols;
                    (row + 1 < *rows).then(|| SlotPart::Cell((row + 1) * *cols + col))
                }
                (MathItem::BigOp { .. }, SlotPart::First, true) => Some(SlotPart::Second),
                (MathItem::BigOp { .. }, SlotPart::Second, false) => Some(SlotPart::First),
                (MathItem::BigOp { .. }, SlotPart::Third, true) => Some(SlotPart::Second),
                (MathItem::BigOp { .. }, SlotPart::Third, false) => Some(SlotPart::First),
                (MathItem::Program { .. }, SlotPart::Cell(idx), true) => {
                    (idx > 0).then(|| SlotPart::Cell(idx - 1))
                }
                (MathItem::Program { lines }, SlotPart::Cell(idx), false) => {
                    (idx + 1 < lines.len()).then(|| SlotPart::Cell(idx + 1))
                }
                _ => None,
            };
            if let Some(target_part) = target {
                self.cursor.path.truncate(depth);
                self.cursor.path.push((item_idx, target_part));
                let target_len = self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len());
                self.cursor.offset = self.cursor.offset.min(target_len);
                return;
            }
        }
    }

    pub fn move_next_slot(&mut self, reverse: bool) {
        self.clamp_cursor();
        let Some((parent_idx, part)) = self.cursor.path.pop() else {
            return;
        };
        let target = self
            .slot_at(&self.cursor.path)
            .and_then(|s| s.items.get(parent_idx))
            .and_then(|item| {
                if reverse {
                    item.prev_part(part)
                } else {
                    item.next_part(part)
                }
            });
        if let Some(next_part) = target {
            self.cursor.path.push((parent_idx, next_part));
            self.cursor.offset = if reverse {
                self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len())
            } else {
                0
            };
        } else {
            self.cursor.offset = if reverse { parent_idx } else { parent_idx + 1 };
        }
    }

    pub fn backspace(&mut self) {
        self.clamp_cursor();
        if self.cursor.offset > 0 {
            let prev_idx = self.cursor.offset - 1;
            let (is_struct, is_empty, last_part) = self
                .slot_at(&self.cursor.path)
                .and_then(|s| s.items.get(prev_idx))
                .map_or((false, false, None), |item| {
                    (
                        item.is_structural(),
                        item.is_empty_structure(),
                        item.last_part(),
                    )
                });
            if !is_struct || is_empty {
                let slot = self.current_slot_mut();
                slot.items.remove(prev_idx);
                self.cursor.offset = prev_idx;
            } else if let Some(part) = last_part {
                self.cursor.path.push((prev_idx, part));
                self.cursor.offset = self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len());
            }
            return;
        }
        let Some(&(parent_idx, part)) = self.cursor.path.last() else {
            return;
        };
        let parent_path = self.cursor.path[..self.cursor.path.len() - 1].to_vec();
        let current_empty = self
            .slot_at(&self.cursor.path)
            .is_some_and(MathSlot::is_empty);
        if !current_empty {
            self.move_left();
            return;
        }
        let is_program = self
            .slot_at(&parent_path)
            .and_then(|s| s.items.get(parent_idx))
            .is_some_and(|item| matches!(item, MathItem::Program { .. }));
        if is_program {
            if let SlotPart::Cell(line_idx) = part {
                let parent_slot = self.slot_at_mut(&parent_path).expect("parent exists");
                if let Some(MathItem::Program { lines }) = parent_slot.items.get_mut(parent_idx) {
                    if lines.len() > 1 {
                        lines.remove(line_idx);
                        let target_line = line_idx.saturating_sub(1).min(lines.len() - 1);
                        let target_len = lines[target_line].items.len();
                        self.cursor.path = parent_path;
                        self.cursor
                            .path
                            .push((parent_idx, SlotPart::Cell(target_line)));
                        self.cursor.offset = target_len;
                        return;
                    }
                }
                parent_slot.items.remove(parent_idx);
                self.cursor.path = parent_path;
                self.cursor.offset = parent_idx;
                return;
            }
        }
        let is_multi_slot = self
            .slot_at(&parent_path)
            .and_then(|s| s.items.get(parent_idx))
            .is_some_and(|item| {
                matches!(
                    item,
                    MathItem::Matrix { .. } | MathItem::BigOp { .. } | MathItem::Derivative { .. }
                )
            });
        if is_multi_slot {
            let all_empty = self
                .slot_at(&parent_path)
                .and_then(|s| s.items.get(parent_idx))
                .is_some_and(MathItem::is_empty_structure);
            if all_empty {
                let parent_slot = self.slot_at_mut(&parent_path).expect("parent exists");
                parent_slot.items.remove(parent_idx);
                self.cursor.path = parent_path;
                self.cursor.offset = parent_idx;
            } else {
                self.move_left();
            }
            return;
        }
        let parent_slot = self.slot_at_mut(&parent_path).expect("parent exists");
        let removed = parent_slot.items.remove(parent_idx);
        let survivor = match (removed, part) {
            (MathItem::Fraction { num, .. }, SlotPart::Second)
            | (MathItem::Power { base: num, .. }, SlotPart::Second)
            | (MathItem::Subscript { base: num, .. }, SlotPart::Second) => num,
            (MathItem::Fraction { den, .. }, SlotPart::First)
            | (MathItem::Power { exp: den, .. }, SlotPart::First)
            | (MathItem::Subscript { sub: den, .. }, SlotPart::First) => den,
            (MathItem::Sqrt(inner) | MathItem::Abs(inner), SlotPart::First) => inner,
            _ => MathSlot::default(),
        };
        let count = survivor.items.len();
        for (i, item) in survivor.items.into_iter().enumerate() {
            parent_slot.items.insert(parent_idx + i, item);
        }
        self.cursor.path = parent_path;
        self.cursor.offset = parent_idx + count;
    }

    pub fn delete(&mut self) {
        self.clamp_cursor();
        let len = self.slot_at(&self.cursor.path).map_or(0, |s| s.items.len());
        if self.cursor.offset < len {
            let idx = self.cursor.offset;
            let (is_struct, is_empty, first_part) = self
                .slot_at(&self.cursor.path)
                .and_then(|s| s.items.get(idx))
                .map_or((false, false, None), |item| {
                    (
                        item.is_structural(),
                        item.is_empty_structure(),
                        item.first_part(),
                    )
                });
            if !is_struct || is_empty {
                let slot = self.current_slot_mut();
                slot.items.remove(idx);
            } else if let Some(part) = first_part {
                self.cursor.path.push((idx, part));
                self.cursor.offset = 0;
            }
        }
    }
}

fn take_preceding_atom(
    slot: &mut MathSlot,
    offset: &mut usize,
    unwrap_parens: bool,
) -> Vec<MathItem> {
    if *offset == 0 {
        return Vec::new();
    }
    let last = *offset - 1;
    if slot.items[last].is_structural() {
        *offset -= 1;
        return vec![slot.items.remove(last)];
    }
    if matches!(slot.items[last], MathItem::Char(')')) {
        let mut depth = 0i32;
        let mut start = None;
        for i in (0..=last).rev() {
            match slot.items[i] {
                MathItem::Char(')') => depth += 1,
                MathItem::Char('(') => {
                    depth -= 1;
                    if depth == 0 {
                        start = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(open_idx) = start {
            let mut fn_start = open_idx;
            while fn_start > 0 {
                match slot.items[fn_start - 1] {
                    MathItem::Char(c) if c.is_alphanumeric() || c == '_' => fn_start -= 1,
                    _ => break,
                }
            }
            let drained: Vec<_> = slot.items.drain(fn_start..*offset).collect();
            *offset = fn_start;
            if unwrap_parens && fn_start == open_idx && drained.len() >= 2 {
                return drained[1..drained.len() - 1].to_vec();
            }
            return drained;
        }
    }
    if matches!(
        slot.items[last],
        MathItem::Char(c) if c.is_alphanumeric() || c == '_' || c == '.'
    ) {
        let mut start = last;
        while start > 0 {
            match slot.items[start - 1] {
                MathItem::Char('.') if matches!(slot.items[start], MathItem::Char('.')) => {
                    break;
                }
                MathItem::Char(c) if c.is_alphanumeric() || c == '_' || c == '.' => {
                    start -= 1;
                }
                _ => break,
            }
        }
        let drained: Vec<_> = slot.items.drain(start..*offset).collect();
        *offset = start;
        return drained;
    }
    Vec::new()
}

// --- Parser from textual formula source into 2D MathSlot ---

pub fn parse_source_to_slot(source: &str) -> MathSlot {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return MathSlot::default();
    }
    for (delim, rendered) in [(":=", " := "), ("===", " === "), ("≡", " === ")] {
        if let Some((lhs, rhs)) = split_top_level(trimmed, delim) {
            let mut items = parse_expr_slot(lhs.trim()).items;
            items.extend(rendered.chars().map(MathItem::Char));
            let (body, has_eq) = strip_trailing_eq(rhs.trim());
            items.extend(parse_expr_slot(body).items);
            if has_eq {
                items.extend(" =".chars().map(MathItem::Char));
            }
            return MathSlot { items };
        }
    }
    if let Some(body) = trimmed
        .strip_suffix("->")
        .or_else(|| trimmed.strip_suffix('→'))
    {
        let mut items = parse_expr_slot(body.trim()).items;
        items.extend(" ->".chars().map(MathItem::Char));
        return MathSlot { items };
    }
    let (body, has_eq) = strip_trailing_eq(trimmed);
    let mut items = parse_expr_slot(body).items;
    if has_eq {
        items.extend(" =".chars().map(MathItem::Char));
    }
    MathSlot { items }
}

fn strip_trailing_eq(s: &str) -> (&str, bool) {
    if let Some(rest) = s.strip_suffix('=') {
        if !rest.ends_with(['=', '!', '<', '>', ':']) {
            return (rest.trim_end(), true);
        }
    }
    (s, false)
}

fn split_top_level<'a>(s: &'a str, delim: &str) -> Option<(&'a str, &'a str)> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            _ => {}
        }
        if paren == 0 && bracket == 0 && s[idx..].starts_with(delim) {
            return Some((&s[..idx], &s[idx + delim.len()..]));
        }
    }
    None
}

fn parse_expr_slot(source: &str) -> MathSlot {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return MathSlot::default();
    }
    let mut p = SourceParser {
        chars: trimmed.chars().collect(),
        pos: 0,
    };
    p.parse_additive()
}

struct SourceParser {
    chars: Vec<char>,
    pos: usize,
}

impl SourceParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn parse_additive(&mut self) -> MathSlot {
        let mut items = self.parse_multiplicative().items;
        loop {
            self.skip_ws();
            let Some(op) = self.peek() else { break };
            if matches!(op, ')' | ']' | ',' | ';') {
                break;
            }
            if op == '.' && self.peek_next() == Some('.') {
                self.pos += 2;
                items.push(MathItem::Char('.'));
                items.push(MathItem::Char('.'));
                items.extend(self.parse_multiplicative().items);
                continue;
            }
            if op == '<' && self.peek_next() == Some('-') {
                self.pos += 2;
                items.extend(" <- ".chars().map(MathItem::Char));
                items.extend(self.parse_multiplicative().items);
                continue;
            }
            if op == '←' {
                self.pos += 1;
                items.extend(" <- ".chars().map(MathItem::Char));
                items.extend(self.parse_multiplicative().items);
                continue;
            }
            if matches!(
                op,
                '+' | '-' | '−' | '<' | '>' | '=' | '!' | '≠' | '≤' | '≥'
            ) {
                self.pos += 1;
                items.push(MathItem::Char(' '));
                items.push(MathItem::Char(op));
                if matches!(op, '<' | '>' | '=' | '!') && self.peek() == Some('=') {
                    self.pos += 1;
                    items.push(MathItem::Char('='));
                }
                items.push(MathItem::Char(' '));
                items.extend(self.parse_multiplicative().items);
            } else {
                break;
            }
        }
        MathSlot { items }
    }

    fn parse_multiplicative(&mut self) -> MathSlot {
        let mut left = self.parse_power();
        loop {
            self.skip_ws();
            let Some(op) = self.peek() else { break };
            if matches!(op, '/' | '÷') {
                self.pos += 1;
                let right = self.parse_power();
                left = MathSlot {
                    items: vec![MathItem::Fraction {
                        num: unwrap_slot_parens(left),
                        den: unwrap_slot_parens(right),
                    }],
                };
            } else if matches!(op, '*' | '·' | '×') {
                self.pos += 1;
                left.items.push(MathItem::Char(' '));
                left.items.push(MathItem::Char(op));
                left.items.push(MathItem::Char(' '));
                left.items.extend(self.parse_power().items);
            } else {
                break;
            }
        }
        left
    }

    fn parse_power(&mut self) -> MathSlot {
        let base = self.parse_unary();
        self.skip_ws();
        if self.peek() == Some('^') {
            self.pos += 1;
            let exp = self.parse_power();
            return MathSlot {
                items: vec![MathItem::Power {
                    base,
                    exp: unwrap_slot_parens(exp),
                }],
            };
        }
        base
    }

    fn parse_unary(&mut self) -> MathSlot {
        self.skip_ws();
        if matches!(self.peek(), Some('-' | '−' | '+')) {
            let sign = self.chars[self.pos];
            self.pos += 1;
            let mut rest = self.parse_unary();
            rest.items.insert(0, MathItem::Char(sign));
            return rest;
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> MathSlot {
        let mut node = self.parse_primary();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('[') => {
                    self.pos += 1;
                    let sub = self.parse_bracket_contents();
                    node = MathSlot {
                        items: vec![MathItem::Subscript {
                            base: node,
                            sub,
                            kind: SubscriptKind::Index,
                        }],
                    };
                }
                Some('(')
                    if !node.is_empty()
                        && !matches!(node.items.last(), Some(MathItem::Char('('))) =>
                {
                    let fname = node.to_source();
                    let fname = fname.trim();
                    self.pos += 1;
                    match fname {
                        "sqrt" => {
                            let inner = self.parse_additive();
                            self.skip_ws();
                            if self.peek() == Some(')') {
                                self.pos += 1;
                            }
                            node = MathSlot {
                                items: vec![MathItem::Sqrt(unwrap_slot_parens(inner))],
                            };
                        }
                        "abs" => {
                            let inner = self.parse_additive();
                            self.skip_ws();
                            if self.peek() == Some(')') {
                                self.pos += 1;
                            }
                            node = MathSlot {
                                items: vec![MathItem::Abs(unwrap_slot_parens(inner))],
                            };
                        }
                        "integ" | "isum" | "iprod" => {
                            let saved = self.pos;
                            let args = self.parse_arg_list(',');
                            if args.len() == 3 {
                                let kind = match fname {
                                    "integ" => BigOpKind::Integral,
                                    "isum" => BigOpKind::Sum,
                                    _ => BigOpKind::Product,
                                };
                                let mut iter = args.into_iter();
                                let func = unwrap_slot_parens(iter.next().unwrap());
                                let lower = unwrap_slot_parens(iter.next().unwrap());
                                let upper = unwrap_slot_parens(iter.next().unwrap());
                                node = MathSlot {
                                    items: vec![MathItem::BigOp {
                                        kind,
                                        lower,
                                        upper,
                                        func,
                                    }],
                                };
                            } else {
                                self.pos = saved;
                                node.items.push(MathItem::Char('('));
                                self.parse_delimited_into(')', &mut node.items);
                            }
                        }
                        "diff" => {
                            let saved = self.pos;
                            let args = self.parse_arg_list(',');
                            if args.len() == 2 {
                                let mut iter = args.into_iter();
                                let func = unwrap_slot_parens(iter.next().unwrap());
                                let point = unwrap_slot_parens(iter.next().unwrap());
                                node = MathSlot {
                                    items: vec![MathItem::Derivative { func, point }],
                                };
                            } else {
                                self.pos = saved;
                                node.items.push(MathItem::Char('('));
                                self.parse_delimited_into(')', &mut node.items);
                            }
                        }
                        "prog" => {
                            let lines = self.parse_arg_list(';');
                            let lines = if lines.is_empty() {
                                vec![MathSlot::default()]
                            } else {
                                lines
                            };
                            node = MathSlot {
                                items: vec![MathItem::Program { lines }],
                            };
                        }
                        _ => {
                            node.items.push(MathItem::Char('('));
                            self.parse_delimited_into(')', &mut node.items);
                        }
                    }
                }
                _ => break,
            }
        }
        node
    }

    fn parse_bracket_contents(&mut self) -> MathSlot {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() || self.peek() == Some(']') {
                if self.peek() == Some(']') {
                    self.pos += 1;
                }
                break;
            }
            items.extend(self.parse_additive().items);
            self.skip_ws();
            if matches!(self.peek(), Some(',' | ';')) {
                let sep = self.chars[self.pos];
                self.pos += 1;
                items.push(MathItem::Char(sep));
                items.push(MathItem::Char(' '));
            } else {
                if self.peek() == Some(']') {
                    self.pos += 1;
                }
                break;
            }
        }
        MathSlot { items }
    }

    fn parse_matrix_literal(&mut self) -> MathSlot {
        let mut rows: Vec<Vec<MathSlot>> = Vec::new();
        let mut current_row: Vec<MathSlot> = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() || self.peek() == Some(']') {
                if self.peek() == Some(']') {
                    self.pos += 1;
                }
                break;
            }
            current_row.push(self.parse_additive());
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                }
                Some(';') => {
                    self.pos += 1;
                    rows.push(std::mem::take(&mut current_row));
                }
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                _ => break,
            }
        }
        if !current_row.is_empty() {
            rows.push(current_row);
        }
        let cols = rows.first().map_or(0, Vec::len);
        let row_count = rows.len();
        if row_count > 0
            && cols > 0
            && rows.iter().all(|r| r.len() == cols)
            && row_count * cols <= 64
        {
            let cells = rows.into_iter().flatten().collect();
            return MathSlot {
                items: vec![MathItem::Matrix {
                    rows: row_count,
                    cols,
                    cells,
                }],
            };
        }
        MathSlot::from_chars("[]")
    }

    fn parse_arg_list(&mut self, sep: char) -> Vec<MathSlot> {
        let mut args = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.peek() == Some(')') {
                self.pos += 1;
                break;
            }
            args.push(self.parse_additive());
            self.skip_ws();
            if self.peek() == Some(sep) {
                self.pos += 1;
            } else if self.peek() == Some(')') {
                self.pos += 1;
                break;
            } else {
                break;
            }
        }
        args
    }

    fn parse_delimited_into(&mut self, close: char, out: &mut Vec<MathItem>) {
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.peek() == Some(close) {
                self.pos += 1;
                out.push(MathItem::Char(close));
                break;
            }
            out.extend(self.parse_additive().items);
            self.skip_ws();
            if matches!(self.peek(), Some(',' | ';')) {
                let sep = self.chars[self.pos];
                self.pos += 1;
                out.push(MathItem::Char(sep));
                out.push(MathItem::Char(' '));
            } else if self.peek() == Some(close) {
                self.pos += 1;
                out.push(MathItem::Char(close));
                break;
            } else {
                break;
            }
        }
    }

    fn parse_primary(&mut self) -> MathSlot {
        self.skip_ws();
        let Some(ch) = self.peek() else {
            return MathSlot::default();
        };
        if ch == '(' {
            self.pos += 1;
            let mut items = vec![MathItem::Char('(')];
            self.parse_delimited_into(')', &mut items);
            return MathSlot { items };
        }
        if ch == '[' {
            self.pos += 1;
            return self.parse_matrix_literal();
        }
        if ch.is_ascii_digit()
            || (ch == '.' && self.peek_next().is_some_and(|c| c.is_ascii_digit()))
        {
            let mut items = Vec::new();
            while let Some(c) = self.peek() {
                if c == '.' && self.peek_next() == Some('.') {
                    break;
                }
                if c.is_ascii_digit() || c == '.' {
                    items.push(MathItem::Char(c));
                    self.pos += 1;
                } else if matches!(c, 'e' | 'E')
                    && self
                        .peek_next()
                        .is_some_and(|n| n.is_ascii_digit() || matches!(n, '+' | '-'))
                {
                    items.push(MathItem::Char(c));
                    self.pos += 1;
                    if matches!(self.peek(), Some('+' | '-')) {
                        items.push(MathItem::Char(self.chars[self.pos]));
                        self.pos += 1;
                    }
                } else if c == 'i'
                    && !self
                        .peek_next()
                        .is_some_and(|n| n.is_alphanumeric() || n == '_')
                {
                    items.push(MathItem::Char(c));
                    self.pos += 1;
                    break;
                } else {
                    break;
                }
            }
            return MathSlot { items };
        }
        if ch.is_alphabetic() || ch == '_' {
            let mut name = Vec::new();
            while let Some(c) = self.peek() {
                if c.is_alphanumeric() || c == '_' {
                    name.push(MathItem::Char(c));
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.peek() == Some('.') && self.peek_next() != Some('.') {
                let save = self.pos;
                self.pos += 1;
                let mut sub = Vec::new();
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        sub.push(MathItem::Char(c));
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if !sub.is_empty() {
                    return MathSlot {
                        items: vec![MathItem::Subscript {
                            base: MathSlot { items: name },
                            sub: MathSlot { items: sub },
                            kind: SubscriptKind::Literal,
                        }],
                    };
                }
                self.pos = save;
            }
            return MathSlot { items: name };
        }
        self.pos += 1;
        MathSlot {
            items: vec![MathItem::Char(ch)],
        }
    }
}

fn unwrap_slot_parens(slot: MathSlot) -> MathSlot {
    if slot.items.len() >= 2
        && matches!(slot.items.first(), Some(MathItem::Char('(')))
        && matches!(slot.items.last(), Some(MathItem::Char(')')))
    {
        let mut depth = 0i32;
        let mut wraps_all = true;
        for (i, item) in slot.items.iter().enumerate() {
            match item {
                MathItem::Char('(') => depth += 1,
                MathItem::Char(')') => {
                    depth -= 1;
                    if depth == 0 && i + 1 < slot.items.len() {
                        wraps_all = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if wraps_all && depth == 0 {
            return MathSlot {
                items: slot.items[1..slot.items.len() - 1].to_vec(),
            };
        }
    }
    slot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::{export_xml, insert_region, NewRegion};
    use crate::sheet::Sheet;

    #[test]
    fn builds_nested_fraction_with_subscripts_and_power_and_navigates_with_arrows() {
        let mut ed = MathEditor::empty();
        ed.insert_text("(x.1");
        ed.move_right();
        ed.insert_char('^');
        ed.insert_text("2");
        ed.move_right();
        ed.insert_text(" + A[1");
        ed.move_right();
        ed.insert_char(')');
        ed.insert_char('/');
        ed.insert_text("1 + 1/2");
        ed.move_up();
        assert_eq!(
            ed.cursor.path,
            vec![(0, SlotPart::Second), (4, SlotPart::First)]
        );
        ed.move_up();
        assert_eq!(ed.cursor.path, vec![(0, SlotPart::First)]);
        ed.move_down();
        assert_eq!(ed.cursor.path, vec![(0, SlotPart::Second)]);
        ed.cursor.offset = ed.slot_at(&ed.cursor.path).unwrap().items.len();
        ed.move_right();
        ed.insert_text(" =");

        let source = ed.to_source();
        assert_eq!(source, "(x.1^2 + A[1]) / (1 + 1 / 2) =");

        let blank = include_str!("../examples/blank.xmcd");
        let (xml, _) = insert_region(blank, 12., 10., NewRegion::Formula("x.1 := 3")).unwrap();
        let (xml, _) = insert_region(&xml, 12., 45., NewRegion::Formula("A := [2; 5]")).unwrap();
        let (xml, id) = insert_region(&xml, 12., 85., NewRegion::Formula(&source)).unwrap();
        let exported = export_xml(&xml, true).unwrap();
        let mut sheet = Sheet::import(&exported).unwrap();
        sheet.full();
        let idx = sheet
            .regions
            .iter()
            .position(|r| r.source_id == id)
            .unwrap();
        let result = sheet.display(idx).unwrap();
        let value: f64 = result.parse().unwrap();
        assert!((value - (14.0 / 1.5)).abs() < 1e-9);
    }

    #[test]
    fn sqrt_and_matrix_grid_navigation_and_roundtrip() {
        let mut ed = MathEditor::empty();
        ed.insert_text("M := ");
        ed.insert_matrix(2, 2);
        ed.insert_text("1");
        ed.move_next_slot(false);
        ed.insert_text("2");
        ed.move_down();
        ed.insert_text("4");
        ed.move_left();
        assert_eq!(ed.to_source(), "M := [1, 2; , 4]");
        ed.move_next_slot(true);
        ed.insert_text("3");
        assert_eq!(ed.to_source(), "M := [1, 2; 3, 4]");

        let mut root_ed = MathEditor::empty();
        root_ed.insert_text("r := ");
        root_ed.insert_sqrt();
        root_ed.insert_text("a^2");
        root_ed.move_right();
        root_ed.insert_text(" + b^2");
        assert_eq!(root_ed.to_source(), "r := sqrt(a^2 + b^2)");

        let parsed = MathEditor::from_source("r := sqrt(a^2 + b^2)");
        assert!(parsed.root.has_2d_structures());
        assert_eq!(parsed.to_source(), "r := sqrt(a^2 + b^2)");

        let parsed_mat = MathEditor::from_source("M := [1, 2; 3, 4]");
        assert_eq!(parsed_mat.to_source(), "M := [1, 2; 3, 4]");
    }

    #[test]
    fn parses_source_to_2d_and_roundtrips() {
        let ed = MathEditor::from_source("Q := (a.1^2 + b[k]) / (c + d) =");
        assert!(ed.root.has_2d_structures());
        assert_eq!(ed.to_source(), "Q := (a.1^2 + b[k]) / (c + d) =");
    }

    #[test]
    fn range_operator_still_works_after_identifier() {
        let mut ed = MathEditor::empty();
        ed.insert_text("k := a..b");
        assert_eq!(ed.to_source(), "k := a..b");
    }

    #[test]
    fn backspace_unwraps_empty_fraction() {
        let mut ed = MathEditor::empty();
        ed.insert_text("a/");
        assert_eq!(ed.cursor.path, vec![(0, SlotPart::Second)]);
        ed.backspace();
        assert!(ed.cursor.path.is_empty());
        assert_eq!(ed.to_source(), "a");
    }

    #[test]
    fn parses_and_serializes_abs_big_ops_derivative_and_program_2d() {
        let mut ed = MathEditor::empty();
        ed.insert_big_op(BigOpKind::Integral);
        ed.insert_text("0");
        ed.move_up();
        ed.insert_text("3");
        ed.move_right();
        ed.insert_text("f");
        assert_eq!(ed.to_source(), "integ(f, 0, 3)");

        let parsed = parse_source_to_slot("integ(f, 0, 3) + isum(g, 1, 5) + diff(f, 2) + abs(x) =");
        assert!(parsed.has_2d_structures());
        assert_eq!(
            parsed.to_source(),
            "integ(f, 0, 3) + isum(g, 1, 5) + diff(f, 2) + abs(x) ="
        );

        let prog = parse_source_to_slot("f(n) := prog(s <- 0; v[0] <- n; return(s))");
        assert!(prog.has_2d_structures());
        assert!(prog
            .to_source()
            .contains("prog(s <- 0; v[0] <- n; return(s))"));
    }

    #[test]
    fn backspace_in_program_removes_only_empty_line_and_preserves_big_op() {
        let mut ed = MathEditor::empty();
        ed.insert_program(2);
        ed.insert_text("s <- 1");
        ed.move_down();
        // Line 1 is empty: Backspace should remove only line 1, keeping "s <- 1"
        ed.backspace();
        assert_eq!(ed.to_source(), "prog(s <- 1)");

        let mut big = MathEditor::empty();
        big.insert_big_op(BigOpKind::Integral);
        big.insert_text("0");
        big.move_up(); // upper limit is empty
        big.backspace(); // must move left, NOT destroy the integral with lower="0"
        assert_eq!(big.to_source(), "integ(, 0, )");
    }
}
