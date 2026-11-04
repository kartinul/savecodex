/// Generates a prompt for generating input based on a slice of code files (filename, content).
pub fn input_generation_prompt(files: &[(&str, &str)]) -> String {
    let mut prompt = String::from("Analyze the following code files and generate appropriate standard input (stdin) that would successfully execute or test this code.\n\n");
    
    for (filename, content) in files {
        prompt.push_str(&format!("--- {} ---\n{}\n\n", filename, content));
    }
    
    prompt.push_str("Based on the code above, generate ONLY the exact raw text that should be passed to stdin. Do not include markdown formatting or explanations. The output must be ready to pipe directly into the program.\n");
    
    prompt
}

/// `solve` step 1: turn a document of tasks (text + page images) into grouped questions.
pub fn extract_prompt(doc_text: &str, session_langs: &str) -> String {
    format!(r#"You are reading a document that asks for things to be made, written or answered (questions, exercises, project briefs, specs, tickets). Its text is below and a photo of every page is attached; use the photos for anything the text misses (tables, figures, scanned pages).

Return ONLY this JSON, no markdown:
{{"language":"...","groups":[{{"setup":{{"lang":"...","code":"..."}},"questions":[{{"title":"Q1","text":"...","parts":["(a) ...","(b) ..."]}}]}}]}}

Rules:
- Include every task that asks for something: questions, exercises, features or deliverables to build, things to write or design. Skip notes, explanations, worked examples, instructions, references and bare lists of topics or links.
- language: the programming language the document asks for (named in it, implied by a file naming convention like .cpp, or by the course), or "" if it doesn't say.
- title: the document's own label (Q3, Exercise 4, Task 2), in one style for the whole document. If there is none, use a short name (Login page). Make titles unique and keep document order.
- text: the full question, self-contained. Write out everything it relies on: schemas, tables, sample data, figures and the earlier question it extends ("Repeat Q20", "modify the above"). Describe figures in words. Copy table data from images exactly.
- parts: the sub-tasks verbatim and in order, or [] if there are none.
- groups: questions that depend on each other (same database, schema, files or workspace; uses a table, file or object another question creates, such as a foreign key to it; extends or reuses an earlier answer; refers to "the above" program, process or script) share one group. List groups in the order their first question appears in the document. Independent questions get a group each, even when they share a topic or a heading like "Practice questions".
- setup: optional per group. Code that must already exist before the group runs but that no question asks to be written: CREATE TABLE and INSERT for a given schema, data files a question assumes exist. lang must be one of: {session_langs}. Copy given data exactly. If tables are given without rows, invent realistic rows so every question returns a non-empty result and its thresholds are met. Omit setup when the questions themselves create everything.

Document text:
{doc_text}"#)
}

/// `solve` step 2: answer one group of questions.
pub fn solve_prompt(group_json: &str, earlier: &str, langs: &str) -> String {
    let earlier = if earlier.is_empty() { String::new() } else {
        format!("\n\nEarlier questions of this group are already answered, and their sessions ran first. Continue from them; don't repeat them or their commands:\n{earlier}")
    };
    format!(r#"Complete these tasks. Return ONLY this JSON, no markdown, one entry per question in order:
[{{"title":"<question title>","answers":[{{"label":"Q3(b)","lang":"mysql","code":"...","answer":"","answer_after_run":false,"files":[],"stdin":""}}]}}]

Languages:
{langs}
- null: nothing to run.

Rules:
- Write only what the question asks for. No extra features, menus, validation, helper classes, comments, columns, constraints or options (AUTO_INCREMENT, defaults, indexes) it didn't ask for or give. Clean code, consistent indentation, clear names.
- label: the question's number alone (Q.3 → 3, Program 1: Student Grouping System → 1, Problem 2 → 2), with the part appended for sub-parts (3(b)); a corrected version of a failing answer adds " (corrected)". Keep a prefix (Q1, P1) only when bare numbers would be ambiguous, and a title without a number as given. A question without parts uses the same label for every answer. No other words.
- One answer per question; split only when its parts are separate tasks (a list of things to print is one task).
- One answer per thing to show. File languages: usually one program per question covering all its parts; split only when the parts are separate programs. Session languages: one answer per part.
- lang: the language the question names. A web page or UI is html (one self-contained file, CSS and JS inline). A chart or graph is plot (Python with matplotlib that ends with plt.savefig(__file__.rsplit('.', 1)[0] + '.png')). A program someone runs is its own language. SQL is mysql (MySQL 8; emulate unsupported features such as FULL OUTER JOIN and say so in answer). Commands typed at a terminal are shell. A shell script run on its own with no arguments is sh; a script that needs arguments, another running program or follow-up commands is a shell answer with the script in files, run with real arguments (to signal a process, start one in the background first and pass $!). ER, EER, schema, UML diagrams and flowcharts are dot; a question that asks to draw, design or update a diagram always gets a dot answer, never a description of one. vi and other editor tasks are null, with answer listing only what to type for each step, one per line, no explanation (vi practice.txt, i, Esc, :wq, yy, p, 1dd, u, :%s/old/new/g, G, o). Anything else that can't run in a terminal (editing a crontab interactively, theory) is null, with the steps or explanation in answer.
- Every output becomes a screenshot, so keep it short: small sample data, create only the files the task needs, avoid commands that print hundreds of lines.
- Session code is exactly what a person would type, one statement or command per line, no comments. Each blank-line-separated step becomes its own screenshot, so a part is usually one step; split only before a command with long output. The group's setup has already run: never repeat its statements. Consecutive session answers in a group share one live session, so later answers continue where earlier ones stopped: don't kill, delete or redo what a later answer needs, and don't repeat earlier commands. Use DELIMITER $$ for mysql procedures and triggers. mysql already runs inside a fresh empty database: never CREATE DATABASE or USE, and don't wrap statements in transactions or ROLLBACK unless the question asks. Show structure with DESC; if SHOW CREATE TABLE is needed, end it with \G. When a command reads input, type the input on the next lines as a person would; don't pipe or echo it in. After starting a program in the background, sleep 1 before signalling or inspecting it. Never use commands that don't return (top, watch, tail -f, interactive editors); use one-shot forms.
- A script is always a file (sh, or a files entry of a shell answer), never typed line by line at a prompt.
- files: scripts or files the task asks you to write that a session answer then uses, as [{{"name":"sendsig.sh","code":"..."}}]. They are saved in the working directory before the session runs and shown above it; code holds only the commands that use them (chmod +x, run with arguments). Never create files in a session with cat, echo or heredocs.
- File answers are saved in the working directory as the label lowercased without punctuation plus the extension (Q1 → q1.c, Q3(b) → q3b.sh). A session in the same group that uses one types that exact name and compiles it itself (gcc q1.c -o q1); the file answer is then only shown, not run on its own, so put the compile-and-run commands that show that question's own output in a session answer under that same question.
- main exercises everything the question asks for (create, insert, union, display ...); never leave it empty or stubbed.
- File code is a complete program that reads stdin when the question needs user input. stdin: the exact input typed into it, in the order it reads, or "" if it reads none.
- One run shows every case the question is about: found and not found, valid and invalid input, the exception thrown and caught. Use realistic values.
- Shell scripts start with #!/bin/bash. In bc, compute with scale=6 or more, use ^ for integer powers, and round only when printing. Strip the padding wc adds (wc -l < file | tr -d ' ').
- Never change the real system: no installing crontabs (show the crontab line and run the script once instead), no sudo, launchctl, systemctl, package installs or deleting anything outside the working directory.
- Write files only in the working directory with relative paths, never /tmp or other absolute paths.
- Code given in the question to run, test or analyse is used verbatim; if it fails, that failure is part of the answer and a corrected version follows as another answer.
- dot: Chen notation for ER (entity box, weak entity peripheries=2, attribute ellipse, key attribute underlined with an HTML label, multivalued peripheries=2, derived style=dashed, relationship diamond, identifying relationship peripheries=2, cardinality as edge labels, total participation color="black:black"). EER uses a small circle labelled d or o. Use layout=neato with overlap=false and splines=true for ER and EER, keep the drawing about as tall as wide so it stays legible on a page, use the default font, and include every attribute and key the written answers mention.
- Most answers have answer "" and answer_after_run false. Never describe the code, the steps or the output unless the question asks for it.
- answer: shown below the screenshot. Only when the task asks to explain, compare, identify, justify, list or write something that doesn't depend on the output (an essay, report or email goes here). Plain text only, no markdown and no # headings. Never put code or scripts here; they go in code or files. Otherwise "".
- answer_after_run: true only when the question asks for something that depends on the actual output (observe the behaviour, compare counts, analyse the output). It is written after the run and shown below the screenshot; don't repeat it in answer.

Questions:
{group_json}{earlier}"#)
}

/// `solve` step 3: write an answer from the real output.
pub fn after_run_prompt(question: &str, code: &str, output: &str) -> String {
    format!("Question:\n{question}\n\nCode:\n{code}\n\nActual output:\n{output}\n\nWrite only what the question asks about this output (an observation, comparison or conclusion), in 2 to 4 sentences. Don't restate the code, list the steps, use headings or write in the first person, and don't comment on the environment or the run. If commands failed and the question isn't about the error, answer from what the question asks without mentioning the failure, the machine or the operating system. Plain text, no markdown.")
}

/// `solve`: fix an answer that failed to render or crashed when run.
pub fn fix_prompt(code: &str, error: &str) -> String {
    format!("This code fails:\n{code}\n\nError:\n{error}\n\nReturn ONLY the fixed, complete code, no markdown.")
}
