//! A short piece of code drawn like a dark editor (line numbers, highlighting), for report snippets.

use ab_glyph::{FontRef, PxScale};
use anyhow::Result;
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::term_gen::{draw_text, text_width};

/// Lines `from` to `to` (1-based, inclusive) of an uploaded code file.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Snippet {
    pub file: String,
    pub from: usize,
    pub to: usize,
}

const MAX_LINES: usize = 40;

const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "break", "continue", "return", "try", "catch", "finally", "throw", "throws",
    "class", "struct", "enum", "interface", "extends", "implements", "new", "delete", "public", "private", "protected", "static",
    "final", "const", "void", "int", "long", "short", "char", "float", "double", "bool", "boolean", "string", "auto", "unsigned",
    "import", "package", "include", "using", "namespace", "template", "typename", "virtual", "override", "this", "self", "super",
    "true", "false", "null", "nullptr", "None", "True", "False", "def", "lambda", "in", "is", "not", "and", "or", "pass", "yield",
    "from", "as", "with", "elif", "fn", "let", "mut", "pub", "impl", "match", "use", "mod", "func", "var", "go", "defer", "then",
    "fi", "done", "esac", "function", "local", "echo", "export", "async", "await", "SELECT", "FROM", "WHERE", "INSERT", "INTO",
    "VALUES", "UPDATE", "SET", "CREATE", "TABLE", "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "JOIN", "ON", "ORDER", "BY", "GROUP",
];

const TEXT: Rgba<u8> = Rgba([212, 212, 212, 255]);
const KEYWORD: Rgba<u8> = Rgba([86, 156, 214, 255]);
const STRING: Rgba<u8> = Rgba([206, 145, 120, 255]);
const COMMENT: Rgba<u8> = Rgba([106, 153, 85, 255]);
const NUMBER: Rgba<u8> = Rgba([181, 206, 168, 255]);
const CALL: Rgba<u8> = Rgba([220, 220, 170, 255]);
const TYPE: Rgba<u8> = Rgba([78, 201, 176, 255]);
const GUTTER: Rgba<u8> = Rgba([133, 133, 133, 255]);
const BACK: Rgba<u8> = Rgba([30, 30, 30, 255]);

/// Splits a line into colored pieces. `in_block` carries a `/* */` comment over to the next line.
/// shortcut: a keyword list shared by all languages, not a real grammar; fine for short snippets.
fn tokens(line: &str, hash_comments: bool, in_block: &mut bool) -> Vec<(String, Rgba<u8>)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let rest = |i: usize| chars[i..].iter().collect::<String>();
    while i < chars.len() {
        if *in_block {
            let end = rest(i).find("*/").map(|e| i + rest(i)[..e].chars().count() + 2).unwrap_or(chars.len());
            *in_block = end == chars.len() && !rest(i).contains("*/");
            out.push((chars[i..end].iter().collect(), COMMENT));
            i = end;
            continue;
        }
        let c = chars[i];
        let tail = rest(i);
        if tail.starts_with("//") || tail.starts_with("--") && !hash_comments || (hash_comments && c == '#') {
            out.push((tail, COMMENT));
            break;
        }
        if tail.starts_with("/*") {
            *in_block = true;
            continue;
        }
        let start = i;
        let color = if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != c { i += if chars[i] == '\\' { 2 } else { 1 } }
            i = (i + 1).min(chars.len());
            STRING
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') { i += 1 }
            NUMBER
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1 }
            let word: String = chars[start..i].iter().collect();
            if KEYWORDS.contains(&word.as_str()) { KEYWORD }
            else if chars.get(i) == Some(&'(') { CALL }
            else if word.starts_with(char::is_uppercase) { TYPE }
            else { TEXT }
        } else {
            i += 1;
            TEXT
        };
        out.push((chars[start..i].iter().collect(), color));
    }
    out
}

/// Draws `lines`, numbered from `first`.
pub fn render(lines: &[&str], first: usize, ext: &str) -> Result<Vec<u8>> {
    let font = FontRef::try_from_slice(include_bytes!("../assets/CascadiaMono.ttf"))?;
    let scale = PxScale::from(22.0);
    let lines: Vec<String> = lines.iter().take(MAX_LINES).map(|l| l.replace('\t', "    ")).collect();
    let line_h = 33;
    let pad = 30;
    let gutter = text_width(&font, scale, &format!("{}", first + lines.len())) + 28;
    let widest = lines.iter().map(|l| text_width(&font, scale, l)).max().unwrap_or(0);
    let (w, h) = ((pad * 2 + gutter + widest).max(400) as u32, (pad * 2 + line_h * lines.len() as i32) as u32);
    let mut img = RgbaImage::from_pixel(w, h, BACK);
    let hash_comments = matches!(ext, "py" | "sh" | "bash" | "rb" | "pl" | "r" | "yaml" | "yml" | "toml");
    let mut in_block = false;
    for (k, line) in lines.iter().enumerate() {
        let y = pad + k as i32 * line_h + 4;
        let number = (first + k).to_string();
        draw_text(&mut img, GUTTER, pad + gutter - 28 - text_width(&font, scale, &number), y, scale, &font, &number);
        let mut x = pad + gutter;
        for (text, color) in tokens(line, hash_comments, &mut in_block) {
            draw_text(&mut img, color, x, y, scale, &font, &text);
            x += text_width(&font, scale, &text);
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageOutputFormat::Png)?;
    Ok(png.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(line: &str, hash: bool) -> Vec<(String, Rgba<u8>)> {
        tokens(line, hash, &mut false).into_iter().filter(|(t, _)| !t.trim().is_empty()).collect()
    }

    #[test]
    fn highlights_the_usual_pieces() {
        let got = colors(r#"int n = read("a b"); // note"#, false);
        let find = |t: &str| got.iter().find(|(s, _)| s == t).map(|(_, c)| *c);
        assert_eq!(find("int"), Some(KEYWORD));
        assert_eq!(find("read"), Some(CALL));
        assert_eq!(find("\"a b\""), Some(STRING));
        assert_eq!(find("// note"), Some(COMMENT));
        assert_eq!(colors("x = 1 # hi", true).last().unwrap().1, COMMENT);
    }

    #[test]
    fn block_comments_span_lines() {
        let mut block = false;
        tokens("a /* start", false, &mut block);
        assert!(block);
        let rest = tokens("end */ b", false, &mut block);
        assert!(!block && rest[0] == ("end */".to_string(), COMMENT));
    }

    #[test]
    fn renders_png() {
        assert!(render(&["fn main() {", "    println!(\"hi\");", "}"], 10, "rs").unwrap().starts_with(b"\x89PNG"));
    }
}
