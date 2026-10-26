use anyhow::{Context, Result, bail};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::timeout;
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use directories::ProjectDirs;

#[derive(Deserialize, Debug, Clone)]
pub struct LangConfig {
    pub build: Option<Vec<String>>,
    pub run: Vec<String>,
    #[serde(default)]
    windows: Option<OsSpecificConfig>,
    #[serde(default)]
    macos: Option<OsSpecificConfig>,
    #[serde(default)]
    linux: Option<OsSpecificConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OsSpecificConfig {
    pub build: Option<Vec<String>>,
    pub run: Option<Vec<String>>,
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

/// Make a path absolute without resolving symlinks (lexical join with cwd).
fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// The directory commands are executed from: the requested working directory when it is a
/// usable directory, otherwise the file's parent, otherwise the current directory.
fn resolve_work_dir(file: &Path, work_dir: Option<&Path>) -> PathBuf {
    let candidate = work_dir
        .map(Path::to_path_buf)
        .or_else(|| file.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let mut candidate = absolutize(&candidate);

    // The pack target may itself be a file (e.g. `savecodex pack main.rs`) — fall back to its parent.
    if !candidate.is_dir() {
        if let Some(parent) = candidate.parent() {
            if parent.is_dir() {
                candidate = parent.to_path_buf();
            }
        }
    }
    if !candidate.is_dir() {
        candidate = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    }
    candidate
}

/// Substitute `{file}`, `{dir}` and `{file_stem}` in a command template.
/// Paths are made relative to `work_dir`, since commands run from there.
fn replace_vars(args: &[String], file: &Path, work_dir: &Path) -> Vec<String> {
    let file_abs = absolutize(file);
    let rel = file_abs.strip_prefix(work_dir).unwrap_or(&file_abs);

    let file_str = rel.to_string_lossy().to_string();
    let dir_str = match rel.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().to_string(),
        _ => ".".to_string(),
    };
    let stem_str = rel.file_stem().unwrap_or_default().to_string_lossy().to_string();

    args.iter().map(|arg| {
        arg.replace("{file}", &file_str)
           .replace("{dir}", &dir_str)
           .replace("{file_stem}", &stem_str)
    }).collect()
}

pub fn load_configurations() -> Result<HashMap<String, LangConfig>> {
    let embedded_str = include_str!("default_languages.json");
    let mut configs: HashMap<String, LangConfig> = serde_json::from_str(embedded_str)
        .context("Failed to parse embedded default_languages.json")?;

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

    let local_config = Path::new("languages.json");
    if local_config.exists() {
        if let Ok(content) = std::fs::read_to_string(local_config) {
            if let Ok(local_configs) = serde_json::from_str::<HashMap<String, LangConfig>>(&content) {
                configs.extend(local_configs);
            }
        }
    }

    let configs = configs.into_iter().map(|(k, v)| (k, v.resolve_os())).collect();

    Ok(configs)
}

fn bwrap_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let ok = cfg!(target_os = "linux")
            && std::process::Command::new("bwrap").arg("--version").output().is_ok_and(|o| o.status.success());
        if !ok { tracing::warn!("bwrap not available, running code unsandboxed"); }
        ok
    })
}

fn rustup_home() -> Option<String> {
    std::env::var("RUSTUP_HOME").ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.rustup")))
        .filter(|p| Path::new(p).is_dir())
}

/// Wrap a command in bwrap: read-only system dirs, no home dir, private /tmp, pid namespace and rlimits.
fn sandboxed(args: &[String], work_dir: &Path) -> Vec<String> {
    if !bwrap_available() { return args.to_vec(); }
    let dir = work_dir.to_string_lossy().to_string();
    let mut argv: Vec<String> = [
        "bwrap", "--unshare-all", "--share-net", "--die-with-parent", "--new-session",
        "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
    ].map(String::from).to_vec();
    for sys_dir in ["/usr", "/bin", "/lib", "/lib64", "/etc", "/run/systemd/resolve"] {
        argv.extend(["--ro-bind-try", sys_dir, sys_dir].map(String::from));
    }
    if let Some(rustup) = rustup_home() {
        argv.extend(["--ro-bind".into(), rustup.clone(), rustup]);
    }
    argv.extend(["--bind", &dir, &dir, "--chdir", &dir].map(String::from));
    argv.extend(["prlimit", "--nproc=128", "--cpu=10", "--fsize=64000000", "--as=8000000000", "--"].map(String::from));
    argv.extend_from_slice(args);
    argv
}

/// Allowlisted env for child processes, so server secrets never reach user code.
fn sandbox_env() -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["PATH", "LANG", "TERM", "JAVA_HOME", "RUSTUP_HOME", "CARGO_HOME", "HOME"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|val| (key.to_string(), val)))
        .collect();
    if bwrap_available() {
        env.retain(|(key, _)| key != "HOME" && key != "RUSTUP_HOME");
        env.push(("HOME".into(), "/tmp".into()));
        if let Some(rustup) = rustup_home() { env.push(("RUSTUP_HOME".into(), rustup)); }
    }
    env
}

/// Run a single file. Commands execute from the file's parent directory.
pub async fn run_file(file: &Path, input_text: Option<&str>) -> Result<(String, String)> {
    run_file_in(file, input_text, None).await
}

