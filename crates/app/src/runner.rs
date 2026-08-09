use anyhow::{Context, Result, bail};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use directories::ProjectDirs;

#[derive(Deserialize, Debug, Clone)]
struct LangConfig {
    build: Option<Vec<String>>,
    run: Vec<String>,
    #[serde(default)]
    windows: Option<OsSpecificConfig>,
    #[serde(default)]
    macos: Option<OsSpecificConfig>,
    #[serde(default)]
    linux: Option<OsSpecificConfig>,
}

#[derive(Deserialize, Debug, Clone)]
struct OsSpecificConfig {
    build: Option<Vec<String>>,
    run: Option<Vec<String>>,
}

impl LangConfig {
    fn resolve_os(mut self) -> Self {
        let os_config = if cfg!(windows) {
            self.windows.take()
        } else if cfg!(target_os = "macos") {
            self.macos.take()
        } else {
            self.linux.take()
        };
        
        if let Some(spec) = os_config {
            if spec.build.is_some() { self.build = spec.build; }
            if let Some(run) = spec.run { self.run = run; }
        }
        self
    }
}

fn replace_vars(args: &[String], file: &Path) -> Vec<String> {
    let file_str = file.to_string_lossy().to_string();
    let dir_str = file.parent().unwrap_or(Path::new("")).to_string_lossy().to_string();
    let dir_str = if dir_str.is_empty() { ".".to_string() } else { dir_str };
    let stem_str = file.file_stem().unwrap_or_default().to_string_lossy().to_string();
    
    args.iter().map(|arg| {
        arg.replace("{file}", &file_str)
           .replace("{dir}", &dir_str)
           .replace("{file_stem}", &stem_str)
    }).collect()
}

fn load_configurations() -> Result<HashMap<String, LangConfig>> {
    // 1. Embedded Defaults
    let embedded_str = include_str!("default_languages.json");
    let mut configs: HashMap<String, LangConfig> = serde_json::from_str(embedded_str)
        .context("Failed to parse embedded default_languages.json")?;

    // 2. Global Overrides
    if let Some(proj_dirs) = ProjectDirs::from("", "", "savecodex") {
        let global_config = proj_dirs.config_dir().join("languages.json");
        if global_config.exists() {
            if let Ok(content) = std::fs::read_to_string(&global_config) {
                if let Ok(global_configs) = serde_json::from_str::<HashMap<String, LangConfig>>(&content) {
                    configs.extend(global_configs);
                }
            }
        }
    }

    // 3. Local Overrides
    let local_config = Path::new("languages.json");
    if local_config.exists() {
        if let Ok(content) = std::fs::read_to_string(local_config) {
            if let Ok(local_configs) = serde_json::from_str::<HashMap<String, LangConfig>>(&content) {
                configs.extend(local_configs);
            }
        }
    }

    // 4. Resolve OS overrides
    let configs = configs.into_iter().map(|(k, v)| (k, v.resolve_os())).collect();

    Ok(configs)
}

pub async fn run_file(file: &Path, input_text: Option<&str>) -> Result<(String, String)> {
    let ext = file.extension().and_then(|s| s.to_str()).context("File has no extension")?;
    let configs = load_configurations()?;
    let config = configs.get(ext).with_context(|| format!("No configuration for extension: {}", ext))?;
    
    // 1. Build
    if let Some(build_args) = &config.build {
        let args = replace_vars(build_args, file);
        if args.is_empty() { bail!("Empty build command"); }
        
        let child = std::process::Command::new(&args[0])
            .args(&args[1..])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
            
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let executed_cmd = args.join(" ");
            return Ok((executed_cmd, stderr.to_string()));
        }
    }
    
    // 2. Run with PTY
    let run_args = replace_vars(&config.run, file);
    if run_args.is_empty() { bail!("Empty run command"); }
    
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    
    // Read thread setup (clone before spawn to prevent os error 5 on macOS if child exits fast)
    let mut reader = pair.master.try_clone_reader()?;
    let (tx, rx) = mpsc::channel();
    
    let mut cmd = CommandBuilder::new(&run_args[0]);
    cmd.args(&run_args[1..]);
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    
    let child = Arc::new(Mutex::new(pair.slave.spawn_command(cmd)?));
    let child_clone = Arc::clone(&child);
    
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut output = String::new();
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            output.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        let _ = tx.send(output);
    });
    
    // Keep the writer alive so we don't accidentally send EOF to the program
    let mut _kept_writer = None;
    
    // Write input
    if let Some(inp) = input_text {
        std::thread::sleep(Duration::from_millis(50)); // Allow process to launch
        
        let processed_inp = inp.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t");
        
        // We ignore EIO (os error 5) when taking writer or writing, 
        // as it happens if the process exits instantly before we can write.
        if let Ok(mut writer) = pair.master.take_writer() {
            let _ = writer.write_all(processed_inp.as_bytes());
            _kept_writer = Some(writer);
        }
    }
    
    // Wait for process with 10s timeout
    let wait_res = timeout(Duration::from_secs(10), tokio::task::spawn_blocking(move || {
        loop {
            let mut c = child_clone.lock().unwrap();
            match c.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {},
                Err(e) => return Err(e),
            }
            drop(c);
            std::thread::sleep(Duration::from_millis(100));
        }
    })).await;
    
    match wait_res {
        Ok(Ok(Ok(_status))) => {
            drop(pair.master);
        },
        Ok(Ok(Err(e))) => {
            drop(pair.master);
            bail!("Execution failed: {}", e);
        },
        Ok(Err(e)) => {
            drop(pair.master);
            bail!("Join error: {}", e);
        },
        Err(_) => {
            let _ = child.lock().unwrap().kill(); // Kill the runaway process!
            drop(pair.master);
            let partial = rx.recv().unwrap_or_default();
            let executed_cmd = run_args.join(" ");
            return Ok((executed_cmd, format!("{}\n\n[Timeout after 10 seconds]", partial)));
        }
    }
    
    let out = rx.recv().unwrap_or_default();
    // Strip literal ^D (often echoed by the terminal driver on EOF) and its trailing backspaces
    let out = out.replace("^D\x08\x08", "").replace("^D", "");
    
    let executed_cmd = run_args.join(" ");
    Ok((executed_cmd, out))
}
