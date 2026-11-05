//! `--level`: how professional solved code looks, from 1 (careless student) to 5 (clean).

use std::hash::{DefaultHasher, Hash, Hasher};

/// Naming and layout habits asked of the model at each level.
pub fn rules(level: u8) -> &'static str {
    match level {
        1 => "Write like a careless first-year student: one- or two-letter and meaningless names (a, b, x1, abc, temp2, ans), several declarations on one line, no spaces around operators, the odd stray blank line.",
        2 => "Write like a sloppy student: short names (x, n, arr, num1, flag), spaces around operators sometimes missing.",
        3 => "Write like a student in a hurry: short, generic names (num, arr, res, sum1, temp, i, j), mostly tidy layout with the odd cramped line.",
        4 => "Write tidy code with mostly clear names; one or two may be short or generic.",
        _ => "Write clean code with clear, descriptive names.",
    }
}

/// Odds that a block's indent width is off, and that a single line drifts.
fn odds(level: u8) -> (f64, f64) {
    match level {
        1 => (1.0, 0.4),
        2 => (0.35, 0.2),
        3 => (0.1, 0.1),
        4 => (0.03, 0.02),
        _ => (0.0, 0.0),
    }
}

/// Messes up `code`'s indentation for `level`, the same way every time for the same code.
/// Offside code (Python) only gets whole blocks shifted, so it still runs.
// shortcut: lines inside multi-line strings get reindented too, fine until answers print docstrings.
pub fn indent(code: &str, level: u8, offside: bool) -> String {
    let (block_odds, line_odds) = odds(level);
    if block_odds == 0.0 { return code.to_string() }
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    let mut rng = Rng(hasher.finish());
    let min = offside as usize;
    // (original indent, new indent) of each open block
    let mut stack = vec![(0, 0)];
    let mut messy = code.lines().map(|line| {
        let body = line.trim_start();
        if body.is_empty() { return String::new() }
        let old: usize = line[..line.len() - body.len()].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
        while stack.last().unwrap().0 > old { stack.pop(); }
        let (top_old, top_new) = *stack.last().unwrap();
        let mut new = if top_old == old { top_new } else {
            let step = old - top_old;
            let width = if level == 1 { min.max(rng.below(3)) }
                else if rng.chance(block_odds) { (step + rng.below(5)).saturating_sub(2).max(1) }
                else { step };
            stack.push((old, top_new + width));
            top_new + width
        };
        if !offside && rng.chance(line_odds) { new = (new + rng.below(4)).saturating_sub(1) }
        format!("{}{body}", " ".repeat(new))
    }).collect::<Vec<_>>().join("\n");
    if code.ends_with('\n') { messy.push('\n') }
    messy
}

/// Output that looks AI-made: separator or border lines (===, ---, ***) or emojis.
pub fn decorated(out: &str) -> bool {
    let emoji = |c: char| matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF);
    out.lines().any(|line| {
        let chars: Vec<char> = line.chars().collect();
        chars.iter().any(|&c| emoji(c)) || chars.windows(3).any(|w| "=-*#~_".contains(w[0]) && w[0] == w[1] && w[1] == w[2])
    })
}

/// Questions whose output is meant to be drawn with symbols.
pub fn draws(question: &str) -> bool {
    let q = question.to_lowercase();
    ["pattern", "pyramid", "triangle", "diamond", "star", "histogram", "table", "tabular", "border", "box"].iter().any(|w| q.contains(w))
}

struct Rng(u64);

impl Rng {
    // splitmix64
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
    fn chance(&mut self, p: f64) -> bool { p > (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PY: &str = "def f(n):\n    if n > 0:\n        for i in range(n):\n            print(i)\n        return 1\n    return 0\n\nprint(f(3))\n";

    fn depth_moves(code: &str) -> Vec<std::cmp::Ordering> {
        let indents: Vec<usize> = code.lines().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).collect();
        indents.windows(2).map(|w| w[1].cmp(&w[0])).collect()
    }

    #[test]
    fn offside_keeps_block_structure() {
        for level in 1..=4 {
            let messy = indent(PY, level, true);
            assert_eq!(depth_moves(&messy), depth_moves(PY), "level {level}:\n{messy}");
        }
    }

    #[test]
    fn level_five_is_untouched() {
        assert_eq!(indent(PY, 5, true), PY);
    }

    #[test]
    fn spots_decoration() {
        assert!(decorated("===== Menu =====\n1. Add"));
        assert!(decorated("Done ✅"));
        assert!(!decorated("Enter n: 5\nSum = 15\ni-- done"));
    }
}
