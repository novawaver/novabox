//! The unread count, drawn onto the taskbar button and the tray icon.
//!
//! Windows has no numbered badge for desktop apps: the taskbar takes a small overlay picture
//! and the tray takes a whole icon. Both are drawn here pixel by pixel, with a tiny built-in
//! set of digits, so no font or image library is needed.

use std::sync::atomic::Ordering;

use tauri::image::Image;
use tauri::{AppHandle, Manager};

use crate::AppState;

const RED: [u8; 3] = [0xD9, 0x30, 0x25];
const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];
/// The tray icon is drawn at this size and scaled down by Windows, which keeps the digits sharp.
const TRAY_SIZE: u32 = 64;
const OVERLAY_SIZE: u32 = 32;

/// Digits 0-9 and "+", three dots wide and five tall, one row per entry with the left dot
/// in the high bit.
const GLYPHS: [[u8; 5]; 11] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
    [0b000, 0b010, 0b111, 0b010, 0b000],
];

struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        Canvas { width, height, rgba: vec![0; (width * height * 4) as usize] }
    }

    /// Paints one pixel over what is there, `cover` being how much of the pixel it fills.
    fn blend(&mut self, x: i32, y: i32, color: [u8; 3], cover: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || cover <= 0.0 {
            return;
        }
        let at = ((y as u32 * self.width + x as u32) * 4) as usize;
        let cover = cover.min(1.0);
        let under = self.rgba[at + 3] as f32 / 255.0;
        let alpha = cover + under * (1.0 - cover);
        for channel in 0..3 {
            let mixed = color[channel] as f32 * cover + self.rgba[at + channel] as f32 * under * (1.0 - cover);
            self.rgba[at + channel] = (mixed / alpha).round() as u8;
        }
        self.rgba[at + 3] = (alpha * 255.0).round() as u8;
    }

    /// A filled circle with a smooth edge.
    fn disc(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 3]) {
        let (left, right) = ((cx - radius).floor() as i32, (cx + radius).ceil() as i32);
        let (top, bottom) = ((cy - radius).floor() as i32, (cy + radius).ceil() as i32);
        for y in top..=bottom {
            for x in left..=right {
                let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                self.blend(x, y, color, radius - distance + 0.5);
            }
        }
    }

    /// The text in the built-in digits, as large as fits the circle, centred on it.
    fn digits(&mut self, cx: f32, cy: f32, radius: f32, text: &str, color: [u8; 3]) {
        let glyphs: Vec<usize> = text
            .chars()
            .filter_map(|ch| match ch {
                '0'..='9' => Some(ch as usize - '0' as usize),
                '+' => Some(10),
                _ => None,
            })
            .collect();
        if glyphs.is_empty() {
            return;
        }
        // Each glyph is three dots wide with one dot between glyphs.
        let columns = glyphs.len() as f32 * 4.0 - 1.0;
        let dot = ((radius * 1.45 / columns).min(radius * 1.3 / 5.0).floor() as i32).max(1);
        let left = (cx - columns * dot as f32 / 2.0).round() as i32;
        let top = (cy - 5.0 * dot as f32 / 2.0).round() as i32;
        for (index, glyph) in glyphs.iter().enumerate() {
            for (row, bits) in GLYPHS[*glyph].iter().enumerate() {
                for column in 0..3 {
                    if bits & (0b100 >> column) == 0 {
                        continue;
                    }
                    let x = left + (index as i32 * 4 + column) * dot;
                    let y = top + row as i32 * dot;
                    for dy in 0..dot {
                        for dx in 0..dot {
                            self.blend(x + dx, y + dy, color, 1.0);
                        }
                    }
                }
            }
        }
    }

    fn into_image(self) -> Image<'static> {
        Image::new_owned(self.rgba, self.width, self.height)
    }
}

/// What the badge says: the count, or "99+" (or a shorter `cap`) when it will not fit.
fn label(count: i64, cap: i64) -> String {
    if count > cap {
        format!("{cap}+")
    } else {
        count.to_string()
    }
}

/// The picture Windows lays over the corner of the taskbar button.
fn overlay(count: i64) -> Image<'static> {
    let mut canvas = Canvas::new(OVERLAY_SIZE, OVERLAY_SIZE);
    let middle = OVERLAY_SIZE as f32 / 2.0;
    canvas.disc(middle, middle, middle, RED);
    canvas.digits(middle, middle, middle, &label(count, 99), WHITE);
    canvas.into_image()
}

