//! `--linux`: run code in a throwaway Linux container (Apple `container` on macOS, else Docker).
//! Only code runs inside; AI calls and screenshots stay on the host.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

const DOCKERFILE: &str = include_str!("linux.Dockerfile");

static RUNTIME: OnceLock<&'static str> = OnceLock::new();
static IMAGE: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
/// Language binaries found in the image, so only those are offered to the AI.
static BINS: OnceLock<HashSet<String>> = OnceLock::new();
/// Mounted host dir → container id; commands run in the container whose mount holds their work dir.
static MOUNTS: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    RUNTIME.get().is_some()
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
}

fn ok(rt: &str, args: &[&str]) -> bool {
    Command::new(rt).args(args).output().is_ok_and(|o| o.status.success())
}

/// Pick the runtime and make sure it's running. Never installs anything.
pub fn enable() -> Result<()> {
    let candidates: &[&'static str] = if cfg!(target_os = "macos") { &["container", "docker"] } else { &["docker"] };
    let Some(&rt) = candidates.iter().find(|b| on_path(b)) else {
        bail!("--linux needs {}; install it first", if cfg!(target_os = "macos") { "Apple container (or Docker)" } else { "Docker" });
    };
    if rt == "container" && !ok(rt, &["system", "status"]) && !ok(rt, &["system", "start", "--disable-kernel-install"]) {
        bail!("Apple container isn't running and couldn't start; run `container system start` once (it installs a Linux kernel)");
    }
    if rt == "docker" && !ok(rt, &["info"]) {
        bail!("Docker isn't running; start it and try again");
    }
    let _ = RUNTIME.set(rt);
    Ok(())
}

/// Build the image on first use; its tag follows the Dockerfile, so editing it rebuilds.
async fn image() -> Result<String> {
    IMAGE.get_or_try_init(|| async {
        let rt = RUNTIME.get().context("--linux not enabled")?;
        let tag = format!("savecodex-linux:{:x}", DOCKERFILE.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)));
        if ok(rt, &["image", "inspect", &tag]) { return Ok(tag) }
        println!("🐧 Building the Linux image (first run only, a few minutes)…");
        let ctx = tempfile::tempdir()?;
        let file = ctx.path().join("Dockerfile");
        std::fs::write(&file, DOCKERFILE)?;
        let built = tokio::process::Command::new(rt)
            .args(["build", "-t", &tag, "-f"]).arg(&file).arg(ctx.path())
            .output().await?;
        if *rt == "container" { let _ = Command::new(rt).args(["builder", "stop"]).output(); }
        if !built.status.success() {
            bail!("building the Linux image failed:\n{}", String::from_utf8_lossy(&built.stderr).trim());
        }
        Ok(tag)
    }).await.cloned()
}

/// A running container with `dir` mounted at the same path; removed on drop.
pub struct Container {
    id: String,
}

impl Container {
    /// `None` without `--linux`.
    pub async fn start(dir: &Path) -> Result<Option<Container>> {
        let Some(rt) = RUNTIME.get() else { return Ok(None) };
        let image = image().await?;
        let dir = std::fs::canonicalize(dir)?;
        let mount = format!("{0}:{0}", dir.display());
        let out = tokio::process::Command::new(rt)
            .args(["run", "-d", "--rm", "-v", &mount, &image])
            .output().await?;
        if !out.status.success() {
            bail!("starting the Linux container failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let container = Container { id: id.clone() };
        let ready = tokio::process::Command::new(rt)
            .args(["exec", &id, "sh", "-c", "for i in $(seq 300); do [ -e /tmp/ready ] && exit 0; sleep 0.1; done; exit 1"])
            .output().await?;
        if !ready.status.success() { bail!("the Linux container didn't start") }
        if BINS.get().is_none() {
            let bins: Vec<String> = crate::runner::load_configurations()?.values().filter_map(|c| c.bin().map(str::to_string)).collect();
            let found = tokio::process::Command::new(rt)
                .args(["exec", &id, "sh", "-c", "for b; do command -v \"$b\" >/dev/null && echo \"$b\"; done", "sh"]).args(&bins)
                .output().await?;
            let _ = BINS.set(String::from_utf8_lossy(&found.stdout).lines().map(str::to_string).collect());
        }
        MOUNTS.lock().unwrap().push((dir, id));
        Ok(Some(container))
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        MOUNTS.lock().unwrap().retain(|(_, id)| *id != self.id);
        if let Some(rt) = RUNTIME.get() {
            let _ = Command::new(rt).args(["rm", "-f", &self.id]).output();
        }
    }
}

/// Whether `bin` exists in the image (true until the first container has been probed).
pub fn has(bin: &str) -> bool {
    BINS.get().is_none_or(|bins| bins.contains(bin))
}

/// `args` as a command run inside the container that mounts `work_dir`, or `None` if there is none.
pub fn wrap(args: &[String], work_dir: &Path, tty: bool) -> Option<Vec<String>> {
    let rt = RUNTIME.get()?;
    let dir = std::fs::canonicalize(work_dir).ok()?;
    let id = MOUNTS.lock().unwrap().iter().find(|(mount, _)| dir.starts_with(mount))?.1.clone();
    let dir = dir.to_string_lossy().to_string();
    let mut argv: Vec<String> = vec![rt.to_string(), "exec".into()];
    if tty { argv.extend(["-i".into(), "-t".into()]) } else { argv.push("-i".into()) }
    argv.extend(["-w", &dir, "-e", &format!("HOME={dir}"), "-e", "TERM=xterm", "-e", "LANG=C.UTF-8", &id].map(String::from));
    argv.extend_from_slice(args);
    Some(argv)
}
