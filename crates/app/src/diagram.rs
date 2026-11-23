//! Report diagrams: the model only lists nodes and edges, graphviz draws them in one fixed style,
//! so a weak model can't break the syntax or the look.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Diagram {
    /// flowchart, class, er, dfd or usecase.
    pub kind: String,
    /// LR or TB; empty picks one per kind.
    pub direction: String,
    /// Use-case diagrams: the system box around the use cases.
    pub system: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub note: String,
    /// step, decision, start, end, class, entity, relation, process, external, store, actor or usecase.
    pub kind: String,
    pub fields: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: String,
    /// Class diagrams: inherits, has or uses; use cases: include or extend.
    pub kind: String,
}

const LINE: &str = "#c8c8c0";
const NOTE: &str = "#5f5f5f";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Breaks text into lines of about `width` characters at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.len() + word.len() < width => { line.push(' '); line.push_str(word) }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

fn label(title: &str, note: &str) -> String {
    let title = wrap(title, 24).iter().map(|l| esc(l)).collect::<Vec<_>>().join("<BR/>");
    let note = wrap(note, 30).iter().map(|l| esc(l)).collect::<Vec<_>>().join("<BR/>");
    if note.is_empty() { format!("<<B>{title}</B>>") } else { format!("<<B>{title}</B><BR/><FONT POINT-SIZE=\"11\" COLOR=\"{NOTE}\">{note}</FONT>>") }
}

/// A class or entity box: name, then fields, then methods, each part ruled off.
fn record(node: &Node) -> String {
    let rows = |lines: &[String]| lines.iter().map(|l| format!("{}<BR ALIGN=\"LEFT\"/>", esc(l))).collect::<String>();
    let mut html = format!("<<TABLE BORDER=\"1\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"7\" COLOR=\"{LINE}\" STYLE=\"ROUNDED\"><TR><TD BGCOLOR=\"#f4f4f1\"><B>{}</B></TD></TR>", esc(&node.title));
    for part in [&node.fields, &node.methods] {
        if !part.is_empty() { html.push_str(&format!("<HR/><TR><TD ALIGN=\"LEFT\" BALIGN=\"LEFT\">{}</TD></TR>", rows(part))) }
    }
    html + "</TABLE>>"
}

impl Diagram {
    /// Ids the model left out become the title, and edges may name a node by its title.
    fn ids(&self) -> HashMap<String, String> {
        let mut ids = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let id = format!("n{i}");
            for key in [&n.id, &n.title] {
                if !key.trim().is_empty() { ids.entry(key.trim().to_lowercase()).or_insert_with(|| id.clone()); }
            }
        }
        ids
    }

    /// What's wrong with the graph, for the model to fix.
    pub fn problems(&self) -> Vec<String> {
        if self.nodes.is_empty() { return vec!["the diagram has no nodes".into()] }
        let ids = self.ids();
        self.edges.iter()
            .flat_map(|e| [&e.from, &e.to])
            .filter(|end| !ids.contains_key(&end.trim().to_lowercase()))
            .map(|end| format!("an edge points at \"{end}\", which is not a node id"))
            .collect()
    }

    pub fn to_dot(&self, actor_png: &str) -> String {
        let kind = self.kind.to_lowercase();
        let ortho = matches!(kind.as_str(), "flowchart" | "dfd" | "");
        let dir = match self.direction.to_uppercase().as_str() {
            d @ ("LR" | "TB") => d.to_string(),
            _ if kind == "usecase" => "LR".into(),
            _ => "TB".into(),
        };
        let mut dot = format!(
            "digraph G {{\n  graph [rankdir={dir}, bgcolor=\"white\", pad=\"0.3\", nodesep=\"0.5\", ranksep=\"0.55\", splines={}, fontname=\"Helvetica\"];\n  \
             node [fontname=\"Helvetica\", fontsize=13, shape=box, style=\"rounded\", color=\"{LINE}\", penwidth=1.4, margin=\"0.25,0.12\", fontcolor=\"#1a1a1a\"];\n  \
             edge [fontname=\"Helvetica\", fontsize=11, color=\"#b4b4ac\", fontcolor=\"{NOTE}\", arrowsize=0.7, penwidth=1.2];\n",
            if ortho { "ortho" } else { "spline" });
        let mut boxed = String::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let title = if n.title.trim().is_empty() { &n.id } else { &n.title };
            let attrs = match n.kind.to_lowercase().as_str() {
                "decision" | "relation" => format!("shape=diamond, style=\"\", label={}", label(title, &n.note)),
                "end" => format!("style=\"rounded,filled\", fillcolor=\"#e8f0fb\", color=\"#2f6fd0\", penwidth=2, label={}", label(title, &n.note)),
                "class" | "entity" => format!("shape=none, margin=0, label={}", record(&Node { title: title.clone(), ..n.clone() })),
                "process" | "usecase" => format!("shape=ellipse, style=\"\", label={}", label(title, &n.note)),
                "external" => format!("style=\"\", label={}", label(title, &n.note)),
                "store" => format!("shape=none, margin=0, label=<<TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\" CELLPADDING=\"8\" COLOR=\"{LINE}\"><TR><TD SIDES=\"TB\"><B>{}</B></TD></TR></TABLE>>", esc(title)),
                "actor" => format!("shape=none, image=\"{}\", imagescale=false, labelloc=b, height=1.3, label=\"{}\"", esc(actor_png), esc(title)),
                _ => format!("label={}", label(title, &n.note)),
            };
            let line = format!("  n{i} [{attrs}];\n");
            if !self.system.is_empty() && n.kind.eq_ignore_ascii_case("usecase") { boxed.push_str(&line) } else { dot.push_str(&line) }
        }
        if !boxed.is_empty() {
            dot.push_str(&format!("  subgraph cluster_system {{\n    label=\"{}\"; style=\"rounded\"; color=\"{LINE}\"; fontname=\"Helvetica\"; fontsize=13;\n  {boxed}  }}\n", esc(&self.system)));
        }
        let ids = self.ids();
        for e in &self.edges {
            let (Some(from), Some(to)) = (ids.get(&e.from.trim().to_lowercase()), ids.get(&e.to.trim().to_lowercase())) else { continue };
            let mut attrs = match (kind.as_str(), e.kind.to_lowercase().as_str()) {
                (_, "inherits") => vec!["arrowhead=empty".to_string()],
                (_, "has") => vec!["dir=back".into(), "arrowtail=diamond".into()],
                (_, "uses" | "include" | "extend") => vec!["style=dashed".into()],
                ("er" | "usecase", _) => vec!["dir=none".into()],
                _ => vec![],
            };
            if !e.label.is_empty() { attrs.push(format!("{}=\"{}\"", if ortho { "xlabel" } else { "label" }, esc(&e.label))) }
            dot.push_str(&format!("  {from} -> {to} [{}];\n", attrs.join(", ")));
        }
        dot + "}\n"
    }

    pub fn render(&self) -> Result<Vec<u8>> {
        let dir = tempfile::tempdir()?;
        let actor = dir.path().join("actor.png");
        if self.nodes.iter().any(|n| n.kind.eq_ignore_ascii_case("actor")) { stick_figure().save(&actor)? }
        dot_png(&self.to_dot(&actor.to_string_lossy()))
    }
}

