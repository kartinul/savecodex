use anyhow::{Context, Result};
use docx_rs::{Docx, Paragraph, Run, AlignmentType, Pic, BreakType};
use std::path::Path;

pub struct PackItem {
    pub filename: String,
    pub content: String,
    pub img_bytes: Vec<u8>,
    pub img_width: u32,
    pub img_height: u32,
}

pub fn generate_docx(
    output: &Path,
    doc_title: &str,
    doc_text: Option<&str>,
    items: &[PackItem],
    page_break: bool,
) -> Result<()> {
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

        let heading_text = format!("{}. {}", index + 1, filename_no_ext);
        let h2_para = Paragraph::new().add_run(Run::new().add_text(heading_text).size(36).bold());
        
        let mut code_run = Run::new();
        for (i, line) in item.content.split('\n').enumerate() {
            if i > 0 {
                code_run = code_run.add_break(BreakType::TextWrapping);
            }
            code_run = code_run.add_text(line);
        }
        let code_para = Paragraph::new().add_run(code_run);
        
        let mut pic = Pic::new(&item.img_bytes);
        // Scale to fit on a standard page width (approx 600 pixels)
        // docx-rs 0.4 uses EMUs for image dimensions (1 pixel ≈ 9525 EMUs)
        let display_w = 600 * 9525;
        let display_h = if item.img_width > 0 {
            (600.0 * (item.img_height as f64 / item.img_width as f64) * 9525.0) as u32
        } else {
            600 * 9525
        };
        pic = pic.size(display_w, display_h); 
        
        let img_run = Run::new()
            .add_break(BreakType::TextWrapping) // new line before image
            .add_image(pic);
            
        let img_para = Paragraph::new().add_run(img_run);
        
        docx = docx.add_paragraph(h2_para).add_paragraph(code_para).add_paragraph(img_para);
    }

    let docx_file = std::fs::File::create(output).context("Failed to create DOCX")?;
    docx.build().pack(docx_file).context("Failed to pack DOCX")?;
    
    Ok(())
}
