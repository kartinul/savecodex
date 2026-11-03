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
    /// Empty when there is nothing to show (written-only answers).
    pub img_bytes: Vec<u8>,
    pub img_width: u32,
    pub img_height: u32,
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
        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(heading_text).size(36).bold()));
        for text in [&item.question, &item.content, &item.answer] {
            if !text.is_empty() {
                docx = docx.add_paragraph(text_para(text));
            }
        }
        if item.img_bytes.is_empty() {
            continue;
        }

        // Scale to fit on a standard page width (approx 600 pixels)
        // docx-rs 0.4 uses EMUs for image dimensions (1 pixel ≈ 9525 EMUs)
        let display_w = 600 * 9525;
        let display_h = if item.img_width > 0 {
            (600.0 * (item.img_height as f64 / item.img_width as f64) * 9525.0) as u32
        } else {
            600 * 9525
        };
        let pic = Pic::new(&item.img_bytes).size(display_w, display_h);
        let img_run = Run::new()
            .add_break(BreakType::TextWrapping) // new line before image
            .add_image(pic);
        docx = docx.add_paragraph(Paragraph::new().add_run(img_run));
    }

    tracing::debug!("Creating DOCX file at {:?}", output);
    let docx_file = std::fs::File::create(output).context("Failed to create DOCX")?;
    tracing::debug!("Packing DOCX file...");
    docx.build().pack(docx_file).context("Failed to pack DOCX")?;
    tracing::debug!("Successfully packed DOCX file");
    
    Ok(())
}
