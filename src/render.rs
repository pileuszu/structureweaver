use crate::domain::{Bounds, CellKind, Grid};
use anyhow::{Context, Result, bail};
use fontdue::{Font, FontSettings};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use std::{io::Cursor, path::PathBuf};

pub fn load_font() -> Result<Font> {
    let mut paths: Vec<PathBuf> = std::env::var_os("STRUCTUREWEAVER_FONT")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    paths.extend(
        [
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "C:/Windows/Fonts/malgun.ttf",
        ]
        .map(PathBuf::from),
    );
    for path in paths {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(font) = Font::from_bytes(bytes, FontSettings::default())
        {
            return Ok(font);
        }
    }
    bail!("No rendering font found. Set STRUCTUREWEAVER_FONT to a Unicode TTF/OTF file")
}
fn text(
    img: &mut RgbImage,
    font: &Font,
    label: &str,
    x: u32,
    y: u32,
    max_width: u32,
    color: [u8; 3],
) {
    let mut pen = x as f32;
    for ch in label.chars() {
        let (m, bitmap) = font.rasterize(ch, 14.0);
        if pen + m.advance_width > (x + max_width) as f32 {
            break;
        }
        for dy in 0..m.height {
            for dx in 0..m.width {
                let px = pen as i32 + m.xmin + dx as i32;
                let py = y as i32 + 17 - m.ymin - m.height as i32 + dy as i32;
                if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                    continue;
                }
                let a = bitmap[dy * m.width + dx] as f32 / 255.;
                let old = img.get_pixel_mut(px as u32, py as u32);
                for (k, color_channel) in color.iter().enumerate() {
                    old.0[k] = (old.0[k] as f32 * (1. - a) + *color_channel as f32 * a) as u8;
                }
            }
        }
        pen += m.advance_width;
    }
}
pub fn column_label(mut col: u32) -> String {
    let mut label = String::new();
    while col > 0 {
        col -= 1;
        label.insert(0, (b'A' + (col % 26) as u8) as char);
        col /= 26;
    }
    label
}
pub fn tile_png(grid: &Grid, bounds: Bounds, font: &Font) -> Result<Vec<u8>> {
    let cw = 150;
    let rh = 28;
    let margin = 64;
    let width = (bounds.right - bounds.left + 1) * cw + margin;
    let height = (bounds.bottom - bounds.top + 1) * rh + rh * 2;
    let mut img = RgbImage::from_pixel(width, height, Rgb([248, 250, 252]));
    text(
        &mut img,
        font,
        "cell types: text blue / number green / formula purple",
        8,
        4,
        width - 16,
        [44, 55, 72],
    );
    for col in bounds.left..=bounds.right {
        text(
            &mut img,
            font,
            &column_label(col),
            margin + (col - bounds.left) * cw + 6,
            rh + 4,
            cw - 12,
            [32, 44, 66],
        );
    }
    let mut labels = Vec::new();
    for row in bounds.top..=bounds.bottom {
        let y = (row - bounds.top + 2) * rh;
        text(
            &mut img,
            font,
            &row.to_string(),
            4,
            y + 3,
            margin - 8,
            [32, 44, 66],
        );
        for col in bounds.left..=bounds.right {
            let x = margin + (col - bounds.left) * cw;
            let merge = grid.merges.iter().find(|m| m.contains(row, col));
            let cell = grid.resolved_cell(row, col);
            let color = match cell {
                Some(c) if c.formula.is_some() => [235, 224, 251],
                Some(c) => match c.kind {
                    CellKind::Number => [222, 245, 230],
                    CellKind::Text => [224, 237, 253],
                    CellKind::Error => [255, 220, 220],
                    _ => [242, 239, 222],
                },
                None => [255, 255, 255],
            };
            for yy in y..(y + rh).min(height) {
                for xx in x..(x + cw).min(width) {
                    img.put_pixel(
                        xx,
                        yy,
                        Rgb(
                            if (yy == y && merge.is_none_or(|m| row == m.top.max(bounds.top)))
                                || (xx == x && merge.is_none_or(|m| col == m.left.max(bounds.left)))
                            {
                                [200, 209, 220]
                            } else {
                                color
                            },
                        ),
                    );
                }
            }
            if let Some(c) = cell
                && merge
                    .is_none_or(|m| row == m.top.max(bounds.top) && col == m.left.max(bounds.left))
            {
                let label = if c.formula.is_some() {
                    format!("ƒ {}", c.value)
                } else {
                    c.value.clone()
                };
                let span = merge.map_or(1, |m| {
                    m.right.min(bounds.right) - m.left.max(bounds.left) + 1
                });
                labels.push((label, x + 5, y + 3, cw * span - 10));
            }
        }
    }
    for (label, x, y, max_width) in labels {
        text(&mut img, font, &label, x, y, max_width, [25, 35, 52]);
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut bytes, ImageFormat::Png)
        .context("Cannot render worksheet tile")?;
    Ok(bytes.into_inner())
}
