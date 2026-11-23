//! Bar and pie charts for reports. A chart can name a column of an uploaded CSV (a Google Form
//! export); then its values are counted here, never by the model.

use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use anyhow::{bail, Context, Result};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_filled_rect_mut;
use imageproc::rect::Rect;
use serde::{Deserialize, Serialize};

use crate::term_gen::{draw_text, text_width};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Chart {
    /// bar or pie.
    pub kind: String,
    pub labels: Vec<String>,
    pub values: Vec<f64>,
    /// An uploaded CSV and one of its columns: the chart counts each answer in it.
    pub csv: String,
    pub column: String,
}

const PALETTE: [[u8; 3]; 8] = [[68, 114, 196], [237, 125, 49], [165, 165, 165], [255, 192, 0], [91, 155, 213], [112, 173, 71], [38, 68, 120], [158, 72, 14]];
const INK: Rgba<u8> = Rgba([50, 50, 50, 255]);
const MOST: usize = 8;

/// Fields of one CSV line, with quoted fields that hold commas or doubled quotes.
fn fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => { cur.push('"'); chars.next(); }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// How often each answer appears in `column`, most common first; the rest fold into "Other".
/// shortcut: one record per line (no newlines inside quoted cells); fine for form exports.
pub fn count(csv: &str, column: &str) -> Result<(Vec<String>, Vec<f64>)> {
    let mut lines = csv.lines().filter(|l| !l.trim().is_empty());
    let header = fields(lines.next().context("the CSV is empty")?);
    let want = column.trim().to_lowercase();
    let col = header.iter().position(|h| h.trim().to_lowercase() == want)
        .or_else(|| header.iter().position(|h| h.to_lowercase().contains(&want)))
        .with_context(|| format!("no column \"{column}\" in the CSV (columns: {})", header.join(", ")))?;
    let mut counts: Vec<(String, f64)> = Vec::new();
    for line in lines {
        let cell = fields(line).get(col).cloned().unwrap_or_default();
        for answer in cell.split(';').map(str::trim).filter(|a| !a.is_empty()) {
            match counts.iter_mut().find(|(a, _)| a.eq_ignore_ascii_case(answer)) {
                Some((_, n)) => *n += 1.0,
                None => counts.push((answer.to_string(), 1.0)),
            }
        }
    }
    if counts.is_empty() { bail!("column \"{column}\" has no answers") }
    counts.sort_by(|a, b| b.1.total_cmp(&a.1));
    if counts.len() > MOST {
        let other = counts.split_off(MOST - 1).iter().map(|(_, n)| n).sum();
        counts.push(("Other".into(), other));
    }
    Ok(counts.into_iter().unzip())
}

impl Chart {
    /// The labels and values to draw, counted from the CSV in `data` when the chart names one.
    pub fn data(&self, data: &Path) -> Result<(Vec<String>, Vec<f64>)> {
        if !self.csv.is_empty() {
            let name = Path::new(&self.csv).file_name().context("bad csv name")?;
            let text = std::fs::read_to_string(data.join(name)).with_context(|| format!("no uploaded file {}", self.csv))?;
            return count(&text, &self.column);
        }
        let n = self.labels.len().min(self.values.len());
        if n == 0 { bail!("the chart has no data") }
        Ok((self.labels[..n].to_vec(), self.values[..n].to_vec()))
    }

    pub fn render(&self, data: &Path) -> Result<Vec<u8>> {
        let (labels, values) = self.data(data)?;
        let font = FontRef::try_from_slice(include_bytes!("../assets/Selawik.ttf"))?;
        let mut img = RgbaImage::from_pixel(1200, 680, Rgba([255, 255, 255, 255]));
        if self.kind.eq_ignore_ascii_case("pie") { pie(&mut img, &font, &labels, &values) } else { bars(&mut img, &font, &labels, &values) }
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageOutputFormat::Png)?;
        Ok(png.into_inner())
    }
}

fn color(i: usize) -> Rgba<u8> {
    let [r, g, b] = PALETTE[i % PALETTE.len()];
    Rgba([r, g, b, 255])
}

fn number(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v}") } else { format!("{v:.1}") }
}

/// Fits a label in `width` px, cutting it short with "..".
fn fit(font: &FontRef, scale: PxScale, text: &str, width: i32) -> String {
    if text_width(font, scale, text) <= width { return text.into() }
    let mut s = text.to_string();
    while s.chars().count() > 1 && text_width(font, scale, &format!("{s}..")) > width { s.pop(); }
    format!("{}..", s.trim_end())
}

/// The axis top: four steps of 1, 2, 2.5 or 5 times a power of ten that reach `max`.
fn nice_top(max: f64) -> f64 {
    let raw = (max / 4.0).max(1e-9);
    let power = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].into_iter().map(|m| m * power).find(|s| *s >= raw).unwrap_or(10.0 * power);
    step * 4.0
}

