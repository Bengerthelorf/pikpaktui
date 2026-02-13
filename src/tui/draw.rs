use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::config::{BorderStyle, ColorScheme};
use crate::pikpak::{Entry, EntryKind};
use crate::theme;

use super::completion::PathInput;
use super::download::TaskStatus;
use super::local_completion::LocalPathInput;
use super::{centered_rect, format_size, App, InputMode, LoginField, PreviewState, SPINNER_FRAMES};

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
    fn styled_block(&self) -> Block<'static> {
        let block = Block::default().borders(Borders::ALL);
        match self.config.border_style {
            BorderStyle::Rounded => block.border_type(BorderType::Rounded),
            BorderStyle::Thick | BorderStyle::ThickRounded => {
                block.border_type(BorderType::Thick)
            }
            BorderStyle::Double => block.border_type(BorderType::Double),
        }
    }

    fn is_vibrant(&self) -> bool {
        self.config.color_scheme == ColorScheme::Vibrant
    }

    /// File-type color respecting the selected color scheme.
    fn file_color(&self, cat: theme::FileCategory) -> Color {
        theme::color_for_scheme(cat, self.config.color_scheme)
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
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(75),
                ])
                .split(main_area);

            self.draw_parent_pane(f, chunks[0]);
            self.draw_current_pane(f, chunks[1]);

            // Log overlay on rightmost 40%
            if self.show_logs_overlay {
                let log_area = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(60),
                        Constraint::Percentage(40),
                    ])
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
            let p = Paragraph::new(Text::from(vec![]))
                .block(
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
                state.select(Some(self.parent_selected.min(self.parent_entries.len() - 1)));
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
                let items: Vec<ListItem> = children
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

                let title = if children.is_empty() {
                    " Preview (empty) ".to_string()
                } else {
                    format!(" Preview ({}) ", children.len())
                };

                let list = List::new(items)
                    .block(
                        self.styled_block()
                            .title(title)
                            .title_style(Style::default().fg(Color::DarkGray))
                            .border_style(Style::default().fg(Color::DarkGray)),
                    );
                f.render_widget(list, area);
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
                            Span::styled(
                                &entry.created_time,
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "  Press Space for details",
                        Style::default().fg(Color::DarkGray),
                    )));
                }

                let p = Paragraph::new(Text::from(lines))
                    .block(
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

    fn help_pairs(&self) -> Vec<(&str, &str)> {
        match &self.input {
            InputMode::Normal => {
                let mut pairs = vec![
                    ("j/k", "move"),
                    ("Enter", "open"),
                    ("Bksp", "back"),
                    ("r", "refresh"),
                    ("c", "copy"),
                    ("m", "move"),
                    ("n", "rename"),
                    ("d", "rm"),
                    ("f", "mkdir"),
                    ("s", "star"),
                    ("a", "cart"),
                    ("o", "offline"),
                ];
                // show_preview=false: i=info popup; show_preview=true + !lazy: i=preview
                // show_preview=true + lazy: i hidden (auto-loads)
                if !self.config.show_preview {
                    pairs.push(("Space", "info"));
                } else if !self.config.lazy_preview {
                    pairs.push(("Space", "preview"));
                }
                pairs.extend_from_slice(&[
                    ("l", "logs"),
                    ("D", "dl"),
                    ("O", "tasks"),
                    ("h", "help"),
                    ("q", "quit"),
                ]);
                pairs
            }
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => vec![
                ("j/k", "nav"),
                ("Enter", "open"),
                ("Bksp", "back"),
                ("Space", "confirm"),
                ("/", "input"),
                ("h", "help"),
                ("Esc", "cancel"),
            ],
            InputMode::MoveInput { .. } | InputMode::CopyInput { .. } => vec![
                ("Tab", "complete"),
                ("Enter", "confirm"),
                ("Ctrl+B", "picker"),
                ("Esc", "cancel"),
            ],
            InputMode::Rename { .. } | InputMode::Mkdir { .. } => vec![
                ("Enter", "confirm"),
                ("Esc", "cancel"),
            ],
            InputMode::ConfirmDelete => vec![
                ("y", "confirm"),
                ("p", "permanent"),
                ("n/Esc", "cancel"),
            ],
            InputMode::ConfirmPermanentDelete { .. } => vec![
                ("Enter", "confirm"),
                ("Esc", "cancel"),
            ],
            InputMode::CartView => vec![
                ("j/k", "nav"),
                ("x", "remove"),
                ("a", "clear all"),
                ("Enter", "download"),
                ("Esc", "close"),
            ],
            InputMode::DownloadInput { .. } => vec![
                ("Tab", "complete"),
                ("Enter", "confirm"),
                ("Esc", "cancel"),
            ],
            InputMode::DownloadView => vec![
                ("j/k", "nav"),
                ("p", "pause/resume"),
                ("x", "cancel"),
                ("r", "retry"),
                ("Esc", "back"),
            ],
            InputMode::OfflineInput { .. } => vec![
                ("Enter", "submit"),
                ("Esc", "cancel"),
            ],
            InputMode::OfflineTasksView { .. } => vec![
                ("j/k", "nav"),
                ("r", "refresh"),
                ("R", "retry"),
                ("x", "delete"),
                ("Esc", "back"),
            ],
            InputMode::InfoLoading => vec![
                ("Esc", "cancel"),
            ],
            InputMode::InfoView { .. } | InputMode::InfoFolderView { .. } => vec![
                ("any key", "close"),
            ],
            _ => vec![],
        }
    }

    fn styled_help_spans(pairs: &[(&str, &str)]) -> Vec<Span<'static>> {
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
                let del_hints = vec![
                    ("y", "trash"),
                    ("p", "permanent"),
                    ("n/Esc", "cancel"),
                ];
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
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
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

                let p = Paragraph::new(Text::from(lines))
                .block(
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
        let total_lines = base_height + if candidate_lines > 0 { candidate_lines + 1 } else { 0 };
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
            format!(
                " {} to: {} {} ",
                op, pp, SPINNER_FRAMES[self.spinner_idx]
            )
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
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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

    fn draw_help_sheet(&self, f: &mut Frame) {
        let area = f.area();

        let (left_title, left_items, right_title, right_items): (
            &str,
            Vec<(&str, &str)>,
            &str,
            Vec<(&str, &str)>,
        ) = match &self.input {
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => (
                "Navigation",
                vec![
                    ("j / \u{2193}", "Move down"),
                    ("k / \u{2191}", "Move up"),
                    ("Enter", "Open folder"),
                    ("Bksp", "Go back"),
                ],
                "Actions",
                vec![
                    ("Space", "Confirm dest"),
                    ("/", "Text input"),
                    ("Esc", "Cancel"),
                ],
            ),
            _ => (
                "Navigation",
                vec![
                    ("j / \u{2193}", "Move down"),
                    ("k / \u{2191}", "Move up"),
                    ("Enter", "Open folder"),
                    ("Bksp", "Go back"),
                ],
                "Actions",
                {
                    let mut actions = vec![
                        ("c", "Copy"),
                        ("m", "Move"),
                        ("n", "Rename"),
                        ("d", "Remove/Delete"),
                        ("f", "New folder"),
                        ("s", "Star/Unstar"),
                        ("a", "Add to cart"),
                        ("A", "View cart"),
                        ("D", "Downloads"),
                        ("o", "Offline DL"),
                        ("O", "Offline tasks"),
                    ];
                    if !self.config.show_preview {
                        actions.push(("Space", "File info"));
                    } else if !self.config.lazy_preview {
                        actions.push(("Space", "Preview"));
                    }
                    actions.extend_from_slice(&[
                        ("l", "Logs"),
                        ("r", "Refresh"),
                        ("h", "Help"),
                        ("q", "Quit"),
                    ]);
                    actions
                },
            ),
        };

        let max_rows = left_items.len().max(right_items.len());
        // title row + item rows + hint row + 2 borders
        let sheet_h = ((max_rows + 3) as u16).min(area.height);
        let sheet_w = area.width.saturating_sub(4).min(56).max(30);
        let x = (area.width.saturating_sub(sheet_w)) / 2;
        let y = area.height.saturating_sub(sheet_h);
        let sheet_area = ratatui::layout::Rect::new(x, y, sheet_w, sheet_h);

        f.render_widget(Clear, sheet_area);

        let inner_w = sheet_w.saturating_sub(2) as usize; // inside borders
        let half = inner_w / 2;

        let title_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let key_style = Style::default().fg(Color::Yellow);

        let mut lines: Vec<Line> = Vec::new();

        // Section titles row
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<w$}", left_title, w = half - 1), title_style),
            Span::styled(right_title, title_style),
        ]));

        // Item rows — left and right side by side
        for i in 0..max_rows {
            let mut spans = Vec::new();
            if i < left_items.len() {
                let (k, d) = left_items[i];
                let key_w = 8.min(half.saturating_sub(2));
                let desc_w = half.saturating_sub(key_w + 2);
                spans.push(Span::styled(
                    format!(" {:<key_w$} ", k, key_w = key_w),
                    key_style,
                ));
                spans.push(Span::raw(format!("{:<desc_w$}", d, desc_w = desc_w)));
            } else {
                spans.push(Span::raw(format!("{:<half$}", "", half = half)));
            }
            if i < right_items.len() {
                let (k, d) = right_items[i];
                let key_w = 8.min(half.saturating_sub(2));
                spans.push(Span::styled(
                    format!("{:<key_w$} ", k, key_w = key_w),
                    key_style,
                ));
                spans.push(Span::raw(d));
            }
            lines.push(Line::from(spans));
        }

        // Close hint
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
                .title_style(Style::default().fg(hp_tc))
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
            for (i, entry) in self.cart.iter().enumerate().take(max_items) {
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
                    Span::styled(
                        format!("  {}", size),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            if self.cart.len() > max_items {
                lines.push(Line::from(Span::styled(
                    format!("   ... and {} more", self.cart.len() - max_items),
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
        let total_lines = base_height + if candidate_lines > 0 { candidate_lines + 1 } else { 0 };
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

    // --- Download view (full screen) ---

    fn draw_download_view(&self, f: &mut Frame) {
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

        let ds = &self.download_state;
        let done = ds.done_count();
        let total = ds.tasks.len();
        let title = format!(" Downloads ({}/{} done) ", done, total);

        let bar_width = 20usize;

        let items: Vec<ListItem> = ds
            .tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                let is_sel = i == ds.selected;
                let prefix = if is_sel { "\u{203a} " } else { "  " };

                let (status_icon, status_color) = match &task.status {
                    TaskStatus::Pending => ("\u{2026}", Color::DarkGray), // …
                    TaskStatus::Downloading => ("\u{2193}", Color::Cyan), // ↓
                    TaskStatus::Paused => ("\u{23f8}", Color::Yellow),    // ⏸
                    TaskStatus::Done => ("\u{2713}", Color::Green),       // ✓
                    TaskStatus::Failed(_) => ("\u{2717}", Color::Red),    // ✗
                };

                let pct = if task.total_size > 0 {
                    ((task.downloaded as f64 / task.total_size as f64) * 100.0) as u64
                } else {
                    0
                };

                let filled = if task.total_size > 0 {
                    (bar_width as u64 * task.downloaded / task.total_size.max(1)) as usize
                } else {
                    0
                };
                let empty = bar_width.saturating_sub(filled);
                let bar = format!(
                    "{}{}",
                    "\u{2588}".repeat(filled),
                    "\u{2591}".repeat(empty)
                );

                let speed_str = if task.status == TaskStatus::Downloading && task.speed > 0.0 {
                    format!("  {}/s", format_size(task.speed as u64))
                } else {
                    String::new()
                };

                let name_style = if is_sel {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(prefix, name_style),
                    Span::styled(
                        format!("{} ", status_icon),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        format!("{:<30}", truncate_name(&task.name, 30)),
                        name_style,
                    ),
                    Span::styled(format!("{:>3}%  ", pct), Style::default().fg(Color::White)),
                    Span::styled(bar, Style::default().fg(status_color)),
                    Span::styled(speed_str, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let (dv_bc, dv_tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Green)
        };
        if items.is_empty() {
            let empty_msg = Paragraph::new(Text::from(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No downloads. Add files to cart with 'a', then 'A' to download.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]))
            .block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(dv_tc))
                    .border_style(Style::default().fg(dv_bc)),
            );
            f.render_widget(empty_msg, main_area);
        } else {
            let mut state = ListState::default();
            if !ds.tasks.is_empty() {
                state.select(Some(ds.selected.min(ds.tasks.len() - 1)));
            }

            let list = List::new(items)
                .block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(dv_tc))
                        .border_style(Style::default().fg(dv_bc)),
                )
                .highlight_style(Style::default())
                .highlight_symbol("");
            f.render_stateful_widget(list, main_area, &mut state);
        }

        // Help bar
        if self.config.show_help_bar {
            let pairs = self.help_pairs();
            let mut spans = vec![Span::raw(" ")];
            spans.extend(Self::styled_help_spans(&pairs));
            let bar = Paragraph::new(Line::from(spans));
            f.render_widget(bar, outer[1]);
        }

        if self.show_help_sheet {
            self.draw_help_sheet(f);
        }
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

            for (i, task) in tasks.iter().enumerate().take(15) {
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
                    Span::styled(
                        truncate_name(&task.name, 35),
                        name_style,
                    ),
                    Span::styled(
                        format!("  {:>3}%", task.progress),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("  {}", size),
                        Style::default().fg(Color::DarkGray),
                    ),
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

            if tasks.len() > 15 {
                lines.push(Line::from(Span::styled(
                    format!("   ... and {} more", tasks.len() - 15),
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
                .title(format!(" \u{2139} Info: {} ", truncate_name(&info.name, 30)))
                .title_style(Style::default().fg(in_tc).bg(Color::DarkGray).add_modifier(Modifier::BOLD))
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
        let title = format!(
            " {} ({}) ",
            truncate_name(name, 25),
            entries.len()
        );
        let p = Paragraph::new(Text::from(lines)).block(
            self.styled_block()
                .title(title)
                .title_style(
                    Style::default()
                        .fg(in_tc)
                        .add_modifier(Modifier::BOLD),
                )
                .border_style(Style::default().fg(in_bc)),
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
    let gap = |width: usize| -> Vec<Span<'static>> {
        vec![Span::styled(" ".repeat(width), bg)]
    };

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
            format!(
                "{}{}",
                " ".repeat(3),
                "\u{2588}".repeat(16)
            ),
            w,
        )),
    ]
}
