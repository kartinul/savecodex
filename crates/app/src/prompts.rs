/// Generates a prompt for generating input based on a slice of code files (filename, content).
pub fn input_generation_prompt(files: &[(&str, &str)]) -> String {
    let mut prompt = String::from("Analyze the following code files and generate appropriate standard input (stdin) that would successfully execute or test this code.\n\n");
    
    for (filename, content) in files {
        prompt.push_str(&format!("--- {} ---\n{}\n\n", filename, content));
    }
    
    prompt.push_str("Based on the code above, generate ONLY the exact raw text that should be passed to stdin. Do not include markdown formatting or explanations. The output must be ready to pipe directly into the program. If the code reads no input, return nothing.\n");
    
    prompt
}

/// `solve` step 1: turn a document of tasks (text + page images) into grouped questions.
pub fn extract_prompt(doc_text: &str, langs: &str, session_langs: &str) -> String {
    format!(r#"You are reading a document that asks for things to be made, written or answered (questions, exercises, project briefs, specs, tickets). Its text is below and a photo of every page is attached; use the photos for anything the text misses (tables, figures, scanned pages).

Return ONLY this JSON, no markdown:
{{"language":"<language or empty>","groups":[{{"setup":{{"lang":"<language>","code":"<code>"}},"questions":[{{"title":"<title>","text":"<full question>","parts":["<part>"]}}]}}]}}

Rules:
- Include every task that asks for something: questions, exercises, features or deliverables to build, things to write or design. Skip notes, explanations, worked examples, instructions, references and bare lists of topics or links.
- language: the programming language the document asks for (named in it, implied by a file naming convention like .cpp, or by the course), as one of: {langs}; or "" if it doesn't say.
- title: the document's own label (Q3, Exercise 4, Task 2), in one style for the whole document, kept even when the task also has a name ("Task 2: Login page" → Task 2). If there is none, use a short name (Login page). A section that restarts numbering keeps its name in the title (Bonus Q1). Make titles unique and keep document order.
- text: the full question, self-contained. Write out everything it relies on: schemas, tables, sample data, figures and the earlier question it extends ("Repeat Q20", "modify the above"). Describe figures in words. Copy table data from images exactly.
- parts: the sub-tasks verbatim and in order, or [] if there are none. Sub-tasks inside a question (Tasks 1-3, a-d, i-iv) are its parts, never questions of their own.
- groups: questions that depend on each other (same database, schema, files or workspace; uses a table, file or object another question creates, such as a foreign key to it; extends or reuses an earlier answer; refers to "the above" program, process or script) share one group. List groups in the order their first question appears in the document. Independent questions get a group each, even when they share a topic or a heading like "Practice questions". A question that says to start fresh (a fresh or empty database, a new directory) gets a group of its own.
- setup: optional per group. Code that must already exist before the group runs but that no question asks to be written: CREATE TABLE and INSERT for a given schema, data files a question assumes exist. lang must be one of: {session_langs}. Copy given data exactly. If tables are given without rows, invent realistic rows so every question returns a non-empty result and its thresholds are met. Omit setup when the questions themselves create everything.

Document text:
{doc_text}"#)
}

const ANSWER_FORMAT: &str = r#"[{"title":"<question title>","answers":[{"label":"<label>","lang":"<language or null>","code":"<code>","answer":"","answer_after_run":false,"files":[],"stdin":"","args":[],"expects_error":false}]}]"#;

/// How an answer is written, shared by solving and redoing.
fn answer_rules(langs: &str) -> String {
    format!(r#"Languages:
{langs}
- null: nothing to run.

Rules:
- title: the question's title exactly as given.
- label: dotted numbers only: the number in the question's label (2 or Practice Q2 → 2; 1 if it has none), then the position of the part at each level, counted in order whatever markers the question uses (whole question 2 → 2; its second part → 2.2; that part's first sub-part → 2.2.1). A corrected version of a failing answer adds " (corrected)". No other words.
- Write only what the question asks for. No extra features, menus, validation, helper code, comments or options it didn't ask for or give. Clean code, consistent indentation, clear names.
- One answer per thing to show. A program usually covers all parts of its question; split only when the parts are separate programs. Session languages and answers that don't run get one answer per part. A question gets either one answer for all of it or one per part, never both.
- lang: the language the question names, else the one that fits. Anything a person does by hand rather than runs (keys typed in an editor such as vi, vim or nano, GUI steps, theory) is null with code "": answer holds just the keys, commands or explanation, one short answer per part. Something to draw (a diagram, chart or page) is never null, even when the question names a tool to draw it with.
- Every output becomes a screenshot, so keep it short: small realistic sample data, only the files the task needs, no commands that print hundreds of lines. When a task works on existing files or data, create a few small samples first so the run shows real results.
- Session code is exactly what a person would type, one statement or command per line, no comments. Each blank-line-separated step becomes its own screenshot, so a part is usually one step. The group's setup has already run: never repeat it. Consecutive session answers in a group share one live session, so later answers continue where earlier ones stopped: don't undo what a later answer needs and don't repeat earlier commands.
- files: files the task asks you to write that a session answer then uses, and sample data a program reads, as [{{"name":"<file name>","code":"<code>"}}]. They are saved in the working directory before anything runs; a name with a folder (docs/a.txt) creates that folder. Code files are shown above the answer, data files are not.
- File answers are saved in the working directory as its heading lowercased without punctuation plus the extension, where parts become letters then roman numerals (label 2.2 → 2(b) → q2b.<ext>; 2.1.3 → q2aiii.<ext>). A session in the same group that uses one types that exact name and builds it itself; the file answer is then only shown, not run on its own, so put the commands that show that question's output in a session answer under that same question.
- A program is complete, exercises everything the question asks for, and reads stdin when the question needs user input. stdin: the exact input typed into it, in the order it reads, or "". Read each input once: stdin ends after its last line, so never loop asking again after invalid input. args: the command-line arguments it runs with, or [].
- One run shows every case the question is about (found and not found, valid and invalid input, the error raised and handled), with realistic values. Give a program the input or arguments it is about; never fall back to made-up defaults.
- Name anything a later step refers to.
- Write files only in the working directory with relative paths.
- expects_error: true only when the error is what the question wants shown (given code that fails, an invalid case it asks to demonstrate). Any other error is taken as your mistake and sent back to be fixed.
- Code given in the question to run, test, analyse or correct is used verbatim with expects_error true; if it fails, a corrected version follows as another answer. Whether given code works and what is wrong with it is answer_after_run, never answer.
- Most answers have answer "" and answer_after_run false. Never describe the code, the steps or the output unless the question asks for it.
- answer: shown below the screenshot. Only when the task asks to explain, compare, identify, justify, list or write something that doesn't depend on the output (an essay, report or email goes here). Plain text only, no markdown. Never put code here. Otherwise "".
- answer_after_run: true only when the question asks for something that depends on the actual output (observe the behaviour, compare counts, analyse the output). It is written after the run and shown below the screenshot; don't repeat it in answer."#)
}

fn earlier_answers(earlier: &str) -> String {
    if earlier.is_empty() { String::new() } else {
        format!("\n\nEarlier questions of this group are already answered, and their sessions ran first. Continue from them; don't repeat them or their commands:\n{earlier}")
    }
}

/// `solve` step 2: answer one group of questions.
pub fn solve_prompt(group_json: &str, earlier: &str, langs: &str) -> String {
    format!("Complete these tasks. Return ONLY this JSON, no markdown, one entry per question in order:\n{ANSWER_FORMAT}\n\n{}\n\nQuestions:\n{group_json}{}",
        answer_rules(langs), earlier_answers(earlier))
}

/// `solve`: redo questions whose answers went wrong when they ran.
pub fn review_prompt(group_json: &str, answers_json: &str, earlier: &str, langs: &str) -> String {
    format!(r#"These questions' answers went wrong when they ran: each lists its problems, and each answer shows what it printed. Redo every one of them so its problems are gone. Keep the kind of answer it has (written answers stay written, diagrams stay diagrams), what already works, and every name, table, file and object it creates, since later answers use them. Show every case in one run, as the rules say: never split one program into near-duplicate copies, and add no comments.

Return ONLY JSON, no markdown, one entry per question, each with all of its answers, in this format:
{ANSWER_FORMAT}

{}

Questions:
{group_json}

Answers, what they printed and their problems:
{answers_json}{}"#, answer_rules(langs), earlier_answers(earlier))
}

/// `solve --check`: grade each answer from what it printed or drew.
pub fn check_prompt(group_json: &str, answers_json: &str) -> String {
    format!(r#"Grade these answers to the questions below as a strict examiner would, from their code and what each printed; written answers, which don't run, from their answer text; pictures are attached in the same order as the answers that drew them. The setup, when given, already ran before the answers with exactly that data. An answer is bad when what it shows doesn't answer its question: a part or case left out, an empty, trivial or wrong result, an error the question isn't about, or a diagram missing what was asked.

Return ONLY this JSON, no markdown, one entry per answer:
[{{"title":"<question title>","label":"<answer label>","verdict":"good or bad","confidence":<0 to 1>,"problem":"<what is wrong, or empty>"}}]

Questions:
{group_json}

Answers and what they printed:
{answers_json}"#)
}

/// `solve` step 3: write an answer from the real output.
pub fn after_run_prompt(question: &str, code: &str, output: &str) -> String {
    format!("Question:\n{question}\n\nCode:\n{code}\n\nActual output:\n{output}\n\nWrite only what the question asks about this output (an observation, comparison or conclusion), in 2 to 4 sentences. Don't restate the code, list the steps, use headings or write in the first person, and don't comment on the environment or the run. If commands failed and the question isn't about the error, answer from what the question asks without mentioning the failure, the machine or the operating system. Plain text, no markdown.")
}

/// `solve`: fix an answer that failed to render or crashed when run.
pub fn fix_prompt(question: &str, before: &str, code: &str, error: &str) -> String {
    let question = if question.is_empty() { String::new() } else { format!("Question:\n{question}\n\n") };
    let before = if before.is_empty() { String::new() } else {
        format!("It runs in one live session right after these earlier steps, so everything they created already exists: never create it again.\n{before}\n\n")
    };
    format!("{question}{before}This code fails:\n{code}\n\nError:\n{error}\n\nFix the cause of the error so the code still does what it did, in the same language, keeping every step that shows a result (tests, calls, queries, each case it demonstrates). Keep every name it uses (files, tables, databases, variables) and invent none. Return ONLY the fixed, complete code, no markdown.")
}
