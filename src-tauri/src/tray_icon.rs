//! Renders the tray icon (presets and the custom module strip) into an RGBA bitmap.
//!
//! Drawing is done with tiny-skia for shapes and the embedded 8x8 bitmap font for text, so no
//! font files or webview are needed. Icons are drawn at 64x64 and downscaled by the OS.

use crate::model::{BasedOn, Settings, TrayMode, TrayPreset};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub const SIZE: u32 = 64;

/// Everything the renderer needs about the active account.
#[derive(Clone, Debug, Default)]
pub struct IconData {
    pub name: String,
    pub five: Option<f64>,
    pub seven: Option<f64>,
    pub five_reset_ms: Option<i64>,
    pub seven_reset_ms: Option<i64>,
    pub cost_today: Option<f64>,
    /// Binding usage samples, oldest first.
    pub history: Vec<f64>,
    pub now_ms: i64,
    pub critical_at: f64,
}

pub struct Rendered {
    pub rgba: Vec<u8>,
    pub size: u32,
}

#[derive(Clone, Copy)]
struct Palette {
    fg: Color,
    dim: Color,
    ok: Color,
    warn: Color,
    crit: Color,
    ink: Color,
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

fn palette(light_taskbar: bool) -> Palette {
    if light_taskbar {
        Palette {
            fg: rgb(0x27, 0x27, 0x2a),
            dim: Color::from_rgba8(0x27, 0x27, 0x2a, 70),
            ok: rgb(0x27, 0x27, 0x2a),
            warn: rgb(0xb9, 0x80, 0x3f),
            crit: rgb(0xb6, 0x52, 0x4e),
            ink: rgb(0xff, 0xff, 0xff),
        }
    } else {
        Palette {
            fg: rgb(0xff, 0xff, 0xff),
            dim: Color::from_rgba8(0xff, 0xff, 0xff, 80),
            ok: rgb(0xff, 0xff, 0xff),
            warn: rgb(0xe8, 0xa0, 0x50),
            crit: rgb(0xe5, 0x64, 0x5e),
            ink: rgb(0x1c, 0x1c, 0x1f),
        }
    }
}

fn tone(p: &Palette, pct: f64, critical_at: f64) -> Color {
    if pct >= critical_at {
        p.crit
    } else if pct >= 60.0 {
        p.warn
    } else {
        p.ok
    }
}

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill_rr(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if let Some(path) = rounded_rect(x, y, w, h, r) {
        pm.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
    }
}

fn circle_path(cx: f32, cy: f32, r: f32) -> Option<tiny_skia::Path> {
    PathBuilder::from_circle(cx, cy, r)
}

/// A pill-shaped progress bar: dim track plus a filled portion.
#[allow(clippy::too_many_arguments)]
fn bar(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, pct: f64, color: Color, track: Color) {
    fill_rr(pm, x, y, w, h, h / 2.0, track);
    let frac = (pct / 100.0).clamp(0.0, 1.0) as f32;
    if frac > 0.0 {
        fill_rr(pm, x, y, (w * frac).max(h), h, h / 2.0, color);
    }
}

/// Draw text with the 8x8 bitmap font; returns the drawn width. `scale` is the pixel size of one font dot.
fn text_width(s: &str, scale: f32) -> f32 {
    s.chars().count() as f32 * 8.0 * scale
}

fn text(pm: &mut Pixmap, s: &str, x: f32, y: f32, scale: f32, color: Color) {
    let p = paint(color);
    let mut cx = x;
    for ch in s.chars() {
        if let Some(glyph) = BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        if let Some(r) = Rect::from_xywh(cx + col as f32 * scale, y + row as f32 * scale, scale, scale) {
                            pm.fill_rect(r, &p, Transform::identity(), None);
                        }
                    }
                }
            }
        }
        cx += 8.0 * scale;
    }
}

