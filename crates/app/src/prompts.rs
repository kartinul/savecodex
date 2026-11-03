/// Generates a prompt for generating input based on a slice of code files (filename, content).
pub fn input_generation_prompt(files: &[(&str, &str)]) -> String {
    let mut prompt = String::from("Analyze the following code files and generate appropriate standard input (stdin) that would successfully execute or test this code.\n\n");
    
    for (filename, content) in files {
        prompt.push_str(&format!("--- {} ---\n{}\n\n", filename, content));
    }
    
    prompt.push_str("Based on the code above, generate ONLY the exact raw text that should be passed to stdin. Do not include markdown formatting or explanations. The output must be ready to pipe directly into the program.\n");
    
    prompt
}

/// `solve` step 1: turn an assignment sheet (text + page images) into grouped questions.
pub fn extract_prompt(sheet_text: &str, session_langs: &str) -> String {
    format!(r#"You are reading a lab assignment sheet. Its text is below and a photo of every page is attached; use the photos for anything the text misses (tables, figures, scanned pages).

Return ONLY this JSON, no markdown:
{{"groups":[{{"setup":{{"lang":"...","code":"..."}},"questions":[{{"title":"Q1","text":"...","parts":["(a) ...","(b) ..."]}}]}}]}}

Rules:
- Include every task the student must answer: numbered questions, "Try it" tasks, exercises, practice and challenge questions that have a full statement. Skip notes, theory, worked examples, instructions, code of conduct, references and bare lists of topics or links.
- title: the sheet's own label (Q3, T2, Exercise 4, Challenge Q1). Make labels unique and keep sheet order.
- text: the full question, self-contained. Write out everything it relies on: schemas, tables, sample data, figures and the earlier question it extends ("Repeat Q20", "modify the above"). Describe figures in words. Copy table data from images exactly.
- parts: the sub-tasks verbatim and in order, or [] if there are none.
- groups: questions that depend on each other (same database, schema, files or workspace; extends or reuses an earlier answer) share one group, in sheet order. Independent questions get a group each.
- setup: optional per group. Code that must already exist before the group runs but that no question asks the student to write: CREATE TABLE and INSERT for a given schema, data files a question assumes exist. lang must be one of: {session_langs}. Copy given data exactly. If tables are given without rows, invent realistic rows so every question returns a non-empty result and its thresholds are met. Omit setup when the questions themselves create everything.

Sheet text:
{sheet_text}"#)
}

/// `solve` step 2: answer one group of questions.
pub fn solve_prompt(group_json: &str, langs: &str) -> String {
    format!(r#"Solve these lab questions. Return ONLY this JSON, no markdown, one entry per question in order:
[{{"title":"<question title>","answers":[{{"label":"Q3(b)","lang":"mysql","code":"...","answer":"","answer_after_run":false}}]}}]

Languages:
{langs}
- null: nothing to run.

Rules:
- Write only what the question asks for. No extra features, menus, validation, helper classes, comments or output it didn't ask for. Clean code, consistent indentation, clear names.
- One answer per thing to show. File languages: usually one program per question covering all its parts; split only when the parts are separate programs. Session languages: one answer per part.
- lang: the language the sheet names. SQL is mysql (MySQL 8; emulate unsupported features such as FULL OUTER JOIN and say so in answer). Commands typed at a terminal are shell, shell scripts are sh. ER, EER, schema, UML diagrams and flowcharts are dot. Anything that can't run in a terminal (vi keystrokes, editing a crontab interactively, theory) is null, with the steps or explanation in answer.
- Session code is exactly what the student types, one statement or command per line, no comments. Use DELIMITER for mysql procedures and triggers. Never use commands that don't return (top, watch, tail -f, interactive editors); use one-shot forms.
- File code is a complete program that reads stdin when the question needs user input.
- Code given in the sheet to run, test or analyse is used verbatim; if it fails, that failure is part of the answer and a corrected version follows as another answer.
- dot: Chen notation for ER (entity box, weak entity peripheries=2, attribute ellipse, key attribute underlined with an HTML label, multivalued peripheries=2, derived style=dashed, relationship diamond, identifying relationship peripheries=2, cardinality as edge labels, total participation color="black:black"). EER uses a small circle labelled d or o. Use layout=neato with overlap=false and splines=true for ER and EER.
- answer: plain text only (no markdown), only when the question asks to explain, compare, identify, justify, list or answer something. Otherwise "".
- answer_after_run: true when the written answer depends on the actual output (observe, compare counts, analyse the output). Leave answer "" then; it is written after the run.

Questions:
{group_json}"#)
}

/// `solve` step 3: write an answer from the real output.
pub fn after_run_prompt(question: &str, code: &str, output: &str) -> String {
    format!("Question:\n{question}\n\nCode:\n{code}\n\nActual output:\n{output}\n\nWrite the written part of the answer, based on this actual output. Plain text only, no markdown, concise.")
}

/// `solve`: fix a diagram that failed to render.
pub fn fix_diagram_prompt(code: &str, error: &str) -> String {
    format!("This Graphviz dot code fails to render:\n{code}\n\nError:\n{error}\n\nReturn ONLY the fixed dot code, no markdown.")
}
