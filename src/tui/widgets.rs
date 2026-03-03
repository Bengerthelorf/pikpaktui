use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Compute the scroll offset so that `selected` is always visible
/// within a window of `max_visible` items.
pub(super) fn scroll_offset(selected: usize, max_visible: usize) -> usize {
    if selected >= max_visible {
        selected - max_visible + 1
    } else {
        0
    }
}

/// Append an "... and N more" indicator when there are items beyond the visible window.
pub(super) fn push_remaining_indicator<'a>(
    lines: &mut Vec<Line<'a>>,
    total: usize,
    window_start: usize,
    window_size: usize,
) {
    let remaining = total.saturating_sub(window_start + window_size);
    if remaining > 0 {
        lines.push(Line::from(Span::styled(
            format!("   ... and {} more", remaining),
            Style::default().fg(Color::DarkGray),
        )));
    }
}

/// Create a styled gray empty-state message line.
pub(super) fn empty_state_line(message: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}", message),
        Style::default().fg(Color::DarkGray),
    ))
}

/// Compute a dynamic popup height percentage based on item count.
///
/// Returns a percentage of terminal height clamped between `min_pct` and `max_pct`.
pub(super) fn dynamic_overlay_height(
    item_count: usize,
    max_items: usize,
    terminal_height: u16,
    min_pct: u16,
    max_pct: u16,
) -> u16 {
    let visible = item_count.min(max_items);
    let total_lines = 2 + visible.max(1) + 2; // padding + items + hint + padding
    ((total_lines as u16 * 100) / terminal_height.max(1))
        .max(min_pct)
        .min(max_pct)
}
