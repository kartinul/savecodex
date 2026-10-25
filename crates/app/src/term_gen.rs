use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_filled_rect_mut;
use imageproc::rect::Rect;
use anyhow::Result;

#[derive(Clone, Copy, PartialEq)]
struct Rgb(u8, u8, u8);

#[derive(Clone, Copy, Debug, clap::ValueEnum, Default, PartialEq)]
pub enum WindowStyle {
    #[default]
    Windows,
    Macos,
    #[value(alias = "linux")]
    Ubuntu,
}

impl WindowStyle {
    pub fn padding(self) -> i32 {
        match self {
            WindowStyle::Windows => 11,
            WindowStyle::Macos => 6,
            WindowStyle::Ubuntu => 2,
        }
    }
}

#[derive(Clone)]
pub struct TermGenOptions {
    pub font_size: f32,
    pub theme: String,
    pub prompt: String,
    pub no_prompt_highlight: bool,
    pub padding: i32,
    pub style: WindowStyle,
    pub username: String,
    pub hostname: String,
    pub cwd: String,
    pub messy: u8,
    pub seed: Option<u64>,
    pub banner: bool,
    pub min_cols: u16,
}

impl Default for TermGenOptions {
    fn default() -> Self {
        Self {
            font_size: 18.0,
            theme: "dark".to_string(),
            prompt: "$ ".to_string(),
            no_prompt_highlight: false,
            padding: 8,
            style: WindowStyle::Windows,
            username: "local".to_string(),
            hostname: "host".to_string(),
            cwd: "~".to_string(),
            messy: 0,
            seed: None,
            banner: false,
            min_cols: 80,
        }
    }
}

fn palette(idx: u8, bright: bool) -> Rgb {
    match (idx, bright) {
        (0, false) => Rgb(0x2e, 0x34, 0x36),
        (0, true) => Rgb(0x55, 0x57, 0x53),
        (1, false) => Rgb(0xcc, 0x00, 0x00),
        (1, true) => Rgb(0xef, 0x29, 0x29),
        (2, false) => Rgb(0x4e, 0x9a, 0x06),
        (2, true) => Rgb(0x8a, 0xe2, 0x34),
        (3, false) => Rgb(0xc4, 0xa0, 0x00),
        (3, true) => Rgb(0xfc, 0xe9, 0x4f),
        (4, false) => Rgb(0x34, 0x65, 0xa4),
        (4, true) => Rgb(0x72, 0x9f, 0xcf),
        (5, false) => Rgb(0x75, 0x50, 0x7b),
        (5, true) => Rgb(0xad, 0x7f, 0xa8),
        (6, false) => Rgb(0x06, 0x98, 0x9a),
        (6, true) => Rgb(0x34, 0xe2, 0xe2),
        (7, false) => Rgb(0xd3, 0xd7, 0xcf),
        (7, true) => Rgb(0xee, 0xee, 0xec),
        _ => Rgb(0xd3, 0xd7, 0xcf),
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: i32) -> i32 {
        (self.next() % n.max(1) as u64) as i32
    }

    fn chance(&mut self, pct: i32) -> bool {
        self.below(100) < pct
    }
}

struct Run {
    text: String,
    color: Rgb,
    bold: bool,
}

fn parse_ansi_line(line: &str, default_fg: Rgb) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut cur = String::new();
    let mut fg = default_fg;
    let mut bold = false;
    let mut default_color = default_fg;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let flush = |runs: &mut Vec<Run>, cur: &mut String, fg: Rgb, bold: bool| {
        if !cur.is_empty() {
            runs.push(Run { text: std::mem::take(cur), color: fg, bold });
        }
    };
    while i < chars.len() {
        if chars[i] == '\u{1b}' && i + 1 < chars.len() && chars[i + 1] == '[' {
            let mut j = i + 2;
            while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j < chars.len() {
                let term = chars[j];
                let params: String = chars[i + 2..j].iter().collect();
                if term == 'm' {
                    flush(&mut runs, &mut cur, fg, bold);
                    let codes: Vec<i32> = if params.is_empty() {
                        vec![0]
                    } else {
                        params.split(';').filter_map(|s| s.parse().ok()).collect()
                    };
                    for code in codes {
                        match code {
                            0 => {
                                fg = default_color;
                                bold = false;
                            }
                            1 => bold = true,
                            22 => bold = false,
                            30..=37 => fg = palette((code - 30) as u8, bold),
                            90..=97 => fg = palette((code - 90) as u8, true),
                            39 => fg = default_color,
                            _ => {}
                        }
                    }
                }
                i = j + 1;
                continue;
            }
        }
        
        // Skip unprintable control characters (like \r, \x04, etc.) to prevent missing glyph boxes
        if chars[i].is_control() && chars[i] != '\u{1b}' {
            i += 1;
            continue;
        }
        
        cur.push(chars[i]);
        i += 1;
    }
    flush(&mut runs, &mut cur, fg, bold);
    let _ = &mut default_color;
    runs
}

