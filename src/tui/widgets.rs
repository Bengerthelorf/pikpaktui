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

/// Rendered line index of each settings item's NAME row, given the number of
/// items in each category. The lines vec rendered by `draw_settings_overlay`
/// has a leading blank at index 0, then per category one header line followed
/// by two lines (name + description) per item. Both the overlay's scroll math
/// and the mouse hit-test derive their layout from this, so a click can't land
/// on a different item than the one drawn under the cursor.
pub(super) fn settings_item_line_map(category_item_counts: &[usize]) -> Vec<usize> {
    let mut map = Vec::new();
    let mut line = 1; // leading blank occupies line 0
    for &count in category_item_counts {
        line += 1; // category header
        for _ in 0..count {
            map.push(line);
            line += 2; // name row + description row
        }
    }
    map
}

/// Scroll offset that keeps the selected settings item visible within
/// `inner_height` rows, using the item->line map from `settings_item_line_map`.
/// The +2 keeps the item's description row on screen too.
pub(super) fn settings_scroll_offset(
    item_line_map: &[usize],
    selected: usize,
    inner_height: usize,
) -> usize {
    let selected_line = item_line_map.get(selected).copied().unwrap_or(0);
    if selected_line >= inner_height {
        (selected_line + 2).saturating_sub(inner_height)
    } else {
        0
    }
}

/// Reverse of `settings_item_line_map`: given a click at visual row `content_y`
/// inside the scrolled overlay, return the clicked item index and whether the
/// click landed on its name row (vs its description row). Returns `None` for the
/// leading blank, category headers, and the trailing hint area.
pub(super) fn settings_item_at_row(
    item_line_map: &[usize],
    scroll_offset: usize,
    content_y: usize,
) -> Option<(usize, bool)> {
    let abs_line = scroll_offset + content_y;
    item_line_map
        .iter()
        .position(|&name_line| name_line == abs_line || name_line + 1 == abs_line)
        .map(|idx| (idx, item_line_map[idx] == abs_line))
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

#[cfg(test)]
mod tests {
    use super::*;

    // The real Settings overlay: 7 categories with these item counts (17 items).
    const COUNTS: [usize; 7] = [5, 5, 2, 2, 1, 1, 1];

    #[test]
    fn line_map_accounts_for_blank_and_headers() {
        let map = settings_item_line_map(&COUNTS);
        assert_eq!(map.len(), 17);
        // blank=0, cat0 header=1, item0 name=2, item0 desc=3, item1 name=4...
        assert_eq!(map[0], 2);
        assert_eq!(map[1], 4);
        assert_eq!(map[4], 10); // last item of cat0 (names at 2,4,6,8,10)
        // cat1 header at line 12 (after cat0 consumed 5*2 lines), item5 name at 13.
        assert_eq!(map[5], 13);
        assert!(map.windows(2).all(|w| w[0] < w[1])); // strictly increasing
    }

    #[test]
    fn item_at_row_unscrolled_maps_name_and_desc_rows() {
        let map = settings_item_line_map(&COUNTS);
        // Blank, category header, and a gap line select nothing.
        assert_eq!(settings_item_at_row(&map, 0, 0), None); // leading blank
        assert_eq!(settings_item_at_row(&map, 0, 1), None); // cat0 header
        assert_eq!(settings_item_at_row(&map, 0, 12), None); // cat1 header
        // Name row toggles (is_name = true); description row selects only.
        assert_eq!(settings_item_at_row(&map, 0, 2), Some((0, true)));
        assert_eq!(settings_item_at_row(&map, 0, 3), Some((0, false)));
        assert_eq!(settings_item_at_row(&map, 0, 4), Some((1, true)));
    }

    #[test]
    fn item_at_row_honors_scroll_offset() {
        let map = settings_item_line_map(&COUNTS);
        // Scrolled so that line 13 (item5 name) is the first visible row.
        let scroll = 13;
        assert_eq!(settings_item_at_row(&map, scroll, 0), Some((5, true)));
        assert_eq!(settings_item_at_row(&map, scroll, 1), Some((5, false)));
        // Without the scroll compensation this same click would hit item 0.
        assert_eq!(settings_item_at_row(&map, 0, 0), None);
    }

    #[test]
    fn scroll_offset_keeps_selection_visible() {
        let map = settings_item_line_map(&COUNTS);
        let inner = 11;
        assert_eq!(settings_scroll_offset(&map, 0, inner), 0); // top item, no scroll
        let off = settings_scroll_offset(&map, 16, inner); // last item
        assert!(off > 0);
        // The selected item's name row must be within the visible window.
        assert!(map[16] >= off && map[16] < off + inner);
    }
}
