//! Image post-processing: long-edge resize and terminal-mask overlay.

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};

/// Class names whose windows are masked in screenshots.
///
/// Phase 1 ships this hardcoded; Phase 3 replaces it with a lookup against
/// the policy table. The set covers the dozen most common Linux terminals.
pub const TERMINAL_CLASSES: &[&str] = &[
    "konsole",
    "yakuake",
    "alacritty",
    "kitty",
    "wezterm",
    "foot",
    "ghostty",
    "tilix",
    "gnome-terminal",
    "xterm",
    "xfce4-terminal",
    "ptyxis",
    "warp-terminal",
];

pub fn is_terminal_class(class: &str) -> bool {
    let lower = class.to_ascii_lowercase();
    TERMINAL_CLASSES.iter().any(|c| lower == *c)
}

pub fn resize_long_edge(img: DynamicImage, max: u32) -> (DynamicImage, f32) {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max {
        return (img, 1.0);
    }
    let scale = max as f32 / long as f32;
    let (nw, nh) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
    (
        img.resize(nw, nh, image::imageops::FilterType::Triangle),
        scale,
    )
}

/// Overlay solid black rectangles on the image. Rectangles are in image
/// pixels (post-scaling). Clamps to bounds.
pub fn mask_rectangles(img: DynamicImage, rects: &[[i32; 4]]) -> DynamicImage {
    let (w, h) = img.dimensions();
    let mut buf: ImageBuffer<Rgba<u8>, _> = img.to_rgba8();
    for r in rects {
        let x0 = r[0].max(0).min(w as i32) as u32;
        let y0 = r[1].max(0).min(h as i32) as u32;
        let x1 = ((r[0] + r[2]).max(0).min(w as i32)) as u32;
        let y1 = ((r[1] + r[3]).max(0).min(h as i32)) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                buf.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
    }
    DynamicImage::ImageRgba8(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_terminal_class_is_case_insensitive() {
        assert!(is_terminal_class("Konsole"));
        assert!(is_terminal_class("KONSOLE"));
        assert!(is_terminal_class("konsole"));
        assert!(!is_terminal_class("kate"));
    }

    #[test]
    fn terminal_class_list_contains_thirteen_entries() {
        // Sanity: if Phase 3 grows the list, this assertion will trip and
        // remind us to re-evaluate the masking decision.
        assert_eq!(TERMINAL_CLASSES.len(), 13);
    }

    #[test]
    fn resize_no_op_when_below_max() {
        let img = DynamicImage::new_rgba8(100, 50);
        let (out, scale) = resize_long_edge(img, 200);
        assert_eq!(out.dimensions(), (100, 50));
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn resize_shrinks_to_long_edge() {
        let img = DynamicImage::new_rgba8(400, 100);
        let (out, scale) = resize_long_edge(img, 200);
        assert_eq!(out.width(), 200);
        assert!((scale - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mask_overrides_pixels_in_rectangle() {
        let mut img = DynamicImage::new_rgba8(8, 8);
        let buf = img.as_mut_rgba8().unwrap();
        for x in 0..8 {
            for y in 0..8 {
                buf.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let out = mask_rectangles(img, &[[2, 2, 4, 4]]);
        let buf = out.as_rgba8().unwrap();
        assert_eq!(buf.get_pixel(3, 3).0, [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn mask_clamps_out_of_bounds_rect() {
        let img = DynamicImage::new_rgba8(8, 8);
        // Rectangle extending past the image — should not panic.
        let out = mask_rectangles(img, &[[-10, -10, 100, 100]]);
        assert_eq!(out.dimensions(), (8, 8));
        // All pixels masked black.
        let buf = out.as_rgba8().unwrap();
        assert_eq!(buf.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(7, 7).0, [0, 0, 0, 255]);
    }
}
