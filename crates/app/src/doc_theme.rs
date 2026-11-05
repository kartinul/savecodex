use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How one kind of paragraph looks. An empty font or colour follows the body text.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Look {
    pub font: String,
    pub size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub center: bool,
    /// A line under the paragraph, in `rule_color` or else the text's colour.
    pub rule: bool,
    pub rule_color: String,
    /// Kerning from 14pt, as Word's Title has it.
    pub kern: bool,
    /// Space above, in points.
    pub before: f32,
    /// Space below in points; None keeps the text's.
    pub after: Option<f32>,
    /// Line spacing multiple; None keeps the text's.
    pub line: Option<f32>,
    /// Extra space between letters in points, negative to tighten.
    pub spacing: f32,
}

/// The whole look of a generated DOCX. Sizes are points, margins centimetres.
#[derive(Clone, Deserialize, Serialize)]
pub struct DocTheme {
    pub name: String,
    pub paper: String,
    pub margin: f32,
    pub margin_top: f32,
    pub font: String,
    pub size: f32,
    pub line: f32,
    pub gap: f32,
    /// Kerning and ligatures on all text, like Word 365's own defaults.
    pub kern: bool,
    pub title: Look,
    pub headings: [Look; 3],
    pub question: Look,
    pub code_font: String,
    pub code_size: f32,
    /// none, shade or border.
    pub code_box: String,
    /// small, medium or full: how much of the text width a screenshot may take.
    pub shots: String,
    pub shots_center: bool,
    pub page_numbers: bool,
    /// "Code:" and "Output:" above the code and the screenshots.
    pub labels: bool,
    pub show_question: bool,
    /// Put before a numbered question heading, like "Q" or "Question ".
    pub prefix: String,
    /// 1(b)(i), 1(2)(1), 1.b.i or 1.2.1.
    pub numbering: String,
    /// Sub-part headings show the whole label (1(b)(i)) instead of only their own part ((i)).
    pub full_labels: bool,
}

impl Default for DocTheme {
    /// What Word showed before themes existed: no fonts set, so Times New Roman 10.
    fn default() -> Self {
        let heading = |size| Look { size, bold: true, ..Default::default() };
        Self {
            name: "current".into(),
            paper: "a4".into(),
            margin: 3.0,
            margin_top: 3.5,
            font: "Times New Roman".into(),
            size: 10.0,
            line: 1.0,
            gap: 0.0,
            kern: false,
            title: Look { size: 24.0, center: true, ..Default::default() },
            headings: [heading(13.0), heading(12.0), heading(11.0)],
            question: Look::default(),
            code_font: String::new(),
            code_size: 10.0,
            code_box: "none".into(),
            shots: "full".into(),
            shots_center: false,
            page_numbers: false,
            labels: false,
            show_question: false,
            prefix: String::new(),
            numbering: "1(b)(i)".into(),
            full_labels: false,
        }
    }
}

/// Fields `patch` leaves out keep `base`'s value, so presets and saved themes only list what they change.
fn merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => p.into_iter().for_each(|(k, v)| merge(b.entry(k).or_insert(Value::Null), v)),
        (b, p) => *b = p,
    }
}

impl DocTheme {
    pub fn presets() -> Vec<DocTheme> {
        let list: Vec<Value> = serde_json::from_str(include_str!("default_doc_themes.json")).expect("embedded doc themes");
        list.into_iter().map(|p| Self::from_value(p).expect("embedded doc theme")).collect()
    }

    fn from_value(patch: Value) -> Result<DocTheme> {
        let mut theme = serde_json::to_value(DocTheme::default())?;
        merge(&mut theme, patch);
        Ok(serde_json::from_value::<DocTheme>(theme)?.sanitized())
    }

    /// A preset name, a theme as JSON, or a path to a theme .json file.
    pub fn load(spec: &str) -> Result<DocTheme> {
        let spec = spec.trim();
        if spec.starts_with('{') {
            return Self::from_value(serde_json::from_str(spec).context("bad doc theme JSON")?);
        }
        if let Some(theme) = Self::presets().into_iter().find(|t| t.name == spec) {
            return Ok(theme);
        }
        let text = std::fs::read_to_string(spec).with_context(|| format!("no doc theme or file named {spec:?}"))?;
        Self::from_value(serde_json::from_str(&text).with_context(|| format!("bad doc theme in {spec}"))?)
    }

