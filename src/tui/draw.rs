use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};

use crate::config::{BorderStyle, ColorScheme};
use crate::pikpak::{Entry, EntryKind};
use crate::theme;

use super::completion::PathInput;
use super::local_completion::LocalPathInput;
use super::{App, InputMode, LoginField, PreviewState, SPINNER_FRAMES, centered_rect, format_size};

impl App {
    pub(super) fn draw(&self, f: &mut Frame) {
        match &self.input {
            InputMode::Login { .. } => self.draw_login_screen(f),
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => self.draw_picker(f),
            InputMode::DownloadView => self.draw_download_view(f),
            _ => self.draw_main(f),
        }
    }

    /// Build a `Block` with the configured border style applied.
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

    /// File-type color respecting the selected color scheme.
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
        let bg = Block::default().style(Style::default().bg(Color::Black));
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
            f.render_widget(Clear, area);
            let email_style = match field {
                LoginField::Email => Style::default().fg(Color::Yellow),
                LoginField::Password => Style::default().fg(Color::White),
            };
            let pass_style = match field {
                LoginField::Password => Style::default().fg(Color::Yellow),
                LoginField::Email => Style::default().fg(Color::White),
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

            let (bc, tc) = if self.is_vibrant() {
                (Color::LightCyan, Color::LightCyan)
            } else {
                (Color::Cyan, Color::Cyan)
            };
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
        // Outer vertical split: main area + optional help bar
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
        let main_area = outer[0];

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
        if self.config.show_help_bar {
            let pairs = self.help_pairs();
            let mut spans = vec![Span::raw(" ")];
            spans.extend(Self::styled_help_spans(&pairs));
            let bar = Paragraph::new(Line::from(spans));
            f.render_widget(bar, outer[1]);
        }

        self.draw_overlay(f);

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
                let star_marker = if e.starred { " \u{2605}" } else { "" };
                let cart_marker = if self.cart_ids.contains(&e.id) {
                    " \u{2606}"
                } else {
                    ""
                };
                ListItem::new(Line::from(vec![
                    Span::styled(ico, Style::default().fg(c)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&e.name, Style::default().fg(c)),
                    Span::styled(size_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(star_marker, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        cart_marker,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::DIM),
                    ),
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
    }

    fn draw_preview_pane(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        match &self.preview_state {
            PreviewState::Empty => {
                let hint = if self.config.lazy_preview {
                    "Select an item"
                } else {
                    "Press Space to load preview"
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
                let mut lines = vec![Line::from("")];
                if let Some(entry) = self.entries.get(self.selected) {
                    lines.push(Line::from(vec![
                        Span::styled("  Name:  ", Style::default().fg(Color::Cyan)),
                        Span::styled(&entry.name, Style::default().fg(Color::White)),
                    ]));
                    if entry.kind == EntryKind::File {
                        lines.push(Line::from(vec![
                            Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format_size(entry.size),
                                Style::default().fg(Color::White),
                            ),
                        ]));
                    }
                    if !entry.created_time.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  Time:  ", Style::default().fg(Color::Cyan)),
                            Span::styled(&entry.created_time, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    lines.push(Line::from(""));
                    let hint = if entry.kind == EntryKind::File
                        && crate::theme::is_text_previewable(entry)
                    {
                        if entry.size <= self.config.preview_max_size {
                            "  Press Space to preview"
                        } else {
                            "  Press p to preview (large file)"
                        }
                    } else {
                        "  Press Space for details"
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
            PreviewState::FileDetailedInfo(info) => {
                let mut lines = vec![Line::from("")];
                lines.push(Line::from(vec![
                    Span::styled("  Name:  ", Style::default().fg(Color::Cyan)),
                    Span::styled(&info.name, Style::default().fg(Color::White)),
                ]));
                if let Some(size) = &info.size {
                    let size_n: u64 = size.parse().unwrap_or(0);
                    lines.push(Line::from(vec![
                        Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            format!("{} ({})", format_size(size_n), size),
                            Style::default().fg(Color::White),
                        ),
                    ]));
                }
                if let Some(hash) = &info.hash {
                    lines.push(Line::from(vec![
                        Span::styled("  Hash:  ", Style::default().fg(Color::Cyan)),
                        Span::styled(hash.as_str(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
                if let Some(link) = &info.web_content_link {
                    let display = if link.len() > 50 {
                        format!("{}...", &link[..50])
                    } else {
                        link.clone()
                    };
                    lines.push(Line::from(vec![
                        Span::styled("  Link:  ", Style::default().fg(Color::Cyan)),
                        Span::styled(display, Style::default().fg(Color::Blue)),
                    ]));
                }

                let p = Paragraph::new(Text::from(lines))
                    .block(
                        self.styled_block()
                            .title(format!(" \u{2139} {} ", truncate_name(&info.name, 25)))
                            .title_style(
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            )
                            .border_style(Style::default().fg(Color::DarkGray)),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, area);
            }
        }
    }

    fn draw_log_overlay(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        f.render_widget(Clear, area);
        let log_lines: Vec<Line> = self
            .logs
            .iter()
            .rev()
            .take(area.height.saturating_sub(2) as usize)
            .rev()
            .map(|s| Line::from(s.as_str()))
            .collect();
        let (log_bc, log_tc) = if self.is_vibrant() {
            (Color::Magenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Green)
        };
        let logs = Paragraph::new(Text::from(log_lines))
            .block(
                self.styled_block()
                    .title(" Logs (l to close) ")
                    .title_style(Style::default().fg(log_tc))
                    .border_style(Style::default().fg(log_bc)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(logs, area);
    }

    pub(super) fn help_pairs(&self) -> Vec<(&str, &str)> {
        match &self.input {
            InputMode::Normal => {
                vec![
                    ("j/k", "nav"),
                    ("Enter", "open"),
                    ("Bksp", "back"),
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
            InputMode::MoveInput { .. } | InputMode::CopyInput { .. } => vec![
                ("Tab", "complete"),
                ("Enter", "confirm"),
                ("Ctrl+B", "picker"),
                ("Esc", "cancel"),
            ],
            InputMode::Rename { .. } | InputMode::Mkdir { .. } => {
                vec![("Enter", "confirm"), ("Esc", "cancel")]
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
                ("Esc", "close"),
            ],
            InputMode::DownloadInput { .. } => {
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
            | InputMode::DownloadView => {}

            InputMode::MoveInput { input, .. } => {
                self.draw_path_input_overlay(f, "Move", "Move to path", input, cur);
            }
            InputMode::CopyInput { input, .. } => {
                self.draw_path_input_overlay(f, "Copy", "Copy to path", input, cur);
            }
            InputMode::Rename { value } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let rename_hints = vec![("Enter", "confirm"), ("Esc", "cancel")];
                let mut hint_spans = vec![Span::raw("  ")];
                hint_spans.extend(Self::styled_help_spans(&rename_hints));
                let (rn_bc, rn_tc) = if self.is_vibrant() {
                    (Color::LightYellow, Color::LightYellow)
                } else {
                    (Color::Cyan, Color::Yellow)
                };
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  New name: ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            format!("{}{}", value, cur),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(hint_spans),
                ]))
                .block(
                    self.styled_block()
                        .title(" Rename ")
                        .title_style(Style::default().fg(rn_tc))
                        .border_style(Style::default().fg(rn_bc)),
                );
                f.render_widget(p, area);
            }
            InputMode::Mkdir { value } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let mkdir_hints = vec![("Enter", "confirm"), ("Esc", "cancel")];
                let mut hint_spans = vec![Span::raw("  ")];
                hint_spans.extend(Self::styled_help_spans(&mkdir_hints));
                let (mk_bc, mk_tc) = if self.is_vibrant() {
                    (Color::LightYellow, Color::LightYellow)
                } else {
                    (Color::Cyan, Color::Yellow)
                };
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  Folder name: ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            format!("{}{}", value, cur),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(hint_spans),
                ]))
                .block(
                    self.styled_block()
                        .title(" New Folder ")
                        .title_style(Style::default().fg(mk_tc))
                        .border_style(Style::default().fg(mk_bc)),
                );
                f.render_widget(p, area);
            }
            InputMode::ConfirmDelete => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let name = self
                    .current_entry()
                    .map(|e| e.name.as_str())
                    .unwrap_or("<none>");
                let del_hints = vec![("y", "trash"), ("p", "permanent"), ("n/Esc", "cancel")];
                let mut hint_spans = vec![Span::raw("  ")];
                hint_spans.extend(Self::styled_help_spans(&del_hints));
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  Delete ", Style::default().fg(Color::Red)),
                        Span::styled(
                            format!("`{}`", name),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" to trash?", Style::default().fg(Color::Red)),
                    ]),
                    Line::from(""),
                    Line::from(hint_spans),
                ]))
                .block({
                    let (del_bc, del_tc) = if self.is_vibrant() {
                        (Color::LightRed, Color::LightRed)
                    } else {
                        (Color::Red, Color::Red)
                    };
                    self.styled_block()
                        .title(" Confirm Remove ")
                        .title_style(Style::default().fg(del_tc))
                        .border_style(Style::default().fg(del_bc))
                });
                f.render_widget(p, area);
            }
            InputMode::ConfirmPermanentDelete { value } => {
                let area = centered_rect(60, 55, f.area());
                f.render_widget(Clear, area);
                let name = self
                    .current_entry()
                    .map(|e| e.name.as_str())
                    .unwrap_or("<none>");
                let perm_hints = vec![("Enter", "confirm"), ("Esc", "cancel")];
                let mut hint_spans = vec![Span::raw("  ")];
                hint_spans.extend(Self::styled_help_spans(&perm_hints));

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
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    "        This cannot be undone!",
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(
                        "  Type 'yes' to confirm: ",
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("{}{}", value, cur),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(hint_spans));

                let p = Paragraph::new(Text::from(lines)).block(
                    self.styled_block()
                        .title(" \u{26a0} Permanent Delete ")
                        .title_style(Style::default().fg(Color::Red))
                        .border_style(Style::default().fg(Color::Red)),
                );
                f.render_widget(p, area);
            }
            InputMode::CartView => {
                self.draw_cart_overlay(f);
            }
            InputMode::DownloadInput { input } => {
                self.draw_download_input_overlay(f, input, cur);
            }
            InputMode::OfflineInput { value } => {
                self.draw_offline_input_overlay(f, value, cur);
            }
            InputMode::OfflineTasksView { tasks, selected } => {
                self.draw_offline_tasks_overlay(f, tasks, *selected);
            }
            InputMode::InfoLoading => {
                self.draw_info_loading_overlay(f);
            }
            InputMode::InfoView { info } => {
                self.draw_info_overlay(f, info);
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
        }
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
        f.render_widget(Clear, area);

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
        let input_hints = vec![
            ("Tab", "complete"),
            ("Enter", "confirm"),
            ("Ctrl+B", "picker"),
            ("Esc", "cancel"),
        ];
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&input_hints));
        lines.push(Line::from(hint_spans));

        let (mc_bc, mc_tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightYellow)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(format!(" {} ", title))
                .title_style(Style::default().fg(mc_tc))
                .border_style(Style::default().fg(mc_bc)),
        );
        f.render_widget(p, area);
    }

    fn draw_picker(&self, f: &mut Frame) {
        // Outer vertical split: main area + optional help bar
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
        let main_area = outer[0];

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_area);

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

        let (pk_bc, pk_tc) = if self.is_vibrant() {
            (Color::LightYellow, Color::LightYellow)
        } else {
            (Color::Yellow, Color::Yellow)
        };
        let plist = List::new(picker_items)
            .block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(pk_tc))
                    .border_style(Style::default().fg(pk_bc)),
            )
            .highlight_style(self.highlight_style())
            .highlight_symbol("› ");
        f.render_stateful_widget(plist, chunks[1], &mut picker_state);

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
                let mut views: Vec<(&str, &str)> = Vec::new();
                if !self.config.show_preview {
                    views.push(("Space", "File info"));
                } else if !self.config.lazy_preview {
                    views.push(("Space", "Load preview"));
                }
                views.push(("p", "Text preview"));
                views.extend_from_slice(&[
                    ("l", "Toggle logs"),
                    ("D", "Downloads"),
                    ("A", "View cart"),
                    ("o", "Cloud download"),
                    ("O", "Offline tasks"),
                    (",", "Settings"),
                    ("h", "Toggle help"),
                    ("q", "Quit"),
                ]);

                vec![
                    (
                        "Navigation",
                        vec![
                            ("j / \u{2193}", "Move down"),
                            ("k / \u{2191}", "Move up"),
                            ("Enter", "Open folder"),
                            ("Bksp", "Go to parent"),
                            ("r", "Refresh"),
                        ],
                    ),
                    (
                        "File Operations",
                        vec![
                            ("c", "Copy"),
                            ("m", "Move"),
                            ("n", "Rename"),
                            ("d", "Delete"),
                            ("f", "New folder"),
                            ("s", "Star / Unstar"),
                            ("a", "Add to cart"),
                        ],
                    ),
                    ("Views & More", views),
                ]
            }
        };

        let col_count = sections.len();
        let max_rows = sections.iter().map(|(_, items)| items.len()).max().unwrap_or(0);
        let col_w = inner_w / col_count;
        let key_w: usize = 7;

        // Height — help content takes priority over ASCII art
        let min_content_h = 1 + max_rows + 2 + 2; // blank + titles + items + hint/blank + borders
        let art_lines: usize = 7; // 5 art + 2 blank lines
        let show_art = show_art && (term.height as usize) >= min_content_h + art_lines;
        let art_h: usize = if show_art { art_lines } else { 1 }; // art or just 1 blank
        let content_h = art_h + 1 + max_rows + 2; // art + titles + items + blank + hint
        let sheet_h = ((content_h + 2) as u16).min(term.height); // +2 borders

        // Center the popup
        let x = (term.width.saturating_sub(sheet_w)) / 2;
        let y = (term.height.saturating_sub(sheet_h)) / 2;
        let sheet_area = ratatui::layout::Rect::new(x, y, sheet_w, sheet_h);

        f.render_widget(Clear, sheet_area);

        let title_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let key_style = Style::default().fg(Color::Yellow);
        let desc_style = Style::default().fg(Color::White);

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

        // Section title row
        let mut title_spans = Vec::new();
        for (i, (name, _)) in sections.iter().enumerate() {
            let prefix = if i == 0 { " " } else { "" };
            let w = col_w.saturating_sub(prefix.len());
            title_spans.push(Span::styled(
                format!("{}{:<width$}", prefix, name, width = w),
                title_style,
            ));
        }
        lines.push(Line::from(title_spans));

        // Item rows — all columns side by side
        for row in 0..max_rows {
            let mut spans = Vec::new();
            for (ci, (_, items)) in sections.iter().enumerate() {
                if row < items.len() {
                    let (key, desc) = items[row];
                    let prefix = if ci == 0 { " " } else { "" };
                    let dw = col_w.saturating_sub(key_w + 1 + prefix.len());
                    spans.push(Span::styled(
                        format!("{}{:<kw$} ", prefix, key, kw = key_w),
                        key_style,
                    ));
                    spans.push(Span::styled(
                        format!("{:<dw$}", desc, dw = dw),
                        desc_style,
                    ));
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
            " Cart ({} files, {}) ",
            self.cart.len(),
            format_size(total_size)
        );

        let max_items = 12;
        let visible_items = self.cart.len().min(max_items);
        let total_lines = 2 + visible_items + 2; // padding + items + hint + padding
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1))
            .max(25)
            .min(70);
        let area = centered_rect(65, pct, f.area());
        f.render_widget(Clear, area);

        let mut lines = vec![Line::from("")];

        if self.cart.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Cart is empty. Press 'a' on files to add them.",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let cart_offset = if self.cart_selected >= max_items {
                self.cart_selected - max_items + 1
            } else {
                0
            };
            for (i, entry) in self.cart.iter().enumerate().skip(cart_offset).take(max_items) {
                let is_sel = i == self.cart_selected;
                let prefix = if is_sel { " \u{203a} " } else { "   " };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let size = format_size(entry.size);
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&entry.name, style),
                    Span::styled(format!("  {}", size), Style::default().fg(Color::DarkGray)),
                ]));
            }
            let remaining = self.cart.len().saturating_sub(cart_offset + max_items);
            if remaining > 0 {
                lines.push(Line::from(Span::styled(
                    format!("   ... and {} more", remaining),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        lines.push(Line::from(""));
        let cart_hints = vec![
            ("j/k", "nav"),
            ("x", "remove"),
            ("a", "clear"),
            ("Enter", "download"),
            ("Esc", "close"),
        ];
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&cart_hints));
        lines.push(Line::from(hint_spans));

        let (ct_bc, ct_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Yellow, Color::Yellow)
        };
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(title)
                .title_style(Style::default().fg(ct_tc))
                .border_style(Style::default().fg(ct_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Download input overlay ---

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
        f.render_widget(Clear, area);

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
        let dl_hints = vec![("Tab", "complete"), ("Enter", "confirm"), ("Esc", "cancel")];
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&dl_hints));
        lines.push(Line::from(hint_spans));

        let (dl_bc, dl_tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Yellow)
        };
        let cart_count = self.cart.len();
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(format!(" Download {} files ", cart_count))
                .title_style(Style::default().fg(dl_tc))
                .border_style(Style::default().fg(dl_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Offline input overlay ---

    fn draw_offline_input_overlay(&self, f: &mut Frame, value: &str, cur: &str) {
        let area = centered_rect(70, 25, f.area());
        f.render_widget(Clear, area);

        let hints = vec![("Enter", "submit"), ("Esc", "cancel")];
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&hints));

        let p = Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Enter URL or magnet link for cloud download:",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  URL: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}{}", value, cur),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(""),
            Line::from(hint_spans),
        ]))
        .block({
            let (ol_bc, ol_tc) = if self.is_vibrant() {
                (Color::LightCyan, Color::LightCyan)
            } else {
                (Color::Cyan, Color::Yellow)
            };
            self.styled_block()
                .title(" Offline Download ")
                .title_style(Style::default().fg(ol_tc))
                .border_style(Style::default().fg(ol_bc))
        });
        f.render_widget(p, area);
    }

    // --- Offline tasks view (full screen) ---

    fn draw_offline_tasks_overlay(
        &self,
        f: &mut Frame,
        tasks: &[crate::pikpak::OfflineTask],
        selected: usize,
    ) {
        let visible = tasks.len().min(15);
        let total_lines = 2 + visible.max(1) + 2; // padding + items + hint + padding
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1))
            .max(25)
            .min(75);
        let area = centered_rect(75, pct, f.area());
        f.render_widget(Clear, area);

        let title = format!(" Offline Tasks ({}) ", tasks.len());

        let (ot_bc, ot_tc) = if self.is_vibrant() {
            (Color::LightBlue, Color::LightBlue)
        } else {
            (Color::Cyan, Color::Green)
        };

        if tasks.is_empty() {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No offline tasks. Press 'o' to add a URL.",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            lines.push(Line::from(""));
            let hints = self.help_pairs();
            let mut hint_spans = vec![Span::raw("  ")];
            hint_spans.extend(Self::styled_help_spans(&hints));
            lines.push(Line::from(hint_spans));

            let p = Paragraph::new(Text::from(lines)).block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(ot_tc))
                    .border_style(Style::default().fg(ot_bc)),
            );
            f.render_widget(p, area);
        } else {
            let mut lines = vec![Line::from("")];

            let max_visible = 15;
            let task_offset = if selected >= max_visible {
                selected - max_visible + 1
            } else {
                0
            };
            for (i, task) in tasks.iter().enumerate().skip(task_offset).take(max_visible) {
                let is_sel = i == selected;
                let prefix = if is_sel { " \u{203a} " } else { "   " };

                let (icon, color) = match task.phase.as_str() {
                    "PHASE_TYPE_COMPLETE" => ("\u{2713}", Color::Green),
                    "PHASE_TYPE_RUNNING" => ("\u{2193}", Color::Cyan),
                    "PHASE_TYPE_PENDING" => ("\u{2026}", Color::DarkGray),
                    "PHASE_TYPE_ERROR" => ("\u{2717}", Color::Red),
                    _ => ("?", Color::White),
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
                    Style::default().fg(Color::White)
                };

                let mut spans = vec![
                    Span::styled(prefix, name_style),
                    Span::styled(format!("{} ", icon), Style::default().fg(color)),
                    Span::styled(truncate_name(&task.name, 35), name_style),
                    Span::styled(
                        format!("  {:>3}%", task.progress),
                        Style::default().fg(Color::White),
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

            let remaining = tasks.len().saturating_sub(task_offset + max_visible);
            if remaining > 0 {
                lines.push(Line::from(Span::styled(
                    format!("   ... and {} more", remaining),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            lines.push(Line::from(""));
            let hints = self.help_pairs();
            let mut hint_spans = vec![Span::raw("  ")];
            hint_spans.extend(Self::styled_help_spans(&hints));
            lines.push(Line::from(hint_spans));

            let p = Paragraph::new(Text::from(lines)).block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(ot_tc))
                    .border_style(Style::default().fg(ot_bc)),
            );
            f.render_widget(p, area);
        }
    }

    // --- Info loading overlay (show_preview=false) ---

    fn draw_info_loading_overlay(&self, f: &mut Frame) {
        let area = centered_rect(45, 20, f.area());
        f.render_widget(Clear, area);

        let spinner = SPINNER_FRAMES[self.spinner_idx];
        let (in_bc, in_tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::Cyan)
        };

        let p = Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {} Loading...", spinner),
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
                .title(" \u{2139} Info ")
                .title_style(Style::default().fg(in_tc).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(in_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Info overlay (show_preview=false) ---

    fn draw_info_overlay(&self, f: &mut Frame, info: &crate::pikpak::FileInfoResponse) {
        let area = centered_rect(65, 40, f.area());
        f.render_widget(Clear, area);

        let mut lines = vec![Line::from("")];

        lines.push(Line::from(vec![
            Span::styled("  Name:  ", Style::default().fg(Color::Cyan)),
            Span::styled(&info.name, Style::default().fg(Color::White)),
        ]));

        if let Some(size) = &info.size {
            let size_n: u64 = size.parse().unwrap_or(0);
            lines.push(Line::from(vec![
                Span::styled("  Size:  ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{} ({})", format_size(size_n), size),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        if let Some(hash) = &info.hash {
            lines.push(Line::from(vec![
                Span::styled("  Hash:  ", Style::default().fg(Color::Cyan)),
                Span::styled(hash.as_str(), Style::default().fg(Color::DarkGray)),
            ]));
        }

        if let Some(link) = &info.web_content_link {
            let display = if link.len() > 60 {
                format!("{}...", &link[..60])
            } else {
                link.clone()
            };
            lines.push(Line::from(vec![
                Span::styled("  Link:  ", Style::default().fg(Color::Cyan)),
                Span::styled(display, Style::default().fg(Color::Blue)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )));

        let (in_bc, in_tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::Cyan)
        };
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(format!(
                    " \u{2139} Info: {} ",
                    truncate_name(&info.name, 30)
                ))
                .title_style(
                    Style::default()
                        .fg(in_tc)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .border_style(Style::default().fg(in_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Text preview overlay (show_preview=false) ---

    fn draw_text_preview_overlay(
        &self,
        f: &mut Frame,
        name: &str,
        highlighted: &[Line],
        truncated: bool,
    ) {
        let area = centered_rect(60, 70, f.area());
        f.render_widget(Clear, area);

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

        let (in_bc, in_tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::Cyan)
        };
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(format!(" {} ", truncate_name(name, 40)))
                .title_style(Style::default().fg(in_tc).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(in_bc)),
        );
        f.render_widget(p, area);
    }

    // --- Folder listing popup (show_preview=false) ---

    fn draw_info_folder_overlay(&self, f: &mut Frame, name: &str, entries: &[Entry]) {
        let visible = entries.len().min(20);
        let total_lines = 2 + visible + 2; // padding + items + hint + padding
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1))
            .max(25)
            .min(70);
        let area = centered_rect(60, pct, f.area());
        f.render_widget(Clear, area);

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

        let (in_bc, in_tc) = if self.is_vibrant() {
            (Color::LightCyan, Color::LightCyan)
        } else {
            (Color::Cyan, Color::Cyan)
        };
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
        f.render_widget(Clear, area);

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
                        if draft.cli_nerd_font { "[✓]" } else { "[ ]" }.to_string(),
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
                    Style::default().fg(Color::White)
                };

                let value_style = if is_selected && editing {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Green)
                };

                let mut name_value_spans = vec![
                    Span::styled(prefix, name_style),
                    Span::styled(name.clone(), name_style),
                ];

                // Right-align value with padding
                let terminal_width = (f.area().width * 70 / 100).saturating_sub(4) as usize;
                let name_len = prefix.len() + name.len();
                let value_len = value.len();
                let padding = terminal_width.saturating_sub(name_len + value_len + 1);

                name_value_spans.push(Span::raw(" ".repeat(padding)));
                name_value_spans.push(Span::styled(value.clone(), value_style));

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
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&hints));
        lines.push(Line::from(hint_spans));

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

        let title = if modified {
            " Settings * "
        } else {
            " Settings "
        };

        let p = Paragraph::new(Text::from(visible_lines)).block(
            self.styled_block()
                .title(title)
                .title_style(Style::default().fg(st_tc))
                .border_style(Style::default().fg(st_bc)),
        );
        f.render_widget(p, area);
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
        f.render_widget(Clear, area);

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
                Style::default().fg(Color::White)
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

        // Help bar
        let hints = if editing_rgb {
            vec![
                ("0-9", "input"),
                ("Enter", "confirm"),
                ("Esc", "cancel"),
            ]
        } else {
            vec![
                ("j/k", "nav"),
                ("r/g/b", "edit RGB"),
                ("s", "save"),
                ("Esc", "back"),
            ]
        };
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&hints));
        lines.push(Line::from(hint_spans));

        let (st_bc, st_tc) = if self.is_vibrant() {
            (Color::LightMagenta, Color::LightMagenta)
        } else {
            (Color::Cyan, Color::Yellow)
        };

        let title = if modified {
            " Custom Colors * "
        } else {
            " Custom Colors "
        };

        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(title)
                .title_style(Style::default().fg(st_tc))
                .border_style(Style::default().fg(st_bc)),
        );
        f.render_widget(p, area);
    }
}

fn truncate_name(name: &str, max_len: usize) -> String {
    let char_count: usize = name.chars().count();
    if char_count <= max_len {
        name.to_string()
    } else {
        let truncated: String = name.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
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
