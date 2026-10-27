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

### `serve`

Starts the HTTP server that also serves the web frontend.

```bash
savecodex serve --host 127.0.0.1 --port 7878
```

- `--host`: Address to bind to (default: `127.0.0.1`).
- `--port`: Port to listen on (default: `7878`).
- `--no-key`: Use the server's `.env` AI config for everyone; users need no API key or admin password (env: `SAVECODEX_NO_KEY`).

On Linux, user code runs sandboxed with `bwrap` when it is installed.

### `pack`

Finds source files in one or more files/folders, generates `stdin` for each with AI, runs them, renders the terminal output as screenshots and writes a DOCX per folder.

```bash
savecodex pack test_folder -o "test_{}" --ext java,py --doc-title "Assignment {}"
savecodex pack test_folder -r   # recurse into subdirectories
```

- `<paths...>`: Files or folders to pack.
- `-r, --recursive`: Recurse into subdirectories (default: only the folder itself).
- `-o, --output`: Output file name. `{}` is replaced with the folder name, and is required when packing multiple inputs.
- `--ext`: Comma-separated extensions to include (e.g. `java,py,rs`).
- `--doc-title`: Document heading (default: folder name). Supports `{}`.
- `--doc-text`: Text under the heading. Supports `{}`.
- `--page-break`: Page break after each file's output.
- `-j, --jobs`: Max files processed in parallel (default: `15`).

`pack` also takes all the screenshot options from [`term`](#term).

**Defaults via `.env`:** `SAVECODEX_OUTPUT`, `SAVECODEX_EXT`, `SAVECODEX_DOC_TITLE`, `SAVECODEX_DOC_TEXT`, `SAVECODEX_STYLE` and `SAVECODEX_JOBS` set defaults for the matching flags:

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
savecodex term --runfile main.py --input "5" -o out.png --style macos --show
savecodex term --text-file output.txt -o out.png
```

- `--runfile`: Source file to run and render.
- `--input`: stdin for `--runfile`.
- `--text-file`: Text file to render instead. Reads stdin if neither is given.
- `-o, --output`: PNG path (default: `term.png`).
- `--show`: Open the image after writing it.
- `--style`: `windows` (default), `macos` or `ubuntu` (`linux` also works).
- `--theme`: `dark` (default) or `light`.
- `--font-size`: Font size in pixels (default: `18`).
- `--padding`: Padding around the text (default: matches the real terminal).
- `--min-cols`: Minimum window width in columns (default: `80`).
- `--prompt`: Prompt string to highlight (default: `"$ "`); `--no-prompt-highlight` disables it.
- `--messy`: Realistic imperfections, `0` (off) to `3`; `--seed` fixes the randomness.
- `--banner`: Print the PowerShell startup banner (windows style).
- `--username`, `--hostname`, `--cwd`: Prompt details (default: your current user, machine and directory).

### `solve` (not implemented yet)

Planned: read questions from a PDF/DOCX, write solutions with AI, run them and export to DOCX + PDF.
