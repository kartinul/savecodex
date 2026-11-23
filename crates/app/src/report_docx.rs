//! A report `Draft` as a docx: the cover, an optional contents page, then numbered sections with
//! captioned tables and figures, in the chosen doc theme (Word 365's look by default).

use std::path::Path;

use anyhow::{Context, Result};
use docx_rs::{
    AbstractNumbering, AlignmentType, BreakType, Docx, Footer, IndentLevel, Level, LevelJc, LevelOverride, LevelText, LineSpacing, NumberFormat, Numbering,
    NumberingId, PageMargin, PageNum, Paragraph, Pic, Run, Shading, ShdType, SpecialIndentType, Start, Table, TableCell, TableOfContents, TableRow, WidthType,
};

use crate::doc_theme::{cm, DocTheme, Look};
use crate::docx_gen::{fonts, kerned, styled, with_styles, word_settings};
use crate::report::{Block, Draft, Figure};

pub const JIIT_LOGO: &[u8] = include_bytes!("../assets/jiit.png");

const BULLETS: usize = 1;
const DECIMAL: usize = 2;

/// Writes `draft` to `out`. `picture` draws a figure; one it can't draw shows why in its place.
pub fn generate(out: &Path, draft: &Draft, theme: &DocTheme, logo: Option<&[u8]>, picture: &dyn Fn(&Figure) -> Result<Vec<u8>>) -> Result<()> {
    let theme = theme.patched(draft.look.clone());
    let (paper_w, paper_h) = if theme.paper == "letter" { (12240, 15840) } else { (11906, 16838) };
    let side = cm(theme.margin);
    let margin = PageMargin { top: cm(theme.margin_top), left: side, right: side, bottom: side, header: 708, footer: 708, gutter: 0 };
    let caption = Look { size: 9.0, italic: true, color: "0E2841".into(), after: Some(10.0), line: Some(1.0), ..Default::default() };
    let level = |format: &str, text: &str| Level::new(0, Start::new(1), NumberFormat::new(format), LevelText::new(text), LevelJc::new("left"))
        .indent(Some(720), Some(SpecialIndentType::Hanging(360)), None, None);
    let mut docx = with_styles(Docx::new(), &theme)
        .add_style(styled("Caption", "caption", &caption))
        .page_size(paper_w, paper_h)
        .page_margin(margin)
        .title_pg()
        .first_footer(Footer::new())
        .footer(Footer::new().add_paragraph(Paragraph::new().align(AlignmentType::Center).add_page_num(PageNum::new())))
        .add_abstract_numbering(AbstractNumbering::new(BULLETS).add_level(level("bullet", "•")))
        .add_abstract_numbering(AbstractNumbering::new(DECIMAL).add_level(level("decimal", "%1.")))
        .add_numbering(Numbering::new(BULLETS, BULLETS));
    let width = theme.text_width();

    docx = cover(docx, draft, logo.unwrap_or(JIIT_LOGO), width);
    if draft.toc {
        docx = docx.add_paragraph(page_break())
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Contents").bold().size(32)))
            .add_table_of_contents(TableOfContents::new().heading_styles_range(1, 2).auto().dirty())
            .add_paragraph(page_break());
    } else {
        docx = docx.add_paragraph(Paragraph::new());
    }

    let code_font = if theme.code_font.is_empty() { "Consolas".to_string() } else { theme.code_font.clone() };
    let (mut chapter, mut tables, mut figures, mut lists) = (0, 0, 0, DECIMAL);
    for section in &draft.sections {
        if section.numbered { chapter += 1 }
        let title = plain_title(&section.title);
        docx = docx.add_paragraph(styled_text("Heading1", &if section.numbered { format!("{chapter}. {title}") } else { title.to_string() }));
        let mut sub = 0;
        for block in &section.blocks {
            match block {
                Block::Heading { text } => {
                    sub += 1;
                    let text = plain_title(text);
                    docx = docx.add_paragraph(styled_text("Heading2", &if section.numbered { format!("{chapter}.{sub} {text}") } else { text.to_string() }));
                }
                Block::Para { text } => {
                    for p in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) { docx = docx.add_paragraph(inline(Paragraph::new(), p, &code_font)) }
                }
                Block::Bullets { items } => {
                    for item in items { docx = docx.add_paragraph(inline(Paragraph::new(), item, &code_font).numbering(NumberingId::new(BULLETS), IndentLevel::new(0))) }
                }
                Block::Numbered { items } => {
                    lists += 1;
                    docx = docx.add_numbering(Numbering::new(lists, DECIMAL).add_override(LevelOverride::new(0).start(1)));
                    for item in items { docx = docx.add_paragraph(inline(Paragraph::new(), item, &code_font).numbering(NumberingId::new(lists), IndentLevel::new(0))) }
                }
                Block::Table { caption, header, rows } => {
                    tables += 1;
                    docx = docx.add_paragraph(styled_text("Caption", &format!("Table {tables}: {caption}")).keep_next(true)).add_table(table(header, rows, width, &code_font));
                }
                Block::Figure(figure) => {
                    figures += 1;
                    let shown = match picture(figure).and_then(|png| fitted(&png, width)) {
                        Ok(pic) => Paragraph::new().add_run(Run::new().add_image(pic)),
                        Err(e) => Paragraph::new().add_run(Run::new().add_text(format!("[{}: {e:#}]", figure.caption))),
                    };
                    docx = docx.add_paragraph(shown.align(AlignmentType::Center).keep_next(true))
                        .add_paragraph(styled_text("Caption", &format!("Figure {figures}: {}", figure.caption)).align(AlignmentType::Center));
                }
            }
        }
    }

    let file = std::fs::File::create(out).with_context(|| format!("can't write {}", out.display()))?;
    let mut xml = docx.build();
    xml.styles = kerned(&xml.styles, &theme);
    xml.settings = word_settings(&xml.settings);
    if draft.toc {
        // the contents' page numbers are only placeholders; Word fills them in on open
        xml.settings = String::from_utf8_lossy(&xml.settings).replacen("</w:settings>", r#"<w:updateFields w:val="true" /></w:settings>"#, 1).into_bytes();
    }
    xml.pack(file).context("Failed to pack DOCX")?;
    Ok(())
}

