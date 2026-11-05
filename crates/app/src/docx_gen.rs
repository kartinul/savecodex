use anyhow::{Context, Result};
use crate::doc_theme::{cm, part_label, DocTheme, Look};
use docx_rs::{AlignmentType, BorderType, BreakType, Docx, Footer, LineSpacing, LineSpacingType, PageMargin, PageNum, Paragraph, ParagraphBorder, ParagraphBorderPosition, ParagraphBorders, Pic, Run, RunFonts, Shading, ShdType, Style, StyleType};
use std::path::Path;

#[derive(Default)]
pub struct PackItem {
    pub filename: String,
    /// Replaces the numbered filename heading (solve uses the question label).
    pub heading: Option<String>,
    /// Part positions under the heading (1(b)(i) → [2, 1]), each shown as a smaller heading.
    pub parts: Vec<usize>,
    pub question: String,
    pub content: String,
    pub answer: String,
    /// Written after the run (observations), shown below the screenshots.
    pub answer_after: String,
    /// Empty when there is nothing to show (written-only answers).
    pub shots: Vec<Shot>,
}

pub struct Shot {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn text_para(text: &str) -> Paragraph {
    let mut run = Run::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            run = run.add_break(BreakType::TextWrapping);
        }
        run = run.add_text(line);
    }
    Paragraph::new().add_run(run)
}

/// Written answers: blank-line separated plain paragraphs.
fn answer_paras(text: &str) -> Vec<Paragraph> {
    text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).map(text_para).collect()
}

fn fonts(font: &str) -> RunFonts {
    RunFonts::new().ascii(font).hi_ansi(font).cs(font).east_asia(font)
}

fn pt(v: f32) -> u32 {
    (v * 20.0).round() as u32
}

fn styled(id: &str, name: &str, look: &Look) -> Style {
    let mut style = Style::new(id, StyleType::Paragraph).name(name).line_spacing(LineSpacing::new().before(pt(look.before)));
    if look.size > 0.0 { style = style.size((look.size * 2.0).round() as usize) }
    if !look.font.is_empty() { style = style.fonts(fonts(&look.font)) }
    if !look.color.is_empty() { style = style.color(&look.color) }
    if look.bold { style = style.bold() }
    if look.italic { style = style.italic() }
    if look.underline { style = style.underline("single") }
    if look.center { style = style.align(AlignmentType::Center) }
    if look.rule {
        let rule = ParagraphBorder::new(ParagraphBorderPosition::Bottom).val(BorderType::Single).size(6).space(1).color(if look.color.is_empty() { "auto" } else { &look.color });
        style.paragraph_property = style.paragraph_property.set_borders(ParagraphBorders::with_empty().set(rule));
    }
    style
}

/// Paragraph styles every theme fills in, so Word's navigation pane and style list work.
fn with_styles(docx: Docx, theme: &DocTheme) -> Docx {
    let mut docx = docx
        .default_fonts(fonts(&theme.font))
        .default_size((theme.size * 2.0).round() as usize)
        .default_line_spacing(LineSpacing::new().line_rule(LineSpacingType::Auto).line((theme.line * 240.0).round() as i32).after(pt(theme.gap)))
        .add_style(styled("Title", "Title", &theme.title));
    for (level, look) in theme.headings.iter().enumerate() {
        let mut heading = styled(&format!("Heading{}", level + 1), &format!("heading {}", level + 1), look);
        heading.paragraph_property = heading.paragraph_property.keep_next(true).outline_lvl(level);
        docx = docx.add_style(heading);
    }
    let code_look = Look { font: theme.code_font.clone(), size: theme.code_size, ..Default::default() };
    let mut code = styled("Code", "Code", &code_look);
    code.paragraph_property = match theme.code_box.as_str() {
        "shade" => code.paragraph_property.shading(Shading::new().shd_type(ShdType::Clear).color("auto").fill("F2F2F2")),
        "border" => {
            let side = |p| ParagraphBorder::new(p).val(BorderType::Single).size(4).space(4).color("808080");
            code.paragraph_property.set_borders(ParagraphBorders::with_empty()
                .set(side(ParagraphBorderPosition::Top)).set(side(ParagraphBorderPosition::Bottom))
                .set(side(ParagraphBorderPosition::Left)).set(side(ParagraphBorderPosition::Right)))
        }
        _ => code.paragraph_property,
    };
    docx.add_style(code).add_style(styled("Question", "Question", &theme.question))
}

fn heading(style: &str, text: &str) -> Paragraph {
    Paragraph::new().style(style).add_run(Run::new().add_text(text))
}

fn label(text: &str) -> Paragraph {
    Paragraph::new().keep_next(true).add_run(Run::new().add_text(text).bold())
}

