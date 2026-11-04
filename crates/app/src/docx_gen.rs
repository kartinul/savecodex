use anyhow::{Context, Result};
use docx_rs::{Docx, Paragraph, Run, AlignmentType, Pic, BreakType};
use std::path::Path;

#[derive(Default)]
pub struct PackItem {
    pub filename: String,
    /// Replaces the numbered filename heading (solve uses the question label).
    pub heading: Option<String>,
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

pub fn generate_docx(
    output: &Path,
    doc_title: &str,
    doc_text: Option<&str>,
    items: &[PackItem],
    page_break: bool,
) -> Result<()> {
    tracing::debug!("generate_docx called for output: {:?}, title: {:?}, items count: {}", output, doc_title, items.len());
    let mut docx = Docx::new();
    let title_para = Paragraph::new()
        .align(AlignmentType::Center)
        .add_run(Run::new().add_text(doc_title).size(48));
    docx = docx.add_paragraph(title_para);
    
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

    for (index, item) in items.iter().enumerate() {
        let filename_no_ext = Path::new(&item.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&item.filename);

        if index > 0 {
            if page_break {
                docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
            } else {
                // Add a blank line (empty paragraph) between items if no page break is used
                docx = docx.add_paragraph(Paragraph::new());
            }
        }

        let heading_text = item.heading.clone().unwrap_or_else(|| format!("{}. {}", index + 1, filename_no_ext));
        if !heading_text.is_empty() {
            docx = docx.add_paragraph(Paragraph::new().keep_next(true).add_run(Run::new().add_text(heading_text).size(26).bold()));
        }
        for text in [&item.question, &item.content] {
            if !text.is_empty() {
                docx = docx.add_paragraph(text_para(text));
            }
        }
        for shot in &item.shots {
            // fit a 600x520 px box (the tallest terminal shot) without enlarging; 1 px = 9525 EMU
            let (w, h) = (shot.width.max(1) as f64, shot.height.max(1) as f64);
            let scale = (600.0 / w).min(520.0 / h).min(1.0) * 9525.0;
            let pic = Pic::new(&shot.bytes).size((w * scale) as u32, (h * scale) as u32);
            let img_run = Run::new()
                .add_break(BreakType::TextWrapping) // new line before image
                .add_image(pic);
            docx = docx.add_paragraph(Paragraph::new().add_run(img_run));
        }
        let after: Vec<Paragraph> = answer_paras(&item.answer).into_iter().chain(answer_paras(&item.answer_after)).collect();
        if !item.shots.is_empty() && !after.is_empty() {
            docx = docx.add_paragraph(Paragraph::new());
        }
        for para in after {
            docx = docx.add_paragraph(para);
        }
    }

    tracing::debug!("Creating DOCX file at {:?}", output);
    let docx_file = std::fs::File::create(output).context("Failed to create DOCX")?;
    tracing::debug!("Packing DOCX file...");
    docx.build().pack(docx_file).context("Failed to pack DOCX")?;
    tracing::debug!("Successfully packed DOCX file");
    
    Ok(())
}