/// Logo, title, course and semester centred, then who submitted it to whom side by side.
fn cover(mut docx: Docx, draft: &Draft, logo: &[u8], width: u32) -> Docx {
    let c = &draft.cover;
    if let Ok(pic) = fitted(logo, cm(8.0) as u32) {
        docx = docx.add_paragraph(Paragraph::new().align(AlignmentType::Center).add_run(Run::new().add_image(pic)));
    }
    docx = docx.add_paragraph(styled_text("Title", &c.title).align(AlignmentType::Center));
    let course = match (c.course.trim(), c.code.trim()) {
        ("", code) => code.to_string(),
        (course, "") => course.to_string(),
        (course, code) => format!("{course} ({code})"),
    };
    for line in [course.as_str(), c.semester.trim()].into_iter().filter(|l| !l.is_empty()) {
        docx = docx.add_paragraph(Paragraph::new().align(AlignmentType::Center).add_run(Run::new().add_text(line)));
    }
    if c.members.is_empty() && c.supervisors.is_empty() { return docx }
    docx = docx.add_paragraph(Paragraph::new()).add_paragraph(Paragraph::new());
    let by = c.members.iter().filter(|m| !m.name.trim().is_empty())
        .map(|m| if m.enrollment.trim().is_empty() { m.name.trim().to_string() } else { format!("{} - {}", m.name.trim(), m.enrollment.trim()) });
    let to = c.supervisors.iter().filter(|s| !s.name.trim().is_empty()).flat_map(|s| {
        let name = format!("{} {}", s.honorific.trim(), s.name.trim()).trim().to_string();
        std::iter::once(name).chain(Some(s.designation.trim().to_string()).filter(|d| !d.is_empty()))
    });
    let tight = |run: Run| Paragraph::new().line_spacing(LineSpacing::new().after(0)).add_run(run);
    let column = |head: &str, lines: Vec<String>| lines.into_iter()
        .fold(TableCell::new().width(width as usize / 2, WidthType::Dxa).add_paragraph(tight(Run::new().add_text(head).bold())),
            |cell, line| cell.add_paragraph(tight(Run::new().add_text(line))));
    let row = TableRow::new(vec![column("Submitted by", by.collect()), column("Submitted to", to.collect())]);
    docx.add_table(Table::without_borders(vec![row]).set_grid(vec![width as usize / 2; 2]).width(width as usize, WidthType::Dxa))
}

fn table(header: &[String], rows: &[Vec<String>], width: u32, code_font: &str) -> Table {
    let cols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).max(1);
    let cell = |text: &str, head: bool| {
        let cell = TableCell::new().width(width as usize / cols, WidthType::Dxa);
        if head { cell.shading(Shading::new().shd_type(ShdType::Clear).color("auto").fill("F2F2F2")).add_paragraph(Paragraph::new().add_run(Run::new().add_text(text).bold())) }
        else { cell.add_paragraph(inline(Paragraph::new(), text, code_font)) }
    };
    let row = |cells: &[String], bold: bool| TableRow::new((0..cols).map(|i| cell(cells.get(i).map_or("", String::as_str), bold)).collect());
    let body = std::iter::once(header).filter(|h| !h.is_empty()).map(|h| row(h, true)).chain(rows.iter().map(|r| row(r, false)));
    Table::new(body.collect()).set_grid(vec![width as usize / cols; cols]).width(width as usize, WidthType::Dxa)
}

