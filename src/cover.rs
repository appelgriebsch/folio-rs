//! Portrait EPUB cover. No I/O: the caller supplies a decoded image, if any.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, Rgb, RgbImage};
use unicode_normalization::UnicodeNormalization;

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/IBMPlexSerif-Bold.ttf");

const COVER_W: u32 = 1600;
const COVER_H: u32 = 2560;
const MIN_IMAGE_W: u32 = 400;
const MAX_SCALE: f64 = 1.5;
const PORTRAIT_MIN_H: u32 = 1000;
const IMAGE_TOP: f64 = 0.12;
const IMAGE_BAND: f64 = 0.55;
const MARGIN: f32 = 120.0;
const PUSH_DARK_L: f64 = 0.10;
const JPEG_LIMIT: usize = 500 * 1024;
const TITLE_MAX: f32 = 180.0;
const TITLE_MIN: f32 = 148.0;
const TITLE_STEP: f32 = 8.0;
const MAX_LINES: usize = 4;
const LINE_HEIGHT: f32 = 1.15;
const FOOTER_PX: f32 = 56.0;
const FOOTER_FROM_BOTTOM: f32 = 140.0;
const RULE_PX: i32 = 4;
const RULE_W: i32 = 480;

pub struct Cover {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

enum Placement {
    Portrait,
    Composite,
}

pub fn make_cover(image: Option<DynamicImage>, title: &str, host: &str) -> Cover {
    let site = site_label(host);
    let title = display_title(title, &site);
    let raster = match image.as_ref().filter(|img| img.width() >= MIN_IMAGE_W) {
        Some(img) => {
            let fill = fill_from_image(img);
            let flat = flatten(img, fill);
            let ink = text_ink(fill);
            match classify(flat.width(), flat.height()) {
                Placement::Portrait => render_portrait(&flat),
                Placement::Composite => render_composite(&flat, &title, &site, fill, ink),
            }
        }
        None => {
            let fill = site_colour(&site);
            render_typographic(&title, &site, fill, text_ink(fill))
        }
    };
    let (width, height) = raster.dimensions();
    let bytes = encode_jpeg_capped(&raster);
    Cover {
        bytes,
        width,
        height,
    }
}

fn classify(width: u32, height: u32) -> Placement {
    // h >= 1.3w and h >= 1000.
    if height >= PORTRAIT_MIN_H && u64::from(height) * 10 >= u64::from(width) * 13 {
        Placement::Portrait
    } else {
        Placement::Composite
    }
}

fn dominant_colour(image: &DynamicImage) -> [u8; 3] {
    let small = image.resize_exact(32, 32, FilterType::Triangle).to_rgb8();
    let mut counts = [0u32; 4096];
    let mut sums = [[0u32; 3]; 4096];
    for pixel in small.pixels() {
        let bin = (((pixel.0[0] as usize) >> 4) << 8)
            | (((pixel.0[1] as usize) >> 4) << 4)
            | ((pixel.0[2] as usize) >> 4);
        counts[bin] += 1;
        sums[bin][0] += u32::from(pixel.0[0]);
        sums[bin][1] += u32::from(pixel.0[1]);
        sums[bin][2] += u32::from(pixel.0[2]);
    }
    let mut best = 0;
    for i in 1..4096 {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    let n = counts[best].max(1);
    [
        ((sums[best][0] + n / 2) / n) as u8,
        ((sums[best][1] + n / 2) / n) as u8,
        ((sums[best][2] + n / 2) / n) as u8,
    ]
}

fn fill_from_image(image: &DynamicImage) -> [u8; 3] {
    let rgb = dominant_colour(image);
    if is_flat(rgb) {
        // Flat light image: off-white with black text, not pushed dark (#10, option b).
        if relative_luminance(rgb) > 0.5 {
            return OFF_WHITE;
        }
        return push_dark([0x16, 0x16, 0x16]);
    }
    push_dark(rgb)
}

const OFF_WHITE: [u8; 3] = [0xF4, 0xF1, 0xEA];

fn is_flat(rgb: [u8; 3]) -> bool {
    let max = rgb.into_iter().max().unwrap_or(0);
    let min = rgb.into_iter().min().unwrap_or(0);
    f64::from(max - min) / 255.0 < 0.08
}

fn push_dark(rgb: [u8; 3]) -> [u8; 3] {
    let mut value = rgb.map(f64::from);
    for _ in 0..64 {
        let quantised = quantize(value);
        if relative_luminance(quantised) <= PUSH_DARK_L {
            return quantised;
        }
        value = value.map(|channel| channel * 0.9);
    }
    quantize(value)
}

fn quantize(rgb: [f64; 3]) -> [u8; 3] {
    rgb.map(|channel| channel.round().clamp(0.0, 255.0) as u8)
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let left = relative_luminance(a);
    let right = relative_luminance(b);
    let (hi, lo) = if left >= right {
        (left, right)
    } else {
        (right, left)
    };
    (hi + 0.05) / (lo + 0.05)
}

fn relative_luminance(rgb: [u8; 3]) -> f64 {
    fn linear(channel: u8) -> f64 {
        let s = f64::from(channel) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn text_ink(background: [u8; 3]) -> [u8; 3] {
    let white = [255, 255, 255];
    let black = [0, 0, 0];
    if contrast(background, black) > contrast(background, white) {
        black
    } else {
        white
    }
}

fn site_colour(host: &str) -> [u8; 3] {
    let label = site_label(host);
    let hue = f64::from((fnv1a64(label.as_bytes()) % 360) as u32);
    push_dark(hsl_to_rgb(hue, 0.45, 0.22))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [u8; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let x = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (r, g, b) = match sector.floor() as i32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    quantize([r + m, g + m, b + m].map(|channel| channel * 255.0))
}

fn site_label(host: &str) -> String {
    let lower = host.trim().to_ascii_lowercase();
    let name = lower.strip_prefix("www.").unwrap_or(&lower);
    if name.is_empty() {
        "folio".to_string()
    } else {
        name.to_string()
    }
}

fn display_title(title: &str, site: &str) -> String {
    let text: String = title.trim().nfc().collect();
    if text.is_empty() || !title_drawable(&text) {
        site.to_string()
    } else {
        text
    }
}

fn title_drawable(text: &str) -> bool {
    let font = plex();
    text.chars().all(|ch| {
        if ch.is_whitespace() || ch.is_control() {
            return true;
        }
        if needs_shaping(ch) {
            return false;
        }
        font.glyph_id(ch).0 != 0
    })
}

fn needs_shaping(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0590..=0x05FF | // Hebrew
        0x0600..=0x06FF | // Arabic
        0x0700..=0x074F | // Syriac
        0x0750..=0x077F | // Arabic Supplement
        0x0780..=0x07BF | // Thaana
        0x07C0..=0x07FF | // N'Ko
        0x0860..=0x086F | // Syriac Supplement
        0x08A0..=0x08FF | // Arabic Extended-A
        0x0900..=0x0DFF | // Indic
        0x0E00..=0x0E7F | // Thai
        0x0E80..=0x0EFF | // Lao
        0x0F00..=0x0FFF | // Tibetan
        0x1000..=0x109F | // Myanmar
        0x1780..=0x17FF | // Khmer
        0x1800..=0x18AF | // Mongolian
        0x19E0..=0x19FF | // Khmer symbols
        0xAA60..=0xAA7F | // Myanmar Extended-A
        0xFB1D..=0xFB4F | // Hebrew presentation forms
        0xFB50..=0xFDFF | // Arabic presentation forms-A
        0xFE70..=0xFEFF // Arabic presentation forms-B
    )
}

fn layout_title(title: &str, max_width: f32) -> (f32, Vec<String>) {
    layout_title_within(title, max_width, f32::MAX)
}

/// Horizontal wrap, then a vertical budget (`lines × size × 1.15 ≤ avail`).
/// Shrink 180→148 by 8, then drop lines (ellipsis on a grapheme boundary).
fn layout_title_within(title: &str, max_width: f32, avail: f32) -> (f32, Vec<String>) {
    let text: String = title.nfc().collect();
    let mut size = TITLE_MAX;
    let mut lines;
    loop {
        lines = wrap_lines(&text, size, max_width);
        if lines.len() > MAX_LINES {
            if size > TITLE_MIN {
                size -= TITLE_STEP;
                continue;
            }
            lines.truncate(MAX_LINES);
            ellipsize_last(&mut lines[MAX_LINES - 1], size, max_width);
        }
        if lines.len() as f32 * size * LINE_HEIGHT <= avail || size <= TITLE_MIN {
            break;
        }
        size -= TITLE_STEP;
    }
    while lines.len() as f32 * size * LINE_HEIGHT > avail && lines.len() > 1 {
        lines.pop();
        let last = lines.len() - 1;
        ellipsize_last(&mut lines[last], size, max_width);
    }
    (size, lines)
}

fn wrap_lines(text: &str, size: f32, max_width: f32) -> Vec<String> {
    let font = plex();
    let mut lines = Vec::new();
    let mut current = String::new();
    for (token, mandatory) in break_tokens(text) {
        if !token.is_empty() {
            let addition = if current.is_empty() {
                token.trim_start()
            } else {
                token
            };
            if !addition.is_empty() {
                let trial = format!("{current}{addition}");
                if measure(font, trial.trim_end(), size) <= max_width {
                    current.push_str(addition);
                } else if current.is_empty() {
                    let (mut extra, rest) =
                        split_overflow(addition.trim_start(), font, size, max_width);
                    lines.append(&mut extra);
                    current = rest;
                } else {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        lines.push(trimmed);
                    }
                    current = addition.trim_start().to_string();
                    if measure(font, current.trim_end(), size) > max_width {
                        let (mut extra, rest) = split_overflow(&current, font, size, max_width);
                        lines.append(&mut extra);
                        current = rest;
                    }
                }
            }
        }
        if mandatory {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                lines.push(trimmed);
            }
            current.clear();
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        lines.push(trimmed);
    }
    lines
}

fn break_tokens(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut prev = 0;
    for (idx, opp) in unicode_linebreak::linebreaks(text) {
        let mandatory = matches!(opp, unicode_linebreak::BreakOpportunity::Mandatory);
        if idx > prev || mandatory {
            out.push((&text[prev..idx], mandatory));
        }
        prev = idx;
    }
    out
}

fn split_overflow(
    token: &str,
    font: &FontRef<'_>,
    size: f32,
    max_width: f32,
) -> (Vec<String>, String) {
    let mut lines = Vec::new();
    let mut current = String::new();
    for cluster in clusters(token) {
        let trial = format!("{current}{cluster}");
        if !current.is_empty() && measure(font, &trial, size) > max_width {
            lines.push(std::mem::take(&mut current));
            current = cluster.to_string();
        } else {
            current = trial;
        }
    }
    (lines, current)
}

fn ellipsize_last(line: &mut String, size: f32, max_width: f32) {
    let font = plex();
    let ellipsis = "…";
    let mut base = line.trim_end().to_string();
    while !base.is_empty() && measure(font, &format!("{base}{ellipsis}"), size) > max_width {
        base = trim_one_cluster(&base).trim_end().to_string();
    }
    base.push_str(ellipsis);
    *line = base;
}

/// After NFC, a cluster is a base character plus following combining marks
/// (`ccc != 0`). Extended clusters (ZWJ, flags) are not grouped: those titles
/// fail the coverage check and the cover shows the site name instead.
fn clusters(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if index != start && unicode_normalization::char::canonical_combining_class(ch) == 0 {
            out.push(&text[start..index]);
            start = index;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn trim_one_cluster(text: &str) -> &str {
    let mut end = 0;
    for (index, ch) in text.char_indices().rev() {
        end = index;
        if unicode_normalization::char::canonical_combining_class(ch) == 0 {
            break;
        }
    }
    &text[..end]
}

fn measure(font: &FontRef<'_>, text: &str, size: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut prev: Option<GlyphId> = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(prev) = prev {
            width += scaled.kern(prev, id);
        }
        width += scaled.h_advance(id);
        prev = Some(id);
    }
    width
}

struct CompositeLayout {
    left: u32,
    top: u32,
    dw: u32,
    dh: u32,
    origin: f32,
    size: f32,
    lines: Vec<String>,
    rule_y: i32,
    footer_top: f32,
}

fn render_composite(
    image: &RgbImage,
    title: &str,
    site: &str,
    fill: [u8; 3],
    ink: [u8; 3],
) -> RgbImage {
    let mut canvas = RgbImage::from_pixel(COVER_W, COVER_H, Rgb(fill));
    let layout = composite_layout(image.width(), image.height(), title);
    let resized = imageops::resize(image, layout.dw, layout.dh, FilterType::CatmullRom);
    imageops::overlay(
        &mut canvas,
        &resized,
        i64::from(layout.left),
        i64::from(layout.top),
    );
    paint_text_block(&mut canvas, &layout.lines, layout.size, layout.origin, ink);
    debug_assert!((layout.rule_y + RULE_PX) as f32 <= layout.footer_top);
    paint_rule(&mut canvas, layout.rule_y, ink);
    paint_footer(&mut canvas, site, ink);
    canvas
}

fn footer_top() -> f32 {
    let baseline = COVER_H as f32 - FOOTER_FROM_BOTTOM;
    baseline - plex().as_scaled(PxScale::from(FOOTER_PX)).ascent()
}

fn composite_layout(width: u32, height: u32, title: &str) -> CompositeLayout {
    let max_width = COVER_W as f32 - MARGIN * 2.0;
    let foot = footer_top();
    let (mut left, top, mut dw, mut dh) = composite_box(width, height);
    let mut origin = top as f32 + dh as f32 + 64.0;
    // Title block must end before the 40 px gap, the rule, and the footer.
    let mut avail = foot - 40.0 - RULE_PX as f32 - origin;
    let (mut size, mut lines) = layout_title_within(title, max_width, avail);
    let mut block = lines.len() as f32 * size * LINE_HEIGHT;
    if block > avail {
        // One line at 148 px still does not fit: shorten the band, keep the 12 % top.
        let max_dh = (foot - 40.0 - RULE_PX as f32 - TITLE_MIN * LINE_HEIGHT - 64.0 - top as f32)
            .floor()
            .max(1.0) as u32;
        dh = max_dh.min(dh.saturating_sub(1)).max(1);
        let scale = f64::from(dh) / f64::from(height);
        dw = (f64::from(width) * scale).round().max(1.0) as u32;
        left = COVER_W.saturating_sub(dw) / 2;
        origin = top as f32 + dh as f32 + 64.0;
        avail = foot - 40.0 - RULE_PX as f32 - origin;
        (size, lines) = layout_title_within(title, max_width, avail);
        block = lines.len() as f32 * size * LINE_HEIGHT;
    }
    CompositeLayout {
        left,
        top,
        dw,
        dh,
        origin,
        size,
        lines,
        rule_y: (origin + block + 40.0).floor() as i32,
        footer_top: foot,
    }
}

fn composite_box(width: u32, height: u32) -> (u32, u32, u32, u32) {
    let scale = (f64::from(COVER_W) / f64::from(width))
        .min(f64::from(COVER_H) * IMAGE_BAND / f64::from(height))
        .min(MAX_SCALE);
    let dw = (f64::from(width) * scale).round().max(1.0) as u32;
    let dh = (f64::from(height) * scale).round().max(1.0) as u32;
    let top = (f64::from(COVER_H) * IMAGE_TOP).round() as u32;
    let left = COVER_W.saturating_sub(dw) / 2;
    (left, top, dw, dh)
}

fn render_portrait(image: &RgbImage) -> RgbImage {
    let cropped = centre_crop_16(image);
    let mut out_h = (f64::from(cropped.height()) * MAX_SCALE).round() as u32;
    out_h = out_h.clamp(1000, COVER_H);
    let out_w = (f64::from(out_h) / 1.6).round() as u32;
    imageops::resize(&cropped, out_w, out_h, FilterType::CatmullRom)
}

fn centre_crop_16(image: &RgbImage) -> RgbImage {
    let width = image.width();
    let height = image.height();
    let (crop_w, crop_h) = if u64::from(height) * 5 >= u64::from(width) * 8 {
        let crop_h = ((u64::from(width) * 8) / 5) as u32;
        (width, crop_h.min(height))
    } else {
        let crop_w = ((u64::from(height) * 5) / 8) as u32;
        (crop_w.min(width), height)
    };
    let x = (width - crop_w) / 2;
    let y = (height - crop_h) / 2;
    imageops::crop_imm(image, x, y, crop_w, crop_h).to_image()
}

fn render_typographic(title: &str, site: &str, fill: [u8; 3], ink: [u8; 3]) -> RgbImage {
    let mut canvas = RgbImage::from_pixel(COVER_W, COVER_H, Rgb(fill));
    let (size, lines) = layout_title(title, COVER_W as f32 - MARGIN * 2.0);
    let block_h = lines.len() as f32 * size * LINE_HEIGHT;
    let origin = 180.0 + (COVER_H as f32 - 180.0 - 320.0 - block_h).max(0.0) / 2.0;
    let bottom = paint_text_block(&mut canvas, &lines, size, origin, ink);
    paint_rule(&mut canvas, (bottom + 48.0).round() as i32, ink);
    paint_footer(&mut canvas, site, ink);
    canvas
}

fn paint_text_block(
    canvas: &mut RgbImage,
    lines: &[String],
    size: f32,
    origin_y: f32,
    colour: [u8; 3],
) -> f32 {
    let scaled = plex().as_scaled(PxScale::from(size));
    let line_h = size * LINE_HEIGHT;
    for (index, line) in lines.iter().enumerate() {
        let baseline = origin_y + scaled.ascent() + index as f32 * line_h;
        draw_line(canvas, line, size, baseline, colour);
    }
    origin_y + lines.len() as f32 * line_h
}

fn paint_rule(canvas: &mut RgbImage, y: i32, colour: [u8; 3]) {
    let x0 = (canvas.width() as i32 - RULE_W) / 2;
    for row in 0..RULE_PX {
        let yy = y + row;
        if yy < 0 || yy >= canvas.height() as i32 {
            continue;
        }
        for x in x0..(x0 + RULE_W) {
            if x >= 0 && (x as u32) < canvas.width() {
                canvas.put_pixel(x as u32, yy as u32, Rgb(colour));
            }
        }
    }
}

fn paint_footer(canvas: &mut RgbImage, site: &str, colour: [u8; 3]) {
    let baseline = canvas.height() as f32 - FOOTER_FROM_BOTTOM;
    draw_line(canvas, site, FOOTER_PX, baseline, colour);
}

fn draw_line(canvas: &mut RgbImage, text: &str, size: f32, baseline: f32, colour: [u8; 3]) {
    let font = plex();
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let mut x = (canvas.width() as f32 - measure(font, text, size)) / 2.0;
    let mut prev: Option<GlyphId> = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(prev) = prev {
            x += scaled.kern(prev, id);
        }
        let glyph = id.with_scale_and_position(scale, point(x, baseline));
        if let Some(outlined) = scaled.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|px, py, coverage| {
                paint(
                    canvas,
                    bounds.min.x + px as f32,
                    bounds.min.y + py as f32,
                    coverage,
                    colour,
                );
            });
        }
        x += scaled.h_advance(id);
        prev = Some(id);
    }
}

