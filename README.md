# savecodex

savecodex reads the code files in a folder you select, uses AI to generate inputs for them, runs them with those inputs, screenshots the terminal output and packs everything into a DOCX document.
Just select your folder and enter your Gemini API key to begin.

## Demo

### Gemini key variant

<https://youtu.be/-i_L3jEiWxc>

### Admin key variant

<https://youtu.be/8jD1Prdp1cc>

## Installation

Requires Rust and [bun](https://bun.sh). The frontend is embedded into the binary, so build it first:

```bash
make build   # bun install + bun run build + cargo build --release
```

Or run everything with Docker (see `Dockerfile` / `render.yaml`).

For development (frontend on `:5173`, backend on `:7878`):

```bash
make dev
```

## AI & Environment Setup

savecodex uses AI (Gemini, or any OpenAI-compatible provider like Groq or Ollama) to generate program inputs.

1. Copy `.env.example` to `.env`:

   ```bash
   cp .env.example .env
   ```

2. Fill in your keys. Gemini is the default; set `CODE_PROVIDER` / `INPUT_PROVIDER` to `openai_compatible` with a `*_BASE_URL` and `*_API_KEY` to use another provider. Multiple comma-separated Gemini models fall back in order.
3. Optionally set `ADMIN_PASSWORD` so users can log in on the frontend without their own API key.
4. Check your keys work:

   ```bash
   ./apitest
   ```

## CLI Usage

Run via `cargo run -- <command>` or the compiled binary `./target/release/savecodex <command>`.

### `pack`

Finds source files in one or more files/folders, generates `stdin` with AI for files that read input, runs them, renders the terminal output as screenshots and writes a DOCX per folder.

```bash
savecodex pack test_folder -o "test_{}" --ext java,py --doc-title "Assignment {}"
savecodex pack test_folder -r   # recurse into subdirectories
```

- `<paths...>`: Files or folders to pack.
- `-o, --output`: Output file name. `{}` is replaced with the folder name, and is required when packing multiple inputs.
- `-r, --recursive`: Recurse into subdirectories (default: only the folder itself).
- `--ext`: Comma-separated extensions to include (e.g. `java,py,rs`).
- `--doc-title`: Document heading (default: folder name). Supports `{}`.
- `--doc-text`: Text under the heading. Supports `{}`.
- `--page-break`: Page break after each file's output.
- `--ai-input`: When to ask the AI for stdin: `auto` (default, only files that contain a stdin marker such as `input(` or `scanf`), `always` or `never`.
- `--no-trim`: Don't trim output to 150 lines / 200 chars per line (a hard cap of 1000 lines / 300 chars still applies).
- `-j, --jobs`: Max files processed in parallel (default: `15`).

`pack` also takes the screenshot options from [`term`](#term) (`--style`, `--theme`, `--username`, ...), except `--no-prompt`.

Stdin markers live in the `stdin` list of each language in `default_languages.json` and can be overridden in your own `languages.json`. A language without a `stdin` list always gets AI input.

**Defaults via `.env`:** `SAVECODEX_OUTPUT`, `SAVECODEX_EXT`, `SAVECODEX_DOC_TITLE`, `SAVECODEX_DOC_TEXT`, `SAVECODEX_STYLE`, `SAVECODEX_JOBS`, `SAVECODEX_AI_INPUT` and `SAVECODEX_NO_TRIM` set defaults for the matching flags:

```env
SAVECODEX_STYLE=macos
SAVECODEX_OUTPUT="1234567890_{}.docx"
SAVECODEX_DOC_TEXT="\nJohn Doe - 1234567890 - {}\n"
```

### `run`

Compiles and runs a single source file in a pseudo-terminal and prints the transcript.

```bash
savecodex run main.py -i "5"
```

- `-i, --input`: stdin to send to the program.
- `-o, --output`: Write the transcript to a file instead of stdout.

### `term`

Renders a fake terminal window screenshot (PNG), either from running a file or from text (plain or ANSI-colored).

```bash
savecodex term --runfile main.py -i "5" -o out.png --style macos
savecodex term --text-file output.txt -o out.png
```

- `--runfile`: Source file to run and render.
- `-i, --input`: stdin for `--runfile`.
- `--text-file`: Text file to render instead. Reads stdin if neither is given. A prompt line is drawn above the text unless it already starts with one.
- `-o, --output`: PNG path (default: `term.png`).
- `--style`: `windows` (default), `macos` or `ubuntu` (`linux` also works). Defaults from `SAVECODEX_STYLE`.
- `--theme`: `dark` (default) or `light`.
- `--username`, `--hostname`, `--cwd`: Prompt details (default: your current user, machine and directory).
- `--no-prompt`: Don't draw the prompt line (alias `--noprompt`).
- `--messy`: Realistic imperfections, `0` (off) to `3`; `--seed` fixes the randomness.
- `--banner`: Print the PowerShell startup banner (windows style).
- `--min-cols`: Minimum window width in columns (default: `80`).
- `--prompt`: Prompt string to highlight (default: `"$ "`); `--no-prompt-highlight` disables it.

### `serve`

Starts the HTTP server that also serves the web frontend.

```bash
savecodex serve --host 127.0.0.1 --port 7878
```

- `--port`: Port to listen on (default: `7878`).
- `--host`: Address to bind to (default: `127.0.0.1`).
- `--no-key`: Use the server's `.env` AI config for everyone; users need no API key or admin password (env: `SAVECODEX_NO_KEY`).

### Sandboxing

Every command that runs code (`pack`, `run`, `term --runfile`, the server) uses the same runner:

- **All platforms:** the child gets an allowlisted environment (`PATH`, `LANG`, `TERM`, `HOME`, Java/Rust paths), so your `.env` secrets never reach user code. Builds time out after 30s, runs after 5s.
- **Linux:** runs under `prlimit` (CPU, processes, file size, memory), and inside `bwrap` when it is installed (read-only system dirs, no home dir, private `/tmp`).
- **Linux, server running as root** (Docker/Render): each job also runs as its own throwaway uid. `scripts/sandbox-firewall.sh` blocks LAN/metadata network access.
- **macOS/Windows:** only the environment allowlist and timeouts apply, so only run code you trust.

### `solve` (not implemented yet)

Planned: read questions from a PDF/DOCX, write solutions with AI, run them and export to DOCX + PDF.
