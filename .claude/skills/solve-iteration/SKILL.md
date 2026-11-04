---
name: solve-iteration
description: Run one `savecodex solve` test iteration over a folder of assignment PDFs/DOCX, analyse the output with readdocx.py and report observations. Use when the user says BUILD for an iteration, or asks to run/rerun solve iterations.
---

# solve-iteration

Iterations are numbered from 0 and the number grows each time. Output goes to `<testdir>/res/iteration<N>/`. Default testdir: `~/Desktop/testing` (inputs in `questions/`).

Run only when the user says BUILD. Never fix anything on your own during an iteration. Report, then wait for the user's call on which problems to fix.

## Steps
1. N = next free `res/iteration<N>`. Rebuild the host binary first: `cargo build --release` in the savecodex repo.
2. Same AI setup as the user's `savecodex-free` (defined in `~/.zshrc`, so use `zsh -ic`). Do not hardcode providers or models; read the function and reuse whatever it sets.
3. Unix docs (shell/vi/bash questions, currently `Assignment-3-vi`, `Assignment-6-shell`, `Assignment-9` PDFs) run in Linux Docker, the rest natively:
   - `zsh -ic 'cd <repo>; <tier vars from savecodex-free> scripts/lab/run.sh <testdir> <N> docker questions/<unix>.pdf ...'`
   - `... scripts/lab/run.sh <testdir> <N> native questions/<other> ...`
   - Docker must be running (`open -a Docker`). The image is built from the working tree by `scripts/lab/Dockerfile`; to run another OS or a new language, extend the image and not the code.
4. `python3 scripts/readdocx.py res/iteration<N>/*.docx` (add `-p` for flagged-only first). Also compare with the source questions (`pdftotext`) to find missed, merged or wrong questions.
5. Tell the user what you observed, grouped by document: empty or flagged screenshots, wrong or odd output, missing questions, bad formatting. No `report.md`. Then send a PushNotification.

## Rules
- Fixes must be general: no hardcoding for macOS, one provider, one model or specific files. Per-OS differences go in `default_languages.json` overrides.
- Ask the user which problems to fix before changing code. Do not commit or push.