fn paint(canvas: &mut RgbImage, x: f32, y: f32, coverage: f32, colour: [u8; 3]) {
    if coverage <= 0.0 || x < 0.0 || y < 0.0 {
        return;
    }
    let x = x as u32;
    let y = y as u32;
    if x >= canvas.width() || y >= canvas.height() {
        return;
    }
    let coverage = coverage.clamp(0.0, 1.0);
    let pixel = canvas.get_pixel_mut(x, y);
    for (component, ink) in pixel.0.iter_mut().zip(colour) {
        let src = f32::from(*component);
        let dst = f32::from(ink);
        *component = (src + (dst - src) * coverage).round() as u8;
    }
}

fn flatten(image: &DynamicImage, background: [u8; 3]) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (dst, src) in out.pixels_mut().zip(rgba.pixels()) {
        let alpha = u16::from(src.0[3]);
        let inverse = 255 - alpha;
        for ((component, src_c), bg) in dst.0.iter_mut().zip(src.0).zip(background) {
            let mixed = u16::from(src_c) * alpha + u16::from(bg) * inverse + 127;
            *component = (mixed / 255) as u8;
        }
    }
    out
}

fn encode_jpeg_capped(image: &RgbImage) -> Vec<u8> {
    for quality in [85_u8, 80, 75, 70, 65, 60] {
        let bytes = encode_jpeg(image, quality);
        if bytes.len() <= JPEG_LIMIT {
            return bytes;
        }
    }
    encode_jpeg(image, 55)
}

