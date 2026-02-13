use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use std::collections::VecDeque;

use super::download::TaskStatus;
use super::{App, format_size, centered_rect, SPINNER_FRAMES};

/// Download view mode: collapsed (centered popup) or expanded (full screen)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DownloadViewMode {
    Collapsed,  // Cart-like centered view with summary
    Expanded,   // Full-screen detailed view
}

pub struct NetworkStats {
    pub speed_history: VecDeque<f64>, // Last N data points (MB/s)
    pub max_history_points: usize,
}

impl NetworkStats {
    pub fn new() -> Self {
        Self {
            speed_history: VecDeque::new(),
            max_history_points: 60, // 30 seconds of history at 0.5s interval
        }
    }

    pub fn update(&mut self, current_speed: f64) {
        self.speed_history.push_back(current_speed);
        if self.speed_history.len() > self.max_history_points {
            self.speed_history.pop_front();
        }
    }

    pub fn max_speed(&self) -> f64 {
        self.speed_history
            .iter()
            .copied()
            .fold(0.0, f64::max)
    }

    #[allow(dead_code)]
    pub fn avg_speed(&self) -> f64 {
        if self.speed_history.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.speed_history.iter().sum();
        sum / self.speed_history.len() as f64
    }
}

impl App {
    /// Draw download view based on mode
    pub(super) fn draw_download_view(&self, f: &mut Frame) {
        match self.download_view_mode {
            DownloadViewMode::Collapsed => self.draw_download_collapsed(f),
            DownloadViewMode::Expanded => self.draw_download_expanded(f),
        }
    }

    /// Collapsed view: Cart-like centered popup with summary
    fn draw_download_collapsed(&self, f: &mut Frame) {
        let bg = Block::default().style(Style::default().bg(Color::Black));
        f.render_widget(bg, f.area());

        let ds = &self.download_state;
        let done = ds.done_count();
        let total = ds.tasks.len();

        // Calculate overall stats
        let mut total_downloaded: u64 = 0;
        let mut total_size: u64 = 0;
        let mut current_speed: f64 = 0.0;
        let mut active_count = 0;

        for task in &ds.tasks {
            total_downloaded += task.downloaded;
            total_size += task.total_size;
            if task.status == TaskStatus::Downloading {
                current_speed += task.speed;
                active_count += 1;
            }
        }

        let overall_pct = if total_size > 0 {
            (total_downloaded as f64 / total_size as f64 * 100.0) as u64
        } else {
            0
        };

        // Center area size
        let area = centered_rect(70, 50, f.area());
        f.render_widget(Clear, area);

        // Build content
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Status: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{} / {} completed", done, total),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(""),
        ];

