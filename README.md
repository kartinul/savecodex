# SaveCodex

SaveCodex is a tool designed to turn coding assignments into beautifully formatted documents. It can package existing code, automatically solve assignment sheets using AI, and spin up an API for a frontend interface.

## Installation

Ensure you have Rust installed, then build the project:

```bash
cargo build --release
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
Reads all source code files in a specified folder, executes them to capture their output, and packages everything (code + output) into beautifully formatted DOCX and PDF documents.

**Usage:**
```bash
savecodex pack <folder_path> -o <output_path>
```
- `<folder_path>`: The directory containing your source code files.
- `-o, --output`: The base name for the output files. SaveCodex will generate both `<output_path>.docx` and `<output_path>.pdf`.

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