    /// A theme sent by the web UI: JSON or a preset name, never a path. Anything unreadable gets the current look.
    pub fn from_web(spec: &str) -> DocTheme {
        let spec = spec.trim();
        let parsed = if spec.starts_with('{') { serde_json::from_str(spec).ok().and_then(|v| Self::from_value(v).ok()) } else { Self::presets().into_iter().find(|t| t.name == spec) };
        parsed.unwrap_or_default()
    }

    /// Themes can come from the web, so every value is kept within what Word handles.
    fn sanitized(mut self) -> Self {
        let pick = |v: &mut String, ok: &[&str]| if !ok.contains(&v.as_str()) { *v = ok[0].into() };
        let font = |f: &mut String| f.truncate(f.char_indices().nth(48).map_or(f.len(), |(i, _)| i));
        pick(&mut self.paper, &["a4", "letter"]);
        pick(&mut self.code_box, &["none", "shade", "border"]);
        pick(&mut self.shots, &["full", "medium", "small"]);
        pick(&mut self.numbering, &["1(b)(i)", "1(2)(1)", "1.b.i", "1.2.1"]);
        self.margin = self.margin.clamp(0.5, 6.0);
        self.margin_top = self.margin_top.clamp(0.5, 6.0);
        self.size = self.size.clamp(6.0, 36.0);
        self.code_size = self.code_size.clamp(6.0, 36.0);
        self.line = self.line.clamp(0.8, 3.0);
        self.gap = self.gap.clamp(0.0, 36.0);
        font(&mut self.font);
        font(&mut self.code_font);
        font(&mut self.prefix);
        for look in std::iter::once(&mut self.title).chain(&mut self.headings).chain(std::iter::once(&mut self.question)) {
            font(&mut look.font);
            for color in [&mut look.color, &mut look.rule_color] {
                if color.len() != 6 || !color.chars().all(|c| c.is_ascii_hexdigit()) { color.clear() }
            }
            look.size = if look.size == 0.0 { 0.0 } else { look.size.clamp(6.0, 72.0) };
            look.before = look.before.clamp(0.0, 72.0);
            look.after = look.after.map(|a| a.clamp(0.0, 72.0));
            look.line = look.line.map(|l| l.clamp(0.8, 3.0));
            look.spacing = look.spacing.clamp(-5.0, 10.0);
        }
        self
    }

    /// The text width in twips.
    pub fn text_width(&self) -> u32 {
        let paper = if self.paper == "letter" { 12240 } else { 11906 };
        paper - 2 * cm(self.margin) as u32
    }
}

pub fn cm(v: f32) -> i32 {
    (v * 567.0).round() as i32
}

/// The heading of a part `parts` deep under `base`: the whole label, or only the last part.
pub fn part_label(theme: &DocTheme, base: &str, parts: &[usize]) -> String {
    let segment = |depth: usize, n: usize| {
        let letter = || if depth == 0 { crate::solve::letter(n) } else { crate::solve::roman(n) };
        match theme.numbering.as_str() {
            "1(2)(1)" => format!("({n})"),
            "1.b.i" => format!(".{}", letter()),
            "1.2.1" => format!(".{n}"),
            _ => format!("({})", letter()),
        }
    };
    if theme.full_labels {
        return std::iter::once(base.to_string()).chain(parts.iter().enumerate().map(|(d, &n)| segment(d, n))).collect();
    }
    let last = segment(parts.len() - 1, parts[parts.len() - 1]);
    match last.strip_prefix('.') {
        Some(dotted) => format!("{dotted}."),
        None => last,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        let mut t = DocTheme::default();
        assert_eq!(part_label(&t, "1", &[2, 1]), "(i)");
        assert_eq!(part_label(&t, "1", &[2]), "(b)");
        t.full_labels = true;
        assert_eq!(part_label(&t, "1", &[2, 3]), "1(b)(iii)");
        t.numbering = "1.2.1".into();
        assert_eq!(part_label(&t, "1", &[2, 3]), "1.2.3");
        t.full_labels = false;
        assert_eq!(part_label(&t, "1", &[2]), "2.");
    }

    #[test]
    fn presets_and_sanitizing() {
        assert_eq!(DocTheme::presets()[0].name, "current");
        let t = DocTheme::load(r#"{"size": 500, "title": {"color": "red\"/>"}, "code_box": "x"}"#).unwrap();
        assert_eq!((t.size, t.title.color.as_str(), t.code_box.as_str(), t.title.size), (36.0, "", "none", 24.0));
    }
}