pub fn generate_docx(
    output: &Path,
    doc_title: &str,
    doc_text: Option<&str>,
    items: &[PackItem],
    page_break: bool,
    theme: &DocTheme,
) -> Result<()> {
    tracing::debug!("generate_docx called for output: {:?}, title: {:?}, items count: {}", output, doc_title, items.len());
    let (paper_w, paper_h) = if theme.paper == "letter" { (12240, 15840) } else { (11906, 16838) };
    let side = cm(theme.margin);
    let margin = PageMargin { top: cm(theme.margin_top), left: side, right: side, bottom: side, ..PageMargin::new() };
    let mut docx = with_styles(Docx::new(), theme).page_size(paper_w, paper_h).page_margin(margin);
    if theme.page_numbers {
        docx = docx.footer(Footer::new().add_paragraph(Paragraph::new().align(AlignmentType::Center).add_page_num(PageNum::new())));
    }
    docx = docx.add_paragraph(heading("Title", doc_title));

    if let Some(text) = doc_text {
        let text = text.replace("\\n", "\n");
        let mut desc_run = Run::new();
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                desc_run = desc_run.add_break(BreakType::TextWrapping);
            }
            desc_run = desc_run.add_text(line);
        }
        let desc_para = Paragraph::new().add_run(desc_run);
        docx = docx.add_paragraph(desc_para);
        
        // Default newline after doctext
        docx = docx.add_paragraph(Paragraph::new());
    }

    let fraction = match theme.shots.as_str() { "small" => 0.5, "medium" => 0.75, _ => 1.0 };
    let max_width = theme.text_width() as f64 / 15.0 * fraction; // 15 twips per px
    let mut prev: Option<(String, &[usize])> = None;
    for (index, item) in items.iter().enumerate() {
        let filename_no_ext = Path::new(&item.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&item.filename);
        let base = item.heading.clone().unwrap_or_else(|| format!("{}. {}", index + 1, filename_no_ext));
        // parts the previous item already headed stay unheaded; a new question starts from its heading 1
        let shared = match &prev {
            Some((b, parts)) if *b == base => Some(parts.iter().zip(&item.parts).take_while(|(a, b)| a == b).count()),
            _ => None,
        };

        if index > 0 {
            if page_break && shared.is_none() {
                docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
            } else {
                docx = docx.add_paragraph(Paragraph::new());
            }
        }

        if shared.is_none() && !base.is_empty() {
            let numbered = base.starts_with(|c: char| c.is_ascii_digit()) && item.heading.is_some();
            docx = docx.add_paragraph(heading("Heading1", &if numbered { format!("{}{base}", theme.prefix) } else { base.clone() }));
        }
        for depth in shared.unwrap_or(0)..item.parts.len() {
            docx = docx.add_paragraph(heading(&format!("Heading{}", (depth + 2).min(3)), &part_label(theme, &base, &item.parts[..=depth])));
        }
        if !item.question.is_empty() {
            docx = docx.add_paragraph(text_para(&item.question).style("Question"));
        }
        if !item.content.is_empty() {
            if theme.labels { docx = docx.add_paragraph(label("Code:")) }
            docx = docx.add_paragraph(text_para(&item.content).style("Code"));
        }
        if theme.labels && !item.shots.is_empty() { docx = docx.add_paragraph(label("Output:")) }
        for shot in &item.shots {
            // fit the text width and a 520 px height (the tallest terminal shot) without enlarging; 1 px = 9525 EMU
            let (w, h) = (shot.width.max(1) as f64, shot.height.max(1) as f64);
            let scale = (max_width / w).min(520.0 / h).min(1.0) * 9525.0;
            let pic = Pic::new(&shot.bytes).size((w * scale) as u32, (h * scale) as u32);
            let img_run = Run::new()
                .add_break(BreakType::TextWrapping) // new line before image
                .add_image(pic);
            let para = Paragraph::new().add_run(img_run);
            docx = docx.add_paragraph(if theme.shots_center { para.align(AlignmentType::Center) } else { para });
        }
        let after: Vec<Paragraph> = answer_paras(&item.answer).into_iter().chain(answer_paras(&item.answer_after)).collect();
        if !item.shots.is_empty() && !after.is_empty() {
            docx = docx.add_paragraph(Paragraph::new());
        }
        for para in after {
            docx = docx.add_paragraph(para);
        }
        prev = Some((base, &item.parts));
    }

    tracing::debug!("Creating DOCX file at {:?}", output);
    let docx_file = std::fs::File::create(output).context("Failed to create DOCX")?;
    tracing::debug!("Packing DOCX file...");
    docx.build().pack(docx_file).context("Failed to pack DOCX")?;
    tracing::debug!("Successfully packed DOCX file");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn sub_parts_get_smaller_headings_once() {
        let item = |parts: Vec<usize>| PackItem { heading: Some("1".into()), parts, content: "x".into(), ..Default::default() };
        let items = [item(vec![1, 1]), item(vec![1, 2]), item(vec![1, 2]), item(vec![2, 1]), PackItem { heading: Some("2".into()), ..Default::default() }];
        let dir = tempfile::tempdir().unwrap();
        for theme in DocTheme::presets() {
            let out = dir.path().join(format!("{}.docx", theme.name));
            generate_docx(&out, "T", None, &items, true, &theme).unwrap();
            let mut xml = String::new();
            zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap().by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
            let headings: Vec<&str> = xml.split("<w:pStyle w:val=\"Heading").skip(1)
                .map(|h| h.split("<w:t xml:space=\"preserve\">").nth(1).unwrap().split('<').next().unwrap()).collect();
            assert_eq!(headings, ["1", "(a)", "(i)", "(ii)", "(b)", "(i)", "2"], "{}", theme.name);
            assert_eq!(xml.matches("w:type=\"page\"").count(), 1, "one break, before question 2");
        }
    }
}
