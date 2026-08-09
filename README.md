# SaveCodex

SaveCodex is a tool designed to turn coding assignments into beautifully formatted documents. It can package existing code, automatically solve assignment sheets using AI, and spin up an API for a frontend interface.

## Installation

Ensure you have Rust installed, then build the project:

```bash
cargo build --release
```

## AI & Environment Setup

SaveCodex leverages AI (like Gemini, Groq, or Ollama) for intelligent code generation and automation (used by the `solve` command or internal AI modules). 

1. Copy `.env.example` to `.env`:
   ```bash
   cp .env.example .env
   ```
2. Open `.env` and fill in your preferred API keys. By default, SaveCodex uses Google Gemini, but you can configure any OpenAI-compatible provider (Groq, Ollama, etc.) for either code generation or input generation.
3. Verify your keys are working by running the included utility script:
   ```bash
   ./apitest
   ```

## CLI Usage


SaveCodex provides several commands to handle different workflows. You can run the CLI via `cargo run -- <command>` or by executing the compiled binary directly.

### 1. `serve`
Starts the backend HTTP API server. This is used to connect the SaveCodex frontend application.

**Usage:**
```bash
savecodex serve --host 127.0.0.1 --port 7878
```
- `--host`: The address to bind the server to (default: `127.0.0.1`).
- `--port`: The port to listen on (default: `7878`).

---

### 2. `pack`
Reads all source code files matching specific extensions in one or more files/folders, uses AI to generate mock `stdin` for each, executes them to capture their output (with `term_gen`), and packages everything into beautifully formatted DOCX documents.

**Usage:**
```bash
savecodex pack <folder_paths...> -o <output_path> --ext java,py --doc-title "Assignment {}"
```
- `<folder_paths...>`: One or more files or directories containing your source code (e.g., `Week*/` or `main.rs`).
- `-o, --output`: The base name for the output files (`.docx`). Supports the `{}` placeholder to dynamically insert the file/folder name.
- `--ext`: Comma-separated list of file extensions to include (e.g., `java,py,rs`).
- `--doc-title`: Main heading for the document. Supports the `{}` placeholder.
- `--doc-text`: Optional description text under the heading. Supports the `{}` placeholder.

*(Note: `pack` also supports all the terminal styling arguments available in the `term` command, such as `--theme`, `--font-size`, `--style`, etc.)*

**Configuring Defaults via `.env`:**
You can set default values for any `pack` argument in your `.env` file to avoid passing them in the CLI:
```env
SAVECODEX_STYLE=macos
SAVECODEX_OUTPUT="1234567890_{}.docx"
SAVECODEX_DOC_TEXT="\nJohn Doe - 1234567890 - {}\n"
```
---

### 3. `solve`
Automates the assignment workflow entirely. It reads questions from an input PDF or DOCX file, uses AI to write the code solutions, runs the code to verify it and capture output, and finally exports the results into DOCX and PDF.

**Usage:**
```bash
savecodex solve <input_file> -o <output_path>
```
- `<input_file>`: The assignment file to parse (PDF or DOCX).
- `-o, --output`: The base name for the generated solutions files (`.docx` and `.pdf`).

---

### 4. `term` (Dev/Testing Tool)
A utility command that reads text (plain or ANSI-colored) from a file or standard input and renders it into a PNG image of a fake terminal window. This is highly customizable and useful for generating code snippets or terminal outputs for documentation.

**Usage:**
```bash
savecodex term -i <input_file> -o <output_file.png> --style <style> --title "Title" --theme <theme>
```
- `-i, --input`: The file to read text from. If omitted, reads from `stdin`.
- `-o, --output`: The path to save the generated PNG (default: `term.png`).
- `--style`: The window control style. Options are `windows` (default), `macos`, or `linux`.
- `--title`: The text to display in the window's title bar (default: `bash`).
- `--theme`: The color theme of the window. Options are `dark` (default) or `light`.
- `--font-size`: The font size in pixels (default: `18.0`).
- `--padding`: Padding around the text inside the window (default: `28`).
- `--username`: Realistic prompt username (default: `local`).
- `--hostname`: Realistic prompt hostname (default: `host`).
- `--cwd`: Realistic prompt current working directory (default: `~`).