fn encode_jpeg(image: &RgbImage, quality: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(image)
        .expect("jpeg encode");
    bytes
}

fn plex() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("IBM Plex Serif Bold"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn image_top() -> u32 {
        (2560.0_f64 * 0.12).round() as u32
    }

    fn red(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([230, 20, 20])))
    }

    fn decode(bytes: &[u8]) -> RgbImage {
        image::load_from_memory(bytes).unwrap().to_rgb8()
    }

    fn bright_red(pixel: &Rgb<u8>) -> bool {
        pixel.0[0] > 180 && pixel.0[1] < 90 && pixel.0[2] < 90
    }

    fn near_white(pixel: &Rgb<u8>) -> bool {
        pixel.0[0] > 220 && pixel.0[1] > 220 && pixel.0[2] > 220
    }

    fn cap_ratio() -> f32 {
        let data = FONT_BYTES;
        let tables = u16::from_be_bytes([data[4], data[5]]) as usize;
        let mut head = None;
        let mut os2 = None;
        for index in 0..tables {
            let rec = 12 + index * 16;
            let tag = &data[rec..rec + 4];
            let offset = u32::from_be_bytes(data[rec + 8..rec + 12].try_into().unwrap()) as usize;
            if tag == b"head" {
                head = Some(offset);
            } else if tag == b"OS/2" {
                os2 = Some(offset);
            }
        }
        let head = head.unwrap();
        let os2 = os2.unwrap();
        let units = u16::from_be_bytes([data[head + 18], data[head + 19]]);
        let cap = i16::from_be_bytes([data[os2 + 88], data[os2 + 89]]);
        f32::from(cap) / f32::from(units)
    }

    fn assert_landscape(width: u32, height: u32) {
        let cover = make_cover(Some(red(width, height)), "Hello", "example.com");
        let again = make_cover(Some(red(width, height)), "Hello", "example.com");
        assert_eq!(cover.bytes, again.bytes);
        assert_eq!((cover.width, cover.height), (1600, 2560));
        assert!(cover.bytes.len() <= 500 * 1024, "{}", cover.bytes.len());
        let image = decode(&cover.bytes);
        let top = image_top();
        assert!(
            bright_red(image.get_pixel(800, top + 12)),
            "image should start at 12% ({top})"
        );
        assert!(
            !bright_red(image.get_pixel(800, top.saturating_sub(12))),
            "fill should sit above the image"
        );
        let fill = image.get_pixel(8, 8).0;
        let mut ink = None;
        for y in 0..image.height() {
            let pixel = image.get_pixel(800, y).0;
            if near_white(&Rgb(pixel)) {
                ink = Some(pixel);
                break;
            }
        }
        let ink = ink.expect("white text or rule");
        assert!(
            contrast(fill, ink) >= 7.0,
            "fill {fill:?} ink {ink:?} ratio {}",
            contrast(fill, ink)
        );
        assert!(rule_rows(&image) >= 4, "rule thinner than 4 px");
    }

    fn rule_rows(image: &RgbImage) -> u32 {
        let mut best = 0;
        let mut run = 0;
        for y in 0..image.height() {
            let whites = (0..image.width())
                .filter(|x| near_white(image.get_pixel(*x, y)))
                .count();
            if whites >= 300 {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
        best
    }

    #[test]
    fn landscape_1200x630_is_a_full_cover() {
        assert_landscape(1200, 630);
    }

    #[test]
    fn landscape_2400x1260_is_a_full_cover() {
        assert_landscape(2400, 1260);
    }

    #[test]
    fn landscape_600_wide_scales_by_one_and_a_half_and_is_centred() {
        let cover = make_cover(Some(red(600, 400)), "Hello", "example.com");
        assert_eq!((cover.width, cover.height), (1600, 2560));
        let image = decode(&cover.bytes);
        let top = image_top();
        let left = (1600 - 900) / 2;
        assert!(bright_red(image.get_pixel(left + 30, top + 30)));
        assert!(bright_red(image.get_pixel(left + 860, top + 30)));
        assert!(!bright_red(image.get_pixel(left - 24, top + 30)));
        assert!(!bright_red(image.get_pixel(left + 924, top + 30)));
    }

    #[test]
    fn portrait_1000x1600_is_cropped_without_a_title() {
        let cover = make_cover(Some(red(1000, 1600)), "Hello", "example.com");
        assert_eq!((cover.width, cover.height), (1500, 2400));
        let ratio = f64::from(cover.height) / f64::from(cover.width);
        assert!((ratio - 1.6).abs() < 0.01, "{ratio}");
        let image = decode(&cover.bytes);
        assert!(bright_red(image.get_pixel(20, 20)));
        assert!(bright_red(image.get_pixel(1480, 2380)));
        assert!(!image.pixels().any(near_white));
        assert!(cover.bytes.len() <= 500 * 1024);
    }

    #[test]
    fn short_portrait_still_uses_the_1_3_threshold() {
        let cover = make_cover(Some(red(900, 1200)), "Hello", "example.com");
        assert_eq!((cover.width, cover.height), (1125, 1800));
        assert!(!decode(&cover.bytes).pixels().any(near_white));
    }

    #[test]
    fn square_900_is_a_composite_within_the_band() {
        let cover = make_cover(Some(red(900, 900)), "Hello", "example.com");
        assert_eq!((cover.width, cover.height), (1600, 2560));
        let image = decode(&cover.bytes);
        let rows = (0..image.height())
            .filter(|y| bright_red(image.get_pixel(800, *y)))
            .count();
        assert!(rows > 800, "rows {rows}");
        assert!(rows <= (2560.0 * 0.55) as usize + 4, "rows {rows}");
    }

    #[test]
    fn tiny_image_matches_the_typographic_cover() {
        let tiny = make_cover(Some(red(320, 200)), "Hello", "example.com");
        let none = make_cover(None, "Hello", "example.com");
        assert_eq!(tiny.bytes, none.bytes);
        assert_eq!((tiny.width, tiny.height), (1600, 2560));
        assert!(tiny.bytes.len() <= 500 * 1024);
    }

    #[test]
    fn typographic_colour_is_stable_for_a_host() {
        let a = make_cover(None, "Hello", "Example.COM");
        let b = make_cover(None, "Hello", "www.example.com");
        assert_eq!(a.bytes, b.bytes);
        let c = make_cover(None, "Hello", "other.test");
        assert_ne!(a.bytes, c.bytes);
        assert!(rule_rows(&decode(&a.bytes)) >= 4);
    }

    #[test]
    fn very_long_title_is_four_lines_at_least_148_px() {
        let title = "Extremely long title word ".repeat(40);
        let (size, lines) = layout_title(&title, 1360.0);
        assert!(lines.len() <= 4, "{}", lines.len());
        assert_eq!(lines.len(), 4);
        assert!(lines[3].ends_with('…'), "{:?}", lines[3]);
        assert_eq!(size, 148.0);
        assert!(measure(plex(), &lines[3], size) <= 1360.0);
        assert!(size * cap_ratio() >= 102.0, "{}", size * cap_ratio());
    }

    #[test]
    fn title_shrinks_by_one_step_to_fit_four_lines() {
        let title = "n".repeat(60);
        assert_eq!(wrap_lines(&title, 180.0, 1360.0).len(), 5);
        assert!(wrap_lines(&title, 172.0, 1360.0).len() <= 4);
        let (size, lines) = layout_title(&title, 1360.0);
        assert_eq!(size, 172.0);
        assert!(lines.len() <= 4, "{}", lines.len());
        assert!(
            lines
                .iter()
                .all(|line| measure(plex(), line, size) <= 1360.0)
        );
    }

    #[test]
    fn ellipsis_keeps_the_widest_prefix_that_fits() {
        let font = plex();
        let size = 148.0;
        let kept = "n".repeat(8);
        let exact = measure(font, &format!("{kept}…"), size);
        let mut line = "n".repeat(40);
        ellipsize_last(&mut line, size, exact);
        assert_eq!(line, format!("{kept}…"));
    }

    #[test]
    fn combining_marks_stay_on_their_base_and_lines_fit() {
        assert_eq!(
            clusters("x\u{301}yz\u{308}"),
            vec!["x\u{301}", "y", "z\u{308}"]
        );
        let title = "x\u{301}".repeat(120);
        let (size, lines) = layout_title(&title, 1360.0);
        let font = plex();
        assert!(!lines.is_empty());
        for line in &lines {
            assert!(!line.starts_with('\u{0301}'), "{line:?}");
            assert!(measure(font, line, size) <= 1360.0, "{line:?}");
            let body = line.strip_suffix('…').unwrap_or(line);
            let mut chars = body.chars();
            while let Some(ch) = chars.next() {
                assert_eq!(ch, 'x', "{body:?}");
                assert_eq!(chars.next(), Some('\u{0301}'), "{body:?}");
            }
        }
    }

    #[test]
    fn plex_cap_height_is_the_font_value_and_clears_102_px() {
        let ratio = cap_ratio();
        assert!((ratio - 0.698).abs() < 0.0001, "{ratio}");
        assert!(148.0 * ratio >= 102.0, "{}", 148.0 * ratio);
    }

    #[test]
    fn scripts_without_coverage_or_that_need_shaping_show_the_site_name() {
        let site = make_cover(None, "example.com", "example.com");
        for title in [
            "記事のタイトルです",
            "مرحبا بالعالم",
            "שלום עולם",
            "สวัสดีชาวโลก",
        ] {
            let cover = make_cover(None, title, "example.com");
            assert_eq!(cover.bytes, site.bytes, "{title}");
        }
    }

    #[test]
    fn png_alpha_is_flattened_to_opaque_jpeg() {
        let mut image = RgbaImage::new(1200, 630);
        for pixel in image.pixels_mut() {
            *pixel = Rgba([255, 0, 0, 128]);
        }
        let cover = make_cover(
            Some(DynamicImage::ImageRgba8(image)),
            "Hello",
            "example.com",
        );
        assert_eq!(cover.bytes[0], 0xFF);
        assert_eq!(cover.bytes[1], 0xD8);
        let top = image_top();
        let pixel = decode(&cover.bytes).get_pixel(800, top + 40).0;
        assert!(pixel[0] > 60 && pixel[0] < 230, "{pixel:?}");
        assert!(pixel[1] < 80 && pixel[2] < 80, "{pixel:?}");
    }

    #[test]
    fn two_renders_of_no_image_match() {
        let a = make_cover(None, "Hello", "example.com");
        let b = make_cover(None, "Hello", "example.com");
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn flat_light_image_gets_off_white_fill_and_black_text() {
        let light = DynamicImage::ImageRgb8(RgbImage::from_pixel(800, 500, Rgb([235, 235, 232])));
        let fill = fill_from_image(&light);
        assert_eq!(fill, OFF_WHITE);
        assert_eq!(text_ink(fill), [0, 0, 0]);
        assert!(contrast(fill, [0, 0, 0]) >= 7.0);

        let cover = make_cover(Some(light), "Flat light", "example.com");
        let out = decode(&cover.bytes);
        // Fill below the image box and above the title, away from text.
        let px = out.get_pixel(40, 2560 - 40).0;
        for (got, want) in px.iter().zip(OFF_WHITE) {
            assert!(got.abs_diff(want) <= 6, "fill {px:?}");
        }
        let dark_ink = out.pixels().filter(|p| p.0.iter().all(|&c| c < 60)).count();
        assert!(dark_ink > 500, "black title pixels: {dark_ink}");
    }

    #[test]
    fn push_dark_still_applies_to_flat_dark_and_coloured_images() {
        let grey = DynamicImage::ImageRgb8(RgbImage::from_pixel(800, 500, Rgb([60, 60, 60])));
        let fill = fill_from_image(&grey);
        assert!(relative_luminance(fill) <= 0.10);
        assert_eq!(text_ink(fill), [255, 255, 255]);

        let coloured = DynamicImage::ImageRgb8(RgbImage::from_pixel(800, 500, Rgb([250, 200, 40])));
        let fill = fill_from_image(&coloured);
        assert!(relative_luminance(fill) <= 0.10, "{fill:?}");
        assert_eq!(text_ink(fill), [255, 255, 255]);
        assert!(contrast(fill, [255, 255, 255]) >= 7.0);
    }

    #[test]
    fn push_dark_keeps_white_text_at_seven_to_one() {
        let dark = push_dark([255, 255, 255]);
        assert!(
            relative_luminance(dark) <= 0.10,
            "{}",
            relative_luminance(dark)
        );
        assert!(contrast(dark, [255, 255, 255]) >= 7.0);
    }

    #[test]
    fn ellipsis_does_not_split_a_combining_mark_from_its_base() {
        assert_eq!(trim_one_cluster("abc\u{0301}"), "ab");
        let (size, lines) = layout_title("Cafe\u{0301}", 1360.0);
        assert_eq!(size, 180.0);
        assert_eq!(lines, vec!["Café".to_string()]);
    }

    #[test]
    fn jpeg_quality_loop_brings_noise_under_500_kib() {
        // Soft 2 px noise: q85 is over the cap and a later step in the loop fits.
        let image = RgbImage::from_fn(1600, 2560, |x, y| {
            let n = (x / 2)
                .wrapping_mul(3)
                .wrapping_add((y / 2).wrapping_mul(5));
            Rgb([
                90 + (n % 24) as u8,
                70 + ((n / 3) % 24) as u8,
                50 + ((n / 5) % 24) as u8,
            ])
        });
        let high = encode_jpeg(&image, 85);
        assert!(
            high.len() > 500 * 1024,
            "q85 was {} bytes; the cap test would not fail",
            high.len()
        );
        let capped = encode_jpeg_capped(&image);
        assert!(capped.len() <= 500 * 1024, "{}", capped.len());
        assert!(capped.len() < high.len());
        // Literal cap, not `JPEG_LIMIT`, so a broken constant cannot satisfy itself.
        let limit = 500 * 1024;
        let mut expected = None;
        for quality in [85_u8, 80, 75, 70, 65, 60] {
            let bytes = if quality == 85 {
                high.clone()
            } else {
                encode_jpeg(&image, quality)
            };
            if bytes.len() <= limit {
                expected = Some(bytes);
                break;
            }
        }
        assert_eq!(capped, expected.unwrap_or_else(|| encode_jpeg(&image, 55)));
    }

    #[test]
    fn flat_jpeg_stays_at_quality_85() {
        let image = RgbImage::from_pixel(32, 32, Rgb([40, 80, 120]));
        assert_eq!(encode_jpeg_capped(&image), encode_jpeg(&image, 85));
    }

    fn histogram_bin(rgb: [u8; 3]) -> usize {
        (((rgb[0] as usize) >> 4) << 8) | (((rgb[1] as usize) >> 4) << 4) | ((rgb[2] as usize) >> 4)
    }

    #[test]
    fn dominant_colour_uses_the_majority_mean_and_the_lower_bin() {
        let mut image = RgbImage::new(32, 32);
        for (index, pixel) in image.pixels_mut().enumerate() {
            *pixel = if index < 717 {
                Rgb([30, 90, 160])
            } else {
                Rgb([200, 40, 40])
            };
        }
        assert_eq!(
            dominant_colour(&DynamicImage::ImageRgb8(image)),
            [30, 90, 160]
        );

        // Equal counts in two bins. `>` keeps the lower index; `>=` would keep the later one.
        // Each pair collapses to one bin if the matching `<<` in the index is flipped.
        for (low, high) in [
            ([16, 0, 0], [32, 0, 0]),
            ([0, 16, 0], [0, 32, 0]),
            ([0, 0, 16], [0, 0, 32]),
        ] {
            assert!(
                histogram_bin(low) < histogram_bin(high),
                "{low:?} vs {high:?}"
            );
            let mut tie = RgbImage::new(32, 32);
            for (index, pixel) in tie.pixels_mut().enumerate() {
                *pixel = if index < 512 { Rgb(low) } else { Rgb(high) };
            }
            assert_eq!(dominant_colour(&DynamicImage::ImageRgb8(tie)), low);
        }

        // 512×30 + 512×31 on R and B, 512×32 + 512×33 on G, one bin.
        // Half-up is [31, 33, 31]. G stays out of nibble 1 so a blue `<< 4`
        // cannot OR the two colours back together.
        let mut mixed = RgbImage::new(32, 32);
        for (index, pixel) in mixed.pixels_mut().enumerate() {
            *pixel = if index < 512 {
                Rgb([30, 32, 30])
            } else {
                Rgb([31, 33, 31])
            };
        }
        let scaled = DynamicImage::ImageRgb8(mixed.clone())
            .resize_exact(32, 32, FilterType::Triangle)
            .to_rgb8();
        assert_eq!(scaled, mixed);
        assert_eq!(
            dominant_colour(&DynamicImage::ImageRgb8(mixed)),
            [31, 33, 31]
        );
    }

    #[test]
    fn centre_crop_keeps_the_middle_band_and_the_composite_band_binds() {
        let tall = RgbImage::from_fn(1000, 2000, |_x, y| {
            Rgb([(y >> 8) as u8, (y & 255) as u8, 7])
        });
        let cropped = centre_crop_16(&tall);
        let crop_h = 1000 * 8 / 5;
        let y0 = (2000 - crop_h) / 2;
        assert_eq!((cropped.width(), cropped.height()), (1000, crop_h));
        for frac in [1_u32, 3] {
            let y = crop_h * frac / 4;
            assert_eq!(cropped.get_pixel(10, y), tall.get_pixel(10, y0 + y));
        }
        assert_eq!(cropped.get_pixel(0, 0), tall.get_pixel(0, y0));
        assert_ne!(*cropped.get_pixel(0, 0), *tall.get_pixel(0, 0));
        assert_ne!(
            *cropped.get_pixel(0, crop_h - 1),
            *tall.get_pixel(0, tall.height() - 1)
        );

        let wide = RgbImage::from_fn(1000, 1400, |x, y| {
            Rgb([(x >> 8) as u8, (x & 255) as u8, (y & 255) as u8])
        });
        let cropped = centre_crop_16(&wide);
        let crop_w = 1400 * 5 / 8;
        let x0 = (1000 - crop_w) / 2;
        assert_eq!((cropped.width(), cropped.height()), (crop_w, 1400));
        for frac in [1_u32, 3] {
            let y = 1400 * frac / 4;
            let x = crop_w * frac / 4;
            assert_eq!(cropped.get_pixel(x, y), wide.get_pixel(x0 + x, y));
        }
        assert_eq!(cropped.get_pixel(0, 700), wide.get_pixel(x0, 700));
        assert_ne!(*cropped.get_pixel(0, 0), *wide.get_pixel(0, 0));
        assert_ne!(
            *cropped.get_pixel(crop_w - 1, 0),
            *wide.get_pixel(wide.width() - 1, 0)
        );

        assert_eq!(composite_box(800, 1000).3, 1408);
    }

    #[test]
    fn webp_is_decoded_and_used() {
        let source = RgbImage::from_pixel(400, 300, Rgb([230, 20, 20]));
        let mut bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .encode(source.as_raw(), 400, 300, image::ExtendedColorType::Rgb8)
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (400, 300));
        let cover = make_cover(Some(decoded), "Hello", "example.com");
        let image = decode(&cover.bytes);
        let top = image_top();
        assert!(bright_red(image.get_pixel(800, top + 20)));
    }

    fn gap_is_fill(image: &RgbImage, fill: [u8; 3], y0: u32, y1: u32) -> bool {
        (y0..y1).all(|y| {
            (0..image.width()).all(|x| {
                image
                    .get_pixel(x, y)
                    .0
                    .iter()
                    .zip(fill)
                    .all(|(got, want)| got.abs_diff(want) <= 12)
            })
        })
    }

    #[test]
    fn four_line_title_on_a_1280_band_keeps_the_rule_in_the_budget() {
        let title = "n".repeat(72);
        for (width, height) in [(1600u32, 1280u32), (900, 853)] {
            let layout = composite_layout(width, height, &title);
            assert!(
                (layout.rule_y + RULE_PX) as f32 <= layout.footer_top,
                "{width}x{height}: rule {} footer {}",
                layout.rule_y,
                layout.footer_top
            );
        }
        // Hits the debug_assert in render_composite.
        let cover = make_cover(Some(red(1600, 1280)), &title, "example.com");
        assert_eq!((cover.width, cover.height), (COVER_W, COVER_H));
    }

    #[test]
    fn composite_title_shrinks_through_the_vertical_budget() {
        // 36 n's stay 3 lines from 180 to 156. dh 1408 leaves avail 552.8:
        // 621, 593, 566, then 538, so the size stops at 156.
        let title = "n".repeat(36);
        assert_eq!(wrap_lines(&title, 180.0, 1360.0).len(), 3);
        let layout = composite_layout(800, 1000, &title);
        assert_eq!(layout.dh, 1408);
        assert_eq!((layout.size, layout.lines.len()), (156.0, 3));
        // dh 1400 leaves avail 560.8, still under 3×164×1.15 = 565.8.
        // Putting the 4 px rule back (`-` → `+`) would keep 164.
        let tight = composite_layout(900, 933, &title);
        assert_eq!((tight.dh, tight.size, tight.lines.len()), (1400, 156.0, 3));
    }

    #[test]
    fn long_composite_title_stays_above_the_footer() {
        let title = "gypq".repeat(24);
        assert!(
            wrap_lines(&title, 180.0, 1360.0).len() >= 4,
            "fixture should be four lines at 180 px"
        );
        assert_eq!(composite_box(3200, 1000), (0, 307, 1600, 500));
        for (width, height) in [(900u32, 900u32), (800, 1000)] {
            let layout = composite_layout(width, height, &title);
            let block = layout.origin + layout.lines.len() as f32 * layout.size * LINE_HEIGHT;
            // floor(origin + 3×148×1.15 + 40). round() was 2272 and 2330.
            assert_eq!(layout.rule_y, if height == 900 { 2271 } else { 2329 });
            for line in &layout.lines {
                let width_px = measure(plex(), line, layout.size);
                assert!(width_px <= 1360.0, "{line} {width_px}");
                // A collapsed max_width leaves a one-glyph line, well under 1000.
                assert!(width_px > 1000.0, "{line} {width_px}");
            }
            assert!(
                block + 40.0 <= layout.footer_top,
                "{width}x{height}: title {block} footer {}",
                layout.footer_top
            );
            assert!(layout.rule_y >= 0, "{}", layout.rule_y);
            assert!(
                layout.rule_y + RULE_PX <= COVER_H as i32
                    && (layout.rule_y + RULE_PX) as f32 <= layout.footer_top,
                "rule {} footer {} canvas {COVER_H}",
                layout.rule_y,
                layout.footer_top
            );
            assert!(block <= COVER_H as f32);
            assert_eq!(
                (layout.left, layout.top, layout.dw, layout.dh),
                composite_box(width, height),
                "default scale still fits"
            );
            let cover = make_cover(Some(red(width, height)), &title, "example.com");
            assert_eq!((cover.width, cover.height), (COVER_W, COVER_H));
            let image = decode(&cover.bytes);
            let fill = fill_from_image(&red(width, height));
            let ink_bottom = (0..layout.rule_y as u32)
                .rev()
                .find(|y| (0..image.width()).any(|x| near_white(image.get_pixel(x, *y))))
                .expect("title ink");
            assert!(
                layout.rule_y as u32 - ink_bottom >= 40,
                "{width}x{height}: ink {ink_bottom} rule {}",
                layout.rule_y
            );
            let gap_start = (layout.rule_y + RULE_PX) as u32;
            let gap_end = layout.footer_top.floor() as u32;
            assert!(gap_start < gap_end, "{gap_start}..{gap_end}");
            assert!(
                gap_is_fill(&image, fill, gap_start, gap_end),
                "{width}x{height}: ink between the rule and the footer"
            );
            let rule_whites = (0..image.width())
                .filter(|x| near_white(image.get_pixel(*x, layout.rule_y as u32)))
                .count();
            assert!(rule_whites >= 300, "rule row {}", layout.rule_y);
            let footer_ink = ((gap_end)..image.height())
                .any(|y| (0..image.width()).any(|x| near_white(image.get_pixel(x, y))));
            assert!(footer_ink, "footer site name missing");
        }
    }

    #[test]
    fn contact_sheet_when_requested() {
        if std::env::var("FOLIO_COVER_SHEET").ok().as_deref() != Some("1") {
            return;
        }
        let long = "Extremely long title word ".repeat(40);
        let cases = [
            make_cover(Some(red(1200, 630)), "Landscape", "example.com"),
            make_cover(Some(red(2400, 1260)), "Wide landscape", "example.com"),
            make_cover(Some(red(600, 400)), "Modest landscape", "example.com"),
            make_cover(Some(red(1000, 1600)), "Portrait", "example.com"),
            make_cover(Some(red(900, 900)), "Square", "example.com"),
            make_cover(Some(red(320, 200)), "Tiny", "example.com"),
            make_cover(None, "No image", "example.com"),
            make_cover(None, &long, "example.com"),
            make_cover(None, "記事のタイトルです", "example.com"),
            make_cover(None, "مرحبا بالعالم", "example.com"),
            make_cover(None, "שלום עולם", "example.com"),
            make_cover(None, "สวัสดีชาวโลก", "example.com"),
        ];
        let thumbs: Vec<image::GrayImage> = cases
            .iter()
            .map(|cover| {
                let rgb = decode(&cover.bytes);
                let height =
                    (100.0 * f64::from(rgb.height()) / f64::from(rgb.width())).round() as u32;
                let resized = imageops::resize(&rgb, 100, height, FilterType::Triangle);
                imageops::grayscale(&resized)
            })
            .collect();
        let total_h = thumbs.iter().map(|thumb| thumb.height()).sum();
        let mut sheet = image::GrayImage::from_pixel(100, total_h, image::Luma([0]));
        let mut y = 0;
        for thumb in &thumbs {
            imageops::overlay(&mut sheet, thumb, 0, i64::from(y));
            y += thumb.height();
        }
        std::fs::create_dir_all("target").unwrap();
        sheet.save("target/cover-sheet-100px-grey.png").unwrap();
    }
}
