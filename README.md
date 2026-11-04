# savecodex

savecodex reads the code files in a folder you select, uses AI to generate inputs for them, runs them with those inputs, screenshots the terminal output and packs everything into a DOCX document.
Just select your folder and enter your Gemini API key to begin.

## Demo

- Gemini key variant: <https://youtu.be/-i_L3jEiWxc>
- Admin key variant: <https://youtu.be/8jD1Prdp1cc>

## Installation

Requires Rust and [bun](https://bun.sh). The frontend is embedded into the binary:

```bash
make build   # release binary at ./target/release/savecodex
make dev     # frontend on :5173, backend on :7878, both auto-reload
```

Or use Docker (`Dockerfile` / `render.yaml`).

## Setup

`cp .env.example .env`, fill in your keys (see [Environment Variables](#environment-variables)), then check them with `./apitest`.

## CLI

### `serve`

Starts the HTTP server with the web frontend.

| Option | Default | |
|---|---|---|
| `--port` | `7878` | |
| `--host` | `127.0.0.1` | |
| `--no-key` | off | Everyone uses the server's AI config; no key or admin password needed |
| `--debug` | off | Debug logging |

### `pack`

Runs every source file in the given files/folders, with AI-generated stdin for the ones that read input, and writes one DOCX of code and terminal screenshots per folder.

```bash
savecodex pack lab5 -o "roll_{}" --doc-title "Assignment {}"
```

| Option | Default | |
|---|---|---|
| `<paths...>` | required | Files or folders to pack |
| `-o, --output` | `<folder>_pack` | Output name; `{}` is the folder name (required with several paths) |
| `-r, --recursive` | off | Include subfolders |
| `--ext` | most common supported extension in the folder | Comma-separated extensions, e.g. `java,py` |
| `--doc-title` | folder name | Heading; supports `{}` |
| `--doc-text` | none | Text under the heading; supports `{}` |
| `--page-break` | off | Page break after each file |
| `--ai-input` | `auto` | `auto`: only files with a stdin marker (`input(`, `scanf`, ...); `always`; `never` |
| `--no-trim` | off | Keep long output (default trims to 150 lines × 200 chars; hard cap 1000 × 300) |
| `-j, --jobs` | `15` | Files (`pack`) or question groups across all documents (`solve`) processed in parallel |

Also takes `term`'s screenshot options (`--style`, `--theme`, `--username`, ...) except `--no-prompt`.
Stdin markers are the `stdin` lists in `default_languages.json`; override them in your own `languages.json`.

### `solve`

Not implemented yet. Planned: questions from a PDF/DOCX → AI solutions → DOCX + PDF.

### `term`

Renders a terminal screenshot (PNG) from a run or from text (plain or ANSI).

```bash
savecodex term --runfile main.py -i 5 --style macos
savecodex term --text-file output.txt
```

| Option | Default | |
|---|---|---|
| `--runfile` | none | Source file to run |
| `-i, --input` | none | stdin for `--runfile` |
| `--text-file` | reads stdin | Text to render instead of a run |
| `-o, --output` | `term.png` | |
| `--style` | `windows` | `windows`, `macos` or `ubuntu` |
| `--theme` | `dark` | `dark` or `light` |
| `--username` | current user | |
| `--hostname` | this machine | |
| `--cwd` | current directory | |
| `--no-prompt` | off | Don't draw the prompt line above the text/command |
| `--messy` | `0` | Realistic imperfections, `0` to `3` |
| `--seed` | derived from the text | Fixes `--messy` randomness |
| `--banner` | off | PowerShell startup banner (windows style) |
| `--min-cols` | `80` | Minimum window width in columns |
| `--prompt` | `"$ "` | Lines starting with this get the styled prompt |
| `--no-prompt-highlight` | off | Don't style `--prompt` lines |

### `run`

Runs one source file in a PTY and prints the transcript.

| Option | Default | |
|---|---|---|
| `-i, --input` | none | stdin |
| `-o, --output` | stdout | Transcript file |

## Sandboxing

All code runs through one runner (`pack`, `run`, `term --runfile`, the server):

- **Everywhere:** only allowlisted env vars reach the program, so `.env` secrets stay hidden. Builds time out after 30s, runs after 5s.
- **Linux:** `prlimit` limits CPU, processes, file size and memory; `bwrap` (if installed) adds read-only system dirs, no home and a private `/tmp`.
- **Linux server as root** (Docker/Render): each job runs as its own throwaway uid; `scripts/sandbox-firewall.sh` blocks LAN/metadata access.
- **macOS/Windows:** only the env allowlist and timeouts, so run only code you trust.

## Environment Variables

Read from the shell or the first `.env` in the current directory or a parent. CLI flags win.

| Variable | Default | |
|---|---|---|
| `SMART_PROVIDER` | see below | AI for extracting questions in `solve` (once per run) |
| `MID_PROVIDER` | `SMART_*` | AI for solving each question group |
| `FAST_PROVIDER` | `MID_*` | AI for generating stdin (`pack`, `solve`) |
| `{TIER}_MODEL` | per provider | Comma-separated Gemini models fall back in order |
| `{TIER}_THINKING` | provider default (agents: `low` for FAST) | `off`, `low`, `medium`, `high`, `xhigh`. Sent as `reasoning_effort` (openai, ollama; dropped if rejected), Gemini `thinkingConfig`, `--effort` (claude) or `model_reasoning_effort` (codex); other agents ignore it |
| `{TIER}_API_KEY`, `{TIER}_BASE_URL` | none | `{TIER}_BASE_URL` is needed for `openai`; Gemini falls back to `GEMINI_API_KEY` |
| `GEMINI_API_KEY` | none | Used for Gemini; with no provider set it's the last fallback after agent CLIs |
| `ADMIN_PASSWORD` | none | Lets frontend users use the server's AI config |
| `SAVECODEX_NO_KEY` | off | `serve --no-key` |
| `SAVECODEX_OUTPUT` | `<folder>_pack` | `pack -o` |
| `SAVECODEX_EXT` | auto-detect | `pack --ext` |
| `SAVECODEX_DOC_TITLE` | folder name | `pack --doc-title` |
| `SAVECODEX_DOC_TEXT` | none | `pack --doc-text` |
| `SAVECODEX_STYLE` | `windows` | `pack` / `term --style` |
| `SAVECODEX_AI_INPUT` | `auto` | `pack --ai-input` |
| `SAVECODEX_NO_TRIM` | off | `pack --no-trim` |
| `SAVECODEX_JOBS` | `15` | `pack` / `solve -j` |
| `SAVECODEX_RETRIES` | `3` | `--retries`: retries for a failed AI call (5xx, 429, agent burst limits, replies that don't parse). Retried right away, except agent burst limits, which wait 5s, 10s, 20s, then 40s |
| `SAVECODEX_AGENT_JOBS` | no limit | Extra cap on calls running at once per agent CLI (claude, codex, ...); otherwise `-j` decides |
| `RUST_LOG` | `info` | Log level, e.g. `debug` |

Providers: `gemini`, `openai` (any OpenAI-compatible API), `ollama` (local, `--local` uses it for every tier with the first installed model), and agent CLIs run in print mode: `claude`, `codex`, `gemini-cli`, `cursor`, `opencode`, `copilot`, `qwen`, `amp`, `antigravity` (`agy`). Every `*_PROVIDER` / `*_MODEL` also has a flag (`--smart-provider`, `--fast-model`, ...).

With no provider set, the first agent CLI on `PATH` is used, in that order, then Gemini if `GEMINI_API_KEY` is set. If one fails (usage limit, not logged in, timeout), the next one takes over for the rest of the run. Default models:

| Provider | SMART | MID | FAST |
|---|---|---|---|
| `gemini` | `gemini-3.6-flash` | `gemini-3.5-flash` | `gemini-3.5-flash-lite` |
| `claude` | `opus` | `sonnet` | `haiku` (low effort) |
| `codex` | CLI default | CLI default | CLI default (low effort) |
| `gemini-cli` | `pro` | `flash` | `flash-lite` |
| others | CLI default | CLI default | CLI default |
