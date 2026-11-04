# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

savecodex takes a folder of code files, asks an LLM for stdin for each one, runs each in a PTY, renders the output as a fake terminal screenshot and packs code and screenshot into a DOCX. It ships as a single Rust binary (CLI + HTTP server) with the React frontend embedded.

Use ponytail. prefer writing clean short minimal code without excessive comments.
comments to be used only on complex code 
and documentation only when writing cli.rs 

## Rule: no building until BUILD
Do not implement features or larger changes until the user says `BUILD`. Until then: investigate, plan, propose. Small tweaks the user explicitly asks for can be done right away.

## Commands

@Makefile

## Architecture

Single crate `crates/app`:

- `cli.rs`: clap commands (`pack`, `solve`, `run`, `term`, `serve`). Screenshot flags (`ShotArgs`) and DOCX flags (`DocArgs`) are shared via `#[command(flatten)]`. Many flags default from `SAVECODEX_*` env vars; `main.rs` loads `.env` via dotenvy.
- `pack.rs` `run_pack`: walks the folder (gitignore-aware, depth 1 unless recursive), auto-detects the most common supported extension if none given, processes files concurrently (semaphore of `jobs`), keeps natural sort order, then calls `docx_gen::generate_docx`. Per file: `ai::generate_input` → `runner::run_file_in` → truncate output → `term_gen::generate_terminal_image` → `PackItem`. AI or run failures don't abort the pack; they become empty input / an error string in the screenshot.
- `runner.rs`: language commands come from `default_languages.json` (embedded), overridden by `<config_dir>/savecodex/languages.json` then `./languages.json`. Each entry has `build`/`run` argv with `{file}`, `{dir}`, `{file_stem}` placeholders and optional `windows`/`macos`/`linux` overrides. Commands run in a PTY from the packed folder.
  - `--linux` (`linux.rs`): one throwaway container per document (Apple `container` on macOS, else Docker) with the work dir mounted at the same path; `runner::prepare` routes every command through `exec` into it. The image is `linux.Dockerfile`, embedded and built on first use. Only code runs inside; AI stays on the host.
  - Sandboxing (Linux only): `setpriv` to a per-job uid when the server runs as root (`isolate_job_dir` chowns the job temp dir to a fresh uid ≥100000), then `bwrap` if usable, then `prlimit`. Child env is an allowlist (`sandbox_env`) so server secrets never reach user code. `scripts/sandbox-firewall.sh` blocks LAN/metadata egress for the server user.
- `solve.rs` `run_solve`: reads PDF (pdftotext + pdftoppm page images) / DOCX / images, one SMART call extracts grouped questions, one MID call per group solves them (several PDF/DOCX inputs are solved in parallel in `cli.rs`, sharing one `jobs` semaphore), then each answer runs by language kind: file (like pack), session (`runner::run_session` types code into `mysql`/`bash -i`, waiting for configured prompts; consecutive answers share one session; `{ns}` = session namespace prefix, unique across runs and documents: `runner::namespaced` prefixes names after the language's `namespace` keywords while typing and `unnamespaced` strips them from the transcript; `reset`/`cleanup` run around the session), image (`dot` → PNG), or none (written answer). First answer of a question keeps the title bar (`TermGenOptions::no_title_bar`).
- Prompts stay generic: language-specific advice goes in that language's `hint` in `default_languages.json`. Headings come from `assign_labels` (one style per document), not from the model.
- `ai.rs`: `ask(tier, ..)` with tiers SMART/MID/FAST. `resolve` reads `{TIER}_PROVIDER/_MODEL/_API_KEY/_BASE_URL` (CLI flags via `set_overrides`), unset tiers fall back upwards, then auto-detects agent CLIs on PATH (`AGENTS`), then `GEMINI_API_KEY`. Providers: `gemini` (comma-separated models fall back in order), `openai`, and agents run in print mode in an empty temp dir (`run_agent`); a failed auto-detected one is skipped for the rest of the run. Prompts live in `prompts.rs`.
- `server.rs` / `routes.rs`: axum. `/api/pack` takes multipart uploads (field name = `webkitRelativePath`; non-`Normal` path components are rejected), writes them to a temp dir, runs `run_pack` and returns the DOCX. Auth: the user's own Gemini key (`AiConfig::gemini`), or `admin_password` matching `ADMIN_PASSWORD` / `--no-key` (`SAVECODEX_NO_KEY`) sets `ai_config` to `None` so the server's own FAST config is used. Nothing else from the browser picks the provider. Other routes: `/api/config`, `/api/verify_admin`; everything else serves the embedded SPA.
- `term_gen.rs`: draws the terminal window (windows/macos/ubuntu styles, dark/light, ANSI colors, "messy" imperfections) with bundled fonts from `crates/app/assets/`.
- `scripts/readdocx.py [-p] [-w N] *.docx`: dumps solved docx files as text with local tesseract OCR of the screenshots; `-p` prints only flagged/empty ones. Use it to check `solve` output cheaply.
- `frontend/src/App.tsx`: the whole UI in one file.

Deployment: `Dockerfile` (Debian with common compilers) / `render.yaml`. Releases: `.github/workflows/release.yml` builds Linux/Windows/macOS binaries on `v*.*.*` tags.