fn styled_text(style: &str, text: &str) -> Paragraph {
    Paragraph::new().style(style).add_run(Run::new().add_text(text))
}

fn page_break() -> Paragraph {
    Paragraph::new().add_run(Run::new().add_break(BreakType::Page))
}

/// Text with `**bold**` and `` `code` `` pieces, the only markup models are told they may use.
fn inline(mut p: Paragraph, text: &str, code_font: &str) -> Paragraph {
    let code_parts: Vec<&str> = text.split('`').collect();
    let code_ok = code_parts.len() % 2 == 1;
    for (i, piece) in code_parts.iter().enumerate() {
        if code_ok && i % 2 == 1 {
            p = p.add_run(Run::new().add_text(*piece).fonts(fonts(code_font)));
            continue;
        }
        let piece = if code_ok || i == 0 { piece.to_string() } else { format!("`{piece}") };
        let bold_parts: Vec<&str> = piece.split("**").collect();
        let bold_ok = bold_parts.len() % 2 == 1;
        for (k, part) in bold_parts.iter().enumerate() {
            let part = if bold_ok || k == 0 { part.to_string() } else { format!("**{part}") };
            for (n, line) in part.split('\n').enumerate() {
                let mut run = Run::new();
                if n > 0 { run = run.add_break(BreakType::TextWrapping) }
                run = run.add_text(line);
                p = p.add_run(if bold_ok && k % 2 == 1 { run.bold() } else { run });
            }
        }
    }
    p
}

/// A heading without the number a model put in front of it ("2.1 Scope", "Chapter 3: Design").
fn plain_title(title: &str) -> &str {
    let t = title.trim();
    let t = t.strip_prefix("Chapter").map_or(t, |rest| rest.trim_start().trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches([':', '.', '-', ' ']));
    let number = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    if number > 0 && t[number..].starts_with([' ', ')']) { t[number..].trim_start_matches([' ', ')']).trim() } else { t }
}

/// A picture no wider than `max_w` twips nor taller than three quarters of an A4 text block, never enlarged past 144 dpi.
fn fitted(png: &[u8], max_w: u32) -> Result<Pic> {
    let (w, h) = image::io::Reader::new(std::io::Cursor::new(png)).with_guessed_format()?.into_dimensions().context("not a picture")?;
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let scale = (max_w as f64 / w).min(10500.0 / h).min(10.0);
    // 1 twip = 635 EMU
    Ok(Pic::new(png).size((w * scale * 635.0) as u32, (h * scale * 635.0) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn titles_lose_model_numbers() {
        for (raw, want) in [("1. Introduction", "Introduction"), ("2.1 Scope", "Scope"), ("Chapter 3: Design", "Design"), ("3D Engine", "3D Engine"), ("Abstract", "Abstract")] {
            assert_eq!(plain_title(raw), want);
        }
    }

    #[test]
    fn draft_becomes_docx() {
        let draft: Draft = serde_json::from_str(r#"{"toc":true,"cover":{"title":"Clipboard-to-MP3","course":"Unix Lab","code":"24B15CS214","semester":"B.Tech II Year",
            "members":[{"name":"Kartik Sharma","enrollment":"992501030333"}],"supervisors":[{"honorific":"Ms.","name":"Pallavi Sharma"}]},
            "sections":[{"title":"Abstract","numbered":false,"blocks":[{"type":"para","text":"A **one-tap** tool using `yt-dlp`."}]},
            {"title":"1. Introduction","blocks":[{"type":"heading","text":"Objectives"},{"type":"bullets","items":["one","two"]},{"type":"numbered","items":["a"]},
             {"type":"table","caption":"Commands","header":["Command","Purpose"],"rows":[["ls","list"]]},{"type":"figure","caption":"Flow","upload":"x.png"}]}]}"#).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("r.docx");
        generate(&out, &draft, &DocTheme::load("word").unwrap(), None, &|_| anyhow::bail!("no such upload")).unwrap();
        let mut xml = String::new();
        zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap().by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
        for want in ["Unix Lab (24B15CS214)", "Kartik Sharma - 992501030333", "Ms. Pallavi Sharma", "1. Introduction", "1.1 Objectives", "Table 1: Commands", "Figure 1: Flow", "[Flow: no such upload]", "w:titlePg", "w:dirty=\"true\""] {
            assert!(xml.contains(want), "missing {want}");
        }
        assert!(!xml.contains("1. 1. Introduction") && !xml.contains("**"));
    }
}
