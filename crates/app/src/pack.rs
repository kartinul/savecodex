use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use tracing::info;
use natord::compare;

pub async fn run_pack(
    folder: &Path,
    output: Option<&Path>,
    extensions: &[String],
    doc_title: Option<&str>,
    doc_text: Option<&str>,
    page_break: bool,
    opts: &crate::term_gen::TermGenOptions,
) -> Result<()> {
    info!("Starting pack command for folder: {}", folder.display());

    let folder_name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("folder");

    let resolved_title = doc_title.map(|s| s.to_string()).unwrap_or_else(|| folder_name.to_string());
    
    let resolved_output = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{}_pack", folder_name)));

    let mut all_found_files = Vec::new();
    let walker = WalkBuilder::new(folder)
        .max_depth(Some(1))
        .git_ignore(true)
        .build();

    for result in walker {
        if let Ok(entry) = result {
            if entry.path().is_file() {
                all_found_files.push(entry.path().to_path_buf());
            }
        }
    }

    let resolved_extensions = if extensions.is_empty() {
        info!("No extensions provided. Auto-detecting most common supported extension...");
        let configs = crate::runner::load_configurations().unwrap_or_default();
        let mut ext_counts: HashMap<String, usize> = HashMap::new();
        
        for path in &all_found_files {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if configs.contains_key(ext) {
                    *ext_counts.entry(ext.to_string()).or_insert(0) += 1;
                }
            }
        }
        
        if ext_counts.is_empty() {
            bail!("Auto-detection failed: no files with supported extensions found in folder.");
        }
        
        let (most_used, _) = ext_counts.into_iter().max_by_key(|(_, count)| *count).unwrap();
        info!("Auto-detected extension: {}", most_used);
        vec![most_used]
    } else {
        extensions.to_vec()
    };

    let mut files: Vec<PathBuf> = Vec::new();
    for path in all_found_files {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if resolved_extensions.iter().any(|e| e == ext) {
                files.push(path);
            }
        }
    }



    if files.is_empty() {
        println!("No matching files found in {}", folder.display());
        return Ok(());
    }

    files.sort_by(|a, b| {
        let name_a = a.file_name().unwrap_or_default().to_string_lossy();
        let name_b = b.file_name().unwrap_or_default().to_string_lossy();
        compare(&name_a, &name_b)
    });

    info!("Found {} files to process.", files.len());

    let mut pack_items = Vec::new();

    for path in &files {
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        info!("Processing file: {}", filename);

        let content = fs::read_to_string(path).context("Failed to read file")?;
        
        let input_text = match crate::ai::generate_input(&[(&filename, &content)]).await {
            Ok(t) => t,
            Err(e) => {
                info!("Failed to generate input for {}: {}", filename, e);
                String::new()
            }
        };

        let (_cmd, out) = crate::runner::run_file(path, if input_text.is_empty() { None } else { Some(&input_text) }).await?;
        
        let mut lines: Vec<&str> = out.split('\n').collect();
        let max_lines = 150;
        let max_line_len = 200;
        
        let mut truncated = false;
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            truncated = true;
        }
        
        let mut safe_out = String::new();
        for line in lines {
            if line.len() > max_line_len {
                safe_out.push_str(&line[..max_line_len]);
                safe_out.push_str("... [line truncated]\n");
            } else {
                safe_out.push_str(line);
                safe_out.push('\n');
            }
        }
        
        if truncated {
            safe_out.push_str("\n... [output truncated due to length] ...\n");
        }

        let fake_prompt = if !opts.no_prompt_highlight {
            format!("{}{}\n", opts.prompt, format!("run {}", filename))
        } else {
            String::new()
        };
        
        let raw = format!("{}{}", fake_prompt, safe_out);

        let mut img = crate::term_gen::generate_terminal_image(&raw, opts)?;
        
        let max_w = 4000;
        let max_h = 10000;
        if img.width() > max_w || img.height() > max_h {
            let crop_w = img.width().min(max_w);
            let crop_h = img.height().min(max_h);
            img = image::imageops::crop(&mut img, 0, 0, crop_w, crop_h).to_image();
        }

        let img_width = img.width();
        let img_height = img.height();

        let mut img_buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut img_buf, image::ImageOutputFormat::Png)?;
        let img_bytes = img_buf.into_inner();

        pack_items.push(crate::docx_gen::PackItem {
            filename,
            content,
            img_bytes,
            img_width,
            img_height,
        });
    }

    let out_docx = resolved_output.with_extension("docx");
    crate::docx_gen::generate_docx(&out_docx, &resolved_title, doc_text, &pack_items, page_break)?;
    info!("Wrote {}", out_docx.display());

    Ok(())
}
