//! graychan — moved out of gray core (`mascot.rs`). `/hehe` hands the
//! terminal to `gray-qol hehe`, which prints the PNG downsampled into
//! half-block (`▀`) truecolor runs, or a luminance ASCII ramp when the
//! terminal can't take the block glyphs.

use std::io::{IsTerminal, Write};

const BLOCK: char = '\u{2580}';
const ASCII_RAMP: &[u8] = b" .:-=+*#%@";

type MascotRun = ((u8, u8, u8), (u8, u8, u8), usize);
type AsciiRun = ((u8, u8, u8), char, usize);

const MASCOT_PNG: &[u8] = include_bytes!("../assets/graychan.png");

struct MascotGrid {
    cols: usize,
    rows: usize,
    px: Vec<u8>,
}

impl MascotGrid {
    fn px(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * self.cols + x) * 3;
        (self.px[i], self.px[i + 1], self.px[i + 2])
    }

    fn runs(&self, row: usize) -> Vec<MascotRun> {
        let mut out = Vec::new();
        let mut x = 0;
        while x < self.cols {
            let top = self.px(x, row * 2);
            let bot = self.px(x, row * 2 + 1);
            let mut n = 1;
            while x + n < self.cols
                && self.px(x + n, row * 2) == top
                && self.px(x + n, row * 2 + 1) == bot
            {
                n += 1;
            }
            out.push((top, bot, n));
            x += n;
        }
        out
    }

    fn ascii_runs(&self, row: usize) -> Vec<AsciiRun> {
        let mut out = Vec::new();
        let mut x = 0;
        while x < self.cols {
            let rgb = self.px(x, row);
            let lum = (rgb.0 as usize * 299 + rgb.1 as usize * 587 + rgb.2 as usize * 114) / 1000;
            let ch = ASCII_RAMP[lum * (ASCII_RAMP.len() - 1) / 255] as char;
            let mut n = 1;
            while x + n < self.cols {
                let rgb2 = self.px(x + n, row);
                let lum2 =
                    (rgb2.0 as usize * 299 + rgb2.1 as usize * 587 + rgb2.2 as usize * 114) / 1000;
                if rgb2 != rgb || ASCII_RAMP[lum2 * (ASCII_RAMP.len() - 1) / 255] as char != ch {
                    break;
                }
                n += 1;
            }
            out.push((rgb, ch, n));
            x += n;
        }
        out
    }
}

fn load_scaled(term_cols: u16, term_rows: u16, px_per_cell: usize) -> Option<MascotGrid> {
    let cols = term_cols as usize;
    let rows = term_rows as usize;
    if cols < 20 || rows < 10 {
        return None;
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(MASCOT_PNG), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?.to_rgb8();
    let (iw, ih) = (img.width() as usize, img.height() as usize);
    if iw == 0 || ih == 0 {
        return None;
    }
    let rows_cap = rows.saturating_sub(9).max(8);
    let cols_fit = (rows_cap as f32 * 2.0 * iw as f32 / ih as f32).round() as usize;
    let grid_cols = cols.min(cols_fit).max(8);
    let grid_rows = ((grid_cols as f32 * ih as f32 / iw as f32) / 2.0)
        .round()
        .max(2.0) as usize;
    let scaled = image::imageops::resize(
        &img,
        grid_cols as u32,
        (grid_rows * px_per_cell) as u32,
        image::imageops::FilterType::Lanczos3,
    );
    Some(MascotGrid {
        cols: grid_cols,
        rows: grid_rows,
        px: scaled.into_raw(),
    })
}

fn decode_grid(term_cols: u16, term_rows: u16) -> Option<MascotGrid> {
    load_scaled(term_cols, term_rows, 2)
}

fn decode_ascii_grid(term_cols: u16, term_rows: u16) -> Option<MascotGrid> {
    load_scaled(term_cols, term_rows, 1)
}

/// Truecolor-capable unless NO_COLOR or a dumb terminal says otherwise —
/// the mascot's whole point is the color, so this is a strict check.
fn color_ok() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    !matches!(
        std::env::var("TERM").as_deref().unwrap_or(""),
        "dumb" | "unknown"
    )
}

fn sgr_fg((r, g, b): (u8, u8, u8)) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

fn sgr_bg((r, g, b): (u8, u8, u8)) -> String {
    format!("\x1b[48;2;{r};{g};{b}m")
}

pub fn print_mascot() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    let (cols, rows) = crossterm::terminal::size().unwrap_or((0, 0));
    let color = color_ok();
    let Some(grid) = decode_grid(cols, rows) else {
        return print_ascii_mascot(cols, rows);
    };
    let pad = (cols as usize).saturating_sub(grid.cols) / 2;
    let mut out = std::io::stdout().lock();
    for row in 0..grid.rows {
        let mut line = String::new();
        if pad > 0 {
            line.push_str(&" ".repeat(pad));
        }
        for (top, bot, n) in grid.runs(row) {
            if color {
                line.push_str(&format!(
                    "{}{}{}",
                    sgr_fg(top),
                    sgr_bg(bot),
                    BLOCK.to_string().repeat(n)
                ));
            } else {
                line.push_str(&BLOCK.to_string().repeat(n));
            }
        }
        if color {
            line.push_str("\x1b[0m");
        }
        line.push_str("\r\n");
        let _ = out.write_all(line.as_bytes());
    }
    let _ = out.flush();
    true
}

fn print_ascii_mascot(term_cols: u16, term_rows: u16) -> bool {
    let Some(grid) = decode_ascii_grid(term_cols, term_rows) else {
        return false;
    };
    let color = color_ok();
    let pad = (term_cols as usize).saturating_sub(grid.cols) / 2;
    let mut out = std::io::stdout().lock();
    for row in 0..grid.rows {
        let mut line = String::new();
        if pad > 0 {
            line.push_str(&" ".repeat(pad));
        }
        for (rgb, ch, n) in grid.ascii_runs(row) {
            if color {
                line.push_str(&format!("{}{}", sgr_fg(rgb), ch.to_string().repeat(n)));
            } else {
                line.push_str(&ch.to_string().repeat(n));
            }
        }
        if color {
            line.push_str("\x1b[0m");
        }
        line.push_str("\r\n");
        let _ = out.write_all(line.as_bytes());
    }
    let _ = out.flush();
    true
}