fn text_width(font: &impl ab_glyph::Font, scale: PxScale, text: &str) -> i32 {
    let sf = font.as_scaled(scale);
    let mut w = 0.0f32;
    for c in text.chars() {
        w += sf.h_advance(sf.glyph_id(c));
    }
    w.ceil() as i32
}

fn draw_text(img: &mut RgbaImage, color: Rgba<u8>, x: i32, y: i32, scale: PxScale, font: &impl Font, text: &str) {
    let scaled = font.as_scaled(scale);
    let mut caret = x as f32;
    let ascent = scaled.ascent();
    let (w, h) = (img.width() as i32, img.height() as i32);
    for c in text.chars() {
        let glyph_id = scaled.glyph_id(c);
        let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(caret, y as f32 + ascent));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && px < w && py < h {
                    let existing = *img.get_pixel(px as u32, py as u32);
                    // gamma boost mimics ClearType / macOS font smoothing weight
                    let a = cov.clamp(0.0, 1.0).powf(0.7);
                    let blend = |e: u8, c: u8| -> u8 {
                        (e as f32 * (1.0 - a) + c as f32 * a).round() as u8
                    };
                    let out = Rgba([
                        blend(existing[0], color[0]),
                        blend(existing[1], color[1]),
                        blend(existing[2], color[2]),
                        255,
                    ]);
                    img.put_pixel(px as u32, py as u32, out);
                }
            });
        }
        caret += scaled.h_advance(glyph_id);
    }
}

const PS_BANNER: &str = "Windows PowerShell\nCopyright (C) Microsoft Corporation. All rights reserved.\n\n";

fn rgba(c: Rgb) -> Rgba<u8> {
    Rgba([c.0, c.1, c.2, 255])
}

fn blend_px(img: &mut RgbaImage, x: i32, y: i32, c: Rgba<u8>, a: f32) {
    if a <= 0.0 || x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    for i in 0..3 {
        p[i] = (p[i] as f32 * (1.0 - a) + c[i] as f32 * a).round() as u8;
    }
}

fn fill_rect(img: &mut RgbaImage, x: f32, y: f32, w: f32, h: f32, c: Rgba<u8>) {
    let rect = Rect::at(x.round() as i32, y.round() as i32).of_size(w.round().max(1.0) as u32, h.round().max(1.0) as u32);
    draw_filled_rect_mut(img, rect, c);
}

fn fill_circle(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: Rgba<u8>) {
    for y in (cy - r - 1.0) as i32..=(cy + r + 1.0) as i32 {
        for x in (cx - r - 1.0) as i32..=(cx + r + 1.0) as i32 {
            let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            blend_px(img, x, y, c, (r - d + 0.5).clamp(0.0, 1.0));
        }
    }
}

fn stroke(img: &mut RgbaImage, points: &[(f32, f32)], w: f32, c: Rgba<u8>) {
    for seg in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (seg[0], seg[1]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in (y0.min(y1) - w) as i32..=(y0.max(y1) + w) as i32 {
            for x in (x0.min(x1) - w) as i32..=(x0.max(x1) + w) as i32 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
                let d = (px - x0 - t * dx).hypot(py - y0 - t * dy);
                blend_px(img, x, y, c, (w / 2.0 - d + 0.5).clamp(0.0, 1.0));
            }
        }
    }
}

