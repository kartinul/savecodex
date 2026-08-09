use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut, draw_hollow_rect_mut};
use imageproc::rect::Rect;
use anyhow::Result;

#[derive(Clone, Copy, PartialEq)]
struct Rgb(u8, u8, u8);

#[derive(Clone, Copy, Debug, clap::ValueEnum, Default, PartialEq)]
pub enum WindowStyle {
    #[default]
    Windows,
    Macos,
    Linux,
}

pub struct TermGenOptions {
    pub title: String,
    pub font_size: f32,
    pub theme: String,
    pub prompt: String,
    pub no_prompt_highlight: bool,
    pub padding: i32,
    pub style: WindowStyle,
    pub username: String,
    pub hostname: String,
    pub cwd: String,
}

impl Default for TermGenOptions {
    fn default() -> Self {
        Self {
            title: "bash".to_string(),
            font_size: 18.0,
            theme: "dark".to_string(),
            prompt: "$ ".to_string(),
            no_prompt_highlight: false,
            padding: 28,
            style: WindowStyle::Windows,
            username: "local".to_string(),
            hostname: "host".to_string(),
            cwd: "~".to_string(),
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
                    let a = cov.clamp(0.0, 1.0);
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

pub fn generate_terminal_image(raw_text: &str, opts: &TermGenOptions) -> Result<RgbaImage> {
    let raw = raw_text.trim_end_matches('\n');

    let (mut bg, mut title_bg, default_fg, _prompt_color, _chrome_border) = if opts.theme == "light" {
        (Rgb(255, 255, 255), Rgb(235, 235, 235), Rgb(0, 0, 0), Rgb(0, 0, 0), Rgb(0, 0, 0)) // Apple Terminal "Basic" profile
    } else {
        (Rgb(30, 30, 30), Rgb(45, 45, 45), Rgb(242, 242, 242), Rgb(0, 0, 0), Rgb(0, 0, 0)) // Apple Terminal "Pro" profile approximation
    };

    if opts.style == WindowStyle::Windows && opts.theme != "light" {
        bg = Rgb(12, 12, 12); // Pitch black for authentic Windows Terminal
        title_bg = Rgb(45, 45, 45);
    }

    if opts.style == WindowStyle::Macos && opts.theme != "light" {
        bg = Rgb(0, 0, 0); // Pure black for Apple's Pro theme body
        title_bg = Rgb(38, 38, 38); // Dark translucent header bar
    }

    let (font_regular_bytes, font_bold_bytes) = match opts.style {
        WindowStyle::Windows => (
            include_bytes!("../assets/font-regular.ttf").as_slice(),
            include_bytes!("../assets/font-bold.ttf").as_slice(),
        ),
        WindowStyle::Linux => {
            (
                include_bytes!("../assets/UbuntuMono-Regular.ttf").as_slice(),
                include_bytes!("../assets/UbuntuMono-Bold.ttf").as_slice(),
            )
        },
        WindowStyle::Macos => {
            (
                include_bytes!("../assets/Meslo.ttf").as_slice(),
                include_bytes!("../assets/Meslo-Bold.ttf").as_slice(),
            )
        },
    };

    let font_regular = FontRef::try_from_slice(font_regular_bytes).expect("embedded font invalid");
    let font_bold = FontRef::try_from_slice(font_bold_bytes).expect("embedded font invalid");
    let scale = PxScale::from(opts.font_size);

    let sf = font_regular.as_scaled(scale);
    let cell_w = sf.h_advance(sf.glyph_id('M'));
    let line_h = (sf.ascent() - sf.descent() + sf.line_gap()) * 1.35;

    let mut lines: Vec<Vec<Run>> = Vec::new();
    for line in raw.split('\n') {
        if !opts.no_prompt_highlight && !opts.prompt.is_empty() && line.starts_with(&opts.prompt) {
            let rest = &line[opts.prompt.len()..];
            
            let cwd_display = if opts.style == WindowStyle::Windows && opts.cwd == "~" {
                format!("C:\\Users\\{}", opts.username)
            } else if opts.style == WindowStyle::Macos && opts.cwd == "~" {
                "~".to_string()
            } else {
                opts.cwd.clone()
            };
            
            let real_prompt_ansi = match opts.style {
                WindowStyle::Macos => format!("\x1b[36m{}@{}\x1b[0m \x1b[34m{}\x1b[0m % ", opts.username, opts.hostname, cwd_display),
                WindowStyle::Windows => format!("\x1b[93mPS {}>\x1b[0m ", cwd_display),
                WindowStyle::Linux => format!("\x1b[32m{}@{}\x1b[0m:\x1b[34m{}\x1b[0m$ ", opts.username, opts.hostname, cwd_display),
            };
            
            let mut runs = parse_ansi_line(&real_prompt_ansi, default_fg);
            runs.extend(parse_ansi_line(rest, default_fg));
            lines.push(runs);
        } else {
            lines.push(parse_ansi_line(line, default_fg));
        }
    }

    let mut max_line_w = 0i32;
    for runs in &lines {
        let mut w = 0i32;
        for r in runs {
            let f_scale = if r.bold { &font_bold } else { &font_regular };
            w += text_width(f_scale, scale, &r.text);
        }
        max_line_w = max_line_w.max(w);
    }
    let min_title_w = text_width(&font_bold, PxScale::from(opts.font_size * 0.85), &opts.title) + 160;
    let content_w = max_line_w.max(min_title_w).max(cell_w as i32 * 20);

    let (title_bar_h, content_padding_top) = match opts.style {
        WindowStyle::Windows => (42i32, 0i32),
        _ => (40i32, 0i32),
    };
    
    let padding = opts.padding;
    let img_w = content_w + padding * 2;
    let img_h = title_bar_h + padding * 2 + content_padding_top + (lines.len() as i32 * line_h as i32).max(line_h as i32);

    let mut img = RgbaImage::from_pixel(img_w as u32, img_h as u32, Rgba([bg.0, bg.1, bg.2, 255]));

    let (border_col, _tab_bg, _tab_active) = if opts.theme == "light" {
        (Rgba([200, 200, 200, 255]), Rgba([243, 243, 243, 255]), Rgba([255, 255, 255, 255]))
    } else {
        (Rgba([51, 51, 51, 255]), Rgba([32, 32, 32, 255]), Rgba([12, 12, 12, 255]))
    };

    match opts.style {
        WindowStyle::Macos => {
            draw_filled_rect_mut(&mut img, Rect::at(0, 0).of_size(img_w as u32, title_bar_h as u32), Rgba([title_bg.0, title_bg.1, title_bg.2, 255]));
            let dot_y = title_bar_h / 2;
            let dot_r = 6i32;
            let dots = [(0xff, 0x5f, 0x56u8), (0xff, 0xbd, 0x2e), (0x27, 0xc9, 0x3f)];
            for (idx, (r, g, b)) in dots.iter().enumerate() {
                let cx = 20 + idx as i32 * 20;
                draw_filled_circle_mut(&mut img, (cx, dot_y), dot_r, Rgba([*r, *g, *b, 255]));
            }
            let title_scale = PxScale::from(opts.font_size * 0.85);
            let display_title = if opts.title == "bash" { "zsh" } else { &opts.title };
            let title_w = text_width(&font_bold, title_scale, display_title);
            let title_x = (img_w - title_w) / 2;
            let title_col = if opts.theme == "light" { Rgb(0x55, 0x55, 0x55) } else { Rgb(0xbb, 0xbb, 0xbb) };
            let tab_text_y = (title_bar_h - title_scale.y as i32) / 2;
            draw_text(&mut img, Rgba([title_col.0, title_col.1, title_col.2, 255]), title_x, tab_text_y, title_scale, &font_bold, display_title);
        }
        WindowStyle::Windows => {
            let win_tab_bg = if opts.theme == "light" { Rgba([243, 243, 243, 255]) } else { Rgba([28, 28, 28, 255]) };
            let win_active_tab = if opts.theme == "light" { Rgba([255, 255, 255, 255]) } else { Rgba([16, 16, 16, 255]) };

            draw_filled_rect_mut(&mut img, Rect::at(0, 0).of_size(img_w as u32, title_bar_h as u32), win_tab_bg);
            let tab_w = 240.min(img_w - 150);
            
            // active tab
            draw_filled_rect_mut(&mut img, Rect::at(10, 8).of_size(tab_w as u32, (title_bar_h - 8) as u32), win_active_tab);
            
            // round top corners
            draw_filled_rect_mut(&mut img, Rect::at(10, 8).of_size(2, 2), win_tab_bg);
            draw_filled_rect_mut(&mut img, Rect::at(10 + tab_w - 2, 8).of_size(2, 2), win_tab_bg);
            draw_filled_rect_mut(&mut img, Rect::at(11, 9).of_size(1, 1), win_active_tab);
            draw_filled_rect_mut(&mut img, Rect::at(10 + tab_w - 2, 9).of_size(1, 1), win_active_tab);

            let title_scale = PxScale::from(opts.font_size * 0.75);
            let title_col = if opts.theme == "light" { Rgb(0x11, 0x11, 0x11) } else { Rgb(0xdd, 0xdd, 0xdd) };
            
            let display_title = if opts.title == "bash" { "Windows PowerShell" } else { &opts.title };
            
            // Center text perfectly in the tab height (using title_scale.y instead of full font_size)
            let tab_text_y = 8 + (title_bar_h - 8 - title_scale.y as i32) / 2;
            draw_text(&mut img, Rgba([title_col.0, title_col.1, title_col.2, 255]), 32, tab_text_y, title_scale, &font_regular, display_title);

            let tab_x_icon_col = if opts.theme == "light" { Rgba([100, 100, 100, 255]) } else { Rgba([150, 150, 150, 255]) };
            let tab_x_cx = 10 + tab_w - 20;
            let tab_x_cy = 8 + (title_bar_h - 8) / 2;
            draw_line_segment_mut(&mut img, ((tab_x_cx - 4) as f32, (tab_x_cy - 4) as f32), ((tab_x_cx + 4) as f32, (tab_x_cy + 4) as f32), tab_x_icon_col);
            draw_line_segment_mut(&mut img, ((tab_x_cx - 4) as f32, (tab_x_cy + 4) as f32), ((tab_x_cx + 4) as f32, (tab_x_cy - 4) as f32), tab_x_icon_col);

            let ctrl_w = 46;
            let right = img_w;
            let icon_col = if opts.theme == "light" { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) };
            draw_line_segment_mut(&mut img, ((right - ctrl_w * 3 + 18) as f32, (title_bar_h / 2) as f32), ((right - ctrl_w * 2 - 18) as f32, (title_bar_h / 2) as f32), icon_col);
            draw_hollow_rect_mut(&mut img, Rect::at(right - ctrl_w * 2 + 18, title_bar_h / 2 - 5).of_size(10, 10), icon_col);
            let cx = right - ctrl_w + 23;
            let cy = title_bar_h / 2;
            draw_line_segment_mut(&mut img, ((cx - 5) as f32, (cy - 5) as f32), ((cx + 5) as f32, (cy + 5) as f32), icon_col);
            draw_line_segment_mut(&mut img, ((cx - 5) as f32, (cy + 5) as f32), ((cx + 5) as f32, (cy - 5) as f32), icon_col);
        }
        WindowStyle::Linux => {
            let hb_bg = if opts.theme == "light" { Rgba([235, 235, 235, 255]) } else { Rgba([36, 36, 36, 255]) };
            draw_filled_rect_mut(&mut img, Rect::at(0, 0).of_size(img_w as u32, title_bar_h as u32), hb_bg);
            let title_scale = PxScale::from(opts.font_size * 0.85);
            let display_title = if opts.title == "bash" { "Terminal" } else { &opts.title };
            let title_w = text_width(&font_bold, title_scale, display_title);
            let title_x = (img_w - title_w) / 2;
            let title_col = if opts.theme == "light" { Rgb(0x22, 0x22, 0x22) } else { Rgb(0xee, 0xee, 0xee) };
            let tab_text_y = (title_bar_h - title_scale.y as i32) / 2;
            draw_text(&mut img, Rgba([title_col.0, title_col.1, title_col.2, 255]), title_x, tab_text_y, title_scale, &font_bold, display_title);

            let cx = img_w - 24;
            let cy = title_bar_h / 2;
            let btn_bg = if opts.theme == "light" { Rgba([210, 210, 210, 255]) } else { Rgba([60, 60, 60, 255]) };
            let icon_col = if opts.theme == "light" { Rgba([50, 50, 50, 255]) } else { Rgba([200, 200, 200, 255]) };
            draw_filled_circle_mut(&mut img, (cx, cy), 12, btn_bg);
            draw_line_segment_mut(&mut img, ((cx - 4) as f32, (cy - 4) as f32), ((cx + 4) as f32, (cy + 4) as f32), icon_col);
            draw_line_segment_mut(&mut img, ((cx - 4) as f32, (cy + 4) as f32), ((cx + 4) as f32, (cy - 4) as f32), icon_col);
        }
    }

    draw_hollow_rect_mut(&mut img, Rect::at(0, 0).of_size(img_w as u32 - 1, img_h as u32 - 1), border_col);
    
    if opts.style == WindowStyle::Macos || opts.style == WindowStyle::Linux {
        let r = 10i32;
        for x in 0..img_w {
            for y in 0..img_h {
                let dx = if x < r { r - x } else if x >= img_w - r { x - (img_w - r) + 1 } else { 0 };
                let dy = if y < r { r - y } else if y >= img_h - r { y - (img_h - r) + 1 } else { 0 };
                if dx * dx + dy * dy > r * r {
                    img.put_pixel(x as u32, y as u32, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    let mut y = title_bar_h + padding + content_padding_top;
    for runs in &lines {
        let mut x = padding;
        for r in runs {
            let f: &FontRef = if r.bold { &font_bold } else { &font_regular };
            draw_text(
                &mut img,
                Rgba([r.color.0, r.color.1, r.color.2, 255]),
                x,
                y,
                scale,
                f,
                &r.text,
            );
            x += text_width(f, scale, &r.text);
        }
        y += line_h as i32;
    }

    Ok(img)
}
