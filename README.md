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
| `-j, --jobs` | `15` | Files processed in parallel |

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
| `GEMINI_API_KEY` | none | Gemini key; also used for admin logins |
| `ADMIN_PASSWORD` | none | Lets frontend users use the server's `GEMINI_API_KEY` |
| `INPUT_PROVIDER` | `gemini` | `gemini` or `openai_compatible` |
| `INPUT_MODEL` | `gemini-1.5-pro` | Comma-separated Gemini models fall back in order |
| `INPUT_API_KEY`, `INPUT_BASE_URL` | none | Needed for `openai_compatible` |
| `CODE_*` | same as `INPUT_*` | Same settings for `solve` |
| `SAVECODEX_NO_KEY` | off | `serve --no-key` |
| `SAVECODEX_OUTPUT` | `<folder>_pack` | `pack -o` |
| `SAVECODEX_EXT` | auto-detect | `pack --ext` |
| `SAVECODEX_DOC_TITLE` | folder name | `pack --doc-title` |
| `SAVECODEX_DOC_TEXT` | none | `pack --doc-text` |
| `SAVECODEX_STYLE` | `windows` | `pack` / `term --style` |
| `SAVECODEX_AI_INPUT` | `auto` | `pack --ai-input` |
| `SAVECODEX_NO_TRIM` | off | `pack --no-trim` |
| `SAVECODEX_JOBS` | `15` | `pack -j` |
| `RUST_LOG` | `info` | Log level, e.g. `debug` |