fn cross(img: &mut RgbaImage, cx: f32, cy: f32, half: f32, w: f32, c: Rgba<u8>) {
    stroke(img, &[(cx - half, cy - half), (cx + half, cy + half)], w, c);
    stroke(img, &[(cx - half, cy + half), (cx + half, cy - half)], w, c);
}

fn square(img: &mut RgbaImage, cx: f32, cy: f32, half: f32, w: f32, c: Rgba<u8>) {
    let (l, t, r, b) = (cx - half, cy - half, cx + half, cy + half);
    stroke(img, &[(l, t), (r, t), (r, b), (l, b), (l, t)], w, c);
}

fn draw_text_centered_y(img: &mut RgbaImage, color: Rgba<u8>, x: f32, center_y: f32, scale: PxScale, font: &FontRef, text: &str) {
    let ascent = font.as_scaled(scale).ascent();
    let y = center_y + scale.y * 0.36 - ascent;
    draw_text(img, color, x.round() as i32, y.round() as i32, scale, font, text);
}

fn round_corners(img: &mut RgbaImage, r: f32, bottom: bool, border: Rgba<u8>) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let on_edge = x == 0 || y == 0 || px > w - 1.0 || py > h - 1.0;
        let cx = if px < r { r } else if px > w - r { w - r } else { -1.0 };
        let cy = if py < r { r } else if bottom && py > h - r { h - r } else { -1.0 };
        if cx < 0.0 || cy < 0.0 {
            if on_edge {
                *p = border;
            }
            continue;
        }
        let d = (px - cx).hypot(py - cy);
        let ring = (1.0 - (d - (r - 0.5)).abs()).clamp(0.0, 1.0);
        for i in 0..3 {
            p[i] = (p[i] as f32 * (1.0 - ring) + border[i] as f32 * ring).round() as u8;
        }
        p[3] = (p[3] as f32 * (r - d + 0.5).clamp(0.0, 1.0)).round() as u8;
    }
}

