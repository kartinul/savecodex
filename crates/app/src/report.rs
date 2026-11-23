//! Reports and synopses: one `Draft` (cover, questions, outline, written sections) goes from the
//! plan through writing and redos into the docx. The browser and the cli each keep it between steps.

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::chart::Chart;
use crate::diagram::Diagram;
use crate::editor_gen::Snippet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Report,
    Synopsis,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Draft {
    pub kind: Kind,
    /// What the project is, in the user's words.
    pub idea: String,
    pub code_based: bool,
    pub pages: usize,
    /// A contents page after the cover.
    pub toc: bool,
    pub cover: Cover,
    /// Theme fields the format PDF asks for, over the chosen doc theme.
    pub look: Value,
    #[serde(deserialize_with = "skip_bad")]
    pub questions: Vec<Question>,
    #[serde(deserialize_with = "skip_bad")]
    pub runs: Vec<RunSpec>,
    #[serde(deserialize_with = "skip_bad")]
    pub sections: Vec<Section>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Cover {
    pub title: String,
    pub course: String,
    pub code: String,
    pub semester: String,
    pub members: Vec<Member>,
    pub supervisors: Vec<Supervisor>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Member {
    pub name: String,
    pub enrollment: String,
    /// What they did, for the team roles table.
    pub role: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Supervisor {
    /// Dr., Mr., Ms., Mrs. or Prof.
    pub honorific: String,
    pub name: String,
    pub designation: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Question {
    pub ask: String,
    #[serde(deserialize_with = "texts")]
    pub options: Vec<String>,
    /// What "assume" answers.
    #[serde(deserialize_with = "text")]
    pub default: String,
    pub answer: String,
}

/// One program run whose terminal screenshot a section shows.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RunSpec {
    pub id: String,
    /// An uploaded code file, relative to the code folder.
    pub file: String,
    #[serde(deserialize_with = "text")]
    pub stdin: String,
    #[serde(deserialize_with = "texts")]
    pub args: Vec<String>,
    /// What it printed, filled in after it runs.
    pub output: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Section {
    pub title: String,
    /// Abstract, references and appendices go without a number.
    pub numbered: bool,
    /// What the section should say.
    pub brief: String,
    pub words: usize,
    /// text, table, diagram, shot, snippet, chart or upload.
    #[serde(deserialize_with = "texts")]
    pub needs: Vec<String>,
    /// waiting, writing, checking, done or failed.
    pub state: String,
    #[serde(deserialize_with = "skip_bad")]
    pub blocks: Vec<Block>,
}

impl Default for Section {
    fn default() -> Self {
        Self { title: String::new(), numbered: true, brief: String::new(), words: 0, needs: vec![], state: "waiting".into(), blocks: vec![] }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    /// A sub-heading inside the section (numbered 2.1, 2.2, ...).
    Heading { #[serde(deserialize_with = "text")] text: String },
    Para { #[serde(deserialize_with = "text")] text: String },
    Bullets { #[serde(deserialize_with = "texts")] items: Vec<String> },
    Numbered { #[serde(deserialize_with = "texts")] items: Vec<String> },
    Table {
        #[serde(default, deserialize_with = "text")]
        caption: String,
        #[serde(default, deserialize_with = "texts")]
        header: Vec<String>,
        #[serde(default, deserialize_with = "rows")]
        rows: Vec<Vec<String>>,
    },
    Figure(Figure),
}

/// A picture with a caption: exactly one of the sources is set.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Figure {
    #[serde(deserialize_with = "text")]
    pub caption: String,
    #[serde(deserialize_with = "lenient")]
    pub diagram: Option<Diagram>,
    /// A `RunSpec` id: its terminal screenshot.
    pub run: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub snippet: Option<Snippet>,
    #[serde(deserialize_with = "lenient")]
    pub chart: Option<Chart>,
    /// An uploaded image's file name.
    pub upload: Option<String>,
}

/// Models write numbers and booleans where text is asked for.
fn as_text(v: Value) -> String {
    match v {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(as_text(Value::deserialize(d)?))
}

fn texts<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items.into_iter().map(as_text).collect(),
        Value::Null => vec![],
        one => vec![as_text(one)],
    })
}

fn rows<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<String>>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(rows) => rows.into_iter().map(|r| match r {
            Value::Array(cells) => cells.into_iter().map(as_text).collect(),
            one => vec![as_text(one)],
        }).collect(),
        _ => vec![],
    })
}

/// A list where one malformed item is dropped instead of failing the whole reply.
fn skip_bad<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(),
        _ => vec![],
    })
}

fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sloppy_model_json_still_reads() {
        let section: Section = serde_json::from_str(r#"{"title":"Results","blocks":[
            {"type":"para","text":"ok"},
            {"type":"table","caption":"Tests","header":["Input",2],"rows":[["a",1],"b"]},
            {"type":"mystery"},
            {"type":"figure","caption":"Flow","diagram":"not an object"}
        ]}"#).unwrap();
        assert!(section.numbered && section.state == "waiting");
        assert_eq!(section.blocks.len(), 3);
        let Block::Table { header, rows, .. } = &section.blocks[1] else { panic!() };
        assert_eq!((header.clone(), rows.clone()), (vec!["Input".into(), "2".into()], vec![vec!["a".to_string(), "1".into()], vec!["b".into()]]));
        let Block::Figure(f) = &section.blocks[2] else { panic!() };
        assert!(f.diagram.is_none() && f.caption == "Flow");
    }
}