/// The app icon scaled to the tray drawing size.
fn scaled(icon: &Image<'_>) -> Canvas {
    let mut canvas = Canvas::new(TRAY_SIZE, TRAY_SIZE);
    let (width, height, source) = (icon.width().max(1), icon.height().max(1), icon.rgba());
    if source.len() < (width * height * 4) as usize {
        return canvas;
    }
    // Each target pixel is the average of the source pixels under it (or the nearest one,
    // when the icon is smaller than the target).
    for y in 0..TRAY_SIZE {
        for x in 0..TRAY_SIZE {
            let (x0, x1) = (x * width / TRAY_SIZE, ((x + 1) * width / TRAY_SIZE).max(x * width / TRAY_SIZE + 1).min(width));
            let (y0, y1) = (y * height / TRAY_SIZE, ((y + 1) * height / TRAY_SIZE).max(y * height / TRAY_SIZE + 1).min(height));
            let mut sum = [0f32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = ((sy * width + sx) * 4) as usize;
                    let alpha = source[at + 3] as f32;
                    for channel in 0..3 {
                        sum[channel] += source[at + channel] as f32 * alpha;
                    }
                    sum[3] += alpha;
                }
            }
            let at = ((y * TRAY_SIZE + x) * 4) as usize;
            let pixels = ((x1 - x0) * (y1 - y0)) as f32;
            if sum[3] > 0.0 {
                for channel in 0..3 {
                    canvas.rgba[at + channel] = (sum[channel] / sum[3]).round() as u8;
                }
                canvas.rgba[at + 3] = (sum[3] / pixels).round() as u8;
            }
        }
    }
    canvas
}

/// The tray icon with the count in its lower right corner. The tray shows icons very small,
/// so the badge takes up much of the icon and stops at "9+".
fn tray_icon(icon: &Image<'_>, count: i64) -> Image<'static> {
    let mut canvas = scaled(icon);
    let size = TRAY_SIZE as f32;
    let radius = size * 0.34;
    let (cx, cy) = (size - radius, size - radius);
    // A white rim keeps the badge apart from the icon behind it.
    canvas.disc(cx, cy, radius, WHITE);
    canvas.disc(cx, cy, radius - size * 0.03, RED);
    canvas.digits(cx, cy, radius - size * 0.03, &label(count, 9), WHITE);
    canvas.into_image()
}

/// Shows `unread` on the taskbar button and the tray icon, or clears both at zero. Does
/// nothing when that count is already showing, unless `force` is set: Windows forgets the
/// taskbar overlay when the window is hidden to the tray and brought back.
pub fn show(app: &AppHandle, unread: i64, force: bool) {
    let shown = app.state::<AppState>().badge_shown.swap(unread, Ordering::Relaxed);
    if shown == unread && !force {
        return;
    }
    #[cfg(target_os = "windows")]
    if let Some(window) = app.get_webview_window("main") {
        let picture = (unread > 0).then(|| overlay(unread));
        if let Err(err) = window.set_overlay_icon(picture) {
            eprintln!("Novabox taskbar badge: {err}");
        }
    }
    if shown == unread {
        return;
    }
    if let (Some(tray), Some(icon)) = (app.tray_by_id("novabox"), app.default_window_icon()) {
        let picture = if unread > 0 { tray_icon(icon, unread) } else { icon.clone() };
        if let Err(err) = tray.set_icon(Some(picture)) {
            eprintln!("Novabox tray badge: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &Image<'_>, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.width() + x) * 4) as usize;
        image.rgba()[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn labels_stop_at_the_cap() {
        assert_eq!(label(7, 99), "7");
        assert_eq!(label(99, 99), "99");
        assert_eq!(label(100, 99), "99+");
        assert_eq!(label(10, 9), "9+");
    }

    #[test]
    fn overlay_is_a_red_disc_with_white_digits() {
        let image = overlay(8);
        assert_eq!((image.width(), image.height()), (OVERLAY_SIZE, OVERLAY_SIZE));
        // Clear outside the circle, red inside it, and the middle bar of the 8 in white.
        assert_eq!(pixel(&image, 0, 0)[3], 0);
        assert_eq!(pixel(&image, 3, 16), [RED[0], RED[1], RED[2], 255]);
        assert_eq!(pixel(&image, 16, 16), [255, 255, 255, 255]);
        // Three characters still fit inside the circle.
        let wide = overlay(250);
        let white = wide.rgba().chunks(4).filter(|px| px == &[255, 255, 255, 255]).count();
        assert!(white > 40);
        assert_eq!(pixel(&wide, 0, 0)[3], 0);
    }

    #[test]
    fn tray_icon_keeps_the_icon_and_adds_the_badge() {
        // A plain blue 32 x 32 icon.
        let blue: Vec<u8> = [0u8, 0, 255, 255].repeat(32 * 32);
        let icon = Image::new_owned(blue, 32, 32);
        let image = tray_icon(&icon, 3);
        assert_eq!((image.width(), image.height()), (TRAY_SIZE, TRAY_SIZE));
        assert_eq!(pixel(&image, 4, 4), [0, 0, 255, 255]);
        let (cx, cy) = (TRAY_SIZE - 22, TRAY_SIZE - 22);
        // Inside the badge: red around the digit, white on it.
        assert_eq!(pixel(&image, cx - 14, cy), [RED[0], RED[1], RED[2], 255]);
        assert_eq!(pixel(&image, cx, cy), [255, 255, 255, 255]);
    }
}
