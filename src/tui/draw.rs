use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::pikpak::EntryKind;
use crate::theme;

use super::completion::PathInput;
use super::{centered_rect, format_size, App, InputMode, LoginField, SPINNER_FRAMES};

impl App {
    pub(super) fn draw(&self, f: &mut Frame) {
        match &self.input {
            InputMode::Login { .. } => self.draw_login_screen(f),
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => self.draw_picker(f),
            _ => self.draw_main(f),
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
            lines.push(Line::from(Span::styled(
                "  Tab: switch field | Enter: login | Esc: quit",
                Style::default().fg(Color::DarkGray),
            )));

            let p = Paragraph::new(Text::from(lines))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" PikPak Login ")
                        .border_style(Style::default().fg(Color::Cyan)),
                )
                .wrap(Wrap { trim: false });
            f.render_widget(p, area);
        }
    }

    fn draw_main(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(f.area());

        // File list
        let path_display = self.current_path_display();
        let left_title = if self.loading {
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
                let c = theme::color(cat);
                let size_str = match e.kind {
                    EntryKind::Folder => String::new(),
                    EntryKind::File => format!("  {}", format_size(e.size)),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(ico, Style::default().fg(c)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&e.name, Style::default().fg(c)),
                    Span::styled(size_str, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let mut state = ListState::default();
        if !self.entries.is_empty() {
            state.select(Some(self.selected.min(self.entries.len() - 1)));
        }

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(left_title)
                    .title_style(Style::default().fg(Color::Green))
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");
        f.render_stateful_widget(list, chunks[0], &mut state);

        // Logs
        let log_lines: Vec<Line> = self
            .logs
            .iter()
            .rev()
            .take(chunks[1].height.saturating_sub(4) as usize)
            .rev()
            .map(|s| Line::from(s.as_str()))
            .collect();
        let help = self.help_text();
        let mut lines = log_lines;
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            help,
            Style::default().fg(Color::DarkGray),
        )));
        let logs = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Logs ")
                    .title_style(Style::default().fg(Color::Green))
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(logs, chunks[1]);

        self.draw_overlay(f);
    }

    fn help_text(&self) -> &str {
        match &self.input {
            InputMode::Normal => {
                "j/k:move Enter:open Bksp:back r:refresh c:copy m:move n:rename d:rm f:mkdir q:quit"
            }
            InputMode::MoveInput { .. } | InputMode::CopyInput { .. } => {
                "Tab:complete Enter:confirm Ctrl+B:picker Esc:cancel"
            }
            InputMode::Rename { .. } => "Enter:confirm Esc:cancel",
            InputMode::Mkdir { .. } => "Enter:confirm Esc:cancel",
            InputMode::ConfirmDelete => "y:confirm n/Esc:cancel",
            _ => "",
        }
    }

    fn draw_overlay(&self, f: &mut Frame) {
        let cur = if self.cursor_visible { "\u{2588}" } else { " " };

        match &self.input {
            InputMode::Normal
            | InputMode::Login { .. }
            | InputMode::MovePicker { .. }
            | InputMode::CopyPicker { .. } => {}

            InputMode::MoveInput { input, .. } => {
                self.draw_path_input_overlay(f, "Move", "Move to path", input, cur);
            }
            InputMode::CopyInput { input, .. } => {
                self.draw_path_input_overlay(f, "Copy", "Copy to path", input, cur);
            }
            InputMode::Rename { value } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
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
                    Line::from(Span::styled(
                        "  Enter: confirm | Esc: cancel",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Rename ")
                        .title_style(Style::default().fg(Color::Yellow))
                        .border_style(Style::default().fg(Color::Cyan)),
                );
                f.render_widget(p, area);
            }
            InputMode::Mkdir { value } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
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
                    Line::from(Span::styled(
                        "  Enter: confirm | Esc: cancel",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" New Folder ")
                        .title_style(Style::default().fg(Color::Yellow))
                        .border_style(Style::default().fg(Color::Cyan)),
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
                    Line::from(Span::styled(
                        "  y: confirm | n/Esc: cancel",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Confirm Remove ")
                        .title_style(Style::default().fg(Color::Red))
                        .border_style(Style::default().fg(Color::Red)),
                );
                f.render_widget(p, area);
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
        lines.push(Line::from(Span::styled(
            "  Tab: complete | Enter: confirm | Ctrl+B: picker | Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )));

        let p = Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", title))
                .title_style(Style::default().fg(Color::Yellow))
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(p, area);
    }

    fn draw_picker(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(f.area());

        // Left: source (read-only)
        let source_items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let cat = theme::categorize(e);
                let ico = theme::icon(cat, self.config.nerd_font);
                let c = theme::color(cat);
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
                Block::default()
                    .borders(Borders::ALL)
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

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(chunks[1]);

        let plist = List::new(picker_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(Style::default().fg(Color::Yellow))
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");
        f.render_stateful_widget(plist, right_chunks[0], &mut picker_state);

        let help = format!(
            " {} '{}' | j/k:nav Enter:open Bksp:back Space:confirm /:input Esc:cancel",
            op, source_entry.name
        );
        let hw = Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray))).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(hw, right_chunks[1]);
    }
}
