//! Terminal-native thumbnail cards also work without an image protocol or Nerd Font.
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThumbnailPlaceholder {
    Pending,
    Loading,
    Unavailable,
}

pub(super) fn draw_thumbnail_placeholder(
    frame: &mut Frame,
    area: Rect,
    state: ThumbnailPlaceholder,
    spinner_idx: usize,
) {
    let area = area.intersection(frame.area());
    if area.width == 0 || area.height == 0 {
        return;
    }
    let style = Style::default().fg(Color::Gray).bg(Color::Rgb(43, 47, 54));
    let card = Block::default()
        .style(style)
        .border_type(BorderType::Rounded);
    let card = if area.width >= 4 && area.height >= 3 {
        card.borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
    } else {
        card
    };
    let inner = card.inner(area);
    frame.render_widget(card, area);
    let symbol = match state {
        ThumbnailPlaceholder::Pending => "◇",
        ThumbnailPlaceholder::Loading => {
            super::SPINNER_FRAMES[spinner_idx % super::SPINNER_FRAMES.len()]
        }
        ThumbnailPlaceholder::Unavailable => "×",
    };
    let show_caption = inner.width >= 22 && inner.height >= 5;
    let y = inner.y
        + inner
            .height
            .saturating_sub(if show_caption { 3 } else { 1 })
            / 2;
    frame.render_widget(
        Paragraph::new(symbol)
            .alignment(Alignment::Center)
            .style(style),
        Rect::new(inner.x, y, inner.width, 1),
    );
    if show_caption {
        let label = match state {
            ThumbnailPlaceholder::Pending => "Waiting for thumbnail",
            ThumbnailPlaceholder::Loading => "Loading thumbnail",
            ThumbnailPlaceholder::Unavailable => "Unavailable · r/p retry",
        };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(style),
            Rect::new(inner.x, y + 2, inner.width, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn all_states_clip_safely_at_thumbnail_and_tiny_terminal_sizes() {
        for (width, height) in [(0, 0), (1, 1), (2, 1), (4, 2), (6, 3), (8, 4), (30, 10)] {
            for state in [
                ThumbnailPlaceholder::Pending,
                ThumbnailPlaceholder::Loading,
                ThumbnailPlaceholder::Unavailable,
            ] {
                let mut terminal =
                    Terminal::new(TestBackend::new(width.max(1), height.max(1))).unwrap();
                terminal
                    .draw(|frame| {
                        draw_thumbnail_placeholder(
                            frame,
                            Rect::new(0, 0, width, height),
                            state,
                            usize::MAX,
                        );
                        draw_thumbnail_placeholder(
                            frame,
                            Rect::new(width + 1, height + 1, 5, 4),
                            state,
                            0,
                        );
                    })
                    .unwrap();
            }
        }
    }

    #[test]
    fn export_thumbnail_cards_for_visual_review() {
        let Ok(path) = std::env::var("PIKPAKTUI_PLACEHOLDER_SNAPSHOT") else {
            return;
        };
        let mut terminal = Terminal::new(TestBackend::new(96, 17)).unwrap();
        terminal
            .draw(|frame| {
                for (index, state) in [
                    ThumbnailPlaceholder::Pending,
                    ThumbnailPlaceholder::Loading,
                    ThumbnailPlaceholder::Unavailable,
                ]
                .into_iter()
                .enumerate()
                {
                    let x = index as u16 * 32;
                    let label = ["Waiting", "Loading", "Unavailable"][index];
                    frame.render_widget(
                        Paragraph::new(label)
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::Gray)),
                        Rect::new(x, 0, 30, 1),
                    );
                    draw_thumbnail_placeholder(frame, Rect::new(x, 2, 30, 9), state, 0);
                    draw_thumbnail_placeholder(frame, Rect::new(x, 12, 8, 4), state, 0);
                    frame.render_widget(
                        Paragraph::new("sample-video.mp4").style(Style::default().fg(Color::Cyan)),
                        Rect::new(x + 9, 12, 21, 1),
                    );
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let value = serde_json::json!({"width":96,"height":17,"cells":buffer.content.iter().map(|cell| serde_json::json!({"s":cell.symbol(),"fg":format!("{:?}",cell.fg),"bg":format!("{:?}",cell.bg)})).collect::<Vec<_>>()});
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
}
