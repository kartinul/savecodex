use anyhow::{Context, Result, bail};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::timeout;
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering};
use directories::ProjectDirs;

#[derive(Deserialize, Debug, Clone)]
pub struct LangConfig {
    pub build: Option<Vec<String>>,
    pub run: Vec<String>,
    #[serde(default)]
    pub stdin: Option<Vec<String>>,
    /// Interactive client: code is typed into `run` line by line instead of saved as a file.
    #[serde(default)]
    pub session: bool,
    /// Prompt suffixes the session waits for before typing the next line.
    #[serde(default)]
    pub prompts: Vec<String>,
    /// Command run (no PTY) before a session group starts, e.g. to recreate a scratch database.
    #[serde(default)]
    pub reset: Option<Vec<String>>,
    /// `run` turns `{file}` into `{dir}/{file_stem}.png`.
    #[serde(default)]
    pub image: bool,
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

/// Unknown languages or configs without `stdin` markers count as needing input.
pub fn needs_stdin(ext: &str, code: &str) -> bool {
    load_configurations().ok()
        .and_then(|c| c.get(ext)?.stdin.clone())
        .map_or(true, |markers| markers.iter().any(|m| code.contains(m.as_str())))
}

fn bwrap_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let ok = cfg!(target_os = "linux")
            && std::process::Command::new("bwrap")
                .args(["--unshare-all", "--share-net", "--ro-bind", "/", "/", "true"])
                .output()
                .is_ok_and(|o| o.status.success());
        if !ok { tracing::warn!("bwrap not available, running code unsandboxed"); }
        ok
    })
}

fn rustup_home() -> Option<String> {
    std::env::var("RUSTUP_HOME").ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.rustup")))
        .filter(|p| Path::new(p).is_dir())
}

#[cfg(unix)]
fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

#[cfg(not(unix))]
fn is_root() -> bool { false }

/// Owner of the job dir when the server runs as root; user code then runs as that uid.
#[cfg(unix)]
fn job_uid(work_dir: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    if !is_root() { return None; }
    std::fs::metadata(work_dir).ok().map(|m| m.uid()).filter(|&uid| uid != 0)
}

#[cfg(not(unix))]
fn job_uid(_: &Path) -> Option<u32> { None }

/// When running as root, give `dir` (recursively) to a fresh unused uid with mode 0700,
/// so concurrent jobs can't read each other's files or the server's /proc/*/environ.
#[cfg(unix)]
pub fn isolate_job_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    static NEXT_UID: AtomicU32 = AtomicU32::new(100_000);
    if !is_root() { return Ok(()); }
    let uid = NEXT_UID.fetch_add(1, Ordering::Relaxed);
    fn chown_all(path: &Path, uid: u32) -> std::io::Result<()> {
        std::os::unix::fs::lchown(path, Some(uid), Some(uid))?;
        if path.is_dir() && !path.is_symlink() {
            for entry in std::fs::read_dir(path)? { chown_all(&entry?.path(), uid)?; }
        }
        Ok(())
    }
    chown_all(dir, uid)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub fn isolate_job_dir(_: &Path) -> Result<()> { Ok(()) }

/// Linux only: drop to the job uid (setpriv), then bwrap if usable, then rlimits.
fn sandboxed(args: &[String], work_dir: &Path) -> Vec<String> {
    if !cfg!(target_os = "linux") { return args.to_vec(); }
    let mut argv = Vec::new();
    if let Some(uid) = job_uid(work_dir) {
        let id = uid.to_string();
        argv.extend(["setpriv", "--reuid", &id, "--regid", &id, "--clear-groups", "--no-new-privs"].map(String::from));
    }
    if bwrap_available() { argv.extend(bwrap_args(work_dir)); }
    argv.extend(["prlimit", "--nproc=128", "--cpu=10", "--fsize=64000000", "--as=8000000000", "--"].map(String::from));
    argv.extend_from_slice(args);
    argv
}

/// Read-only system dirs, no home dir, private /tmp and pid namespace.
fn bwrap_args(work_dir: &Path) -> Vec<String> {
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
    argv
}

/// Allowlisted env for child processes, so server secrets never reach user code.
fn sandbox_env(work_dir: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["PATH", "LANG", "TERM", "JAVA_HOME", "RUSTUP_HOME", "CARGO_HOME", "HOME"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|val| (key.to_string(), val)))
        .collect();
    if bwrap_available() {
        env.retain(|(key, _)| key != "HOME" && key != "RUSTUP_HOME");
        env.push(("HOME".into(), "/tmp".into()));
        if let Some(rustup) = rustup_home() { env.push(("RUSTUP_HOME".into(), rustup)); }
    } else if job_uid(work_dir).is_some() {
        // shared /tmp as HOME would let one job plant ~/.local python packages for another
        env.retain(|(key, _)| key != "HOME");
        env.push(("HOME".into(), work_dir.to_string_lossy().into()));
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
            .envs(sandbox_env(&work_dir))
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
    let output = Arc::new(Mutex::new(String::new()));
    let (idle_tx, idle_rx) = mpsc::channel();
    
    let run_argv = sandboxed(&run_args, &work_dir);
    let mut cmd = CommandBuilder::new(&run_argv[0]);
    cmd.args(&run_argv[1..]);
    cmd.cwd(&work_dir);
    cmd.env_clear();
    for (key, val) in sandbox_env(&work_dir) { cmd.env(key, val); }
    
    let child = Arc::new(Mutex::new(pair.slave.spawn_command(cmd)?));
    let child_clone = Arc::clone(&child);
    drop(pair.slave); 
    
    let reader_output = Arc::clone(&output);
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            reader_output.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
            let _ = idle_tx.send(()); // Signal output activity
        }
        let _ = tx.send(());
    });
    // Some sandboxes (e.g. Render) never deliver PTY EOF after a kill, so don't wait on the reader forever.
    let collect = move || {
        let _ = rx.recv_timeout(Duration::from_secs(1));
        output.lock().unwrap().clone()
    };
    
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
            let partial = collect();
            return Ok((executed_cmd, format!("{}\n\n[Timeout after 5 seconds]", partial)));
        }
    }
    
    tracing::debug!("Child process exited successfully");
    
    let out = collect();
    let out = out.replace("^D\x08\x08", "").replace("^D", "");
    
    Ok((executed_cmd, out))
}