pub fn generate_terminal_image(raw_text: &str, opts: &TermGenOptions) -> Result<RgbaImage> {
    tracing::debug!("generate_terminal_image called, text length: {}, style: {:?}", raw_text.len(), opts.style);
    let style = opts.style;
    let light = opts.theme == "light";
    let banner = if opts.banner && style == WindowStyle::Windows { PS_BANNER } else { "" };
    let raw = format!("{banner}{}", raw_text.trim_end_matches('\n'));

    let (bg, default_fg) = match (style, light) {
        (WindowStyle::Windows, false) => (Rgb(12, 12, 12), Rgb(204, 204, 204)),
        (WindowStyle::Macos, false) => (Rgb(30, 30, 30), Rgb(255, 255, 255)),
        (WindowStyle::Ubuntu, false) => (Rgb(48, 10, 36), Rgb(255, 255, 255)),
        (_, true) => (Rgb(255, 255, 255), Rgb(0, 0, 0)),
    };

    let (font_regular_bytes, font_bold_bytes, title_font_bytes): (&[u8], &[u8], &[u8]) = match style {
        WindowStyle::Windows => (
            include_bytes!("../assets/CascadiaMono.ttf"),
            include_bytes!("../assets/CascadiaMono-Bold.ttf"),
            include_bytes!("../assets/Selawik.ttf"),
        ),
        WindowStyle::Ubuntu => (
            include_bytes!("../assets/UbuntuMono-Regular.ttf"),
            include_bytes!("../assets/UbuntuMono-Bold.ttf"),
            include_bytes!("../assets/Ubuntu-Bold.ttf"),
        ),
        WindowStyle::Macos => (
            include_bytes!("../assets/Meslo.ttf"),
            include_bytes!("../assets/Meslo-Bold.ttf"),
            include_bytes!("../assets/Inter-Bold.ttf"),
        ),
    };

    let font_regular = FontRef::try_from_slice(font_regular_bytes).expect("embedded font invalid");
    let font_bold = FontRef::try_from_slice(font_bold_bytes).expect("embedded font invalid");
    let title_font = FontRef::try_from_slice(title_font_bytes).expect("embedded font invalid");
    let scale = PxScale::from(opts.font_size);

    // chrome proportions measured from real screenshots: (font px, title bar height, line spacing)
    let (base_font, base_title_h, spacing) = match style {
        WindowStyle::Windows => (18.0, 40.0, 1.0),
        WindowStyle::Macos => (16.0, 29.0, 1.0),
        WindowStyle::Ubuntu => (11.0, 18.0, 1.2),
    };
    let u = opts.font_size / base_font;
    let title_bar_h = (base_title_h * u).round() as i32;
    let th = title_bar_h as f32;

    let sf = font_regular.as_scaled(scale);
    let cell_w = sf.h_advance(sf.glyph_id('M'));
    let line_h = (sf.ascent() - sf.descent() + sf.line_gap()) * spacing;

    let cwd = match style {
        WindowStyle::Windows if opts.cwd.contains(":\\") || opts.cwd.starts_with('/') => opts.cwd.clone(),
        WindowStyle::Windows => {
            let home = format!("C:\\Users\\{}", opts.username);
            match opts.cwd.trim_start_matches('~').trim_start_matches('/') {
                "" => home,
                rel => format!("{home}\\{}", rel.replace('/', "\\")),
            }
        }
        WindowStyle::Macos => opts.cwd.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("/").to_string(),
        WindowStyle::Ubuntu => opts.cwd.clone(),
    };
    let prompt_ansi = match style {
        WindowStyle::Macos => format!("{}@{} {} % ", opts.username, opts.hostname, cwd),
        WindowStyle::Windows => format!("PS {}> ", cwd),
        WindowStyle::Ubuntu => format!("\x1b[01;32m{}@{}\x1b[00m:\x1b[01;34m{}\x1b[00m$ ", opts.username, opts.hostname, cwd),
    };

    let mut lines: Vec<Vec<Run>> = Vec::new();
    for line in raw.split('\n') {
        if !opts.no_prompt_highlight && !opts.prompt.is_empty() && line.starts_with(&opts.prompt) {
            let mut runs = parse_ansi_line(&prompt_ansi, default_fg);
            runs.extend(parse_ansi_line(&line[opts.prompt.len()..], default_fg));
            lines.push(runs);
        } else {
            lines.push(parse_ansi_line(line, default_fg));
        }
    }

    let max_line_w = lines
        .iter()
        .map(|runs| runs.iter().map(|r| text_width(if r.bold { &font_bold } else { &font_regular }, scale, &r.text)).sum::<i32>())
        .max()
        .unwrap_or(0);
    let mut content_w = max_line_w.max((cell_w * opts.min_cols as f32).ceil() as i32);

    let title_scale = PxScale::from(match style {
        WindowStyle::Windows => 15.0 * u,
        WindowStyle::Macos => 17.0 * u,
        WindowStyle::Ubuntu => 10.0 * u,
    });
    let title = match style {
        WindowStyle::Windows => "Windows PowerShell".to_string(),
        WindowStyle::Ubuntu => format!("{}@{}: {}", opts.username, opts.hostname, cwd),
        WindowStyle::Macos => {
            let cols = (content_w as f32 / cell_w).round() as usize;
            let rows = ((lines.len() as i32 * line_h as i32 + opts.padding * 2) as f32 / line_h) as usize;
            format!("{} — -zsh — {}×{}", opts.username, cols, rows)
        }
    };
    let title_w = text_width(&title_font, title_scale, &title) as f32;
    let chrome_min_w = match style {
        WindowStyle::Windows => 480.0 * u,
        WindowStyle::Macos => title_w + 180.0 * u,
        WindowStyle::Ubuntu => title_w + 60.0 * u,
    };
    let padding = opts.padding;
    let gutter = if style == WindowStyle::Macos { (16.0 * u).round() as i32 } else { 0 };
    content_w = content_w.max(chrome_min_w as i32 - padding * 2 - gutter);

    let messy = opts.messy.min(3);
    let seed = opts.seed.unwrap_or_else(|| {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut hasher);
        hasher.finish()
    });
    let mut rng = Rng(seed);
    let lh = line_h as i32;
    let mut edge_cut = |max: i32| if messy >= 1 && rng.chance(15 * messy as i32) { 1 + rng.below(max) } else { 0 };
    let top_cut = edge_cut(lh / 3);
    let left_cut = edge_cut(cell_w as i32 / 2);
    let right_cut = edge_cut(cell_w as i32);
    let bottom_cut = edge_cut(lh / 3);
    let top_cut = if top_cut > 0 { padding + top_cut } else { 0 };
    let left_cut = if left_cut > 0 { padding + left_cut } else { 0 };
    let right_cut = if right_cut > 0 { padding + right_cut } else { 0 };
    let bottom_cut = if bottom_cut > 0 { padding + bottom_cut } else { 0 };
    let extra_bottom = if messy >= 1 && bottom_cut == 0 { rng.below(lh * 2) } else { 0 };
    let img_w = content_w + padding * 2 + gutter - left_cut - right_cut;
    let img_h = title_bar_h + padding * 2 + (lines.len() as i32 * lh).max(lh) - top_cut - bottom_cut + extra_bottom;
    let (w, h) = (img_w as f32, img_h as f32);

    let mut img = RgbaImage::from_pixel(img_w as u32, img_h as u32, rgba(bg));

    match style {
        WindowStyle::Macos => {
            let (bar, sep, title_col, gutter_bg, gutter_line, icon) = if light {
                (Rgb(236, 236, 236), Rgb(210, 210, 210), Rgb(77, 77, 77), Rgb(250, 250, 250), Rgb(225, 225, 225), Rgb(130, 130, 130))
            } else {
                (Rgb(54, 54, 59), Rgb(20, 20, 22), Rgb(148, 148, 153), Rgb(44, 45, 50), Rgb(59, 59, 66), Rgb(145, 146, 149))
            };
            fill_rect(&mut img, 0.0, 0.0, w, th, rgba(bar));
            fill_rect(&mut img, 0.0, th - 1.0, w, 1.0, rgba(sep));
            let cy = (th - 1.0) / 2.0;
            for (i, c) in [Rgb(255, 95, 87), Rgb(254, 188, 46), Rgb(40, 200, 64)].into_iter().enumerate() {
                fill_circle(&mut img, (15.5 + 22.0 * i as f32) * u, cy, 6.5 * u, rgba(c));
            }

            let icon_w = 16.0 * u;
            let x = (w - icon_w - 6.0 * u - title_w) / 2.0;
            fill_rect(&mut img, x, cy - 6.5 * u, 7.0 * u, 3.0 * u, rgba(Rgb(70, 160, 220)));
            fill_rect(&mut img, x, cy - 5.0 * u, icon_w, 11.5 * u, rgba(Rgb(70, 175, 230)));
            let (front_y, front_h) = (cy - 3.0 * u, 9.5 * u);
            for row in 0..front_h.ceil() as i32 {
                let t = row as f32 / front_h;
                let shade = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
                fill_rect(&mut img, x, front_y + row as f32, icon_w, 1.0, Rgba([shade(110, 55), shade(205, 160), shade(248, 225), 255]));
            }
            let (hx, hy, house) = (x + 8.0 * u, cy + 1.5 * u, rgba(Rgb(60, 140, 205)));
            stroke(&mut img, &[(hx - 3.5 * u, hy), (hx, hy - 3.0 * u), (hx + 3.5 * u, hy)], 1.2 * u, house);
            fill_rect(&mut img, hx - 2.5 * u, hy, 5.0 * u, 3.0 * u, house);
            fill_rect(&mut img, hx - 0.75 * u, hy + 1.0 * u, 1.5 * u, 2.0 * u, rgba(Rgb(85, 185, 238)));
            draw_text_centered_y(&mut img, rgba(title_col), x + icon_w + 6.0 * u, cy, title_scale, &title_font, &title);

            let gx = w - gutter as f32;
            fill_rect(&mut img, gx, th, gutter as f32, h - th, rgba(gutter_bg));
            fill_rect(&mut img, gx, th, 1.0, h - th, rgba(gutter_line));
            let (ix, iy) = (gx + 3.5 * u, th + 4.0 * u);
            stroke(&mut img, &[(ix, iy), (ix + 9.0 * u, iy), (ix + 9.0 * u, iy + 7.0 * u), (ix, iy + 7.0 * u), (ix, iy)], u, rgba(icon));
            stroke(&mut img, &[(ix, iy + 3.5 * u), (ix + 9.0 * u, iy + 3.5 * u)], u, rgba(icon));

            round_corners(&mut img, 10.0 * u, true, rgba(if light { Rgb(180, 180, 180) } else { Rgb(81, 81, 81) }));
        }
        WindowStyle::Windows => {
            let (bar, tab, fg, dim, sep) = if light {
                (Rgb(238, 238, 238), Rgb(255, 255, 255), Rgb(0, 0, 0), Rgb(90, 90, 90), Rgb(200, 200, 200))
            } else {
                (Rgb(46, 46, 46), bg, Rgb(255, 255, 255), Rgb(200, 200, 200), Rgb(75, 75, 75))
            };
            fill_rect(&mut img, 0.0, 0.0, w, th, rgba(bar));

            let (tx, ty, tw, tr) = (10.0 * u, 8.0 * u, 235.0 * u, 7.0 * u);
            fill_circle(&mut img, tx + tr, ty + tr, tr, rgba(tab));
            fill_circle(&mut img, tx + tw - tr, ty + tr, tr, rgba(tab));
            fill_rect(&mut img, tx + tr, ty, tw - 2.0 * tr, tr, rgba(tab));
            fill_rect(&mut img, tx, ty + tr, tw, th - ty - tr, rgba(tab));

            let cy = ty + (th - ty) / 2.0;
            let (ix, ih) = (20.0 * u, 12.0 * u);
            for row in 0..ih.ceil() as i32 {
                let skew = (ih - row as f32) * 0.25;
                fill_rect(&mut img, ix + skew, cy - ih / 2.0 + row as f32, 14.0 * u, 1.0, rgba(Rgb(36, 108, 220)));
            }
            stroke(&mut img, &[(ix + 5.0 * u, cy - 3.0 * u), (ix + 8.0 * u, cy), (ix + 5.0 * u, cy + 3.0 * u)], 1.4 * u, rgba(Rgb(255, 255, 255)));
            stroke(&mut img, &[(ix + 9.0 * u, cy + 3.0 * u), (ix + 12.0 * u, cy + 3.0 * u)], 1.4 * u, rgba(Rgb(255, 255, 255)));
            draw_text_centered_y(&mut img, rgba(fg), 46.0 * u, cy, title_scale, &title_font, &title);
            cross(&mut img, 229.0 * u, cy, 4.5 * u, u, rgba(dim));

            stroke(&mut img, &[(266.0 * u, cy), (278.0 * u, cy)], u, rgba(dim));
            stroke(&mut img, &[(272.0 * u, cy - 6.0 * u), (272.0 * u, cy + 6.0 * u)], u, rgba(dim));
            fill_rect(&mut img, 287.0 * u, cy - 8.0 * u, 1.0, 16.0 * u, rgba(sep));
            stroke(&mut img, &[(297.0 * u, cy - 2.0 * u), (301.0 * u, cy + 2.0 * u), (305.0 * u, cy - 2.0 * u)], u, rgba(dim));

            let cy = th / 2.0;
            stroke(&mut img, &[(w - 121.0 * u, cy), (w - 111.0 * u, cy)], u, rgba(fg));
            square(&mut img, w - 69.0 * u, cy, 5.0 * u, u, rgba(fg));
            cross(&mut img, w - 23.0 * u, cy, 5.0 * u, u, rgba(fg));

            round_corners(&mut img, 8.0 * u, true, rgba(if light { Rgb(190, 190, 190) } else { Rgb(85, 85, 85) }));
        }
        WindowStyle::Ubuntu => {
            let (top, bottom) = ((87.0, 87.0, 79.0), (64.0, 63.0, 60.0));
            for y in 0..title_bar_h {
                let t = y as f32 / th;
                let mix = |a: f32, b: f32| (a + (b - a) * t) as u8;
                fill_rect(&mut img, 0.0, y as f32, w, 1.0, Rgba([mix(top.0, bottom.0), mix(top.1, bottom.1), mix(top.2, bottom.2), 255]));
            }
            let cy = th / 2.0;
            let (r, glyph) = (5.0 * u, rgba(Rgb(50, 48, 45)));
            let button = |img: &mut RgbaImage, x: f32, base: Rgb, shine: Rgb| {
                fill_circle(img, x * u, cy, r, rgba(base));
                fill_circle(img, x * u, cy - 1.2 * u, r - 1.6 * u, rgba(shine));
            };
            button(&mut img, 9.0, Rgb(205, 66, 18), Rgb(240, 119, 70));
            button(&mut img, 21.0, Rgb(90, 90, 84), Rgb(135, 135, 127));
            button(&mut img, 33.0, Rgb(90, 90, 84), Rgb(135, 135, 127));
            cross(&mut img, 9.0 * u, cy, 1.8 * u, u, rgba(Rgb(70, 30, 15)));
            stroke(&mut img, &[(19.0 * u, cy), (23.0 * u, cy)], u, glyph);
            square(&mut img, 33.0 * u, cy, 2.0 * u, u, glyph);
            draw_text_centered_y(&mut img, rgba(Rgb(223, 219, 210)), 44.0 * u, cy, title_scale, &title_font, &title);

            round_corners(&mut img, 5.0 * u, false, rgba(Rgb(40, 40, 40)));
        }
    }

    let chrome = (messy >= 1).then(|| img.clone());
    let selection = (messy >= 3 && !lines.is_empty()).then(|| {
        let line = rng.below(lines.len() as i32) as usize;
        let len: i32 = lines[line].iter().map(|r| r.text.chars().count() as i32).sum();
        let start = rng.below(len.max(1));
        (line, start, 1 + rng.below((len - start).max(1)))
    });
    let mut cursor = (padding - left_cut, 0);
    let mut y = title_bar_h + padding - top_cut;
    for (line_idx, runs) in lines.iter().enumerate() {
        let mut x = padding - left_cut;
        if let Some((line, start, len)) = selection {
            if line == line_idx {
                let rect = Rect::at(x + (start as f32 * cell_w) as i32, y).of_size((len as f32 * cell_w) as u32, lh as u32);
                draw_filled_rect_mut(&mut img, rect, Rgba([58, 92, 150, 255]));
            }
        }
        for r in runs {
            let f: &FontRef = if r.bold { &font_bold } else { &font_regular };
            draw_text(&mut img, rgba(r.color), x, y, scale, f, &r.text);
            x += text_width(f, scale, &r.text);
        }
        cursor = (x, y);
        y += lh;
    }

    let (cx, cy) = (cursor.0 as f32, cursor.1 as f32);
    match style {
        WindowStyle::Windows => fill_rect(&mut img, cx, cy, u.round(), line_h, rgba(if light { Rgb(0, 0, 0) } else { Rgb(248, 248, 248) })),
        WindowStyle::Macos => fill_rect(&mut img, cx, cy, cell_w, line_h, rgba(if light { Rgb(127, 127, 127) } else { Rgb(161, 161, 161) })),
        WindowStyle::Ubuntu => fill_rect(&mut img, cx, cy, cell_w, line_h, rgba(default_fg)),
    }

    let Some(chrome) = chrome else { return Ok(img) };

    for (px, py, pixel) in img.enumerate_pixels_mut() {
        if py < title_bar_h as u32 {
            *pixel = *chrome.get_pixel(px, py);
        }
    }
    if messy >= 2 && rng.chance(50) {
        let thumb_h = lh * (2 + rng.below(6));
        let top = title_bar_h + 4 + rng.below((img_h - title_bar_h - thumb_h - 8).max(1));
        draw_filled_rect_mut(&mut img, Rect::at(img_w - 9, top).of_size(5, thumb_h as u32), Rgba([128, 128, 128, 140]));
    }
    if messy >= 2 && rng.chance(40) {
        for pixel in img.rows_mut().take(title_bar_h as usize).flatten() {
            let luma = (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3;
            for c in 0..3 {
                pixel[c] = ((pixel[c] as u32 * 45 + luma * 55) / 100) as u8;
            }
        }
    }
    if messy >= 3 {
        img = image::imageops::blur(&img, 0.6);
    }
    Ok(img)
}