        // Overall progress bar
        let bar_width: usize = 40;
        let filled = if total_size > 0 {
            (bar_width as u64 * total_downloaded / total_size) as usize
        } else {
            0
        };
        let empty = bar_width.saturating_sub(filled);
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        lines.push(Line::from(vec![
            Span::styled("  Progress: ", Style::default().fg(Color::Cyan)),
            Span::styled(bar, Style::default().fg(Color::Green)),
            Span::styled(format!(" {}%", overall_pct), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(""));

        // Stats
        lines.push(Line::from(vec![
            Span::styled("  Downloaded: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{} / {}", format_size(total_downloaded), format_size(total_size)),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Speed: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{}/s", format_size(current_speed as u64)),
                Style::default().fg(Color::Green),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Active: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{}", active_count),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(""));

        // Active downloads preview (max 5)
        if !ds.tasks.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Active Downloads:",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            let active_tasks: Vec<_> = ds
                .tasks
                .iter()
                .filter(|t| matches!(t.status, TaskStatus::Downloading | TaskStatus::Pending))
                .take(5)
                .collect();

            if active_tasks.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    No active downloads",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for task in active_tasks {
                    let pct = if task.total_size > 0 {
                        (task.downloaded as f64 / task.total_size as f64 * 100.0) as u64
                    } else {
                        0
                    };
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            truncate_name(&task.name, 35),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!(" {}%", pct),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // Hints
        let hints = vec![
            ("Enter", "expand"),
            ("p", "pause/resume"),
            ("x", "cancel"),
            ("Esc", "close"),
        ];
        let mut hint_spans = vec![Span::raw("  ")];
        hint_spans.extend(Self::styled_help_spans(&hints));
        lines.push(Line::from(hint_spans));

        let (bc, tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Green)
        };

        let p = Paragraph::new(lines).block(
            self.styled_block()
                .title(" Downloads ")
                .title_style(Style::default().fg(tc))
                .border_style(Style::default().fg(bc)),
        );
        f.render_widget(p, area);
    }

    /// Expanded view: Full-screen with list on left, activity/details on right
    fn draw_download_expanded(&self, f: &mut Frame) {
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

        // Main layout: Left (60%) | Right (40%)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(main_area);

        // Left side layout: List (80%) | Overall Progress (20%)
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .split(chunks[0]);

        // Right side layout: Network Activity (60%) | File Details (40%)
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        // Draw sections
        self.draw_download_list(f, left_chunks[0]);
        self.draw_overall_progress(f, left_chunks[1]);
        self.draw_network_activity(f, right_chunks[0]);
        self.draw_file_details(f, right_chunks[1]);

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

    /// Draw download list (left top)
    fn draw_download_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let ds = &self.download_state;
        let done = ds.done_count();
        let total = ds.tasks.len();
        let title = if self.loading {
            format!(
                " {} Downloads ({}/{}) ",
                SPINNER_FRAMES[self.spinner_idx],
                done,
                total
            )
        } else {
            format!(" Downloads ({}/{}) ", done, total)
        };

        let items: Vec<ListItem> = ds
            .tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                let is_sel = i == ds.selected;
                let prefix = if is_sel { "› " } else { "  " };

                let (status_icon, status_color) = match &task.status {
                    TaskStatus::Pending => ("⋯", Color::DarkGray),
                    TaskStatus::Downloading => ("↓", Color::Cyan),
                    TaskStatus::Paused => ("⏸", Color::Yellow),
                    TaskStatus::Done => ("✓", Color::Green),
                    TaskStatus::Failed(_) => ("✗", Color::Red),
                };

                let pct = if task.total_size > 0 {
                    (task.downloaded as f64 / task.total_size as f64 * 100.0) as u64
                } else {
                    0
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
                    Span::styled(truncate_name(&task.name, 40), name_style),
                    Span::styled(
                        format!(" {}%", pct),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let (bc, tc) = if self.is_vibrant() {
            (Color::LightGreen, Color::LightGreen)
        } else {
            (Color::Cyan, Color::Green)
        };

        if items.is_empty() {
            let empty_msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No downloads. Add files to cart (a), then download (A).",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(
                self.styled_block()
                    .title(title)
                    .title_style(Style::default().fg(tc))
                    .border_style(Style::default().fg(bc)),
            );
            f.render_widget(empty_msg, area);
        } else {
            let mut state = ListState::default();
            if !ds.tasks.is_empty() {
                state.select(Some(ds.selected.min(ds.tasks.len() - 1)));
            }

            let list = List::new(items)
                .block(
                    self.styled_block()
                        .title(title)
                        .title_style(Style::default().fg(tc))
                        .border_style(Style::default().fg(bc)),
                )
                .highlight_style(Style::default())
                .highlight_symbol("");
            f.render_stateful_widget(list, area, &mut state);
        }
    }

    /// Draw overall progress (left bottom)
    fn draw_overall_progress(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let ds = &self.download_state;

        let mut total_downloaded: u64 = 0;
        let mut total_size: u64 = 0;
        let mut current_speed: f64 = 0.0;

        for task in &ds.tasks {
            total_downloaded += task.downloaded;
            total_size += task.total_size;
            if task.status == TaskStatus::Downloading {
                current_speed += task.speed;
            }
        }

        let overall_pct = if total_size > 0 {
            (total_downloaded as f64 / total_size as f64 * 100.0) as u64
        } else {
            0
        };

        // Progress bar
        let bar_width = area.width.saturating_sub(6) as usize;
        let filled = if total_size > 0 {
            (bar_width as u64 * total_downloaded / total_size.max(1)) as usize
        } else {
            0
        };
        let empty = bar_width.saturating_sub(filled);
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Overall Progress: ", Style::default().fg(Color::Cyan)),
                Span::styled(format!("{}%", overall_pct), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(bar, Style::default().fg(Color::Green)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Downloaded: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{} / {}", format_size(total_downloaded), format_size(total_size)),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Speed: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}/s", format_size(current_speed as u64)),
                    Style::default().fg(Color::Green),
                ),
            ]),
        ];

        // ETA calculation
        if current_speed > 0.0 && total_size > total_downloaded {
            let remaining = total_size - total_downloaded;
            let eta_secs = remaining as f64 / current_speed;
            let eta_str = format_duration(eta_secs as u64);
            lines.push(Line::from(vec![
                Span::styled("  ETA: ", Style::default().fg(Color::Cyan)),
                Span::styled(eta_str, Style::default().fg(Color::Yellow)),
            ]));
        }

        let p = Paragraph::new(lines).block(
            self.styled_block()
                .title(" Overall Progress ")
                .title_style(Style::default().fg(Color::Cyan))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(p, area);
    }

    /// Draw network activity graph (right top)
    fn draw_network_activity(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let ds = &self.download_state;

        let current_speed: f64 = ds
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Downloading)
            .map(|t| t.speed / 1_048_576.0)
            .sum();

        let history = &self.network_stats.speed_history;
        let max_speed = self.network_stats.max_speed().max(1.0);

        let content_height = area.height.saturating_sub(2) as usize;
        let content_width = area.width.saturating_sub(4) as usize;

        let stats_lines = 4;
        let graph_height = content_height.saturating_sub(stats_lines);

        let mut lines = vec![Line::from("")];

        lines.push(Line::from(vec![
            Span::styled("  Current: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{:.2} MB/s", current_speed),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Peak: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{:.2} MB/s", max_speed),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(""));

        if history.len() > 1 && graph_height > 2 {
            let points_to_show = history.len().min(content_width);
            let data: Vec<f64> = if history.len() > points_to_show {
                history
                    .iter()
                    .skip(history.len() - points_to_show)
                    .copied()
                    .collect()
            } else {
                history.iter().copied().collect()
            };

            for row in 0..graph_height {
                let row_from_bottom = graph_height - 1 - row;
                let mut line_str = "  ".to_string();

                for &value in &data {
                    let bar_height = ((value / max_speed) * graph_height as f64) as usize;

                    let ch = if bar_height > row_from_bottom {
                        "⣿"
                    } else if bar_height == row_from_bottom {
                        "⣀"
                    } else {
                        " "
                    };
                    line_str.push_str(ch);
                }

                lines.push(Line::from(Span::styled(
                    line_str,
                    Style::default().fg(Color::Cyan),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  No data yet...",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let p = Paragraph::new(lines).block(
            self.styled_block()
                .title(" Network Activity ")
                .title_style(Style::default().fg(Color::Cyan))
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(p, area);
    }

    /// Draw file details (right bottom)
    fn draw_file_details(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let ds = &self.download_state;
        let task = ds.tasks.get(ds.selected);

        let mut lines = vec![Line::from("")];

        if let Some(task) = task {
            // File name
            lines.push(Line::from(vec![
                Span::styled("  File: ", Style::default().fg(Color::Cyan)),
                Span::styled(&task.name, Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(""));

            // Status
            let (status_str, status_color) = match &task.status {
                TaskStatus::Pending => ("Pending", Color::DarkGray),
                TaskStatus::Downloading => ("Downloading", Color::Cyan),
                TaskStatus::Paused => ("Paused", Color::Yellow),
                TaskStatus::Done => ("Completed", Color::Green),
                TaskStatus::Failed(e) => {
                    lines.push(Line::from(vec![
                        Span::styled("  Status: ", Style::default().fg(Color::Cyan)),
                        Span::styled("Failed", Style::default().fg(Color::Red)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("  Error: ", Style::default().fg(Color::Red)),
                        Span::styled(
                            truncate_name(e, 40),
                            Style::default().fg(Color::Red),
                        ),
                    ]));
                    lines.push(Line::from(""));
                    ("Failed", Color::Red)
                }
            };

            if !matches!(task.status, TaskStatus::Failed(_)) {
                lines.push(Line::from(vec![
                    Span::styled("  Status: ", Style::default().fg(Color::Cyan)),
                    Span::styled(status_str, Style::default().fg(status_color)),
                ]));
                lines.push(Line::from(""));
            }

            // Size
            lines.push(Line::from(vec![
                Span::styled("  Size: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format_size(task.total_size),
                    Style::default().fg(Color::White),
                ),
            ]));

            // Downloaded
            let pct = if task.total_size > 0 {
                (task.downloaded as f64 / task.total_size as f64 * 100.0) as u64
            } else {
                0
            };
            lines.push(Line::from(vec![
                Span::styled("  Downloaded: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{} ({}%)", format_size(task.downloaded), pct),
                    Style::default().fg(Color::White),
                ),
            ]));

            // Speed
            if task.status == TaskStatus::Downloading && task.speed > 0.0 {
                lines.push(Line::from(vec![
                    Span::styled("  Speed: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{}/s", format_size(task.speed as u64)),
                        Style::default().fg(Color::Green),
                    ),
                ]));

                // ETA
                if task.total_size > task.downloaded {
                    let remaining = task.total_size - task.downloaded;
                    let eta_secs = remaining as f64 / task.speed;
                    let eta_str = format_duration(eta_secs as u64);
                    lines.push(Line::from(vec![
                        Span::styled("  ETA: ", Style::default().fg(Color::Cyan)),
                        Span::styled(eta_str, Style::default().fg(Color::Yellow)),
                    ]));
                }
            }

            // Destination
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Path: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    task.dest_path.to_string_lossy().to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "  No download selected",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let p = Paragraph::new(lines).block(
            self.styled_block()
                .title(" File Details ")
                .title_style(Style::default().fg(Color::DarkGray))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(p, area);
    }
}

fn truncate_name(name: &str, max_len: usize) -> String {
    let char_count = name.chars().count();
    if char_count <= max_len {
        name.to_string()
    } else {
        let truncated: String = name.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}
