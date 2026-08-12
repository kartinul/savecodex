use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
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
    ai_config: Option<crate::ai::AiConfig>,
    jobs: usize,
) -> Result<()> {
    tracing::debug!("run_pack: folder={:?} output={:?} extensions={:?} jobs={}", folder, output, extensions, jobs);
    info!("Starting pack: {}", folder.display());

    let folder_name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("folder");

    let resolved_title = doc_title.map(|s| s.to_string()).unwrap_or_else(|| folder_name.to_string());

    let resolved_output = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{}_pack", folder_name)));

    let mut all_found_files = Vec::new();
    let walker = WalkBuilder::new(folder).git_ignore(true).build();
    for result in walker {
        if let Ok(entry) = result {
            if entry.path().is_file() {
                all_found_files.push(entry.path().to_path_buf());
            }
        }
    }

    let resolved_extensions = if extensions.is_empty() {
        info!("No extensions provided — auto-detecting most common supported extension…");
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
        let (most_used, _) = ext_counts.into_iter().max_by_key(|(_, c)| *c).unwrap();
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
        let name_a = a.strip_prefix(folder).unwrap_or(a).to_string_lossy();
        let name_b = b.strip_prefix(folder).unwrap_or(b).to_string_lossy();
        compare(&name_a, &name_b)
    });

    let total = files.len();
    println!("Processing {} file(s) — up to {} at a time", total, jobs);
    info!("Found {} files to process (concurrency: {})", total, jobs);

    let sem = Arc::new(Semaphore::new(jobs));
    let opts = Arc::new(opts.clone());
    let ai_config = Arc::new(ai_config);
    let folder = Arc::new(folder.to_path_buf());

    // Pre-sized results vec; each slot filled by the task for that index.
    let mut results: Vec<Option<crate::docx_gen::PackItem>> = (0..total).map(|_| None).collect();
    let mut set: JoinSet<(usize, Result<crate::docx_gen::PackItem>)> = JoinSet::new();

    for (idx, path) in files.into_iter().enumerate() {
        let sem = Arc::clone(&sem);
        let opts = Arc::clone(&opts);
        let ai_config = Arc::clone(&ai_config);
        let folder = Arc::clone(&folder);

        set.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let result = process_file(idx, total, &path, &folder, &opts, ai_config.as_ref().as_ref()).await;
            (idx, result)
        });
    }

    while let Some(join_res) = set.join_next().await {
        match join_res {
            Ok((idx, Ok(item))) => { results[idx] = Some(item); }
            Ok((idx, Err(e))) => { tracing::warn!("File index {} failed: {}", idx, e); }
            Err(e) => { tracing::warn!("Task panicked: {}", e); }
        }
    }

    let pack_items: Vec<crate::docx_gen::PackItem> = results.into_iter().flatten().collect();

    let out_docx = resolved_output.with_extension("docx");
    println!("All {} file(s) processed — writing {}…", total, out_docx.display());
    crate::docx_gen::generate_docx(&out_docx, &resolved_title, doc_text, &pack_items, page_break)?;
    info!("Wrote {}", out_docx.display());
    println!("Done → {}", out_docx.display());

    Ok(())
}

async fn process_file(
    idx: usize,
    total: usize,
    path: &Path,
    folder: &Path,
    opts: &crate::term_gen::TermGenOptions,
    ai_config: Option<&crate::ai::AiConfig>,
) -> Result<crate::docx_gen::PackItem> {
    let filename = path.strip_prefix(folder).unwrap_or(path).to_string_lossy().to_string();
    info!("[{}/{}] Starting: {}", idx + 1, total, filename);

    let content = fs::read_to_string(path).context("Failed to read file")?;

    tracing::debug!("[{}] Requesting AI input", filename);
    let input_text = match crate::ai::generate_input(&[(&filename, &content)], ai_config).await {
        Ok(t) => {
            tracing::debug!("[{}] AI input ready", filename);
            t
        }
        Err(e) => {
            tracing::warn!("[{}] AI input failed: {}", filename, e);
            String::new()
        }
    };

    tracing::debug!("[{}] Running via PTY", filename);
    let t0 = std::time::Instant::now();
    let run_result = crate::runner::run_file(path, if input_text.is_empty() { None } else { Some(&input_text) }).await;
    let (_cmd, out) = match run_result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[{}] Run failed: {}", filename, e);
            (filename.clone(), format!("[Error running file: {}]", e))
        }
    };

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

    info!("[{}/{}] Done: {} ({:.1}s)", idx + 1, total, filename, t0.elapsed().as_secs_f32());

    Ok(crate::docx_gen::PackItem { filename, content, img_bytes, img_width, img_height })
}