fn strip_escapes(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            chars.by_ref().find(|c| c.is_ascii_alphabetic());
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// Substitute `{n}` (the solve group number) in a session/reset command.
pub fn with_group(args: &[String], n: usize) -> Vec<String> {
    args.iter().map(|a| a.replace("{n}", &n.to_string())).collect()
}

/// Run `argv` from `work_dir` with `input` on stdin and no PTY; used for resets, setup and diagrams.
pub async fn run_piped(argv: &[String], input: &str, work_dir: &Path) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(work_dir)
        .env_clear()
        .envs(sandbox_env(work_dir))
        .env("HOME", work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start {}", argv[0]))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    stdin.write_all(input.as_bytes()).await?;
    drop(stdin);
    let out = timeout(Duration::from_secs(60), child.wait_with_output()).await.context("timed out after 60 seconds")??;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() { bail!("{}", text.trim()); }
    Ok(text)
}

/// Run an `image` language's command on `file` and return the PNG it writes to `{dir}/{file_stem}.png`.
pub async fn render_image(config: &LangConfig, file: &Path, work_dir: &Path) -> Result<Vec<u8>> {
    run_piped(&replace_vars(&config.run, file, work_dir), "", work_dir).await?;
    std::fs::read(file.with_extension("png")).context("renderer wrote no PNG")
}

/// Type each chunk into one interactive session (e.g. `mysql`, `bash -i`) in a PTY and
/// return each chunk's slice of the transcript, starting at the prompt it was typed at.
pub fn run_session(argv: &[String], prompts: &[String], chunks: &[String], work_dir: &Path) -> Result<Vec<String>> {
    let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })?;
    let mut reader = pair.master.try_clone_reader()?;
    let mut cmd = CommandBuilder::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.cwd(work_dir);
    cmd.env_clear();
    for (key, val) in sandbox_env(work_dir) { cmd.env(key, val); }
    // solve sessions run on the host: keep `~/...` inside the job dir
    cmd.env("HOME", work_dir);
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    let output = Arc::new(Mutex::new(String::new()));
    let reader_output = Arc::clone(&output);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            reader_output.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    let len = || output.lock().unwrap().len();
    let at_prompt = || {
        let out = output.lock().unwrap();
        prompts.iter().any(|p| out.ends_with(p.as_str()))
    };
    // Wait for new output that ends in a prompt; programs reading input (no prompt) fall back to 2s of silence.
    let wait = |since: usize| {
        let (start, mut last_len, mut last_change) = (std::time::Instant::now(), since, std::time::Instant::now());
        while start.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(50));
            let now = len();
            if now != last_len { last_len = now; last_change = std::time::Instant::now(); }
            if now > since && at_prompt() { return; }
            if last_change.elapsed() > Duration::from_secs(2) { return; }
        }
    };

    let mut writer = pair.master.take_writer()?;
    wait(0);
    let mut starts = Vec::new();
    for chunk in chunks {
        let out = output.lock().unwrap().clone();
        starts.push(out.rfind('\n').map_or(0, |i| i + 1));
        for line in chunk.lines().filter(|l| !l.trim().is_empty()) {
            let before = len();
            let _ = writer.write_all(format!("{line}\n").as_bytes());
            wait(before);
        }
    }
    let end = len();
    let _ = writer.write_all(&[0x04]);
    let exited = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        matches!(child.try_wait(), Ok(Some(_)))
    });
    if !exited { let _ = child.kill(); }
    drop(pair.master);

    let out = output.lock().unwrap().clone();
    let ends = starts.iter().skip(1).copied().chain([end]);
    Ok(starts.iter().zip(ends).map(|(&a, b)| {
        let part = out[a..b.max(a)].trim_end();
        // drop the dangling prompt the next command would be typed at
        let last = part.rsplit('\n').next().unwrap_or("");
        let part = if prompts.iter().any(|p| strip_escapes(last).trim() == p.trim()) { &part[..part.len() - last.len()] } else { part };
        part.trim_end().to_string()
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::{needs_stdin, run_session};

    #[test]
    fn detects_stdin() {
        assert!(needs_stdin("py", "n = int(input())"));
        assert!(!needs_stdin("py", "print('hi')"));
        assert!(needs_stdin("zzz", "anything"));
    }

    #[test]
    fn session_splits_per_chunk() {
        let argv: Vec<String> = ["env", "PS1=$ ", "HISTFILE=/dev/null", "BASH_SILENCE_DEPRECATION_WARNING=1", "bash", "--norc", "--noprofile", "-i"]
            .map(String::from).to_vec();
        let chunks = ["x=hi\nsleep 0.5; echo $x".to_string(), "echo bye".to_string()];
        let outs = run_session(&argv, &["$ ".to_string()], &chunks, &std::env::temp_dir()).unwrap();
        assert_eq!(outs.len(), 2);
        assert!(outs[0].contains("$ x=hi") && outs[0].trim_end().ends_with("hi"), "{:?}", outs[0]);
        assert!(outs[1].contains("$ echo bye") && outs[1].trim_end().ends_with("bye"), "{:?}", outs[1]);
    }
}