fn bars(img: &mut RgbaImage, font: &FontRef, labels: &[String], values: &[f64]) {
    let (small, scale) = (PxScale::from(22.0), PxScale::from(24.0));
    let (left, right, top, bottom) = (90, 1160, 50, 590);
    let max = nice_top(values.iter().cloned().fold(0.0, f64::max));
    for k in 0..=4 {
        let y = bottom - (bottom - top) * k / 4;
        draw_filled_rect_mut(img, Rect::at(left, y).of_size((right - left) as u32, 1), Rgba([225, 225, 225, 255]));
        let tick = number(max * k as f64 / 4.0);
        draw_text(img, INK, left - 14 - text_width(font, small, &tick), y - 12, small, font, &tick);
    }
    let slot = (right - left) / labels.len() as i32;
    let bar = (slot * 3 / 5).min(140);
    for (i, (label, v)) in labels.iter().zip(values).enumerate() {
        let h = ((bottom - top) as f64 * v / max).round() as i32;
        let x = left + slot * i as i32 + (slot - bar) / 2;
        if h > 0 { draw_filled_rect_mut(img, Rect::at(x, bottom - h).of_size(bar as u32, h as u32), color(0)) }
        let value = number(*v);
        draw_text(img, INK, x + (bar - text_width(font, scale, &value)) / 2, bottom - h - 32, scale, font, &value);
        let label = fit(font, small, label, slot - 8);
        draw_text(img, INK, left + slot * i as i32 + (slot - text_width(font, small, &label)) / 2, bottom + 16, small, font, &label);
    }
}

fn pie(img: &mut RgbaImage, font: &FontRef, labels: &[String], values: &[f64]) {
    let scale = PxScale::from(26.0);
    let total: f64 = values.iter().sum::<f64>().max(1e-9);
    let (cx, cy, r) = (330.0f64, 340.0f64, 280.0f64);
    let ends: Vec<f64> = values.iter().scan(0.0, |acc, v| { *acc += v / total; Some(*acc) }).collect();
    for y in (cy - r) as u32..(cy + r) as u32 {
        for x in (cx - r) as u32..(cx + r) as u32 {
            let (dx, dy) = (x as f64 - cx, y as f64 - cy);
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > r { continue }
            // clockwise from 12 o'clock, like Word's pies
            let turn = (dx.atan2(-dy) / std::f64::consts::TAU).rem_euclid(1.0);
            let slice = ends.iter().position(|e| turn < *e).unwrap_or(values.len() - 1);
            let mut c = color(slice);
            c.0[3] = ((r - dist).clamp(0.0, 1.0) * 255.0) as u8;
            let back = img.get_pixel(x, y).0;
            let mix = |a: u8, b: u8| ((a as f64 * c.0[3] as f64 + b as f64 * (255 - c.0[3]) as f64) / 255.0) as u8;
            img.put_pixel(x, y, Rgba([mix(c.0[0], back[0]), mix(c.0[1], back[1]), mix(c.0[2], back[2]), 255]));
        }
    }
    let x = 700;
    let mut y = 340 - labels.len() as i32 * 25;
    for (i, (label, v)) in labels.iter().zip(values).enumerate() {
        draw_filled_rect_mut(img, Rect::at(x, y + 6).of_size(22, 22), color(i));
        let text = fit(font, scale, &format!("{label} ({:.0}%)", v / total * 100.0), 440);
        draw_text(img, INK, x + 36, y, scale, font, &text);
        y += 50;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_form_answers() {
        let csv = "Timestamp,\"How often, roughly?\",Causes\n1,Daily,Exams;Sleep\n2,Weekly,Exams\n3,daily,\"Peers\"\n";
        assert_eq!(count(csv, "how often").unwrap(), (vec!["Daily".to_string(), "Weekly".into()], vec![2.0, 1.0]));
        assert_eq!(count(csv, "Causes").unwrap().0, ["Exams", "Sleep", "Peers"]);
        assert!(count(csv, "missing").is_err());
    }

    #[test]
    fn many_answers_fold_into_other() {
        let csv = std::iter::once("q".to_string()).chain((0..12).map(|i| format!("a{i}"))).collect::<Vec<_>>().join("\n");
        let (labels, values) = count(&csv, "q").unwrap();
        assert_eq!((labels.len(), labels.last().unwrap().as_str(), *values.last().unwrap()), (MOST, "Other", 5.0));
    }

    #[test]
    fn axis_tops_are_round() {
        assert_eq!([nice_top(34.0), nice_top(7.0), nice_top(1.0), nice_top(0.3)], [40.0, 8.0, 1.0, 0.4]);
    }

    #[test]
    fn renders_both_kinds() {
        for kind in ["bar", "pie"] {
            let chart = Chart { kind: kind.into(), labels: vec!["A".into(), "B".into()], values: vec![3.0, 1.0], ..Default::default() };
            assert!(chart.render(Path::new(".")).unwrap().starts_with(b"\x89PNG"));
        }
    }
}
