/// Generates a system prompt for code generation.
pub fn code_generation_prompt(system_prompt: &str, question: &str) -> String {
    format!(
        "System Instructions:\n{}\n\nUser Question:\n{}",
        system_prompt, question
    )
}

/// Generates a prompt for generating input based on a slice of code files (filename, content).
pub fn input_generation_prompt(files: &[(&str, &str)]) -> String {
    let mut prompt = String::from("Analyze the following code files and generate appropriate standard input (stdin) that would successfully execute or test this code.\n\n");
    
    for (filename, content) in files {
        prompt.push_str(&format!("--- {} ---\n{}\n\n", filename, content));
    }
    
    prompt.push_str("Based on the code above, generate ONLY the exact raw text that should be passed to stdin. Do not include markdown formatting or explanations. The output must be ready to pipe directly into the program.\n");
    
    prompt
}