/// Run a single file, executing all commands from `work_dir` (the packed folder).
/// Falls back to the file's parent directory when `work_dir` is `None` or unusable.
pub async fn run_file_in(
    file: &Path,
    input_text: Option<&str>,
    work_dir: Option<&Path>,
) -> Result<(String, String)> {
    tracing::debug!("run_file_in called for: {:?} (work_dir: {:?})", file, work_dir);
    tracing::trace!("input_text provided: {:?}", input_text);
    
    let ext = file.extension().and_then(|s| s.to_str()).context("File has no extension")?;
    let configs = load_configurations()?;
    let config = configs.get(ext).with_context(|| format!("No configuration for extension: {}", ext))?;
    tracing::debug!("Loaded config for extension {}: {:?}", ext, config);
    
    let work_dir = resolve_work_dir(file, work_dir);
    tracing::debug!("Executing from work_dir: {:?}", work_dir);
    
    let build_args = config.build.as_ref().map(|b| replace_vars(b, file, &work_dir));
    if let Some(args) = &build_args {
        if args.is_empty() { bail!("Empty build command"); }
        tracing::debug!("Running build command in {:?}: {:?}", work_dir, args);
        
        let argv = sandboxed(args, &work_dir);
        let mut build = tokio::process::Command::new(&argv[0]);
        build.args(&argv[1..])
            .current_dir(&work_dir)
            .env_clear()
            .envs(sandbox_env())
            .kill_on_drop(true);

        let output = match timeout(Duration::from_secs(30), build.output()).await {
            Ok(output) => output?,
            Err(_) => return Ok((args.join(" "), "[Build timeout after 30 seconds]".into())),
        };
        if !output.status.success() {
            let mut err_out = String::from_utf8_lossy(&output.stdout).to_string();
            err_out.push_str(&String::from_utf8_lossy(&output.stderr));
            let executed_cmd = args.join(" ");
            return Ok((executed_cmd, err_out));
        }
    }
    
    let run_args = replace_vars(&config.run, file, &work_dir);
    if run_args.is_empty() { bail!("Empty run command"); }
    let executed_cmd = match &build_args {
        Some(build) => {
            let build: Vec<&str> = build.iter().map(|a| a.strip_prefix("./").unwrap_or(a)).collect();
            format!("{} && {}", build.join(" "), run_args.join(" "))
        }
        None => run_args.join(" "),
    };
    tracing::debug!("Running execution command in PTY (cwd {:?}): {:?}", work_dir, run_args);
    
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
    let (idle_tx, idle_rx) = mpsc::channel();
    
    let run_argv = sandboxed(&run_args, &work_dir);
    let mut cmd = CommandBuilder::new(&run_argv[0]);
    cmd.args(&run_argv[1..]);
    cmd.cwd(&work_dir);
    cmd.env_clear();
    for (key, val) in sandbox_env() { cmd.env(key, val); }
    
    let child = Arc::new(Mutex::new(pair.slave.spawn_command(cmd)?));
    let child_clone = Arc::clone(&child);
    drop(pair.slave); 
    
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut output = String::new();
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            output.push_str(&String::from_utf8_lossy(&buf[..n]));
            let _ = idle_tx.send(()); // Signal output activity
        }
        let _ = tx.send(output);
    });
    
    tracing::debug!("Spawned child process and output reader thread");

    if let Some(inp) = input_text {
        tracing::debug!("Starting to send input text to PTY process");
        std::thread::sleep(Duration::from_millis(50)); // Allow process to launch
        
        let processed_inp = inp.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t");
        let lines: Vec<&str> = processed_inp.lines().collect();
        
        if let Ok(mut writer) = pair.master.take_writer() {
            // Wait for at least one byte of output before sending anything,
            // so the first prompt is visible before the first input arrives.
            // Without this, idle_rx is empty at startup and recv_timeout fires
            // immediately, sending input before the first cout prompt is printed.
            let _ = idle_rx.recv_timeout(Duration::from_secs(2));
            
            for line in lines {
                loop {
                    match idle_rx.recv_timeout(Duration::from_millis(150)) {
                        Ok(_) => continue, // Activity detected, reset timer
                        Err(_) => break,   // Timeout (idle) or disconnected
                    }
                }
                
                let mut l = line.to_string();
                l.push('\n');
                let _ = writer.write_all(l.as_bytes());
            }
            loop {
                match idle_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            #[cfg(windows)]
            let _ = writer.write_all(&[0x1A]); // Ctrl+Z for Windows EOF
            #[cfg(not(windows))]
            let _ = writer.write_all(&[0x04]); // Ctrl+D for UNNIX EOF
        }
    }
    
    let wait_res = timeout(Duration::from_secs(5), tokio::task::spawn_blocking(move || {
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
    
    tracing::debug!("Child process execution finished, wait_res: {:?}", wait_res.is_ok());

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
            tracing::warn!("Execution timeout! Killing child process.");
            let _ = child.lock().unwrap().kill();
            drop(pair.master); // Close PTY — signals reader thread to flush and exit
            let partial = rx.recv().unwrap_or_default(); // Wait for reader thread to finish
            return Ok((executed_cmd, format!("{}\n\n[Timeout after 5 seconds]", partial)));
        }
    }
    
    tracing::debug!("Child process exited successfully");
    
    let out = rx.recv().unwrap_or_default();
    let out = out.replace("^D\x08\x08", "").replace("^D", "");
    
    Ok((executed_cmd, out))
}
