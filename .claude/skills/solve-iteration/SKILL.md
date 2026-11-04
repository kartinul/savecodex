---
name: solve-iteration
description: Run one `savecodex solve` test iteration over a folder of assignment PDFs/DOCX, analyse the output with readdocx.py and report observations. Use when the user says BUILD for an iteration, or asks to run/rerun solve iterations.
---

# solve-iteration

Iterations are numbered from 0 and the number grows each time. Output goes to `<testdir>/res/iteration<N>/`. Default testdir: `~/Desktop/testing` (inputs in `questions/`).

Building, solving and starting containers wait for the user's literal `BUILD`. Invoking this skill or passing an iteration number is not `BUILD`: confirm the plan and wait. During an iteration you only observe: report, then let the user pick which problems to fix.

## Steps
1. N = next free (or empty) `res/iteration<N>`. Rebuild first: `cargo build --release` in the savecodex repo (`savecodex` on PATH is `target/release/savecodex`).
2. Read the user's `savecodex-free` in `~/.zshrc` and reuse exactly the tier vars it sets, through `zsh -ic`.
3. Run both groups at once, each as a background job, logging into the iteration folder:
   - Unix docs (shell/vi/bash/process/cron questions, currently `Assignment-3-vi`, `Assignment-6-shell`, `Assignment-9`) run with `--linux`:
     `zsh -ic 'cd <testdir>; export <tier vars>; savecodex solve --linux questions/<unix>.pdf ... -o "res/iteration<N>/{}.docx"' > res/iteration<N>/linux.log 2>&1`
   - The rest run natively, same command without `--linux`, logging to `native.log`.
   - `--linux` starts Apple container itself and builds its image from `crates/app/src/linux.Dockerfile` on first use (a few minutes). A new OS tool or language goes into that image, not the code.
4. Check every document. Done means each of these is checked for every doc:
   - `python3 scripts/readdocx.py -p res/iteration<N>/*.docx` for flagged/empty screenshots, then the full dump.
   - Against the source (`pdftotext -layout`, or the DOCX text): every question present once, in document order, nothing extra (a heading that isn't in the source is a stray answer).
   - Headings in one style per doc (`2`, `2(a)`, `Challenge Q1`).
   - Output plausible for the code: wrong results, empty demos, crashes, errors that aren't the point of the question.
   - Logs: `WARN` lines (dropped answers, failed resets/cleanups, render fixes).
   - mysql left clean: `mysql -u root -N -e "SHOW DATABASES"` has no `sc…_` databases.
5. Tell the user what you observed, grouped by document: empty or flagged screenshots, wrong or odd output, missing, extra or misordered questions, bad formatting. Reply in chat only (no `report.md`), then send a PushNotification.

## Environment gotchas
- Native mysql logs in through the user's override in `~/Library/Application Support/savecodex/languages.json` (`-u root`; only `root` exists on the local server). It merges over `default_languages.json`, so it holds only `reset`/`run`/`cleanup`.

## Rules
- Fixes are general: any provider, model, OS or document. Per-OS differences go in `default_languages.json` overrides, language advice in its `hint`, never in `prompts.rs`.
- Ask the user which problems to fix before changing code. Leave committing and pushing to the user.