fn dot_png(dot: &str) -> Result<Vec<u8>> {
    let mut child = Command::new("dot").args(["-Tpng", "-Gdpi=150"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().context("graphviz (dot) is not installed")?;
    child.stdin.take().context("no stdin")?.write_all(dot.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() || out.stdout.is_empty() { bail!("graphviz failed: {}", String::from_utf8_lossy(&out.stderr).trim()) }
    Ok(out.stdout)
}

/// The UML actor, drawn once per render since graphviz has no such shape.
fn stick_figure() -> image::RgbaImage {
    use imageproc::drawing::{draw_hollow_circle_mut, draw_line_segment_mut};
    let mut img = image::RgbaImage::from_pixel(70, 100, image::Rgba([255, 255, 255, 0]));
    let ink = image::Rgba([90, 90, 90, 255]);
    for d in [-0.6f32, 0.0, 0.6] {
        draw_hollow_circle_mut(&mut img, (35, 16), 12 + d.round() as i32, ink);
        for ((x1, y1), (x2, y2)) in [((35., 28.), (35., 62.)), ((12., 40.), (58., 40.)), ((35., 62.), (16., 92.)), ((35., 62.), (54., 92.))] {
            draw_line_segment_mut(&mut img, (x1 + d, y1), (x2 + d, y2), ink);
            draw_line_segment_mut(&mut img, (x1, y1 + d), (x2, y2 + d), ink);
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(json: &str) -> Diagram {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn edges_find_nodes_by_id_or_title() {
        let d = graph(r#"{"nodes":[{"id":"a","title":"Start"},{"title":"Read input"}],"edges":[{"from":"a","to":"read input"},{"from":"a","to":"nowhere"}]}"#);
        assert_eq!(d.problems(), ["an edge points at \"nowhere\", which is not a node id"]);
        let dot = d.to_dot("");
        assert!(dot.contains("n0 -> n1") && !dot.contains("nowhere"));
    }

    #[test]
    fn labels_are_escaped() {
        let d = graph(r#"{"nodes":[{"title":"a < b","note":"x\" onclick"}]}"#);
        assert!(d.to_dot("").contains("a &lt; b") && !d.to_dot("").contains("x\" onclick"));
    }

    #[test]
    fn every_kind_renders() {
        if Command::new("dot").arg("-V").output().is_err() { return }
        for json in [
            r#"{"kind":"flowchart","nodes":[{"id":"s","title":"Start"},{"id":"q","title":"Valid?","kind":"decision"},{"id":"e","title":"Done","note":"saved","kind":"end"}],"edges":[{"from":"s","to":"q"},{"from":"q","to":"e","label":"yes"}]}"#,
            r#"{"kind":"class","nodes":[{"id":"A","title":"Animal","kind":"class","fields":["-name: String"],"methods":["+speak(): void"]},{"id":"D","title":"Dog","kind":"class"}],"edges":[{"from":"A","to":"D","kind":"inherits"}]}"#,
            r#"{"kind":"usecase","system":"Library","nodes":[{"id":"u","title":"Student","kind":"actor"},{"id":"b","title":"Borrow book","kind":"usecase"}],"edges":[{"from":"u","to":"b"}]}"#,
            r#"{"kind":"dfd","nodes":[{"id":"p","title":"Validate","kind":"process"},{"id":"d","title":"Books","kind":"store"},{"id":"x","title":"Librarian","kind":"external"}],"edges":[{"from":"x","to":"p","label":"request"},{"from":"p","to":"d"}]}"#,
        ] {
            let png = graph(json).render().unwrap();
            assert!(png.starts_with(b"\x89PNG"), "{json}");
        }
    }
}
