use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};

use crate::config::{BorderStyle, ColorScheme};
use crate::pikpak::{Entry, EntryKind};
use crate::theme;

use super::completion::PathInput;
use super::image_render::{center_image_rect, render_image_to_colored_lines, render_image_to_grayscale_lines, upscale_for_rect};
use super::local_completion::LocalPathInput;
use super::widgets;
use super::{App, InputMode, LoginField, PickerState, PreviewState, SPINNER_FRAMES, centered_rect, format_size, truncate_name};

impl App {
    /// Returns `true` when a popup overlay is active that may cover the preview pane.
    /// Used to suppress terminal-image-protocol rendering so that iTerm2 / Kitty
    /// don't leave stale image data under the overlay.
    fn has_overlay(&self) -> bool {
        if matches!(self.input, InputMode::DownloadView)
            && self.download_view_mode == super::DownloadViewMode::Collapsed
        {
            return true;
        }
        !matches!(
            self.input,
            InputMode::Normal
                | InputMode::Login { .. }
                | InputMode::MovePicker { .. }
                | InputMode::CopyPicker { .. }
                | InputMode::DownloadView
        )
    }

    fn draw_trash_view(
        &self,
        f: &mut Frame,
        entries: &[Entry],
        selected: usize,
        expanded: bool,
    ) {
        let title = format!(" Trash ({}) ", entries.len());
        let (tr_bc, tr_tc) = if self.is_vibrant() {
            (Color::LightRed, Color::LightRed)
        } else {
            (Color::Cyan, Color::Red)
        };

        if expanded {
            let (list_area, help_bar_area) = self.layout_with_help_bar(f.area());

            if entries.is_empty() {
                let lines = vec![
                    Line::from(""),
                    widgets::empty_state_line("Trash is empty."),
                ];
                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(tr_tc))
                        .border_style(Style::default().fg(tr_bc)),
                );
                f.render_widget(p, list_area);
            } else {
                let mut lines = vec![Line::from("")];
                let max_visible = list_area.height.saturating_sub(4) as usize;
                let scroll_offset = widgets::scroll_offset(selected, max_visible);
                let name_max = list_area.width.saturating_sub(20) as usize;

                for (i, entry) in entries.iter().enumerate().skip(scroll_offset).take(max_visible) {
                    let is_sel = i == selected;
                    let prefix = if is_sel { " \u{203a} " } else { "   " };
                    let cat = theme::categorize(entry);
                    let icon = theme::cli_icon(cat, self.config.nerd_font);
                    let icon_color = self.file_color(cat);
                    let size_str = if entry.kind == EntryKind::Folder {
                        "-".to_string()
                    } else {
                        format_size(entry.size)
                    };
                    let name_style = if is_sel {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Reset)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, name_style),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(truncate_name(&entry.name, name_max), name_style),
                        Span::styled(
                            format!("  {:>9}", size_str),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }

                widgets::push_remaining_indicator(&mut lines, entries.len(), scroll_offset, max_visible);

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(tr_tc))
                        .border_style(Style::default().fg(tr_bc)),
                );
                f.render_widget(p, list_area);
            }

            if let Some(bar_area) = help_bar_area {
                let pairs = self.help_pairs();
                let mut spans = vec![Span::raw(" ")];
                spans.extend(Self::styled_help_spans(&pairs));
                let bar = Paragraph::new(Line::from(spans));
                f.render_widget(bar, bar_area);
            }
        } else {
            let pct = widgets::dynamic_overlay_height(entries.len(), 15, f.area().height, 25, 75);
            let area = centered_rect(75, pct, f.area());
            clear_overlay_area(f, area);

            if entries.is_empty() {
                let mut lines = vec![
                    Line::from(""),
                    widgets::empty_state_line("Trash is empty."),
                ];
                lines.push(Line::from(""));
                let hints = vec![("r", "refresh"), ("Esc", "close")];
                let mut hint_spans = vec![Span::raw("  ")];
                hint_spans.extend(Self::styled_help_spans(&hints));
                lines.push(Line::from(hint_spans));

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(tr_tc))
                        .border_style(Style::default().fg(tr_bc)),
                );
                f.render_widget(p, area);
            } else {
                let mut lines = vec![Line::from("")];
                let max_visible = 15;
                let scroll_offset = widgets::scroll_offset(selected, max_visible);

                for (i, entry) in entries.iter().enumerate().skip(scroll_offset).take(max_visible) {
                    let is_sel = i == selected;
                    let prefix = if is_sel { " \u{203a} " } else { "   " };
                    let cat = theme::categorize(entry);
                    let icon = theme::cli_icon(cat, self.config.nerd_font);
                    let icon_color = self.file_color(cat);
                    let size_str = if entry.kind == EntryKind::Folder {
                        "-".to_string()
                    } else {
                        format_size(entry.size)
                    };
                    let name_style = if is_sel {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Reset)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, name_style),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(truncate_name(&entry.name, 35), name_style),
                        Span::styled(
                            format!("  {:>9}", size_str),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }

                widgets::push_remaining_indicator(&mut lines, entries.len(), scroll_offset, max_visible);

                lines.push(Line::from(""));
                let hints = vec![
                    ("j/k", "nav"),
                    ("Enter", "expand"),
                    ("Space", "info"),
                    ("u", "restore"),
                    ("x", "delete"),
                    ("r", "refresh"),
                    ("Esc", "close"),
                ];
                let mut hint_spans = vec![Span::raw("  ")];
                hint_spans.extend(Self::styled_help_spans(&hints));
                lines.push(Line::from(hint_spans));

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(tr_tc))
                        .border_style(Style::default().fg(tr_bc)),
                );
                f.render_widget(p, area);
            }
        }
    }

    fn draw_confirm_play_overlay(&self, f: &mut Frame, name: &str, _url: &str) {
        let area = self.prepare_overlay(f, 60, 20);
        let player_display = self.config.player.as_deref().unwrap_or("not configured");
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        let truncated_name = if name.len() > 40 {
            format!("{}...", &name[..37])
        } else {
            name.to_string()
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Play ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("\"{}\"", truncated_name),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("?", Style::default().fg(Color::Cyan)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Open with: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        player_display,
                        if self.config.player.is_some() {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
                ]),
                Line::from(""),
                Self::hint_line(&[("y/Enter", "play"), ("n/Esc", "cancel")]),
            ])
            .block(self.overlay_block("Play Video", bc, tc)),
            area,
        );
    }

    fn draw_play_picker_overlay(
        &self,
        f: &mut Frame,
        name: &str,
        medias: &[super::PlayOption],
        selected: usize,
    ) {
        let height = std::cmp::min(50, 20 + medias.len() as u16 * 2);
        let area = centered_rect(60, height, f.area());
        clear_overlay_area(f, area);

        let truncated_name = if name.len() > 40 {
            format!("{}...", &name[..37])
        } else {
            name.to_string()
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Play ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("\"{}\"", truncated_name),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
        ];

        for (i, opt) in medias.iter().enumerate() {
            let is_selected = i == selected;
            let prefix = if is_selected { " > " } else { "   " };
            let style = if !opt.available {
                Style::default().fg(Color::DarkGray)
            } else if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Reset)
            };
            let suffix = if !opt.available { " (cold)" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(opt.label.clone(), style),
                Span::styled(suffix, Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[("Enter", "play"), ("Esc", "cancel")]));

        let (bc, tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(self.overlay_block("Select Stream", bc, tc)),
            area,
        );
    }

    fn draw_player_input_overlay(&self, f: &mut Frame, value: &str) {
        let area = self.prepare_overlay(f, 60, 20);
        let cur = if self.cursor_visible { "\u{2588}" } else { " " };
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightYellow, Color::LightYellow)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Enter player command (e.g. mpv, open -a IINA):",
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  > ", Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Self::hint_line(&[("Enter", "confirm"), ("Esc", "cancel")]),
            ])
            .block(self.overlay_block("Player Command", bc, tc)),
            area,
        );
    }

    pub(super) fn draw(&self, f: &mut Frame) {
        match &self.input {
            InputMode::Login { .. } => self.draw_login_screen(f),
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => self.draw_picker(f),
            InputMode::CartMovePicker { .. } | InputMode::CartCopyPicker { .. } => {
                self.draw_cart_picker(f)
            }
            InputMode::DownloadView => {
                if self.download_view_mode == super::DownloadViewMode::Collapsed {
                    self.draw_main(f);
                    self.draw_download_collapsed(f);
                } else {
                    self.draw_download_expanded(f);
                }
            }
            InputMode::TrashView {
                entries,
                selected,
                expanded: true,
            } => {
                if self.loading {
                    self.draw_trash_view(f, entries, *selected, true);
                    self.draw_info_loading_overlay(f);
                } else {
                    self.draw_trash_view(f, entries, *selected, true);
                }
            }
            InputMode::MySharesView {
                shares,
                selected,
                confirm_delete,
            } => {
                self.draw_my_shares_view(f, shares, *selected, confirm_delete.as_deref());
                if self.loading {
                    self.draw_info_loading_overlay(f);
                }
                if self.show_logs_overlay {
                    let log_area = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                        .split(f.area())[1];
                    self.draw_log_overlay(f, log_area);
                }
            }
            InputMode::InfoView { info, image, has_thumbnail } if !self.trash_entries.is_empty() => {
                self.draw_trash_view(
                    f,
                    &self.trash_entries,
                    self.trash_selected,
                    self.trash_expanded,
                );
                self.draw_info_overlay(f, info, image.as_ref(), *has_thumbnail);
            }
            _ => self.draw_main(f),
        }
    }

    pub(super) fn styled_block(&self) -> Block<'static> {
        let block = Block::default().borders(Borders::ALL);
        match self.config.border_style {
            BorderStyle::Rounded => block.border_type(BorderType::Rounded),
            BorderStyle::Thick | BorderStyle::ThickRounded => block.border_type(BorderType::Thick),
            BorderStyle::Double => block.border_type(BorderType::Double),
        }
    }

    pub(super) fn is_vibrant(&self) -> bool {
        self.config.color_scheme == ColorScheme::Vibrant
    }

    /// Returns `(border, title)` colors for a single base color.
    /// In vibrant mode, both are the light variant; otherwise both are `base`.
    fn themed_colors(&self, base: Color) -> (Color, Color) {
        if self.is_vibrant() {
            let v = vibrant(base);
            (v, v)
        } else {
            (base, base)
        }
    }

    /// Returns `(border, title)` colors for distinct base colors.
    /// In vibrant mode, each is mapped to its light variant.
    fn themed_colors_pair(&self, border: Color, title: Color) -> (Color, Color) {
        if self.is_vibrant() {
            (vibrant(border), vibrant(title))
        } else {
            (border, title)
        }
    }

    /// Split the area into a main content region and an optional help bar row.
    fn layout_with_help_bar(&self, area: Rect) -> (Rect, Option<Rect>) {
        if self.config.show_help_bar {
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);
            (outer[0], Some(outer[1]))
        } else {
            (area, None)
        }
    }

    fn prepare_overlay(&self, f: &mut Frame, pct_x: u16, pct_y: u16) -> Rect {
        let area = centered_rect(pct_x, pct_y, f.area());
        clear_overlay_area(f, area);
        area
    }

    fn overlay_block(&self, title: &str, bc: Color, tc: Color) -> Block<'static> {
        self.styled_block()
            .title(Span::styled(
                format!(" {} ", title),
                Style::default().fg(tc),
            ))
            .border_style(Style::default().fg(bc))
    }

    fn hint_line(hints: &[(&str, &str)]) -> Line<'static> {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(Self::styled_help_spans(hints));
        Line::from(spans)
    }

    fn file_color(&self, cat: theme::FileCategory) -> Color {
        self.config.get_color(cat)
    }

    /// Highlight style for selected items.
    fn highlight_style(&self) -> Style {
        if self.is_vibrant() {
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        }
    }

    fn draw_login_screen(&self, f: &mut Frame) {
        let bg = Block::default().style(Style::default().bg(Color::Reset));
        f.render_widget(bg, f.area());
        let area = centered_rect(50, 40, f.area());

        if let InputMode::Login {
            field,
            email,
            password,
            error,
            logging_in,
        } = &self.input
        {
            clear_overlay_area(f, area);
            let email_style = match field {
                LoginField::Email => Style::default().fg(Color::Yellow),
                LoginField::Password => Style::default().fg(Color::Reset),
            };
            let pass_style = match field {
                LoginField::Password => Style::default().fg(Color::Yellow),
                LoginField::Email => Style::default().fg(Color::Reset),
            };
            let masked: String = "*".repeat(password.len());
            let cur = if self.cursor_visible { "\u{2588}" } else { " " };
            let ec = if matches!(field, LoginField::Email) {
                cur
            } else {
                ""
            };
            let pc = if matches!(field, LoginField::Password) {
                cur
            } else {
                ""
            };

            let mut lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Email:    ", email_style),
                    Span::styled(format!("{}{}", email, ec), email_style),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Password: ", pass_style),
                    Span::styled(format!("{}{}", masked, pc), pass_style),
                ]),
                Line::from(""),
            ];
            if *logging_in {
                lines.push(Line::from(Span::styled(
                    "  Logging in...",
                    Style::default().fg(Color::Cyan),
                )));
            } else if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    format!("  {}", err),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(""));
            let login_hints = vec![("Tab", "switch"), ("Enter", "login"), ("Esc", "quit")];
            let mut hint_spans = vec![Span::raw("  ")];
            hint_spans.extend(Self::styled_help_spans(&login_hints));
            lines.push(Line::from(hint_spans));

            let (bc, tc) = self.themed_colors(Color::Cyan);
            let p = Paragraph::new(Text::from(lines))
                .block(
                    self.styled_block()
                        .title(" PikPak Login ")
                        .title_style(Style::default().fg(tc))
                        .border_style(Style::default().fg(bc)),
                )
                .wrap(Wrap { trim: false });
            f.render_widget(p, area);
        }
    }

    fn draw_main(&self, f: &mut Frame) {
        let (main_area, help_bar_area) = self.layout_with_help_bar(f.area());

        if self.config.show_preview {
            // Three-column miller columns: parent 20% | current 40% | preview 40%
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(40),
                    Constraint::Percentage(40),
                ])
                .split(main_area);

            self.parent_pane_area.set(chunks[0]);
            self.current_pane_area.set(chunks[1]);
            self.preview_pane_area.set(chunks[2]);

            self.draw_parent_pane(f, chunks[0]);
            self.draw_current_pane(f, chunks[1]);
            self.draw_preview_pane(f, chunks[2]);

            // Log overlay (covers right pane area)
            if self.show_logs_overlay {
                self.draw_log_overlay(f, chunks[2]);
            }
        } else {
            // Two-column: parent 25% | current 75%
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                .split(main_area);

            self.parent_pane_area.set(chunks[0]);
            self.current_pane_area.set(chunks[1]);
            self.preview_pane_area.set(ratatui::layout::Rect::default());

            self.draw_parent_pane(f, chunks[0]);
            self.draw_current_pane(f, chunks[1]);

            // Log overlay on rightmost 40%
            if self.show_logs_overlay {
                let log_area = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(main_area)[1];
                self.draw_log_overlay(f, log_area);
            }
        }

        // Help bar
        if let Some(bar_area) = help_bar_area {
            let pairs = self.help_pairs();
            let mut help_spans = vec![Span::raw(" ")];
            help_spans.extend(Self::styled_help_spans(&pairs));

            // Build quota spans — style determined by config
            let quota_info = match (self.quota_used, self.quota_limit) {
                (Some(used), Some(limit)) if limit > 0 => {
                    let pct = (used as f64 / limit as f64).clamp(0.0, 1.0);
                    let bar_color = if pct >= 0.9 {
                        Color::Red
                    } else if pct >= 0.7 {
                        Color::Yellow
                    } else {
                        Color::Cyan
                    };
                    use crate::config::QuotaBarStyle;
                    match self.config.quota_bar_style {
                        QuotaBarStyle::Bar => {
                            const BAR_W: usize = 10;
                            let filled = (pct * BAR_W as f64).round() as usize;
                            let used_str = format_size(used);
                            let limit_str = format_size(limit);
                            let total_w = (3 + used_str.len() + 3 + limit_str.len() + 2 + BAR_W + 1) as u16;
                            let spans: Vec<Span<'static>> = vec![
                                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                                Span::styled(used_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                                Span::styled(" / ", Style::default().fg(Color::DarkGray)),
                                Span::styled(limit_str, Style::default().fg(Color::DarkGray)),
                                Span::styled("  ", Style::default()),
                                Span::styled("▪".repeat(filled), Style::default().fg(bar_color)),
                                Span::styled("▫".repeat(BAR_W - filled), Style::default().fg(Color::DarkGray)),
                                Span::styled(" ", Style::default()),
                            ];
                            Some((spans, total_w))
                        }
                        QuotaBarStyle::Percent => {
                            let pct_str = format!("{:.0}%", pct * 100.0);
                            let used_str = format_size(used);
                            let limit_str = format_size(limit);
                            // " │ " + used + " / " + limit + " " + pct + " "
                            let total_w = (3 + used_str.len() + 3 + limit_str.len() + 1 + pct_str.len() + 1) as u16;
                            let spans: Vec<Span<'static>> = vec![
                                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                                Span::styled(used_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                                Span::styled(" / ", Style::default().fg(Color::DarkGray)),
                                Span::styled(limit_str, Style::default().fg(Color::DarkGray)),
                                Span::styled(" ", Style::default()),
                                Span::styled(pct_str, Style::default().fg(bar_color).add_modifier(Modifier::BOLD)),
                                Span::styled(" ", Style::default()),
                            ];
                            Some((spans, total_w))
                        }
                    }
                }
                (Some(used), None) => {
                    let used_str = format_size(used);
                    let total_w = (3 + used_str.len() + 6) as u16;
                    let spans: Vec<Span<'static>> = vec![
                        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(used_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        Span::styled(" used ", Style::default().fg(Color::DarkGray)),
                    ];
                    Some((spans, total_w))
                }
                _ => None,
            };

            if let Some((quota_spans, quota_w)) = quota_info {
                let quota_w = quota_w.min(bar_area.width.saturating_sub(4));
                let help_w = bar_area.width.saturating_sub(quota_w);
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(help_w), Constraint::Length(quota_w)])
                    .split(bar_area);
                f.render_widget(Paragraph::new(Line::from(help_spans)), chunks[0]);
                f.render_widget(Paragraph::new(Line::from(quota_spans)), chunks[1]);
            } else {
                f.render_widget(Paragraph::new(Line::from(help_spans)), bar_area);
            }
        }

        self.draw_overlay(f);

        if self.shares_pending && self.loading {
            self.draw_info_loading_overlay(f);
        }

        if self.show_help_sheet {
            self.draw_help_sheet(f);
        }
    }

    fn draw_parent_pane(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        if self.breadcrumb.is_empty() {
            // At root — show empty panel
            let p = Paragraph::new(Text::from(vec![])).block(
                self.styled_block()
                    .title(" / ")
                    .title_style(Style::default().fg(Color::DarkGray))
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            f.render_widget(p, area);
        } else {
            let parent_path = if self.breadcrumb.len() <= 1 {
                " / ".to_string()
            } else {
                let path: Vec<&str> = self.breadcrumb[..self.breadcrumb.len() - 1]
                    .iter()
                    .map(|(_, n)| n.as_str())
                    .collect();
                format!(" /{} ", path.join("/"))
            };

            let items: Vec<ListItem> = self
                .parent_entries
                .iter()
                .map(|e| {
                    let cat = theme::categorize(e);
                    let ico = theme::icon(cat, self.config.nerd_font);
                    let c = self.file_color(cat);
                    ListItem::new(Line::from(vec![
                        Span::styled(ico, Style::default().fg(c)),
                        Span::styled(" ", Style::default()),
                        Span::styled(&e.name, Style::default().fg(c)),
                    ]))
                })
                .collect();

            let mut state = ListState::default();
            if !self.parent_entries.is_empty() {
                state.select(Some(
                    self.parent_selected.min(self.parent_entries.len() - 1),
                ));
            }

            let list = List::new(items)
                .block(
                    self.styled_block()
                        .title(parent_path)
                        .title_style(Style::default().fg(Color::DarkGray))
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, area, &mut state);
            self.parent_scroll_offset.set(state.offset());
        }
    }

    fn draw_current_pane(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let path_display = self.current_path_display();
        let title = if self.loading {
            format!(" {} {} ", SPINNER_FRAMES[self.spinner_idx], path_display)
        } else {
            format!(" {} ", path_display)
        };

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let cat = theme::categorize(e);
                let ico = theme::icon(cat, self.config.nerd_font);
                let c = self.file_color(cat);
                let size_str = match e.kind {
                    EntryKind::Folder => String::new(),
                    EntryKind::File => format!("  {}", format_size(e.size)),
                };
                let star_marker = if e.starred { "\u{2605} " } else { "" };
                let cart_marker = if self.cart_ids.contains(&e.id) {
                    "\u{2606} "
                } else {
                    ""
                };
                ListItem::new(Line::from(vec![
                    Span::styled(ico, Style::default().fg(c)),
                    Span::styled(" ", Style::default()),
                    Span::styled(star_marker, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        cart_marker,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::DIM),
                    ),
                    Span::styled(&e.name, Style::default().fg(c)),
                    Span::styled(size_str, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let mut state = ListState::default();
        if !self.entries.is_empty() {
            state.select(Some(self.selected.min(self.entries.len() - 1)));
        }

        let (file_bc, file_tc) = if self.is_vibrant() {
            (Color::LightBlue, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Green)
        };
        let list = List::new(items)
            .block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(file_tc))
                    .border_style(Style::default().fg(file_bc)),
            )
            .highlight_style(self.highlight_style())
            .highlight_symbol("\u{203a} ");
        f.render_stateful_widget(list, area, &mut state);
        self.scroll_offset.set(state.offset());
        self.list_area_height.set(area.height);
    }

    fn draw_preview_pane(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        match &self.preview_state {
            PreviewState::Empty => {
                let hint = if self.config.lazy_preview {
                    "Select an item"
                } else {
                    "Press p to load preview"
                };
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("  {}", hint),
                        Style::default().fg(Color::DarkGray),
                    )),
                ]))
                .block(
                    self.styled_block()
                        .title(" Preview ")
                        .title_style(Style::default().fg(Color::DarkGray))
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(p, area);
            }
            PreviewState::Loading => {
                let spinner = SPINNER_FRAMES[self.spinner_idx];
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("  {} Loading...", spinner),
                        Style::default().fg(Color::Cyan),
                    )),
                ]))
                .block(
                    self.styled_block()
                        .title(" Preview ")
                        .title_style(Style::default().fg(Color::DarkGray))
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(p, area);
            }
            PreviewState::FolderListing(children) => {
                let visible_h = area.height.saturating_sub(2) as usize;
                let max_scroll = children.len().saturating_sub(visible_h.max(1));
                let scroll = self.preview_scroll.min(max_scroll);
                let items: Vec<ListItem> = children
                    .iter()
                    .skip(scroll)
                    .map(|e| {
                        let cat = theme::categorize(e);
                        let ico = theme::icon(cat, self.config.nerd_font);
                        let c = self.file_color(cat);
                        ListItem::new(Line::from(vec![
                            Span::styled(ico, Style::default().fg(c)),
                            Span::styled(" ", Style::default()),
                            Span::styled(&e.name, Style::default().fg(c)),
                        ]))
                    })
                    .collect();

                let title = if children.is_empty() {
                    " Preview (empty) ".to_string()
                } else {
                    format!(" Preview ({}) ", children.len())
                };

                let list = List::new(items).block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(Color::DarkGray))
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(list, area);
            }
            PreviewState::FileTextPreview {
                name,
                lines: highlighted,
                size,
                truncated,
            } => {
                let title = format!(" {} ({}) ", truncate_name(name, 25), format_size(*size));

                let inner_height = area.height.saturating_sub(2) as usize;
                let max_lines = inner_height.saturating_sub(if *truncated { 1 } else { 0 });
                let max_scroll = highlighted.len().saturating_sub(max_lines.max(1));
                let scroll = self.preview_scroll.min(max_scroll);
                let mut lines: Vec<Line> =
                    highlighted.iter().skip(scroll).take(max_lines).cloned().collect();

                if *truncated {
                    lines.push(Line::from(Span::styled(
                        format!(
                            " ... truncated at {} ",
                            format_size(self.config.preview_max_size)
                        ),
                        Style::default().fg(Color::DarkGray),
                    )));
                }

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(title)
                        .title_style(
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(p, area);
            }
            PreviewState::FileBasicInfo => {
                let wrap_w = area.width.saturating_sub(2) as usize;
                let mut lines = vec![Line::from("")];
                if let Some(entry) = self.entries.get(self.selected) {
                    lines.extend(wrap_labeled_field(
                        "  Name:  ", &entry.name,
                        Style::default().fg(Color::Cyan),
                        Style::default().fg(Color::Reset),
                        wrap_w,
                    ));
                    if entry.kind == EntryKind::File {
                        lines.push(Line::from(vec![
                            Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format_size(entry.size),
                                Style::default().fg(Color::Reset),
                            ),
                        ]));
                    }
                    if !entry.created_time.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  Created:", Style::default().fg(Color::Cyan)),
                            Span::styled(&entry.created_time, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    if !entry.modified_time.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  Modified:", Style::default().fg(Color::Cyan)),
                            Span::styled(&entry.modified_time, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    let mut markers = Vec::new();
                    if entry.starred {
                        markers.push(Span::styled("\u{2605} Starred", Style::default().fg(Color::Yellow)));
                    }
                    if self.cart_ids.contains(&entry.id) {
                        if !markers.is_empty() { markers.push(Span::raw("  ")); }
                        markers.push(Span::styled(
                            "\u{2606} In cart",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM),
                        ));
                    }
                    if !markers.is_empty() {
                        let mut line = vec![Span::styled("  ", Style::default())];
                        line.extend(markers);
                        lines.push(Line::from(line));
                    }
                    lines.push(Line::from(""));
                    let hint = if entry.kind == EntryKind::File
                        && crate::theme::is_text_previewable(entry)
                        && entry.size > self.config.preview_max_size
                    {
                        "  Press p to preview (large file)"
                    } else {
                        "  Press p to load"
                    };
                    lines.push(Line::from(Span::styled(
                        hint,
                        Style::default().fg(Color::DarkGray),
                    )));
                }

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(" Preview ")
                        .title_style(Style::default().fg(Color::DarkGray))
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(p, area);
            }
            PreviewState::ThumbnailImage { image } if !self.has_overlay() => {
                use crate::config::ThumbnailRenderMode;
                use ratatui_image::{picker::{Picker, ProtocolType}, StatefulImage};

                let panel_width = area.width.saturating_sub(2);
                let panel_height = area.height.saturating_sub(2);
                let wrap_w = panel_width.max(1) as usize;
                let mut info_lines: Vec<Line> = vec![];
                if let Some(entry) = self.entries.get(self.selected) {
                    info_lines.extend(wrap_labeled_field(
                        "  Name:  ", &entry.name,
                        Style::default().fg(Color::Cyan),
                        Style::default().fg(Color::Reset),
                        wrap_w,
                    ));
                    if entry.kind == EntryKind::File {
                        info_lines.push(Line::from(vec![
                            Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format_size(entry.size),
                                Style::default().fg(Color::Reset),
                            ),
                        ]));
                    }
                    if !entry.created_time.is_empty() {
                        info_lines.push(Line::from(vec![
                            Span::styled("  Created:", Style::default().fg(Color::Cyan)),
                            Span::styled(&entry.created_time, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    if !entry.modified_time.is_empty() {
                        info_lines.push(Line::from(vec![
                            Span::styled("  Modified:", Style::default().fg(Color::Cyan)),
                            Span::styled(&entry.modified_time, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    let mut markers = Vec::new();
                    if entry.starred {
                        markers.push(Span::styled("\u{2605} Starred", Style::default().fg(Color::Yellow)));
                    }
                    if self.cart_ids.contains(&entry.id) {
                        if !markers.is_empty() { markers.push(Span::raw("  ")); }
                        markers.push(Span::styled(
                            "\u{2606} In cart",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM),
                        ));
                    }
                    if !markers.is_empty() {
                        let mut line = vec![Span::styled("  ", Style::default())];
                        line.extend(markers);
                        info_lines.push(Line::from(line));
                    }
                }

                let info_visual_lines = info_lines.len() as u16;
                let min_image_height = (panel_height / 2).max(4);
                let info_height = info_visual_lines.min(panel_height.saturating_sub(min_image_height));
                let image_height = panel_height.saturating_sub(info_height);

                let inner_rect = ratatui::layout::Rect {
                    x: area.x + 1,
                    y: area.y + 1,
                    width: panel_width,
                    height: panel_height,
                };
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(image_height),
                        Constraint::Length(info_height),
                    ])
                    .split(inner_rect);

                let image_area = chunks[0];
                let info_area = chunks[1];

                let render_mode = self.config.thumbnail_mode.should_use_color();

                match render_mode {
                    ThumbnailRenderMode::Auto => {
                        if let Ok(mut picker) = Picker::from_query_stdio() {
                            // Apply user-configured protocol override
                            match self.config.current_image_protocol() {
                                crate::config::ImageProtocol::Auto => {
                                    // Fix: iTerm2 incorrectly detected as Kitty
                                    if picker.protocol_type() == ProtocolType::Kitty {
                                        if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
                                            if term_program.contains("iTerm") {
                                                picker.set_protocol_type(ProtocolType::Iterm2);
                                            }
                                        }
                                    }
                                }
                                crate::config::ImageProtocol::Kitty => {
                                    picker.set_protocol_type(ProtocolType::Kitty);
                                }
                                crate::config::ImageProtocol::Iterm2 => {
                                    picker.set_protocol_type(ProtocolType::Iterm2);
                                }
                                crate::config::ImageProtocol::Sixel => {
                                    picker.set_protocol_type(ProtocolType::Sixel);
                                }
                            }

                            let render_rect = center_image_rect(image, image_area);
                            let img_display = upscale_for_rect(image, render_rect, picker.font_size());
                            let mut protocol = picker.new_resize_protocol(img_display);
                            let img_widget = StatefulImage::default();
                            f.render_stateful_widget(img_widget, render_rect, &mut protocol);
                        }
                    }
                    ThumbnailRenderMode::ColoredHalf => {
                        let colored_lines = render_image_to_colored_lines(
                            image,
                            image_area.width as u32,
                            image_area.height as u32,
                        );
                        let colored_para = Paragraph::new(Text::from(colored_lines));
                        f.render_widget(colored_para, image_area);
                    }
                    ThumbnailRenderMode::Grayscale => {
                        let ascii_lines = render_image_to_grayscale_lines(
                            image,
                            image_area.width as u32,
                            image_area.height as u32,
                        );
                        let ascii_para = Paragraph::new(Text::from(ascii_lines))
                            .style(Style::default().fg(Color::DarkGray));
                        f.render_widget(ascii_para, image_area);
                    }
                    ThumbnailRenderMode::Off => {}
                }

                let info_p = Paragraph::new(Text::from(info_lines));
                f.render_widget(info_p, info_area);

                // Render border with title
                let title = self
                    .entries
                    .get(self.selected)
                    .map(|e| format!(" \u{1f5bc} {} ", truncate_name(&e.name, 25)))
                    .unwrap_or_else(|| " Preview ".to_string());

                let border = self
                    .styled_block()
                    .title(title)
                    .title_style(
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    )
                    .border_style(Style::default().fg(Color::DarkGray));
                f.render_widget(border, area);
            }
            // Overlay is active — suppress protocol-image to avoid artifacts in iTerm2
            PreviewState::ThumbnailImage { .. } => {
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  [thumbnail hidden during overlay]",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]))
                .block(
                    self.styled_block()
                        .title(" Preview ")
                        .title_style(Style::default().fg(Color::DarkGray))
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(p, area);
            }
            PreviewState::FileDetailedInfo(info) => {
                let wrap_w = area.width.saturating_sub(2) as usize;
                let mut lines = vec![Line::from("")];
                lines.extend(wrap_labeled_field(
                    "  Name:  ", &info.name,
                    Style::default().fg(Color::Cyan),
                    Style::default().fg(Color::Reset),
                    wrap_w,
                ));
                if let Some(size) = &info.size {
                    let size_n: u64 = size.parse().unwrap_or(0);
                    lines.push(Line::from(vec![
                        Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            format!("{} ({})", format_size(size_n), size),
                            Style::default().fg(Color::Reset),
                        ),
                    ]));
                }
                if let Some(hash) = &info.hash {
                    lines.extend(wrap_labeled_field(
                        "  Hash:  ", hash,
                        Style::default().fg(Color::Cyan),
                        Style::default().fg(Color::DarkGray),
                        wrap_w,
                    ));
                }
                if let Some(link) = &info.web_content_link {
                    lines.extend(wrap_labeled_field(
                        "  Link:  ", link,
                        Style::default().fg(Color::Cyan),
                        Style::default().fg(Color::Blue),
                        wrap_w,
                    ));
                }

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(format!(" \u{2139} {} ", truncate_name(&info.name, 25)))
                        .title_style(
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                f.render_widget(p, area);
            }
        }
    }

    fn draw_log_overlay(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        self.logs_overlay_area.set(area);
        clear_overlay_area(f, area);
        let visible = area.height.saturating_sub(2) as usize;
        let content_width = area.width.saturating_sub(2).max(1) as usize;

        // Pre-wrap all log messages into visual lines
        let all_lines = super::wrap_logs(
            self.logs.iter().map(|s| s.as_str()),
            content_width,
        );
        let total_visual = all_lines.len();
        let max_scroll = total_visual.saturating_sub(visible);

        // None = auto-follow bottom, Some(y) = pinned at absolute offset
        let scroll_y = match self.logs_scroll {
            None => max_scroll,
            Some(y) => y.min(max_scroll),
        };

        // Slice visible window
        let visible_lines: Vec<Line> = all_lines
            .into_iter()
            .skip(scroll_y)
            .take(visible)
            .map(|s| Line::from(s))
            .collect();

        let (log_bc, log_tc) = if self.is_vibrant() {
            (Color::Magenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Green)
        };
        let title = if self.logs_scroll.is_some() {
            format!("Logs [{}/{}] (l to close)", self.logs.len(), total_visual)
        } else {
            format!("Logs [{}] (l to close)", self.logs.len())
        };
        f.render_widget(
            Paragraph::new(Text::from(visible_lines)).block(self.overlay_block(&title, log_bc, log_tc)),
            area,
        );
    }

    pub(super) fn help_pairs(&self) -> Vec<(&str, &str)> {
        match &self.input {
            InputMode::Normal => {
                vec![
                    ("j/k/g/G", "nav"),
                    ("Ctrl+U/D", "half page"),
                    ("Enter", "open"),
                    ("Bksp", "back"),
                    (":", "goto"),
                    ("u", "upload"),
                    ("r", "refresh"),
                    ("h", "help"),
                    ("q", "quit"),
                ]
            }
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => vec![
                ("j/k", "nav"),
                ("Enter", "open"),
                ("Bksp", "back"),
                ("Space", "confirm"),
                ("h", "help"),
                ("Esc", "cancel"),
            ],
            InputMode::MoveInput { .. }
            | InputMode::CopyInput { .. }
            | InputMode::CartMoveInput { .. }
            | InputMode::CartCopyInput { .. } => vec![
                ("Tab", "complete"),
                ("Enter", "confirm"),
                ("Ctrl+B", "picker"),
                ("Esc", "cancel"),
            ],
            InputMode::Rename { .. } | InputMode::Mkdir { .. } => {
                vec![("Enter", "confirm"), ("Esc", "cancel")]
            }
            InputMode::GotoPath { .. } => {
                vec![("Enter", "go"), ("Esc", "cancel")]
            }
            InputMode::ConfirmQuit => {
                vec![("y", "quit"), ("n/Esc", "cancel")]
            }
            InputMode::ConfirmDelete => {
                vec![("y", "confirm"), ("p", "permanent"), ("n/Esc", "cancel")]
            }
            InputMode::ConfirmPermanentDelete { .. } => {
                vec![("Enter", "confirm"), ("Esc", "cancel")]
            }
            InputMode::CartView => vec![
                ("j/k", "nav"),
                ("x", "remove"),
                ("a", "clear all"),
                ("Enter", "download"),
                ("m", "move"),
                ("c", "copy"),
                ("t", "trash"),
                ("s", "share"),
                ("S", "quick share"),
                ("Esc", "close"),
            ],
            InputMode::CartMovePicker { .. } | InputMode::CartCopyPicker { .. } => vec![
                ("j/k", "nav"),
                ("Enter", "open folder"),
                ("Space", "confirm here"),
                ("/", "type path"),
                ("Backspace", "go up"),
                ("Esc", "cancel"),
            ],
            InputMode::ConfirmCartDelete => {
                vec![("y/Enter", "trash"), ("n/Esc", "cancel")]
            }
            InputMode::DownloadInput { .. } | InputMode::UploadInput { .. } => {
                vec![("Tab", "complete"), ("Enter", "confirm"), ("Esc", "cancel")]
            }
            InputMode::DownloadView => vec![
                ("j/k", "nav"),
                ("Enter", "expand"),
                ("p", "pause/resume"),
                ("x", "cancel"),
                ("r", "retry"),
                ("Esc", "back"),
            ],
            InputMode::OfflineInput { .. } => vec![("Enter", "submit"), ("Esc", "cancel")],
            InputMode::OfflineTasksView { .. } => vec![
                ("j/k", "nav"),
                ("r", "refresh"),
                ("R", "retry"),
                ("x", "delete"),
                ("Esc", "back"),
            ],
            InputMode::TrashView { expanded, .. } => {
                if *expanded {
                    vec![
                        ("j/k", "nav"),
                        ("Space", "info"),
                        ("u", "restore"),
                        ("x", "delete"),
                        ("r", "refresh"),
                        ("Enter", "collapse"),
                        ("Esc", "close"),
                    ]
                } else {
                    vec![
                        ("j/k", "nav"),
                        ("Enter", "expand"),
                        ("Space", "info"),
                        ("u", "restore"),
                        ("x", "delete"),
                        ("r", "refresh"),
                        ("Esc", "close"),
                    ]
                }
            }
            InputMode::InfoLoading => vec![("Esc", "cancel")],
            InputMode::InfoView { .. }
            | InputMode::InfoFolderView { .. }
            | InputMode::TextPreviewView { .. } => vec![("any key", "close")],
            InputMode::Settings { editing, .. } => {
                if *editing {
                    vec![
                        ("Left/Right", "change"),
                        ("Space", "toggle"),
                        ("Enter", "confirm"),
                        ("Esc", "cancel"),
                    ]
                } else {
                    vec![
                        ("j/k", "nav"),
                        ("Space/Enter", "edit"),
                        ("s", "save"),
                        ("Esc", "close"),
                    ]
                }
            }
            InputMode::CustomColorSettings { editing_rgb, .. } => {
                if *editing_rgb {
                    vec![("0-9", "input"), ("Enter", "confirm"), ("Esc", "cancel")]
                } else {
                    vec![
                        ("j/k", "nav"),
                        ("r/g/b", "edit RGB"),
                        ("s", "save"),
                        ("Esc", "back"),
                    ]
                }
            }
            InputMode::ImageProtocolSettings { .. } => {
                vec![
                    ("j/k", "nav"),
                    ("Left/Right", "protocol"),
                    ("s", "save"),
                    ("Esc", "back"),
                ]
            }
            InputMode::ConfirmPlay { .. } => {
                vec![("y/Enter", "play"), ("n/Esc", "cancel")]
            }
            InputMode::PlayPicker { .. } => {
                vec![("j/k", "nav"), ("Enter", "play"), ("Esc", "cancel")]
            }
            InputMode::PlayerInput { .. } => {
                vec![("Enter", "confirm"), ("Esc", "cancel")]
            }
            InputMode::SharePrompt => {
                vec![("p", "public share"), ("P", "with password"), ("Esc", "cancel")]
            }
            InputMode::ShareCreatedView { .. } => {
                vec![("y", "copy URL"), ("Esc", "close top"), ("Ctrl+Esc", "close all")]
            }
            InputMode::MySharesView { confirm_delete, .. } => {
                if confirm_delete.is_some() {
                    vec![("y/Enter", "confirm delete"), ("n/Esc", "cancel")]
                } else {
                    vec![
                        ("j/k", "nav"),
                        ("y", "copy URL"),
                        ("d", "delete"),
                        ("r", "refresh"),
                        ("Esc", "back"),
                    ]
                }
            }
            _ => vec![],
        }
    }

    pub(super) fn styled_help_spans(pairs: &[(&str, &str)]) -> Vec<Span<'static>> {
        let key_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(Color::DarkGray);
        let sep_style = Style::default().fg(Color::DarkGray);

        let mut spans = Vec::new();
        for (i, (key, desc)) in pairs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" • ", sep_style));
            }
            spans.push(Span::styled(key.to_string(), key_style));
            spans.push(Span::styled(format!(" {}", desc), desc_style));
        }
        spans
    }

    fn draw_overlay(&self, f: &mut Frame) {
        let cur = if self.cursor_visible { "\u{2588}" } else { " " };

        match &self.input {
            InputMode::Normal
            | InputMode::Login { .. }
            | InputMode::MovePicker { .. }
            | InputMode::CopyPicker { .. }
            | InputMode::CartMovePicker { .. }
            | InputMode::CartCopyPicker { .. }
            | InputMode::DownloadView
            | InputMode::MySharesView { .. } => {}

            InputMode::MoveInput { input, .. } => {
                self.draw_path_input_overlay(f, "Move", "Move to path", input, cur);
            }
            InputMode::CopyInput { input, .. } => {
                self.draw_path_input_overlay(f, "Copy", "Copy to path", input, cur);
            }
            InputMode::CartMoveInput { input } => {
                self.draw_path_input_overlay(f, "Move Cart", "Move all cart items to path", input, cur);
            }
            InputMode::CartCopyInput { input } => {
                self.draw_path_input_overlay(f, "Copy Cart", "Copy all cart items to path", input, cur);
            }
            InputMode::Rename { value } => {
                self.draw_rename_overlay(f, value, cur);
            }
            InputMode::Mkdir { value } => {
                self.draw_mkdir_overlay(f, value, cur);
            }
            InputMode::GotoPath { query } => {
                self.draw_goto_overlay(f, query, cur);
            }
            InputMode::ConfirmQuit => {
                self.draw_confirm_quit_overlay(f);
            }
            InputMode::ConfirmDelete => {
                self.draw_confirm_delete_overlay(f);
            }
            InputMode::ConfirmPermanentDelete { value } => {
                self.draw_confirm_permanent_delete_overlay(f, value, cur);
            }
            InputMode::CartView => {
                self.draw_cart_overlay(f);
            }
            InputMode::ConfirmCartDelete => {
                self.draw_confirm_cart_delete_overlay(f);
            }
            InputMode::DownloadInput { input } => {
                self.draw_download_input_overlay(f, input, cur);
            }
            InputMode::UploadInput { input } => {
                self.draw_upload_input_overlay(f, input, cur);
            }
            InputMode::OfflineInput { value } => {
                self.draw_offline_input_overlay(f, value, cur);
            }
            InputMode::OfflineTasksView { tasks, selected } => {
                self.draw_offline_tasks_overlay(f, tasks, *selected);
            }
            InputMode::TrashView {
                entries,
                selected,
                expanded,
            } => {
                if self.loading {
                    self.draw_info_loading_overlay(f);
                } else {
                    self.draw_trash_view(f, entries, *selected, *expanded);
                }
            }
            InputMode::InfoLoading => {
                self.draw_info_loading_overlay(f);
            }
            InputMode::InfoView { info, image, has_thumbnail } => {
                self.draw_info_overlay(f, info, image.as_ref(), *has_thumbnail);
            }
            InputMode::InfoFolderView { name, entries } => {
                self.draw_info_folder_overlay(f, name, entries);
            }
            InputMode::TextPreviewView {
                name,
                lines,
                truncated,
            } => {
                self.draw_text_preview_overlay(f, name, lines, *truncated);
            }
            InputMode::Settings {
                selected,
                editing,
                draft,
                modified,
            } => {
                self.draw_settings_overlay(f, *selected, *editing, draft, *modified);
            }
            InputMode::CustomColorSettings {
                selected,
                draft,
                modified,
                editing_rgb,
                rgb_input,
                rgb_component,
            } => {
                self.draw_custom_color_overlay(f, *selected, draft, *modified, *editing_rgb, rgb_input, *rgb_component);
            }
            InputMode::ImageProtocolSettings {
                selected,
                draft,
                modified,
                current_terminal,
                terminals,
            } => {
                self.draw_image_protocol_overlay(f, *selected, draft, *modified, current_terminal, terminals);
            }
            InputMode::ConfirmPlay { name, url } => {
                self.draw_confirm_play_overlay(f, name, url);
            }
            InputMode::PlayPicker {
                name,
                medias,
                selected,
            } => {
                self.draw_play_picker_overlay(f, name, medias, *selected);
            }
            InputMode::PlayerInput { value, .. } => {
                self.draw_player_input_overlay(f, value);
            }
            InputMode::SharePrompt => {
                self.draw_cart_overlay(f);
                self.draw_share_prompt_overlay(f);
            }
            InputMode::ShareCreatedView { shares } => {
                self.draw_cart_overlay(f);
                self.draw_share_created_view(f, shares);
            }
        }
    }

    fn draw_rename_overlay(&self, f: &mut Frame, value: &str, cur: &str) {
        let area = self.prepare_overlay(f, 60, 20);
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightYellow, Color::LightYellow)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  New name: ", Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Self::hint_line(&[("Enter", "confirm"), ("Esc", "cancel")]),
            ])
            .block(self.overlay_block("Rename", bc, tc)),
            area,
        );
    }

    fn draw_mkdir_overlay(&self, f: &mut Frame, value: &str, cur: &str) {
        let area = self.prepare_overlay(f, 60, 20);
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightYellow, Color::LightYellow)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Folder name: ", Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Self::hint_line(&[("Enter", "confirm"), ("Esc", "cancel")]),
            ])
            .block(self.overlay_block("New Folder", bc, tc)),
            area,
        );
    }

    fn draw_goto_overlay(&self, f: &mut Frame, query: &str, cur: &str) {
        let area = self.prepare_overlay(f, 70, 20);
        let (bc, tc) = self.themed_colors(Color::Cyan);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Path: ", Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{}{}", query, cur), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(Span::styled(
                    "  e.g. /My Files/Movies",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Self::hint_line(&[("Enter", "go"), ("Esc", "cancel")]),
            ])
            .block(self.overlay_block("Go to Path", bc, tc)),
            area,
        );
    }

    /// Draw a simple confirmation overlay with title, body lines, and base color.
    fn draw_simple_confirm(&self, f: &mut Frame, title: &str, body: Vec<Line<'_>>, base_color: Color) {
        let area = self.prepare_overlay(f, 60, 20);
        let (bc, tc) = self.themed_colors(base_color);
        f.render_widget(
            Paragraph::new(body).block(self.overlay_block(title, bc, tc)),
            area,
        );
    }

    fn draw_confirm_quit_overlay(&self, f: &mut Frame) {
        let active = self
            .download_state
            .tasks
            .iter()
            .filter(|t| matches!(t.status, super::download::TaskStatus::Downloading | super::download::TaskStatus::Pending))
            .count();
        self.draw_simple_confirm(f, "Confirm Quit", vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!("{} download(s) still active.", active),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled("  Quit anyway?", Style::default().fg(Color::Yellow))),
            Line::from(""),
            Self::hint_line(&[("y", "quit"), ("n/Esc", "cancel")]),
        ], Color::Yellow);
    }

    fn draw_confirm_delete_overlay(&self, f: &mut Frame) {
        let name = self
            .current_entry()
            .map(|e| e.name.as_str())
            .unwrap_or("<none>");
        self.draw_simple_confirm(f, "Confirm Remove", vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Delete ", Style::default().fg(Color::Red)),
                Span::styled(
                    format!("`{}`", name),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to trash?", Style::default().fg(Color::Red)),
            ]),
            Line::from(""),
            Self::hint_line(&[("y", "trash"), ("p", "permanent"), ("n/Esc", "cancel")]),
        ], Color::Red);
    }

    fn draw_confirm_permanent_delete_overlay(&self, f: &mut Frame, value: &str, cur: &str) {
        let area = self.prepare_overlay(f, 60, 55);
        let name = self
            .current_entry()
            .map(|e| e.name.as_str())
            .unwrap_or("<none>");
        let warn_lines = warn_triangle_lines();
        let mut lines = vec![Line::from("")];
        lines.extend(warn_lines);
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "      PERMANENTLY DELETE ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("`{}`", name),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            "        This cannot be undone!",
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Type 'yes' to confirm: ", Style::default().fg(Color::Reset)),
            Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[("Enter", "confirm"), ("Esc", "cancel")]));
        f.render_widget(
            Paragraph::new(lines).block(
                self.styled_block()
                    .title(Span::styled(
                        " \u{26a0} Permanent Delete ",
                        Style::default().fg(Color::Red),
                    ))
                    .border_style(Style::default().fg(Color::Red)),
            ),
            area,
        );
    }

    fn draw_confirm_cart_delete_overlay(&self, f: &mut Frame) {
        let count = self.cart.len();
        self.draw_simple_confirm(f, "Confirm Trash Cart", vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Trash ", Style::default().fg(Color::Red)),
                Span::styled(
                    format!("{} item(s)", count),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" from cart?", Style::default().fg(Color::Red)),
            ]),
            Line::from(""),
            Self::hint_line(&[("y/Enter", "trash"), ("n/Esc", "cancel")]),
        ], Color::Red);
    }

    fn draw_path_input_overlay(
        &self,
        f: &mut Frame,
        title: &str,
        label: &str,
        input: &PathInput,
        cur: &str,
    ) {
        // Determine overlay height based on candidates
        let candidate_lines = input.candidates.len().min(8);
        let base_height = 6; // padding + input line + help line
        let total_lines = base_height
            + if candidate_lines > 0 {
                candidate_lines + 1
            } else {
                0
            };
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1))
            .max(20)
            .min(60);
        let area = centered_rect(70, pct, f.area());
        clear_overlay_area(f, area);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("  {}: ", label), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}{}", input.value, cur),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
        ];

        // Show candidates
        if !input.candidates.is_empty() {
            lines.push(Line::from(""));
            for (i, name) in input.candidates.iter().enumerate().take(8) {
                let is_sel = input.candidate_idx == Some(i);
                let prefix = if is_sel { "  > " } else { "    " };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Blue)
                };
                lines.push(Line::from(Span::styled(
                    format!("{}{}/", prefix, name),
                    style,
                )));
            }
            if input.candidates.len() > 8 {
                lines.push(Line::from(Span::styled(
                    format!("    ... and {} more", input.candidates.len() - 8),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[
            ("Tab", "complete"),
            ("Enter", "confirm"),
            ("Ctrl+B", "picker"),
            ("Esc", "cancel"),
        ]));

        let (mc_bc, mc_tc) = self.themed_colors_pair(Color::Cyan, Color::Yellow);
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(self.overlay_block(title, mc_bc, mc_tc)),
            area,
        );
    }

    /// Returns `(outer, chunks)` for the 2-pane picker layout.
    /// `outer[0]` = main area, `outer[1]` = help bar (only if show_help_bar).
    /// `chunks[0]` = left pane, `chunks[1]` = right pane.
    fn build_picker_layout(&self, f: &Frame) -> (std::rc::Rc<[Rect]>, std::rc::Rc<[Rect]>) {
        let outer = if self.config.show_help_bar {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(f.area())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1)])
                .split(f.area())
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(outer[0]);
        (outer, chunks)
    }

    fn draw_picker(&self, f: &mut Frame) {
        let (outer, chunks) = self.build_picker_layout(f);

        // Left: source (read-only)
        let source_items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let cat = theme::categorize(e);
                let ico = theme::icon(cat, self.config.nerd_font);
                let c = self.file_color(cat);
                ListItem::new(Line::from(vec![
                    Span::styled(ico, Style::default().fg(c)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&e.name, Style::default().fg(c)),
                ]))
            })
            .collect();

        let mut source_state = ListState::default();
        if !self.entries.is_empty() {
            source_state.select(Some(self.selected.min(self.entries.len() - 1)));
        }
        let source_list = List::new(source_items)
            .block(
                self.styled_block()
                    .title(format!(" Source: {} ", self.current_path_display()))
                    .title_style(Style::default().fg(Color::DarkGray))
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .highlight_style(Style::default().fg(Color::DarkGray))
            .highlight_symbol("  ");
        f.render_stateful_widget(source_list, chunks[0], &mut source_state);

        // Right: picker
        let (is_move, source_entry, picker) = match &self.input {
            InputMode::MovePicker { source, picker } => (true, source, picker),
            InputMode::CopyPicker { source, picker } => (false, source, picker),
            _ => return,
        };

        let op = if is_move { "Move" } else { "Copy" };
        self.draw_picker_right_pane(f, chunks[1], picker, is_move);

        // Help bar
        if self.config.show_help_bar {
            let pairs = self.help_pairs();
            let mut spans = vec![
                Span::styled(
                    format!(" {} '{}' ", op, source_entry.name),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            ];
            spans.extend(Self::styled_help_spans(&pairs));
            let bar = Paragraph::new(Line::from(spans));
            f.render_widget(bar, outer[1]);
        }

        if self.show_help_sheet {
            self.draw_help_sheet(f);
        }
    }

    fn draw_cart_picker(&self, f: &mut Frame) {
        let (outer, chunks) = self.build_picker_layout(f);

        // Left: cart contents (read-only)
        let cart_items: Vec<ListItem> = self
            .cart
            .iter()
            .map(|e| {
                let cat = theme::categorize(e);
                let ico = theme::icon(cat, self.config.nerd_font);
                let c = self.file_color(cat);
                ListItem::new(Line::from(vec![
                    Span::styled(ico, Style::default().fg(c)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&e.name, Style::default().fg(c)),
                ]))
            })
            .collect();

        let total_size: u64 = self.cart.iter().map(|e| e.size).sum();
        let cart_title = format!(
            " Cart ({} items, {}) ",
            self.cart.len(),
            format_size(total_size)
        );
        let cart_list = List::new(cart_items)
            .block(
                self.styled_block()
                    .title(cart_title)
                    .title_style(Style::default().fg(Color::DarkGray))
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .highlight_style(Style::default().fg(Color::DarkGray))
            .highlight_symbol("  ");
        let mut cart_state = ListState::default();
        f.render_stateful_widget(cart_list, chunks[0], &mut cart_state);

        // Right: destination folder picker
        let (is_move, picker) = match &self.input {
            InputMode::CartMovePicker { picker } => (true, picker),
            InputMode::CartCopyPicker { picker } => (false, picker),
            _ => return,
        };

        let op = if is_move { "Move" } else { "Copy" };
        self.draw_picker_right_pane(f, chunks[1], picker, is_move);

        // Help bar
        if self.config.show_help_bar {
            let pairs = self.help_pairs();
            let mut spans = vec![
                Span::styled(
                    format!(" {} {} item(s) ", op, self.cart.len()),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            ];
            spans.extend(Self::styled_help_spans(&pairs));
            let bar = Paragraph::new(Line::from(spans));
            f.render_widget(bar, outer[1]);
        }

        if self.show_help_sheet {
            self.draw_help_sheet(f);
        }
    }

    /// Shared right-pane renderer for both single-file and cart picker views.
    fn draw_picker_right_pane(
        &self,
        f: &mut Frame,
        area: Rect,
        picker: &PickerState,
        is_move: bool,
    ) {
        let op = if is_move { "Move" } else { "Copy" };
        let pp = Self::picker_path_display(picker);
        let title = if picker.loading {
            format!(" {} to: {} {} ", op, pp, SPINNER_FRAMES[self.spinner_idx])
        } else {
            format!(" {} to: {} ", op, pp)
        };

        let folders: Vec<&crate::pikpak::Entry> = picker
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .collect();

        let picker_items: Vec<ListItem> = folders
            .iter()
            .map(|e| {
                let ico = theme::icon(theme::FileCategory::Folder, self.config.nerd_font);
                ListItem::new(Line::from(vec![
                    Span::styled(ico, Style::default().fg(Color::Blue)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&e.name, Style::default().fg(Color::Blue)),
                ]))
            })
            .collect();

        let mut picker_state = ListState::default();
        if !folders.is_empty() {
            picker_state.select(Some(picker.selected.min(folders.len() - 1)));
        }

        let (pk_bc, pk_tc) = self.themed_colors(Color::Yellow);
        let plist = List::new(picker_items)
            .block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(pk_tc))
                    .border_style(Style::default().fg(pk_bc)),
            )
            .highlight_style(self.highlight_style())
            .highlight_symbol("› ");
        f.render_stateful_widget(plist, area, &mut picker_state);
    }

    pub(super) fn draw_help_sheet(&self, f: &mut Frame) {
        let term = f.area();

        // Adaptive width — wider and flatter
        let sheet_w = term.width.saturating_sub(4).min(92).max(44);
        let inner_w = sheet_w.saturating_sub(2) as usize;
        let show_art = inner_w >= 70;

        // Define help sections based on mode
        type HelpSection<'a> = (&'a str, Vec<(&'a str, &'a str)>);

        let sections: Vec<HelpSection> = match &self.input {
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => vec![
                (
                    "Navigation",
                    vec![
                        ("j / \u{2193}", "Move down"),
                        ("k / \u{2191}", "Move up"),
                        ("Enter", "Open folder"),
                        ("Bksp", "Go back"),
                    ],
                ),
                (
                    "Actions",
                    vec![
                        ("Space", "Confirm destination"),
                        ("/", "Switch to text input"),
                        ("h", "Toggle help"),
                        ("Esc", "Cancel"),
                    ],
                ),
            ],
            _ => {
                let mut nav: Vec<(&str, &str)> = vec![
                    ("j / \u{2193}", "Move down"),
                    ("k / \u{2191}", "Move up"),
                    ("g / Home", "Jump to top"),
                    ("G / End", "Jump to bottom"),
                    ("PgDn/Up", "Page scroll"),
                    ("Enter", "Open / Play"),
                    ("Bksp", "Go to parent"),
                    ("r", "Refresh"),
                    ("S", "Cycle sort"),
                    ("R", "Reverse sort"),
                ];
                if !self.config.show_preview {
                    nav.push(("Space", "File info"));
                } else if !self.config.lazy_preview {
                    nav.push(("Space", "Load preview"));
                }
                nav.push(("p", "Preview"));
                nav.push(("w", "Watch (streams)"));

                vec![
                    ("Navigation", nav),
                    (
                        "Actions",
                        vec![
                            ("c", "Copy"),
                            ("m", "Move"),
                            ("n", "Rename"),
                            ("d", "Delete"),
                            ("f", "New folder"),
                            ("s", "Star / Unstar"),
                            ("y", "Copy link"),
                            ("a", "Add to cart"),
                        ],
                    ),
                    (
                        "Panels",
                        vec![
                            ("D", "Downloads"),
                            ("A", "View cart"),
                            ("M", "My Shares"),
                            ("o", "Cloud download"),
                            ("O", "Offline tasks"),
                            ("t", "Trash"),
                            ("l", "Toggle logs"),
                            (",", "Settings"),
                            ("h", "Toggle help"),
                            ("q", "Quit"),
                        ],
                    ),
                ]
            }
        };

        let key_w: usize = 7;

        // ≤3 sections: one column each. >3: first two share column 0.
        let columns: Vec<Vec<(&str, &Vec<(&str, &str)>)>> = if sections.len() <= 3 {
            // Simple: one section per column
            sections.iter().map(|(name, items)| vec![(*name, items)]).collect()
        } else {
            // Group: first two sections share column 0
            let mut cols: Vec<Vec<(&str, &Vec<(&str, &str)>)>> = Vec::new();
            cols.push(vec![
                (sections[0].0, &sections[0].1),
                (sections[1].0, &sections[1].1),
            ]);
            for s in &sections[2..] {
                cols.push(vec![(s.0, &s.1)]);
            }
            cols
        };

        let col_count = columns.len();
        let col_w = inner_w / col_count;

        // Calculate max rows per column (title line + items for each group, with blank separator)
        let col_heights: Vec<usize> = columns.iter().map(|groups| {
            let mut h = 0;
            for (i, (_, items)) in groups.iter().enumerate() {
                if i > 0 { h += 1; } // blank separator between groups
                h += 1; // title
                h += items.len(); // items
            }
            h
        }).collect();
        let max_rows = col_heights.iter().copied().max().unwrap_or(0);

        // Height — help content takes priority over ASCII art
        let min_content_h = max_rows + 2 + 2; // items + hint/blank + borders
        let art_lines: usize = 7; // 5 art + 2 blank lines
        let show_art = show_art && (term.height as usize) >= min_content_h + art_lines;
        let art_h: usize = if show_art { art_lines } else { 1 }; // art or just 1 blank
        let content_h = art_h + max_rows + 2; // art + items + blank + hint
        let sheet_h = ((content_h + 2) as u16).min(term.height); // +2 borders

        // Center the popup
        let x = (term.width.saturating_sub(sheet_w)) / 2;
        let y = (term.height.saturating_sub(sheet_h)) / 2;
        let sheet_area = ratatui::layout::Rect::new(x, y, sheet_w, sheet_h);

        clear_overlay_area(f, sheet_area);

        let title_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let key_style = Style::default().fg(Color::Yellow);
        let desc_style = Style::default().fg(Color::Reset);

        let mut lines: Vec<Line> = Vec::new();

        // ASCII art banner
        if show_art {
            let art = [
                r#"    dMMMMb  dMP dMP dMP dMMMMb  .aMMMb  dMP dMP dMMMMMMP dMP dMP dMP"#,
                r#"   dMP.dMP amr dMP.dMP dMP.dMP dMP"dMP dMP.dMP    dMP   dMP dMP amr "#,
                r#"  dMMMMP" dMP dMMMMK" dMMMMP" dMMMMMP dMMMMK"    dMP   dMP dMP dMP  "#,
                r#" dMP     dMP dMP"AMF dMP     dMP dMP dMP"AMF    dMP   dMP.aMP dMP   "#,
                r#"dMP     dMP dMP dMP dMP     dMP dMP dMP dMP    dMP    VMMMP" dMP    "#,
            ];
            let colors = [
                Color::LightCyan,
                Color::Cyan,
                Color::LightBlue,
                Color::Blue,
                Color::LightMagenta,
            ];

            lines.push(Line::from(""));
            for (text, &color) in art.iter().zip(colors.iter()) {
                let art_w = text.chars().count();
                let pad = inner_w.saturating_sub(art_w) / 2;
                lines.push(Line::from(Span::styled(
                    format!("{}{}", " ".repeat(pad), text),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(Line::from(""));
        } else {
            lines.push(Line::from(""));
        }

        // Pre-build each column's row content: (RowKind, data)
        // RowKind: Title(name), Item(key, desc), Blank
        enum RowKind<'a> { Title(&'a str), Item(&'a str, &'a str), Blank }
        let col_rows: Vec<Vec<RowKind>> = columns.iter().map(|groups| {
            let mut rows = Vec::new();
            for (i, (name, items)) in groups.iter().enumerate() {
                if i > 0 { rows.push(RowKind::Blank); }
                rows.push(RowKind::Title(name));
                for &(key, desc) in *items {
                    rows.push(RowKind::Item(key, desc));
                }
            }
            rows
        }).collect();

        // Render rows side by side
        for row in 0..max_rows {
            let mut spans = Vec::new();
            for (ci, rows) in col_rows.iter().enumerate() {
                let prefix = if ci == 0 { " " } else { "" };
                if row < rows.len() {
                    match &rows[row] {
                        RowKind::Title(name) => {
                            let w = col_w.saturating_sub(prefix.len());
                            spans.push(Span::styled(
                                format!("{}{:<width$}", prefix, name, width = w),
                                title_style,
                            ));
                        }
                        RowKind::Item(key, desc) => {
                            let dw = col_w.saturating_sub(key_w + 1 + prefix.len());
                            spans.push(Span::styled(
                                format!("{}{:<kw$} ", prefix, key, kw = key_w),
                                key_style,
                            ));
                            spans.push(Span::styled(
                                format!("{:<dw$}", desc, dw = dw),
                                desc_style,
                            ));
                        }
                        RowKind::Blank => {
                            spans.push(Span::raw(format!("{:<width$}", "", width = col_w)));
                        }
                    }
                } else {
                    spans.push(Span::raw(format!("{:<width$}", "", width = col_w)));
                }
            }
            lines.push(Line::from(spans));
        }

        // Close hint
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Press any key to close",
            Style::default().fg(Color::DarkGray),
        )));

        let (hp_bc, hp_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Cyan)
        };
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(" Help ")
                .title_style(Style::default().fg(hp_tc).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(hp_bc)),
        );
        f.render_widget(p, sheet_area);
    }

    // --- Cart overlay ---

    fn draw_cart_overlay(&self, f: &mut Frame) {
        let total_size: u64 = self.cart.iter().map(|e| e.size).sum();
        let title = format!(
            "Cart ({} files, {})",
            self.cart.len(),
            format_size(total_size)
        );

        let max_items = 12;
        let pct = widgets::dynamic_overlay_height(self.cart.len(), max_items, f.area().height, 25, 70);
        let area = centered_rect(65, pct, f.area());
        clear_overlay_area(f, area);

        let mut lines = vec![Line::from("")];

        if self.cart.is_empty() {
            lines.push(widgets::empty_state_line("Cart is empty. Press 'a' on files to add them."));
        } else {
            let cart_offset = widgets::scroll_offset(self.cart_selected, max_items);
            for (i, entry) in self.cart.iter().enumerate().skip(cart_offset).take(max_items) {
                let is_sel = i == self.cart_selected;
                let prefix = if is_sel { " \u{203a} " } else { "   " };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Reset)
                };
                let size = format_size(entry.size);
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&entry.name, style),
                    Span::styled(format!("  {}", size), Style::default().fg(Color::DarkGray)),
                ]));
            }
            widgets::push_remaining_indicator(&mut lines, self.cart.len(), cart_offset, max_items);
        }

        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[
            ("j/k", "nav"),
            ("x", "remove"),
            ("a", "clear"),
            ("Enter", "download"),
            ("m", "move"),
            ("c", "copy"),
            ("t", "trash"),
            ("s", "share"),
            ("Esc", "close"),
        ]));

        let (ct_bc, ct_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Yellow, Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(self.overlay_block(&title, ct_bc, ct_tc)),
            area,
        );
    }

    // --- Download input overlay ---

    /// Appends a blank separator line followed by a sliding-window candidate list to `lines`.
    /// Does nothing if `candidates` is empty.
    fn draw_candidate_list(
        &self,
        lines: &mut Vec<Line<'static>>,
        candidates: &[(String, bool)],
        selected_idx: Option<usize>,
    ) {
        if candidates.is_empty() {
            return;
        }
        lines.push(Line::from(""));
        let total = candidates.len();
        const MAX_VIS: usize = 8;
        let sel = selected_idx.unwrap_or(0);
        // Sliding window: keep selected item visible, reserving 1 row for "above" indicator.
        let has_above_row = sel >= MAX_VIS;
        let item_slots = if has_above_row { MAX_VIS - 1 } else { MAX_VIS };
        let window_start = if sel + 1 <= item_slots { 0 } else { sel + 1 - item_slots };
        let window_end = (window_start + item_slots).min(total);
        if has_above_row {
            lines.push(Line::from(Span::styled(
                format!("    ↑ {} more above", window_start),
                Style::default().fg(Color::DarkGray),
            )));
        }
        for i in window_start..window_end {
            let (name, is_dir) = &candidates[i];
            let is_sel = selected_idx == Some(i);
            let row_prefix = if is_sel { "  > " } else { "    " };
            let style = if is_sel {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Blue)
            };
            let suffix = if *is_dir { "/" } else { "" };
            lines.push(Line::from(Span::styled(
                format!("{}{}{}", row_prefix, name, suffix),
                style,
            )));
        }
        if window_end < total {
            lines.push(Line::from(Span::styled(
                format!("    ... and {} more", total - window_end),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    fn draw_download_input_overlay(&self, f: &mut Frame, input: &LocalPathInput, cur: &str) {
        let candidate_lines = input.candidates.len().min(8);
        let base_height = 6;
        let total_lines = base_height
            + if candidate_lines > 0 {
                candidate_lines + 1
            } else {
                0
            };
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1))
            .max(20)
            .min(60);
        let area = centered_rect(70, pct, f.area());
        clear_overlay_area(f, area);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Save to: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}{}", input.value, cur),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
        ];

        self.draw_candidate_list(&mut lines, &input.candidates, input.candidate_idx);

        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[("Tab", "complete"), ("Enter", "confirm"), ("Esc", "cancel")]));

        let (dl_bc, dl_tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        let cart_count = self.cart.len();
        f.render_widget(
            Paragraph::new(Text::from(lines))
                .block(self.overlay_block(&format!("Download {} files", cart_count), dl_bc, dl_tc)),
            area,
        );
    }

    // --- Upload input overlay ---

    fn draw_upload_input_overlay(&self, f: &mut Frame, input: &LocalPathInput, cur: &str) {
        let candidate_lines = input.candidates.len().min(8);
        let base_height = 7;
        let total_lines = base_height + if candidate_lines > 0 { candidate_lines + 1 } else { 0 };
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1)).max(20).min(60);
        let area = centered_rect(70, pct, f.area());
        clear_overlay_area(f, area);

        let dest = self.current_path_display();
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Upload to: ", Style::default().fg(Color::DarkGray)),
                Span::styled(dest, Style::default().fg(Color::Reset)),
            ]),
            Line::from(vec![
                Span::styled("  File:      ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}{}", input.value, cur),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
        ];

        self.draw_candidate_list(&mut lines, &input.candidates, input.candidate_idx);

        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[("Tab", "complete"), ("Enter", "upload"), ("Esc", "cancel")]));

        let (ul_bc, ul_tc) = self.themed_colors(Color::Yellow);
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(
                self.styled_block()
                    .title(Span::styled(
                        " Upload File ",
                        Style::default().fg(ul_tc).add_modifier(Modifier::BOLD),
                    ))
                    .border_style(Style::default().fg(ul_bc)),
            ),
            area,
        );
    }

    // --- Offline input overlay ---

    fn draw_offline_input_overlay(&self, f: &mut Frame, value: &str, cur: &str) {
        let area = self.prepare_overlay(f, 70, 25);
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Enter URL or magnet link for cloud download:",
                    Style::default().fg(Color::Reset),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  URL: ", Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Self::hint_line(&[("Enter", "submit"), ("Esc", "cancel")]),
            ])
            .block(self.overlay_block("Offline Download", bc, tc)),
            area,
        );
    }

    // --- Offline tasks view (full screen) ---

    fn draw_offline_tasks_overlay(
        &self,
        f: &mut Frame,
        tasks: &[crate::pikpak::OfflineTask],
        selected: usize,
    ) {
        let pct = widgets::dynamic_overlay_height(tasks.len(), 15, f.area().height, 25, 75);
        let area = centered_rect(75, pct, f.area());
        clear_overlay_area(f, area);

        let title = format!("Offline Tasks ({})", tasks.len());

        let (ot_bc, ot_tc) = if self.is_vibrant() {
            (Color::LightBlue, Color::LightBlue)
        } else {
            (Color::Cyan, Color::Green)
        };

        if tasks.is_empty() {
            let hints = self.help_pairs();
            let lines = vec![
                Line::from(""),
                widgets::empty_state_line("No offline tasks. Press 'o' to add a URL."),
                Line::from(""),
                Self::hint_line(&hints),
            ];
            f.render_widget(
                Paragraph::new(Text::from(lines)).block(self.overlay_block(&title, ot_bc, ot_tc)),
                area,
            );
        } else {
            let mut lines = vec![Line::from("")];

            let max_visible = 15;
            let task_offset = widgets::scroll_offset(selected, max_visible);
            for (i, task) in tasks.iter().enumerate().skip(task_offset).take(max_visible) {
                let is_sel = i == selected;
                let prefix = if is_sel { " \u{203a} " } else { "   " };

                let (icon, color) = match task.phase.as_str() {
                    "PHASE_TYPE_COMPLETE" => ("\u{2713}", Color::Green),
                    "PHASE_TYPE_RUNNING" => ("\u{2193}", Color::Cyan),
                    "PHASE_TYPE_PENDING" => ("\u{2026}", Color::DarkGray),
                    "PHASE_TYPE_ERROR" => ("\u{2717}", Color::Red),
                    _ => ("?", Color::Yellow),
                };

                let size = task
                    .file_size
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|n| format_size(n))
                    .unwrap_or_default();

                let name_style = if is_sel {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Reset)
                };

                let mut spans = vec![
                    Span::styled(prefix, name_style),
                    Span::styled(format!("{} ", icon), Style::default().fg(color)),
                    Span::styled(truncate_name(&task.name, 35), name_style),
                    Span::styled(
                        format!("  {:>3}%", task.progress),
                        Style::default().fg(Color::Reset),
                    ),
                    Span::styled(format!("  {}", size), Style::default().fg(Color::DarkGray)),
                ];
                if task.phase == "PHASE_TYPE_ERROR" {
                    if let Some(msg) = &task.message {
                        spans.push(Span::styled(
                            format!("  {}", truncate_name(msg, 20)),
                            Style::default().fg(Color::Red),
                        ));
                    }
                }

                lines.push(Line::from(spans));
            }

            widgets::push_remaining_indicator(&mut lines, tasks.len(), task_offset, max_visible);

            lines.push(Line::from(""));
            let hints = self.help_pairs();
            lines.push(Self::hint_line(&hints));
            f.render_widget(
                Paragraph::new(Text::from(lines)).block(self.overlay_block(&title, ot_bc, ot_tc)),
                area,
            );
        }
    }
    // --- Info loading overlay ---

    fn draw_info_loading_overlay(&self, f: &mut Frame) {
        let area = self.prepare_overlay(f, 45, 20);

        let spinner = SPINNER_FRAMES[self.spinner_idx];
        let (in_bc, in_tc) = self.themed_colors(Color::Cyan);

        let label = self.loading_label.as_deref().unwrap_or("Loading...");
        let title = self
            .loading_label
            .as_ref()
            .map(|l| format!(" {} ", l))
            .unwrap_or_else(|| " \u{2139} Info ".to_string());
        let p = Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {} {}", spinner, label),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Esc to cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ]))
        .block(
            self.styled_block()
                .title(title)
                .title_style(Style::default().fg(in_tc).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(in_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Info overlay ---

    fn draw_info_overlay(
        &self,
        f: &mut Frame,
        info: &crate::pikpak::FileInfoResponse,
        image: Option<&image::DynamicImage>,
        has_thumbnail: bool,
    ) {
        let has_thumb = has_thumbnail;
        let area = if has_thumb {
            centered_rect(80, 55, f.area())
        } else {
            centered_rect(65, 40, f.area())
        };
        clear_overlay_area(f, area);

        let inner_w = area.width.saturating_sub(2);
        // Reserve ~40% of width for thumbnail column; text wraps within the left 60%.
        let thumb_col_w: u16 = if has_thumb { (inner_w * 2 / 5).max(10) } else { 0 };
        let wrap_w = inner_w.saturating_sub(thumb_col_w) as usize;
        let footer_wrap_w = inner_w as usize;

        // meta_lines: rendered side-by-side with the thumbnail (wraps within text column)
        let mut meta_lines = vec![Line::from("")];

        if let Some(id) = &info.id {
            meta_lines.extend(wrap_labeled_field(
                "  ID:    ", id,
                Style::default().fg(Color::Cyan),
                Style::default().fg(Color::DarkGray),
                wrap_w,
            ));
        }

        meta_lines.extend(wrap_labeled_field(
            "  Name:  ", &info.name,
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::Reset),
            wrap_w,
        ));

        if let Some(kind) = &info.kind {
            meta_lines.push(Line::from(vec![
                Span::styled("  Type:  ", Style::default().fg(Color::Cyan)),
                Span::styled(kind.as_str(), Style::default().fg(Color::Reset)),
            ]));
        }

        if let Some(size) = &info.size {
            let size_n: u64 = size.parse().unwrap_or(0);
            meta_lines.push(Line::from(vec![
                Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{} ({})", format_size(size_n), size),
                    Style::default().fg(Color::Reset),
                ),
            ]));
        }

        if let Some(ct) = &info.created_time {
            let date = crate::cmd::format_date(ct);
            meta_lines.push(Line::from(vec![
                Span::styled("  Created:", Style::default().fg(Color::Cyan)),
                Span::styled(date, Style::default().fg(Color::Reset)),
            ]));
        }
        if let Some(mt) = &info.modified_time {
            let date = crate::cmd::format_date(mt);
            meta_lines.push(Line::from(vec![
                Span::styled("  Modified:", Style::default().fg(Color::Cyan)),
                Span::styled(date, Style::default().fg(Color::Reset)),
            ]));
        }

        if let Some(mime) = &info.mime_type {
            meta_lines.push(Line::from(vec![
                Span::styled("  MIME:  ", Style::default().fg(Color::Cyan)),
                Span::styled(mime.as_str(), Style::default().fg(Color::Reset)),
            ]));
        }

        if let Some(hash) = &info.hash {
            meta_lines.extend(wrap_labeled_field(
                "  Hash:  ", hash,
                Style::default().fg(Color::Cyan),
                Style::default().fg(Color::DarkGray),
                wrap_w,
            ));
        }

        if let Some(entry) = self.entries.get(self.selected) {
            let mut markers = Vec::new();
            if entry.starred {
                markers.push(Span::styled("\u{2605} Starred", Style::default().fg(Color::Yellow)));
            }
            if self.cart_ids.contains(&entry.id) {
                if !markers.is_empty() { markers.push(Span::raw("  ")); }
                markers.push(Span::styled(
                    "\u{2606} In cart",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM),
                ));
            }
            if !markers.is_empty() {
                let mut line = vec![Span::styled("  ", Style::default())];
                line.extend(markers);
                meta_lines.push(Line::from(line));
            }
        }

        // footer_lines: full-width below the side-by-side area (link can be long)
        let mut footer_lines = Vec::new();

        if let Some(link) = &info.web_content_link {
            footer_lines.extend(wrap_labeled_field(
                "  Link:  ", link,
                Style::default().fg(Color::Cyan),
                Style::default().fg(Color::Blue),
                footer_wrap_w,
            ));
        }

        footer_lines.push(Line::from(""));
        footer_lines.push(Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )));

        let (in_bc, in_tc) = self.themed_colors(Color::Cyan);

        let title = format!(" \u{2139} Info: {} ", truncate_name(&info.name, 30));
        let title_style = Style::default()
            .fg(in_tc)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);
        let border_style = Style::default().fg(in_bc);

        if has_thumb {
            use crate::config::ThumbnailRenderMode;
            use ratatui_image::{picker::{Picker, ProtocolType}, StatefulImage};

            let inner_h = area.height.saturating_sub(2);
            let footer_h = footer_lines.len() as u16;
            let top_h = inner_h.saturating_sub(footer_h);
            let render_mode = self.config.thumbnail_mode.should_use_color();

            // For Auto mode: call from_query_stdio() ONCE and reuse for both
            // height calculation and rendering (calling twice consumes the terminal response)
            let auto_picker: Option<Picker> = if matches!(render_mode, ThumbnailRenderMode::Auto) {
                Picker::from_query_stdio().ok().map(|mut p| {
                    match self.config.current_image_protocol() {
                        crate::config::ImageProtocol::Auto => {
                            if p.protocol_type() == ProtocolType::Kitty {
                                if std::env::var("TERM_PROGRAM")
                                    .is_ok_and(|t| t.contains("iTerm"))
                                {
                                    p.set_protocol_type(ProtocolType::Iterm2);
                                }
                            }
                        }
                        crate::config::ImageProtocol::Kitty => p.set_protocol_type(ProtocolType::Kitty),
                        crate::config::ImageProtocol::Iterm2 => p.set_protocol_type(ProtocolType::Iterm2),
                        crate::config::ImageProtocol::Sixel => p.set_protocol_type(ProtocolType::Sixel),
                    }
                    p
                })
            } else {
                None
            };

            // Image height based on thumb column width (not full overlay width)
            let image_rows: u16 = if let Some(img) = image {
                match render_mode {
                    ThumbnailRenderMode::Auto => {
                        let (cw, ch) = auto_picker.as_ref()
                            .map(|p| { let f = p.font_size(); (f.0 as u32, f.1 as u32) })
                            .unwrap_or((10, 20));
                        if cw > 0 && ch > 0 && img.width() > 0 {
                            let rows = (img.height() * thumb_col_w as u32 * cw + img.width() * ch - 1)
                                / (img.width() * ch);
                            (rows as u16).min(top_h)
                        } else {
                            top_h
                        }
                    }
                    ThumbnailRenderMode::ColoredHalf | ThumbnailRenderMode::Grayscale => {
                        if img.width() > 0 {
                            let rows = img.height() * thumb_col_w as u32 / (img.width() * 2);
                            (rows as u16).max(1).min(top_h)
                        } else {
                            top_h
                        }
                    }
                    ThumbnailRenderMode::Off => 0,
                }
            } else {
                1
            };

            let inner = ratatui::layout::Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: inner_w,
                height: inner_h,
            };

            // Vertical split: top = side-by-side meta+thumb, bottom = full-width footer (link + hint)
            let v_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(top_h), Constraint::Min(0)])
                .split(inner);

            // Horizontal split within top: metadata text on left, thumbnail on right
            let h_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(thumb_col_w)])
                .split(v_chunks[0]);

            f.render_widget(Paragraph::new(Text::from(meta_lines)), h_chunks[0]);
            f.render_widget(Paragraph::new(Text::from(footer_lines)), v_chunks[1]);

            let thumb_area = h_chunks[1];
            if let Some(img) = image {
                // Vertically center the image in the right column
                let img_rect = if image_rows < thumb_area.height {
                    let y_offset = (thumb_area.height - image_rows) / 2;
                    ratatui::layout::Rect {
                        x: thumb_area.x,
                        y: thumb_area.y + y_offset,
                        width: thumb_col_w,
                        height: image_rows,
                    }
                } else {
                    thumb_area
                };
                match render_mode {
                    ThumbnailRenderMode::Auto => {
                        if let Some(picker) = auto_picker {
                            let render_rect = center_image_rect(img, img_rect);
                            let img_display = upscale_for_rect(img, render_rect, picker.font_size());
                            let mut protocol = picker.new_resize_protocol(img_display);
                            f.render_stateful_widget(StatefulImage::default(), render_rect, &mut protocol);
                        }
                    }
                    ThumbnailRenderMode::ColoredHalf => {
                        let colored_lines = render_image_to_colored_lines(img, thumb_col_w as u32, image_rows as u32);
                        f.render_widget(Paragraph::new(Text::from(colored_lines)), img_rect);
                    }
                    ThumbnailRenderMode::Grayscale => {
                        let ascii_lines = render_image_to_grayscale_lines(img, thumb_col_w as u32, image_rows as u32);
                        f.render_widget(
                            Paragraph::new(Text::from(ascii_lines)).style(Style::default().fg(Color::DarkGray)),
                            img_rect,
                        );
                    }
                    ThumbnailRenderMode::Off => {}
                }
            } else {
                // Spinner centered vertically in the thumbnail column
                let spinner_y = thumb_area.y + thumb_area.height / 2;
                let frame = SPINNER_FRAMES[self.spinner_idx];
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!(" {} Loading...", frame),
                        Style::default().fg(Color::DarkGray),
                    ))),
                    ratatui::layout::Rect { x: thumb_area.x, y: spinner_y, width: thumb_col_w, height: 1 },
                );
            }

            let block = self.styled_block()
                .title(title)
                .title_style(title_style)
                .border_style(border_style);
            f.render_widget(block, area);
        } else {
            let all_lines: Vec<_> = meta_lines.into_iter().chain(footer_lines).collect();
            let p = Paragraph::new(Text::from(all_lines)).block(
                self.styled_block()
                    .title(title)
                    .title_style(title_style)
                    .border_style(border_style),
            );
            f.render_widget(p, area);
        }
    }

    // --- Text preview overlay ---

    fn draw_text_preview_overlay(
        &self,
        f: &mut Frame,
        name: &str,
        highlighted: &[Line],
        truncated: bool,
    ) {
        let area = self.prepare_overlay(f, 60, 70);

        let inner_height = area.height.saturating_sub(2) as usize;
        let max_lines = inner_height.saturating_sub(if truncated { 2 } else { 1 });
        let mut lines: Vec<Line> = highlighted.iter().take(max_lines).cloned().collect();

        if truncated {
            lines.push(Line::from(Span::styled(
                format!(
                    " ... truncated at {} ",
                    format_size(self.config.preview_max_size)
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )));

        let (in_bc, in_tc) = self.themed_colors(Color::Cyan);
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(format!(" {} ", truncate_name(name, 40)))
                .title_style(Style::default().fg(in_tc).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(in_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Folder listing popup ---

    fn draw_info_folder_overlay(&self, f: &mut Frame, name: &str, entries: &[Entry]) {
        let visible = entries.len().min(20);
        let total_lines = 2 + visible + 2; // padding + items + hint + padding
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1))
            .max(25)
            .min(70);
        let area = centered_rect(60, pct, f.area());
        clear_overlay_area(f, area);

        let mut lines = vec![Line::from("")];

        if entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (empty folder)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for e in entries.iter().take(20) {
                let cat = theme::categorize(e);
                let ico = theme::icon(cat, self.config.nerd_font);
                let c = self.file_color(cat);
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(ico, Style::default().fg(c)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&e.name, Style::default().fg(c)),
                ]));
            }
            if entries.len() > 20 {
                lines.push(Line::from(Span::styled(
                    format!("  ... and {} more", entries.len() - 20),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )));

        let (in_bc, in_tc) = self.themed_colors(Color::Cyan);
        let title = format!(" {} ({}) ", truncate_name(name, 25), entries.len());
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(title)
                .title_style(Style::default().fg(in_tc).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(in_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Settings overlay ---

    fn draw_settings_overlay(
        &self,
        f: &mut Frame,
        selected: usize,
        editing: bool,
        draft: &crate::config::TuiConfig,
        modified: bool,
    ) {
        let area = centered_rect(70, 65, f.area());
        self.settings_area.set(area);
        clear_overlay_area(f, area);

        // Categorized settings: (category_name, settings_list)
        // Each setting: (name, description, value)
        type SettingItem = (String, String, String);
        let categories: Vec<(&str, Vec<SettingItem>)> = vec![
            (
                "UI Settings",
                vec![
                    (
                        "Nerd Font Icons".to_string(),
                        "Use Nerd Font icons in TUI".to_string(),
                        if draft.nerd_font { "[✓]" } else { "[ ]" }.to_string(),
                    ),
                    (
                        "Border Style".to_string(),
                        "Window border appearance".to_string(),
                        draft.border_style.as_str().to_string(),
                    ),
                    (
                        "Color Scheme".to_string(),
                        "UI color theme".to_string(),
                        draft.color_scheme.as_str().to_string(),
                    ),
                    (
                        "Show Help Bar".to_string(),
                        "Display keyboard shortcuts".to_string(),
                        if draft.show_help_bar { "[✓]" } else { "[ ]" }.to_string(),
                    ),
                    (
                        "Quota Bar Style".to_string(),
                        "Storage usage display style".to_string(),
                        draft.quota_bar_style.as_str().to_string(),
                    ),
                ],
            ),
            (
                "Preview Settings",
                vec![
                    (
                        "Show Preview Pane".to_string(),
                        "Enable three-column layout".to_string(),
                        if draft.show_preview { "[✓]" } else { "[ ]" }.to_string(),
                    ),
                    (
                        "Lazy Preview".to_string(),
                        "Auto-load preview after delay".to_string(),
                        if draft.lazy_preview { "[✓]" } else { "[ ]" }.to_string(),
                    ),
                    (
                        "Preview Max Size".to_string(),
                        "Maximum bytes for text preview".to_string(),
                        format!("{} KB", draft.preview_max_size / 1024),
                    ),
                    (
                        "Thumbnail Mode".to_string(),
                        "Colored thumbnail rendering".to_string(),
                        draft.thumbnail_mode.display_name().to_string(),
                    ),
                    (
                        "Image Protocol".to_string(),
                        "Terminal image rendering protocol".to_string(),
                        ">".to_string(),
                    ),
                ],
            ),
            (
                "Sort Settings",
                vec![
                    (
                        "Sort Field".to_string(),
                        "Field to sort entries by".to_string(),
                        draft.sort_field.as_str().to_string(),
                    ),
                    (
                        "Reverse Order".to_string(),
                        "Reverse sort direction".to_string(),
                        if draft.sort_reverse { "[\u{2713}]" } else { "[ ]" }.to_string(),
                    ),
                ],
            ),
            (
                "Interface Settings",
                vec![
                    (
                        "Move Mode".to_string(),
                        "Interface for move/copy operations".to_string(),
                        draft.move_mode.clone(),
                    ),
                    (
                        "CLI Nerd Font".to_string(),
                        "Use icons in CLI output".to_string(),
                        if draft.cli_nerd_font { "[\u{2713}]" } else { "[ ]" }.to_string(),
                    ),
                ],
            ),
            (
                "Playback Settings",
                vec![
                    (
                        "Player Command".to_string(),
                        "External player for video playback".to_string(),
                        draft.player.as_deref().unwrap_or("(none)").to_string(),
                    ),
                ],
            ),
            (
                "Download Settings",
                vec![
                    (
                        "Concurrent Downloads".to_string(),
                        "Simultaneous cart downloads (1 = sequential)".to_string(),
                        draft.download_jobs.to_string(),
                    ),
                ],
            ),
        ];

        // Map each item index to its line position for scrolling
        let mut item_line_map: Vec<usize> = Vec::new();
        let mut line_idx = 0;
        for (_cat_name, items) in &categories {
            line_idx += 1; // Category header
            for _ in items {
                item_line_map.push(line_idx);
                line_idx += 2; // Name line + description line
            }
        }

        let inner_height = area.height.saturating_sub(4) as usize; // -2 borders, -2 for blank+help
        let selected_line = item_line_map.get(selected).copied().unwrap_or(0);
        let scroll_offset = if selected_line >= inner_height {
            (selected_line + 2).saturating_sub(inner_height)
        } else {
            0
        };

        let mut lines = vec![Line::from("")];
        let mut global_idx = 0;

        for (cat_name, items) in &categories {
            lines.push(Line::from(Span::styled(
                format!(" {}", cat_name),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));

            for (name, desc, value) in items {
                let is_selected = global_idx == selected;
                let prefix = if is_selected { " › " } else { "   " };

                let name_style = if is_selected && editing {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Reset)
                };

                let value_style = if is_selected && editing {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Green)
                };

                let is_text_input_item = name == "Player Command";
                let cur = if self.cursor_visible { "\u{2588}" } else { " " };

                let mut name_value_spans = vec![
                    Span::styled(prefix, name_style),
                    Span::styled(name.clone(), name_style),
                ];

                if is_text_input_item && is_selected && editing {
                    // Show as inline text input: "Player Command: value█"
                    name_value_spans.push(Span::styled(": ", Style::default().fg(Color::DarkGray)));
                    let display_val = draft.player.as_deref().unwrap_or("");
                    name_value_spans.push(Span::styled(
                        format!("{}{}", display_val, cur),
                        Style::default().fg(Color::Yellow),
                    ));
                } else {
                    // Right-align value with padding
                    let terminal_width = area.width.saturating_sub(4) as usize;
                    let name_len = prefix.len() + name.len();
                    let value_len = value.len();
                    let padding = terminal_width.saturating_sub(name_len + value_len + 1);

                    name_value_spans.push(Span::raw(" ".repeat(padding)));
                    name_value_spans.push(Span::styled(value.clone(), value_style));
                }

                lines.push(Line::from(name_value_spans));
                lines.push(Line::from(Span::styled(
                    format!("     {}", desc),
                    Style::default().fg(Color::DarkGray),
                )));

                global_idx += 1;
            }
        }

        lines.push(Line::from(""));

        // Help bar
        let hints = if editing {
            vec![
                ("Left/Right", "change"),
                ("Space", "toggle"),
                ("Enter", "confirm"),
                ("Esc", "cancel"),
            ]
        } else {
            vec![
                ("j/k", "nav"),
                ("Space/Enter", "edit"),
                ("s", "save"),
                ("Esc", "close"),
            ]
        };
        lines.push(Self::hint_line(&hints));

        // Apply scroll offset
        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(scroll_offset)
            .take(inner_height + 2) // +2 for blank and help
            .collect();

        let (st_bc, st_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Yellow)
        };

        let title = if modified { "Settings *" } else { "Settings" };
        f.render_widget(
            Paragraph::new(Text::from(visible_lines)).block(self.overlay_block(title, st_bc, st_tc)),
            area,
        );
    }

    // --- Image protocol settings overlay ---

    fn draw_image_protocol_overlay(
        &self,
        f: &mut Frame,
        selected: usize,
        draft: &crate::config::TuiConfig,
        modified: bool,
        current_terminal: &str,
        terminals: &[String],
    ) {
        let area = centered_rect(70, 60, f.area());
        self.settings_area.set(area);
        clear_overlay_area(f, area);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Current terminal: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    current_terminal,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
        ];

        for (i, term) in terminals.iter().enumerate() {
            let is_selected = i == selected;
            let is_current = term == current_terminal;
            let prefix = if is_selected { " \u{203a} " } else { "   " };
            let marker = if is_current { " *" } else { "" };

            let proto = draft
                .image_protocols
                .get(term)
                .copied()
                .unwrap_or(crate::config::ImageProtocol::Auto);

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Reset)
            };

            let value_str = format!("< {} >", proto.display_name());
            let value_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };

            let terminal_width = area.width.saturating_sub(4) as usize;
            let name_with_marker = format!("{}{}{}", prefix, term, marker);
            let name_len = name_with_marker.len();
            let value_len = value_str.len();
            let padding = terminal_width.saturating_sub(name_len + value_len + 1);

            lines.push(Line::from(vec![
                Span::styled(prefix, name_style),
                Span::styled(term.as_str(), name_style),
                Span::styled(marker, Style::default().fg(Color::Yellow)),
                Span::raw(" ".repeat(padding)),
                Span::styled(value_str, value_style),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Self::hint_line(&[
            ("j/k", "nav"),
            ("Left/Right", "protocol"),
            ("s", "save"),
            ("Esc", "back"),
        ]));

        let (st_bc, st_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Yellow)
        };

        let title = if modified { "Image Protocol *" } else { "Image Protocol" };
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(self.overlay_block(title, st_bc, st_tc)),
            area,
        );
    }

    // --- Custom color settings overlay ---

    fn draw_custom_color_overlay(
        &self,
        f: &mut Frame,
        selected: usize,
        draft: &crate::config::TuiConfig,
        modified: bool,
        editing_rgb: bool,
        rgb_input: &str,
        rgb_component: usize,
    ) {
        let area = centered_rect(70, 70, f.area());
        self.settings_area.set(area);
        clear_overlay_area(f, area);

        let colors = [
            ("Folder", draft.custom_colors.folder),
            ("Archive", draft.custom_colors.archive),
            ("Image", draft.custom_colors.image),
            ("Video", draft.custom_colors.video),
            ("Audio", draft.custom_colors.audio),
            ("Document", draft.custom_colors.document),
            ("Code", draft.custom_colors.code),
            ("Default", draft.custom_colors.default),
        ];

        let mut lines = vec![Line::from("")];

        for (i, (name, (r, g, b))) in colors.iter().enumerate() {
            let is_selected = i == selected;
            let prefix = if is_selected { " › " } else { "   " };

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Reset)
            };

            // Show color preview and RGB values
            let color_preview = "███";
            let rgb_text = format!("R:{:3} G:{:3} B:{:3}", r, g, b);

            let mut spans = vec![
                Span::styled(prefix, name_style),
                Span::styled(format!("{:<12}", name), name_style),
                Span::styled(color_preview, Style::default().fg(Color::Rgb(*r, *g, *b))),
                Span::raw("  "),
                Span::styled(rgb_text, Style::default().fg(Color::DarkGray)),
            ];

            // Show editing indicator
            if is_selected && editing_rgb {
                let component_name = match rgb_component {
                    0 => "R",
                    1 => "G",
                    2 => "B",
                    _ => "?",
                };
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    format!("  Editing {}: {}_", component_name, rgb_input),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));

        let hints: &[(&str, &str)] = if editing_rgb {
            &[("0-9", "input"), ("Enter", "confirm"), ("Esc", "cancel")]
        } else {
            &[("j/k", "nav"), ("r/g/b", "edit RGB"), ("s", "save"), ("Esc", "back")]
        };
        lines.push(Self::hint_line(hints));

        let (st_bc, st_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Yellow)
        };

        let title = if modified { "Custom Colors *" } else { "Custom Colors" };
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(self.overlay_block(title, st_bc, st_tc)),
            area,
        );
    }

    // --- Share overlays ---

    fn draw_share_prompt_overlay(&self, f: &mut Frame) {
        let area = self.prepare_overlay(f, 50, 20);
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::LightBlue)
        };
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Create share for all cart items:",
                Style::default().fg(Color::Reset),
            )),
            Line::from(""),
            Self::hint_line(&[("p", "public share"), ("P", "with password"), ("Esc", "cancel")]),
        ];
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(self.overlay_block("Create Share", bc, tc)),
            area,
        );
    }

    fn draw_share_created_view(&self, f: &mut Frame, shares: &[(String, String, String)]) {
        if shares.is_empty() {
            return;
        }
        let (bc_top, tc_top) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::LightBlue)
        };
        let frame = f.area();
        let card_w = (frame.width * 65 / 100).max(30).min(frame.width.saturating_sub(4));
        let card_h = 8u16;

        for (i, (title, url, pass_code)) in shares.iter().enumerate() {
            let ox = (i as u16) * 3;
            let oy = (i as u16) * 1;
            let area = Rect {
                x: 2 + ox,
                y: 1 + oy,
                width: card_w.min(frame.width.saturating_sub(2 + ox)),
                height: card_h,
            };
            clear_overlay_area(f, area);

            let is_top = i == shares.len() - 1;
            let (bc, tc) = if is_top {
                (bc_top, tc_top)
            } else {
                (Color::DarkGray, Color::DarkGray)
            };

            let name_max = area.width.saturating_sub(4) as usize;
            let mut lines = vec![Line::from("")];
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    truncate_name(url, name_max),
                    Style::default().fg(if is_top { Color::Reset } else { Color::DarkGray }),
                ),
            ]));
            if !pass_code.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  Password: "),
                    Span::styled(
                        pass_code.clone(),
                        Style::default().fg(if is_top { Color::Yellow } else { Color::DarkGray }),
                    ),
                ]));
            } else {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(""));
            if is_top {
                lines.push(Self::hint_line(&[
                    ("y", "copy URL"),
                    ("Esc", "close"),
                    ("Ctrl+Esc", "close all"),
                ]));
            }

            let card_title = format!("Share: {}", truncate_name(title, 30));
            f.render_widget(
                Paragraph::new(Text::from(lines)).block(self.overlay_block(&card_title, bc, tc)),
                area,
            );
        }
    }

    fn draw_my_shares_view(
        &self,
        f: &mut Frame,
        shares: &[crate::pikpak::MyShare],
        selected: usize,
        confirm_delete: Option<&str>,
    ) {
        let (bc, tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::LightBlue)
        };

        // Outer: content + optional help bar
        let (main_area, help_bar_area) = self.layout_with_help_bar(f.area());

        if shares.is_empty() {
            let lines = vec![
                Line::from(""),
                widgets::empty_state_line("No shares found."),
                Line::from(""),
                Self::hint_line(&[("r", "refresh"), ("Esc", "back")]),
            ];
            f.render_widget(
                Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(Span::styled(" My Shares ", Style::default().fg(tc)))
                        .border_style(Style::default().fg(bc)),
                ),
                main_area,
            );
        } else {
            // Two-pane: list (38%) | detail (62%)
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(main_area);
            let list_area = panes[0];
            let detail_area = panes[1];

            // --- Left pane: list ---
            let list_title = format!(" My Shares ({}) ", shares.len());
            // badge section: "  public  permanent" = 2+7+2+9 = 20 chars; prefix = 3
            const BADGE_W: u16 = 20;
            const PREFIX_W: u16 = 3;
            let name_col = list_area.width.saturating_sub(PREFIX_W + BADGE_W + 2) as usize;
            // usable rows: borders(2) + leading blank(1) + confirm/nothing(1) = subtract 4
            // but we removed the inner hint line, so just borders + blank
            let usable = list_area.height.saturating_sub(3) as usize;
            let scroll_offset = widgets::scroll_offset(selected, usable);

            let mut list_lines = vec![Line::from("")];
            for (i, share) in shares.iter().enumerate().skip(scroll_offset).take(usable) {
                let is_sel = i == selected;
                let prefix = if is_sel { " \u{203a} " } else { "   " };
                let name_style = if is_sel {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Reset)
                };
                let is_pw = share_is_password(share);
                let (type_str, type_color) = if is_pw {
                    ("private  ", Color::Yellow)
                } else {
                    ("public   ", Color::Green)
                };
                let expiry_str = share_expiry_label(&share.expiration_days);
                let expiry_color = share_expiry_color(&share.expiration_days);

                // Pad title so badges start at a fixed column
                let title = truncate_name(&share.title, name_col);
                let pad = " ".repeat(name_col.saturating_sub(title.chars().count()) + 2);

                list_lines.push(Line::from(vec![
                    Span::styled(prefix, name_style),
                    Span::styled(title, name_style),
                    Span::styled(pad, Style::default()),
                    Span::styled(type_str, Style::default().fg(type_color)),
                    Span::styled(expiry_str, Style::default().fg(expiry_color)),
                ]));
            }

            if shares.len() > scroll_offset + usable {
                let n = shares.len() - scroll_offset - usable;
                list_lines.push(Line::from(Span::styled(
                    format!("  +{} more", n),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            // Confirm-delete prompt (only shown when active; no inner hint bar otherwise)
            if confirm_delete.is_some() {
                list_lines.push(Line::from(""));
                list_lines.push(Line::from(vec![
                    Span::styled("  Delete? ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::styled("y", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(" yes  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("n", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled("/Esc no", Style::default().fg(Color::DarkGray)),
                ]));
            }

            f.render_widget(
                Paragraph::new(Text::from(list_lines)).block(
                    self.styled_block()
                        .title(Span::styled(list_title, Style::default().fg(tc)))
                        .border_style(Style::default().fg(bc)),
                ),
                list_area,
            );

            // --- Right pane: detail ---
            let detail_lines = if let Some(share) = shares.get(selected) {
                share_detail_lines(share, detail_area.width)
            } else {
                vec![Line::from("")]
            };
            f.render_widget(
                Paragraph::new(Text::from(detail_lines)).block(
                    self.styled_block()
                        .title(Span::styled(" Detail ", Style::default().fg(Color::DarkGray)))
                        .border_style(Style::default().fg(Color::DarkGray)),
                ),
                detail_area,
            );
        }

        if let Some(bar_area) = help_bar_area {
            let pairs = self.help_pairs();
            let mut spans = vec![Span::raw(" ")];
            spans.extend(Self::styled_help_spans(&pairs));
            f.render_widget(Paragraph::new(Line::from(spans)), bar_area);
        }
    }
}

fn share_is_password(share: &crate::pikpak::MyShare) -> bool {
    !share.pass_code.is_empty() || share.share_to.contains("encrypted")
}

fn share_expiry_label(days: &str) -> String {
    match days {
        "" | "-1" | "0" => "permanent".to_string(),
        d => {
            let n = d.parse::<i64>().unwrap_or(0);
            format!("{} days", n)
        }
    }
}

fn share_expiry_color(days: &str) -> Color {
    match days {
        "" | "-1" | "0" => Color::Green,
        d => match d.parse::<i64>().unwrap_or(99) {
            n if n <= 3 => Color::Red,
            n if n <= 7 => Color::Yellow,
            _ => Color::DarkGray,
        },
    }
}

fn share_detail_lines(share: &crate::pikpak::MyShare, width: u16) -> Vec<Line<'static>> {
    let label = Style::default().fg(Color::DarkGray);
    let value = Style::default().fg(Color::Reset);
    let url_max = width.saturating_sub(14) as usize;

    let is_pw = share_is_password(share);
    let type_str = if is_pw { "private" } else { "public" }.to_string();
    let type_color = if is_pw { Color::Yellow } else { Color::Green };

    let expiry_str = share_expiry_label(&share.expiration_days);
    let expiry_color = share_expiry_color(&share.expiration_days);

    let date = share.create_time.get(..10).unwrap_or(share.create_time.as_str()).to_string();
    let views = share.view_count.parse::<u64>().unwrap_or(0);
    let saves = share.restore_count.parse::<u64>().unwrap_or(0);
    let files = share.file_num.parse::<u64>().unwrap_or(0);

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];

    // Title
    lines.push(Line::from(vec![
        Span::styled("  Title   ", label),
        Span::styled(share.title.clone(), value.add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(""));

    // URL (full, wraps visually by truncation)
    lines.push(Line::from(vec![
        Span::styled("  URL     ", label),
        Span::styled(
            truncate_name(&share.share_url, url_max),
            Style::default().fg(Color::Cyan),
        ),
    ]));
    lines.push(Line::from(""));

    // Type + expiry on one row
    lines.push(Line::from(vec![
        Span::styled("  Type    ", label),
        Span::styled(type_str, Style::default().fg(type_color)),
        Span::styled("   Expiry  ", label),
        Span::styled(expiry_str, Style::default().fg(expiry_color)),
    ]));

    // Password (if set)
    if is_pw && !share.pass_code.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Pass    ", label),
            Span::styled(
                share.pass_code.clone(),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Created
    lines.push(Line::from(vec![
        Span::styled("  Created ", label),
        Span::styled(date, Style::default().fg(Color::Blue)),
    ]));
    lines.push(Line::from(""));

    // Stats row
    let stats = format!(
        "{}   saves {}   files {}",
        views, saves, files
    );
    lines.push(Line::from(vec![
        Span::styled("  Views   ", label),
        Span::styled(stats, value),
    ]));
    lines.push(Line::from(""));

    // Share ID (dimmed, for copy/reference)
    lines.push(Line::from(vec![
        Span::styled("  ID      ", label),
        Span::styled(share.share_id.clone(), Style::default().fg(Color::DarkGray)),
    ]));

    lines
}

pub(super) fn clear_overlay_area(f: &mut Frame, area: ratatui::layout::Rect) {
    let full = f.area();
    let extended = ratatui::layout::Rect {
        x: area.x.saturating_sub(1),
        y: area.y,
        width: area.width + if area.x > 0 { 2 } else { 1 },
        height: area.height,
    };
    f.render_widget(Clear, extended.intersection(full));
}

fn wrap_labeled_field<'a>(
    label: &'a str,
    value: &'a str,
    label_style: Style,
    value_style: Style,
    total_width: usize,
) -> Vec<Line<'a>> {
    use unicode_width::UnicodeWidthChar;

    let label_w: usize = label.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    let first_line_budget = total_width.saturating_sub(label_w);
    if first_line_budget == 0 {
        return vec![Line::from(vec![
            Span::styled(label, label_style),
            Span::styled(value, value_style),
        ])];
    }

    let cont_budget = total_width.saturating_sub(label_w);
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w: usize = 0;
    let mut first = true;
    let mut last_break: Option<(usize, usize)> = None;

    for ch in value.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        let budget = if first { first_line_budget } else { cont_budget };

        if current_w + ch_w > budget && !current.is_empty() {
            if let Some((brk_byte, _)) = last_break {
                if brk_byte < current.len() {
                    let remainder = current[brk_byte..].to_string();
                    current.truncate(brk_byte);
                    segments.push(current.trim_end().to_string());
                    current = remainder.trim_start().to_string();
                    current_w = current.chars()
                        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                        .sum();
                } else {
                    segments.push(std::mem::take(&mut current));
                    current_w = 0;
                }
            } else {
                segments.push(std::mem::take(&mut current));
                current_w = 0;
            }
            last_break = None;
            if first {
                first = false;
            }
            let new_budget = if first { first_line_budget } else { cont_budget };
            if current_w + ch_w > new_budget && !current.is_empty() {
                segments.push(std::mem::take(&mut current));
                current_w = 0;
                last_break = None;
            }
        }

        // Word boundaries: after spaces, before CJK (width >= 2)
        if ch == ' ' {
            current.push(ch);
            current_w += ch_w;
            last_break = Some((current.len(), current_w));
        } else if ch_w >= 2 {
            last_break = Some((current.len(), current_w));
            current.push(ch);
            current_w += ch_w;
        } else {
            current.push(ch);
            current_w += ch_w;
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }

    let indent: String = " ".repeat(label_w);
    let mut result = Vec::with_capacity(segments.len());
    for (i, seg) in segments.into_iter().enumerate() {
        if i == 0 {
            result.push(Line::from(vec![
                Span::styled(label, label_style),
                Span::styled(seg, value_style),
            ]));
        } else {
            result.push(Line::from(vec![
                Span::raw(indent.clone()),
                Span::styled(seg, value_style),
            ]));
        }
    }
    if result.is_empty() {
        result.push(Line::from(Span::styled(label, label_style)));
    }
    result
}

/// Build a large ⚠ warning triangle using block characters.
///
/// Design (█=yellow, inner=black-on-yellow):
/// ```text
///          ▄▄               row 0  tip
///         ▄██▄              row 1
///        ▄█  █▄             row 2  inner 2
///       ▄█ ██ █▄            row 3  inner 4  "!" bar
///      ▄█  ██  █▄           row 4  inner 6  "!" bar
///     ▄█   ██   █▄          row 5  inner 8  "!" bar
///    ▄█          █▄         row 6  inner 10  gap
///   ▄█     ██     █▄        row 7  inner 12  dot
///   ████████████████        row 8  base
/// ```
fn warn_triangle_lines() -> Vec<Line<'static>> {
    let w = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let bg = Style::default().fg(Color::Black).bg(Color::Yellow);

    // Helper: build a row with ▄█ ... █▄ walls and centered inner content
    // pad = leading spaces, inner_w = inner width, content = centered content str
    let row = |pad: usize, inner: Vec<Span<'static>>| -> Line<'static> {
        let mut spans = vec![Span::styled(" ".repeat(pad), Style::default())];
        spans.push(Span::styled("\u{2584}\u{2588}", w)); // ▄█
        spans.extend(inner);
        spans.push(Span::styled("\u{2588}\u{2584}", w)); // █▄
        Line::from(spans)
    };

    // Centered ██ on bg within `width` chars
    let bang = |width: usize| -> Vec<Span<'static>> {
        let side = (width - 2) / 2;
        vec![Span::styled(
            format!("{}\u{2588}\u{2588}{}", " ".repeat(side), " ".repeat(side)),
            bg,
        )]
    };

    // All spaces on bg (gap row)
    let gap = |width: usize| -> Vec<Span<'static>> { vec![Span::styled(" ".repeat(width), bg)] };

    vec![
        // row 0: tip
        Line::from(Span::styled(
            format!("{}\u{2584}\u{2584}", " ".repeat(10)),
            w,
        )),
        // row 1: ▄██▄
        Line::from(Span::styled(
            format!("{}\u{2584}\u{2588}\u{2588}\u{2584}", " ".repeat(9)),
            w,
        )),
        // row 2: inner=2, empty
        row(8, gap(2)),
        // row 3: inner=4, "!" bar  " ██ "
        row(7, bang(4)),
        // row 4: inner=6, "!" bar  "  ██  "
        row(6, bang(6)),
        // row 5: inner=8, "!" bar  "   ██   "
        row(5, bang(8)),
        // row 6: inner=10, gap
        row(4, gap(10)),
        // row 7: inner=12, dot  "     ██     "
        row(3, bang(12)),
        // row 8: base ████████████████
        Line::from(Span::styled(
            format!("{}{}", " ".repeat(3), "\u{2588}".repeat(16)),
            w,
        )),
    ]
}

/// Map a base color to its vibrant (light) variant.
fn vibrant(c: Color) -> Color {
    match c {
        Color::Red => Color::LightRed,
        Color::Green => Color::LightGreen,
        Color::Yellow => Color::LightYellow,
        Color::Blue => Color::LightBlue,
        Color::Cyan => Color::LightCyan,
        Color::Magenta => Color::LightMagenta,
        other => other,
    }
}

