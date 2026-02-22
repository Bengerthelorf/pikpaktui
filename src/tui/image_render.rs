use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Upscale `img` so it fills at least `area` terminal cells (using `font_size` px/cell).
/// If the image is already large enough, returns a clone unchanged.
/// This ensures protocol renderers (Kitty/iTerm2) don't render at native pixel size
/// when the thumbnail API returned a low-resolution image.
pub(super) fn upscale_for_rect(
    img: &image::DynamicImage,
    area: Rect,
    font_size: (u16, u16),
) -> image::DynamicImage {
    let target_px_w = area.width as u32 * font_size.0 as u32;
    let target_px_h = area.height as u32 * font_size.1 as u32;
    if target_px_w == 0 || target_px_h == 0 {
        return img.clone();
    }
    if img.width() < target_px_w || img.height() < target_px_h {
        img.resize(target_px_w, target_px_h, image::imageops::FilterType::Lanczos3)
    } else {
        img.clone()
    }
}

/// Compute a horizontally-centered sub-rect for a protocol image inside `area`.
/// When the image is portrait (height-constrained), it renders narrower than
/// `area.width`; this centers it so the text below is unaffected.
pub(super) fn center_image_rect(img: &image::DynamicImage, area: Rect) -> Rect {
    use image::GenericImageView;
    let (orig_w, orig_h) = img.dimensions();
    if orig_h == 0 || area.height == 0 {
        return area;
    }
    let orig_aspect = orig_w as f32 / orig_h as f32;
    // Natural width (cols) if the image fills all available rows
    let natural_w = (area.height as f32 * 2.0 * orig_aspect) as u16;
    if natural_w >= area.width {
        // Landscape / square: already fills full width, no shift needed
        return area;
    }
    // Portrait: narrower than the panel — center horizontally
    Rect {
        x: area.x + (area.width - natural_w) / 2,
        y: area.y,
        width: natural_w,
        height: area.height,
    }
}

/// Render image to colored halfblock lines.
pub(super) fn render_image_to_colored_lines(
    img: &image::DynamicImage,
    max_width: u32,
    max_height: u32,
) -> Vec<Line<'static>> {
    use image::GenericImageView;

    let (orig_w, orig_h) = img.dimensions();
    let orig_aspect = orig_w as f32 / orig_h as f32;

    // Terminal characters are ~2x taller than wide
    let target_width = max_width;
    let target_height_chars = ((target_width as f32 / orig_aspect) / 2.0) as u32;

    let (final_width, final_height_chars) = if target_height_chars > max_height {
        let h = max_height;
        let w = (h as f32 * 2.0 * orig_aspect) as u32;
        (w, h)
    } else {
        (target_width, target_height_chars)
    };

    let left_pad = (max_width.saturating_sub(final_width) / 2) as usize;

    // Resize to double height (each char shows 2 pixels vertically)
    let img = img.resize(
        final_width,
        final_height_chars * 2,
        image::imageops::FilterType::Lanczos3,
    );

    let (w, h) = img.dimensions();
    let mut lines = Vec::new();

    for y in 0..final_height_chars {
        let mut spans = Vec::new();
        if left_pad > 0 {
            spans.push(Span::raw(" ".repeat(left_pad)));
        }
        for x in 0..w {
            let y_top = (y * 2).min(h - 1);
            let y_bottom = (y * 2 + 1).min(h - 1);

            let top_pixel = img.get_pixel(x, y_top);
            let bottom_pixel = img.get_pixel(x, y_bottom);

            let span = Span::styled(
                "▀",
                Style::default()
                    .fg(Color::Rgb(top_pixel[0], top_pixel[1], top_pixel[2]))
                    .bg(Color::Rgb(bottom_pixel[0], bottom_pixel[1], bottom_pixel[2])),
            );
            spans.push(span);
        }
        lines.push(Line::from(spans));
    }

    lines
}

/// Render image to grayscale ASCII art lines.
pub(super) fn render_image_to_grayscale_lines(
    img: &image::DynamicImage,
    max_width: u32,
    max_height: u32,
) -> Vec<Line<'static>> {
    use image::GenericImageView;

    const ASCII_CHARS: &[char] = &[' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];

    let (orig_w, orig_h) = img.dimensions();
    let orig_aspect = orig_w as f32 / orig_h as f32;

    // Terminal characters are ~2x taller than wide
    let target_width = max_width;
    let target_height_chars = ((target_width as f32 / orig_aspect) / 2.0) as u32;

    let (final_width, final_height_chars) = if target_height_chars > max_height {
        let h = max_height;
        let w = (h as f32 * 2.0 * orig_aspect) as u32;
        (w, h)
    } else {
        (target_width, target_height_chars)
    };

    let left_pad = (max_width.saturating_sub(final_width) / 2) as usize;

    // Resize to double height (sample every 2 rows)
    let img = img.resize(
        final_width,
        final_height_chars * 2,
        image::imageops::FilterType::Lanczos3,
    );

    let (w, h) = img.dimensions();
    let mut lines = Vec::new();

    for y in 0..final_height_chars {
        let mut line_str = if left_pad > 0 { " ".repeat(left_pad) } else { String::new() };
        for x in 0..w {
            // Average 2 vertical pixels
            let y1 = (y * 2).min(h - 1);
            let y2 = (y * 2 + 1).min(h - 1);

            let pixel1 = img.get_pixel(x, y1);
            let pixel2 = img.get_pixel(x, y2);

            let brightness = ((pixel1[0] as u32 + pixel2[0] as u32) / 2
                + (pixel1[1] as u32 + pixel2[1] as u32) / 2
                + (pixel1[2] as u32 + pixel2[2] as u32) / 2)
                / 3;

            let idx = (brightness as usize * ASCII_CHARS.len()) / 256;
            line_str.push(ASCII_CHARS[idx.min(ASCII_CHARS.len() - 1)]);
        }
        lines.push(Line::from(line_str));
    }

    lines
}