fn centered_text(pm: &mut Pixmap, s: &str, cx: f32, cy: f32, max_w: f32, max_scale: f32, color: Color) {
    let scale = max_scale.min(max_w / (s.chars().count().max(1) as f32 * 8.0));
    let w = text_width(s, scale);
    // glyphs occupy rows 0..7 of the 8x8 cell; center on the cell
    text(pm, s, cx - w / 2.0, cy - 4.0 * scale, scale, color);
}

/// The app mark (two account chips) — drawn without the tile, since the tray has its own background.
fn mark(pm: &mut Pixmap, p: &Palette, light_taskbar: bool) {
    let accent = if light_taskbar { rgb(0x5b, 0x6f, 0x99) } else { rgb(0x8f, 0xa3, 0xcf) };
    // the chips span x 8..56 in the 64-unit design; stretch them to use the whole icon
    fill_rr(pm, 2.0, 10.0, 40.0, 19.0, 9.5, p.fg);
    if let Some(path) = circle_path(13.0, 19.5, 5.0) {
        pm.fill_path(&path, &paint(accent), FillRule::Winding, Transform::identity(), None);
    }
    fill_rr(pm, 22.0, 35.0, 40.0, 19.0, 9.5, accent);
    if let Some(path) = circle_path(33.0, 44.5, 5.0) {
        pm.fill_path(&path, &paint(p.fg), FillRule::Winding, Transform::identity(), None);
    }
}

fn driving_value(data: &IconData, based_on: BasedOn) -> Option<f64> {
    match based_on {
        BasedOn::Highest => match (data.five, data.seven) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        },
        BasedOn::FiveHour => data.five,
        BasedOn::SevenDay => data.seven,
    }
}

