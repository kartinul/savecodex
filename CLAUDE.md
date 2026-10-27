# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

savecodex takes a folder of code files, asks an LLM for stdin for each one, runs each in a PTY, renders the output as a fake terminal screenshot and packs code and screenshot into a DOCX. It ships as a single Rust binary (CLI + HTTP server) with the React frontend embedded.

Use ponytail. prefer writing clean short minimal code without excessive comments.
comments to be used only on complex code 
and documentation only when writing cli.rs 

## Commands

@Makefile

## Architecture

Single crate `crates/app`:

- `cli.rs`: clap commands (`pack`, `run`, `term`, `serve`, `solve` stub). Many flags default from `SAVECODEX_*` env vars; `main.rs` loads `.env` via dotenvy.
- `pack.rs` `run_pack`: walks the folder (gitignore-aware, depth 1 unless recursive), auto-detects the most common supported extension if none given, processes files concurrently (semaphore of `jobs`), keeps natural sort order, then calls `docx_gen::generate_docx`. Per file: `ai::generate_input` → `runner::run_file_in` → truncate output → `term_gen::generate_terminal_image` → `PackItem`. AI or run failures don't abort the pack; they become empty input / an error string in the screenshot.
- `runner.rs`: language commands come from `default_languages.json` (embedded), overridden by `<config_dir>/savecodex/languages.json` then `./languages.json`. Each entry has `build`/`run` argv with `{file}`, `{dir}`, `{file_stem}` placeholders and optional `windows`/`macos`/`linux` overrides. Commands run in a PTY from the packed folder.
  - Sandboxing (Linux only): `setpriv` to a per-job uid when the server runs as root (`isolate_job_dir` chowns the job temp dir to a fresh uid ≥100000), then `bwrap` if usable, then `prlimit`. Child env is an allowlist (`sandbox_env`) so server secrets never reach user code. `scripts/sandbox-firewall.sh` blocks LAN/metadata egress for the server user.
- `ai.rs`: `AiConfig::from_env(prefix)` reads `{CODE,INPUT}_PROVIDER/_MODEL/_API_KEY/_BASE_URL`; providers are `gemini` (uses `GEMINI_API_KEY`, comma-separated models fall back in order) and `openai_compatible`. Prompts live in `prompts.rs`.
- `server.rs` / `routes.rs`: axum. `/api/pack` takes multipart uploads (field name = `webkitRelativePath`; non-`Normal` path components are rejected), writes them to a temp dir, runs `run_pack` and returns the DOCX. Auth: the user's own Gemini key, or `admin_password` matching `ADMIN_PASSWORD` swaps in the server's `GEMINI_API_KEY`. With `--no-key` (`SAVECODEX_NO_KEY`), `ai_config` is `None` and the server's env config is used for everyone. Other routes: `/api/config`, `/api/verify_admin`; everything else serves the embedded SPA.
- `term_gen.rs`: draws the terminal window (windows/macos/ubuntu styles, dark/light, ANSI colors, "messy" imperfections) with bundled fonts from `crates/app/assets/`.
- `frontend/src/App.tsx`: the whole UI in one file.

Deployment: `Dockerfile` (Debian with common compilers) / `render.yaml`. Releases: `.github/workflows/release.yml` builds Linux/Windows/macOS binaries on `v*.*.*` tags.
