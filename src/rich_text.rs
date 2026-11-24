use crate::editing::{paragraph_text_format, TextAlignment, TextFont, TextFormat};
use crate::parse::{elements, named, Xml};
use std::ops::Range;

const WS: &str = "http://schemas.mathsoft.com/worksheet30";

#[derive(Clone, Debug)]
pub struct RichText {
    pub body: String,
    pub alignment: TextAlignment,
    formats: Vec<TextFormat>,
    initial: TextFormat,
}

impl RichText {
    pub fn plain(body: &str, format: TextFormat) -> Self {
        Self {
            body: body.to_owned(),
            alignment: format.alignment,
            formats: vec![format; body.chars().count()],
            initial: format,
        }
    }

    pub(crate) fn from_xml(text: Xml<'_, '_>) -> Option<Self> {
        let mut paragraphs = elements(text).peekable();
        let first = *paragraphs.peek()?;
        let initial = paragraph_text_format(first)?;
        let mut rich = Self::plain("", initial);
        for (index, paragraph) in paragraphs.enumerate() {
            if !named(paragraph, WS, "p")
                || paragraph.attribute("style") != first.attribute("style")
            {
                return None;
            }
            let format = paragraph_text_format(paragraph)?;
            if format.alignment != rich.alignment {
                return None;
            }
            if index > 0 {
                rich.push("\n", format);
            }
            rich.read_children(paragraph, format, 0)?;
        }
        Some(rich)
    }

    fn push(&mut self, text: &str, format: TextFormat) {
        self.body.push_str(text);
        self.formats
            .extend(std::iter::repeat_n(format, text.chars().count()));
    }

    fn read_children(
        &mut self,
        parent: Xml<'_, '_>,
        format: TextFormat,
        depth: usize,
    ) -> Option<()> {
        if depth > 24 {
            return None;
        }
        for node in parent.children() {
            if node.is_text() {
                let text = node.text()?;
                if text.trim().is_empty()
                    && text.contains('\n')
                    && elements(parent).next().is_some()
                {
                    continue;
                }
                self.push(text, format);
                continue;
            }
            if !node.is_element() {
                continue;
            }
            if node.tag_name().namespace() != Some(WS) {
                return None;
            }
            let mut next = format;
            match node.tag_name().name() {
                "b" => next.bold = true,
                "i" => next.italic = true,
                "u" => next.underline = true,
                "f" => {
                    if !matches!(node.attribute("charset"), None | Some("0")) {
                        return None;
                    }
                    if let Some(family) = node.attribute("family") {
                        next.font = TextFont::OPTIONS
                            .into_iter()
                            .find(|font| font.name().eq_ignore_ascii_case(family))?;
                    }
                    if let Some(size) = node.attribute("size") {
                        next.size = size
                            .parse::<u16>()
                            .ok()
                            .filter(|size| (6..=72).contains(size))?;
                    }
                }
                _ => return None,
            }
            self.read_children(node, next, depth + 1)?;
        }
        Some(())
    }

    pub fn format_at(&self, index: usize) -> TextFormat {
        self.formats.get(index).copied().unwrap_or(self.initial)
    }

    pub fn apply(&mut self, range: Range<usize>, action: impl Fn(&mut TextFormat)) {
        let end = range.end.min(self.formats.len());
        for format in &mut self.formats[range.start.min(end)..end] {
            action(format);
        }
    }

    pub fn sync(&mut self, new_body: &str) {
        if new_body == self.body {
            return;
        }
        let before: Vec<_> = self.body.chars().collect();
        let after: Vec<_> = new_body.chars().collect();
        let common_start = before
            .iter()
            .zip(&after)
            .take_while(|(a, b)| a == b)
            .count();
        let common_end = before[common_start..]
            .iter()
            .rev()
            .zip(after[common_start..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let inherited = self.format_at(common_start.saturating_sub(1).min(before.len()));
        self.formats.splice(
            common_start..before.len() - common_end,
            std::iter::repeat_n(inherited, after.len() - common_start - common_end),
        );
        self.body = new_body.to_owned();
    }

    pub fn runs(&self) -> Vec<(&str, TextFormat)> {
        let mut runs = Vec::new();
        let mut start = 0;
        let mut current = self.initial;
        for ((byte, _), &format) in self.body.char_indices().zip(&self.formats) {
            if format != current {
                if byte > start {
                    runs.push((&self.body[start..byte], current));
                }
                start = byte;
                current = format;
            }
        }
        if start < self.body.len() {
            runs.push((&self.body[start..], current));
        }
        runs
    }
}