pub fn render(data: Option<&IconData>, settings: &Settings, light_taskbar: bool) -> Rendered {
    let p = palette(light_taskbar);
    let mut pm = Pixmap::new(SIZE, SIZE).expect("pixmap");
    let s = SIZE as f32;

    match data {
        None => mark(&mut pm, &p, light_taskbar),
        Some(d) if driving_value(d, settings.tray_based_on).is_none() && settings.tray_mode == TrayMode::Presets => {
            mark(&mut pm, &p, light_taskbar);
        }
        Some(d) => match settings.tray_mode {
            TrayMode::Presets => preset(&mut pm, d, settings, &p, s),
            TrayMode::Custom => custom(&mut pm, d, settings, &p, s, light_taskbar),
        },
    }

    // tiny-skia stores premultiplied RGBA; the tray wants straight alpha
    let mut rgba = pm.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in px.iter_mut().take(3) {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Rendered { rgba, size: SIZE }
}

fn preset(pm: &mut Pixmap, d: &IconData, settings: &Settings, p: &Palette, s: f32) {
    let value = driving_value(d, settings.tray_based_on).unwrap_or(0.0);
    let color = tone(p, value, d.critical_at);
    match settings.tray_preset {
        TrayPreset::Bar => {
            // outlined pill with an inset fill, like the design preview
            let (x, y, w, h) = (3.0, s / 2.0 - 13.0, s - 6.0, 26.0);
            if let Some(path) = rounded_rect(x + 2.0, y + 2.0, w - 4.0, h - 4.0, (h - 4.0) / 2.0) {
                pm.stroke_path(&path, &paint(p.dim), &Stroke { width: 4.0, ..Default::default() }, Transform::identity(), None);
            }
            let inner_w = w - 12.0;
            let frac = (value / 100.0).clamp(0.0, 1.0) as f32;
            if frac > 0.0 {
                fill_rr(pm, x + 6.0, y + 6.0, (inner_w * frac).max(14.0), h - 12.0, (h - 12.0) / 2.0, color);
            }
        }
        TrayPreset::Percent => {
            let label = format!("{}", value.round().clamp(0.0, 999.0) as u32);
            centered_text(pm, &label, s / 2.0, s / 2.0, s - 2.0, 4.5, color);
        }
        TrayPreset::Ring => {
            let (cx, cy, r) = (s / 2.0, s / 2.0, s / 2.0 - 8.0);
            if let Some(path) = circle_path(cx, cy, r) {
                pm.stroke_path(&path, &paint(p.dim), &Stroke { width: 9.0, ..Default::default() }, Transform::identity(), None);
            }
            let frac = (value / 100.0).clamp(0.0, 1.0);
            if frac > 0.0 {
                let steps = ((frac * 90.0).ceil() as usize).max(2);
                let mut pb = PathBuilder::new();
                for i in 0..=steps {
                    let a = -std::f64::consts::FRAC_PI_2 + frac * std::f64::consts::TAU * (i as f64 / steps as f64);
                    let (x, y) = (cx + r * a.cos() as f32, cy + r * a.sin() as f32);
                    if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
                }
                if let Some(path) = pb.finish() {
                    let stroke = Stroke { width: 9.0, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                    pm.stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
                }
            }
        }
        TrayPreset::Dot => {
            let c = if value >= d.critical_at { p.crit } else if value >= 60.0 { p.warn } else { rgb(0x7d, 0xc7, 0x9b) };
            if let Some(path) = circle_path(s / 2.0, s / 2.0, s / 2.0 - 10.0) {
                pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
            }
        }
        TrayPreset::Face => face(pm, value, d.critical_at, p, s),
        TrayPreset::History => history(pm, d, value, p, s),
    }
}

fn face(pm: &mut Pixmap, value: f64, critical_at: f64, p: &Palette, s: f32) {
    let c = if value >= critical_at { p.crit } else if value >= 60.0 { p.warn } else { rgb(0x7d, 0xc7, 0x9b) };
    let (cx, cy, r) = (s / 2.0, s / 2.0, s / 2.0 - 5.0);
    if let Some(path) = circle_path(cx, cy, r) {
        pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
    let ink = paint(p.ink);
    for dx in [-12.0, 12.0] {
        if let Some(path) = circle_path(cx + dx, cy - 7.0, 4.0) {
            pm.fill_path(&path, &ink, FillRule::Winding, Transform::identity(), None);
        }
    }
    // mouth: smile when there is room, flat in the middle, frown near the limit
    let curve = if value < 40.0 { 9.0 } else if value < 60.0 { 4.0 } else if value < 80.0 { 0.0 } else { -8.0 };
    let mut pb = PathBuilder::new();
    pb.move_to(cx - 12.0, cy + 12.0 - curve / 3.0);
    pb.quad_to(cx, cy + 12.0 + curve, cx + 12.0, cy + 12.0 - curve / 3.0);
    if let Some(path) = pb.finish() {
        let stroke = Stroke { width: 4.0, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
        pm.stroke_path(&path, &ink, &stroke, Transform::identity(), None);
    }
}

fn history(pm: &mut Pixmap, d: &IconData, current: f64, p: &Palette, s: f32) {
    // last 5 samples plus the live reading
    let mut vals: Vec<f64> = d.history.iter().rev().take(5).rev().copied().collect();
    vals.push(current);
    let n = vals.len();
    let gap = 4.0;
    let bw = (s - 8.0 - gap * (n as f32 - 1.0)) / n as f32;
    for (i, v) in vals.iter().enumerate() {
        let h = (s - 12.0) * (*v as f32 / 100.0).clamp(0.08, 1.0);
        let color = if i + 1 == n { tone(p, *v, d.critical_at) } else { p.dim };
        fill_rr(pm, 4.0 + i as f32 * (bw + gap), s - 6.0 - h, bw, h, 2.0, color);
    }
}

fn countdown_label(reset_ms: Option<i64>, now_ms: i64) -> String {
    let Some(r) = reset_ms else { return "--".into() };
    let mins = ((r - now_ms) / 60_000).max(0);
    if mins >= 24 * 60 {
        format!("{}d", mins / (24 * 60))
    } else if mins >= 60 {
        format!("{}h", mins / 60)
    } else {
        format!("{}m", mins)
    }
}

fn custom(pm: &mut Pixmap, d: &IconData, settings: &Settings, p: &Palette, s: f32, light_taskbar: bool) {
    #[derive(Clone, Copy)]
    enum Row { Text, Bar }
    let mut rows: Vec<(String, Row, f64)> = Vec::new();
    for m in settings.tray_modules.iter().filter(|m| m.enabled) {
        match m.id.as_str() {
            "name" => {
                let initials: String = d.name.chars().filter(|c| c.is_alphanumeric()).take(3).collect::<String>().to_uppercase();
                rows.push((if initials.is_empty() { "?".into() } else { initials }, Row::Text, 0.0));
            }
            "5h" => {
                if let Some(v) = d.five { rows.push((String::new(), Row::Bar, v)); }
            }
            "7d" => {
                if let Some(v) = d.seven { rows.push((String::new(), Row::Bar, v)); }
            }
            "cost" => {
                let label = d.cost_today.map(|c| if c >= 10.0 { format!("${:.0}", c) } else { format!("${:.1}", c) }).unwrap_or_else(|| "$-".into());
                rows.push((label, Row::Text, 0.0));
            }
            "reset" => {
                let reset = match settings.tray_based_on {
                    BasedOn::SevenDay => d.seven_reset_ms,
                    _ => d.five_reset_ms.or(d.seven_reset_ms),
                };
                rows.push((countdown_label(reset, d.now_ms), Row::Text, 0.0));
            }
            _ => {}
        }
    }
    if rows.is_empty() {
        mark(pm, p, light_taskbar);
        return;
    }
    // text rows are ~20px tall, bars ~12px, 4px gaps; shrink uniformly if it does not fit
    let heights: Vec<f32> = rows.iter().map(|(_, k, _)| if matches!(k, Row::Text) { 20.0 } else { 12.0 }).collect();
    let total: f32 = heights.iter().sum::<f32>() + 4.0 * (rows.len() as f32 - 1.0);
    let k = ((s - 4.0) / total).min(1.0);
    let mut y = (s - total * k) / 2.0;
    for ((label, kind, value), h) in rows.iter().zip(heights) {
        let h = h * k;
        match kind {
            Row::Text => {
                let scale = (h / 8.0).min(2.5);
                centered_text(pm, label, s / 2.0, y + h / 2.0, s - 4.0, scale, p.fg);
            }
            Row::Bar => bar(pm, 4.0, y, s - 8.0, h, *value, tone(p, *value, d.critical_at), p.dim),
        }
        y += h + 4.0 * k;
    }
}

/// Windows only: is the taskbar/tray using the light theme?
#[cfg(windows)]
pub fn light_taskbar() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
        .map(|v| v == 1)
        .unwrap_or(false)
}

/// macOS: the menu bar follows the system appearance. `AppleInterfaceStyle` is "Dark" in dark mode
/// and missing in light mode. Checked at most every 30 s, since the tray redraws every few seconds.
#[cfg(target_os = "macos")]
pub fn light_taskbar() -> bool {
    use parking_lot::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut cache = CACHE.lock();
    if let Some((at, light)) = *cache {
        if at.elapsed() < Duration::from_secs(30) {
            return light;
        }
    }
    let light = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().eq_ignore_ascii_case("dark"))
        .unwrap_or(true);
    *cache = Some((Instant::now(), light));
    light
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn light_taskbar() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrayModule;

    fn data() -> IconData {
        IconData {
            name: "Personal".into(),
            five: Some(41.0),
            seven: Some(63.0),
            five_reset_ms: Some(3_600_000 * 3),
            seven_reset_ms: Some(86_400_000 * 4),
            cost_today: Some(2.4),
            history: vec![10.0, 20.0, 35.0],
            now_ms: 0,
            critical_at: 90.0,
        }
    }

    fn has_ink(r: &Rendered) -> bool {
        r.rgba.chunks_exact(4).any(|p| p[3] > 0)
    }

    #[test]
    fn every_preset_draws_something() {
        for preset in [TrayPreset::Bar, TrayPreset::Percent, TrayPreset::Ring, TrayPreset::Dot, TrayPreset::Face, TrayPreset::History] {
            let s = Settings { tray_preset: preset, ..Settings::default() };
            let r = render(Some(&data()), &s, false);
            assert_eq!(r.rgba.len(), (SIZE * SIZE * 4) as usize);
            assert!(has_ink(&r), "{preset:?}");
        }
    }

    #[test]
    fn custom_with_all_modules_and_empty_fallback() {
        let mut s = Settings { tray_mode: TrayMode::Custom, ..Settings::default() };
        for m in s.tray_modules.iter_mut() { m.enabled = true; }
        assert!(has_ink(&render(Some(&data()), &s, true)));
        s.tray_modules = vec![TrayModule { id: "name".into(), enabled: false }];
        assert!(has_ink(&render(Some(&data()), &s, true)), "falls back to the app mark");
    }

    #[test]
    fn no_data_shows_the_app_mark() {
        assert!(has_ink(&render(None, &Settings::default(), false)));
    }
}

#[cfg(test)]
mod dump {
    use super::*;
    use crate::model::{TrayModule, TrayMode, TrayPreset};

    /// Manual helper: `FLIPON_DUMP_DIR=... cargo test dump_tray_icons` writes every look as a PNG.
    #[test]
    fn dump_tray_icons() {
        let Some(dir) = std::env::var_os("FLIPON_DUMP_DIR") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let data = IconData {
            name: "Personal".into(), five: Some(41.0), seven: Some(63.0),
            five_reset_ms: Some(3 * 3_600_000), seven_reset_ms: Some(4 * 86_400_000),
            cost_today: Some(2.4), history: vec![10.0, 25.0, 35.0, 50.0], now_ms: 0, critical_at: 90.0,
        };
        for light in [false, true] {
            let tag = if light { "light" } else { "dark" };
            let mut jobs: Vec<(String, Option<&IconData>, Settings)> = vec![("mark".into(), None, Settings::default())];
            for (name, p) in [("bar", TrayPreset::Bar), ("percent", TrayPreset::Percent), ("ring", TrayPreset::Ring),
                              ("dot", TrayPreset::Dot), ("face", TrayPreset::Face), ("history", TrayPreset::History)] {
                jobs.push((name.into(), Some(&data), Settings { tray_preset: p, ..Settings::default() }));
            }
            let mut all = Settings { tray_mode: TrayMode::Custom, ..Settings::default() };
            for m in all.tray_modules.iter_mut() { m.enabled = true; }
            jobs.push(("custom-all".into(), Some(&data), all));
            let mut few = Settings { tray_mode: TrayMode::Custom, ..Settings::default() };
            few.tray_modules = vec![TrayModule { id: "name".into(), enabled: true }, TrayModule { id: "5h".into(), enabled: true }, TrayModule { id: "7d".into(), enabled: true }];
            jobs.push(("custom-default".into(), Some(&data), few));
            for (name, d, s) in jobs {
                let r = render(d, &s, light);
                let mut pm = tiny_skia::Pixmap::new(r.size, r.size).unwrap();
                for (dst, src) in pm.data_mut().chunks_exact_mut(4).zip(r.rgba.chunks_exact(4)) {
                    let a = src[3] as u32;
                    for i in 0..3 { dst[i] = ((src[i] as u32 * a + 127) / 255) as u8; }
                    dst[3] = src[3];
                }
                pm.save_png(dir.join(format!("{tag}-{name}.png"))).unwrap();
            }
        }
    }
}
