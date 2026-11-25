use super::{App, InputMode, OpResult, handle_text_input, operations::WriteGuard, text_input_view};
use crate::cmd::share::terminal_safe_text;
use crate::pikpak::{PikPak, ShareEntry, ShareLocation, SharePreview};
use anyhow::{Result, anyhow, ensure};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Wrap},
};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
};

enum Stage {
    Url,
    PassCode,
    Browse,
    Preview(SharePreview, Option<Arc<image::DynamicImage>>),
    Destination,
    SaveReceipt(String),
}

enum Reply {
    Folder {
        share: String,
        token: String,
        path: Vec<(String, String)>,
        files: Vec<ShareEntry>,
    },
    Preview(SharePreview, Option<Arc<image::DynamicImage>>),
    Saved(String),
    SaveFailed {
        detail: String,
        accepted: bool,
        completed: Vec<String>,
    },
}

pub(super) struct IncomingShare {
    stage: Stage,
    url: String,
    pass_code: String,
    share: String,
    token: String,
    path: Vec<(String, String)>,
    entries: Vec<ShareEntry>,
    cursor: usize,
    marked: HashSet<String>,
    destination: String,
    notice: String,
    pending: Option<Receiver<Result<Reply>>>,
}

impl App {
    pub(super) fn open_incoming_share(&mut self, url: &str) {
        let mut share = IncomingShare::new();
        share.url = url.to_string();
        self.text_cursor = usize::MAX;
        self.input = InputMode::IncomingShare(Box::new(share));
    }
}

impl IncomingShare {
    pub(super) fn new() -> Self {
        Self {
            stage: Stage::Url,
            url: String::new(),
            pass_code: String::new(),
            share: String::new(),
            token: String::new(),
            path: vec![],
            entries: vec![],
            cursor: 0,
            marked: HashSet::new(),
            destination: String::new(),
            notice: String::new(),
            pending: None,
        }
    }

    fn request(&mut self, hint: &str, work: impl FnOnce() -> Result<Reply> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.notice = hint.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
    }

    fn listing(
        client: &PikPak,
        share: String,
        token: String,
        path: Vec<(String, String)>,
    ) -> Result<Reply> {
        let parent = path.last().map_or("", |(id, _)| id);
        let files = client.share_detail(&share, parent, &token)?;
        let mut ids = HashSet::new();
        ensure!(
            files.iter().all(|file| ids.insert(&file.id)),
            "share folder contains duplicate IDs"
        );
        Ok(Reply::Folder {
            share,
            token,
            path,
            files,
        })
    }

    fn navigate(&mut self, client: Arc<PikPak>, path: Vec<(String, String)>) {
        let share = self.share.clone();
        let token = self.token.clone();
        self.request("Loading folder…", move || {
            Self::listing(&client, share, token, path)
        });
    }

    fn selected_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(i, file)| {
                if self.marked.is_empty() {
                    *i == self.cursor
                } else {
                    self.marked.contains(&file.id)
                }
            })
            .map(|(_, file)| file.id.clone())
            .collect()
    }

    pub(super) fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        let result = match rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(anyhow!(
                "share worker disconnected; check the drive before retrying a save"
            )),
            Ok(result) => result,
        };
        self.pending = None;
        self.notice.clear();
        match result {
            Ok(Reply::Folder {
                share,
                token,
                path,
                files,
            }) => {
                self.share = share;
                self.token = token;
                self.path = path;
                self.entries = files;
                self.cursor = 0;
                self.marked.clear();
                self.stage = Stage::Browse;
            }
            Ok(Reply::Preview(preview, thumbnail)) => {
                self.stage = Stage::Preview(preview, thumbnail)
            }
            Ok(Reply::Saved(msg)) => {
                self.marked.clear();
                self.notice = msg;
                self.stage = Stage::Browse;
            }
            Ok(Reply::SaveFailed {
                detail,
                accepted,
                completed,
            }) => {
                for id in &completed {
                    self.marked.remove(id);
                }
                if accepted || !completed.is_empty() {
                    self.stage = Stage::SaveReceipt(detail);
                } else {
                    self.stage = Stage::Destination;
                    self.notice = detail;
                }
            }
            Err(error) => {
                self.notice = terminal_safe_text(&format!("{error:#}"));
            }
        }
    }

    pub(super) fn key(&mut self, app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if self.pending.is_some() {
            return code != KeyCode::Esc;
        }
        match &self.stage {
            Stage::Url | Stage::PassCode => {
                if code == KeyCode::Tab {
                    self.stage = if matches!(self.stage, Stage::Url) {
                        Stage::PassCode
                    } else {
                        Stage::Url
                    };
                    app.text_cursor = usize::MAX;
                    return true;
                }
                let field = if matches!(self.stage, Stage::Url) {
                    &mut self.url
                } else {
                    &mut self.pass_code
                };
                match handle_text_input(field, &mut app.text_cursor, code, modifiers) {
                    Some(false) => return false,
                    Some(true) => {
                        let loc = match ShareLocation::parse(&self.url) {
                            Ok(loc) => loc,
                            Err(e) => {
                                self.notice = e.to_string();
                                return true;
                            }
                        };
                        let code = if self.pass_code.is_empty() {
                            loc.pass_code.clone().unwrap_or_default()
                        } else {
                            self.pass_code.clone()
                        };
                        let client = Arc::clone(&app.client);
                        self.request("Opening share…", move || {
                            let token = client.share_info(&loc.id, &code)?.pass_code_token;
                            let path = loc.breadcrumbs(&client, &token)?;
                            Self::listing(&client, loc.id, token, path)
                        });
                    }
                    None => {}
                }
            }
            Stage::Preview(..) | Stage::SaveReceipt(_) => {
                if matches!(code, KeyCode::Esc | KeyCode::Backspace | KeyCode::Enter) {
                    self.stage = Stage::Browse;
                }
            }
            Stage::Destination => {
                match handle_text_input(
                    &mut self.destination,
                    &mut app.text_cursor,
                    code,
                    modifiers,
                ) {
                    Some(false) => {
                        self.stage = Stage::Browse;
                        self.notice.clear();
                    }
                    Some(true) => {
                        if !self.destination.starts_with('/') {
                            self.notice = "Enter an absolute drive folder path".into();
                            return true;
                        }
                        let dest = self.destination.clone();
                        let ids = self.selected_ids();
                        let ancestors: Vec<_> =
                            self.path.iter().map(|(id, _)| id.clone()).collect();
                        let share = self.share.clone();
                        let token = self.token.clone();
                        let client = Arc::clone(&app.client);
                        let tx = app.result_tx.clone();
                        let guard = WriteGuard::new(&app.background_writes);
                        self.stage = Stage::Browse;
                        self.request("Saving selected items…", move || {
                            let _guard = guard;
                            let result: Result<String> = (|| {
                                let target = client.resolve_folder(&dest)?;
                                let refs: Vec<_> = ids.iter().map(String::as_str).collect();
                                let ancestor_refs: Vec<_> =
                                    ancestors.iter().map(String::as_str).collect();
                                client.save_share_from(
                                    &share,
                                    &token,
                                    &refs,
                                    &target,
                                    &ancestor_refs,
                                )?;
                                Ok(format!(
                                    "Saved {} item(s) to {}",
                                    ids.len(),
                                    terminal_safe_text(&dest)
                                ))
                            })();
                            let view = match &result {
                                Ok(msg) => Reply::Saved(msg.clone()),
                                Err(error) => {
                                    let progress = crate::pikpak::mutation_progress(error);
                                    Reply::SaveFailed {
                                        detail: terminal_safe_text(&format!("{error:#}")),
                                        accepted: progress.accepted || progress.pending,
                                        completed: ids
                                            .into_iter()
                                            .take(progress.completed_items)
                                            .collect(),
                                    }
                                }
                            };
                            let _ = tx.send(OpResult::IncomingSaved(result));
                            Ok(view)
                        });
                    }
                    None => {}
                }
            }
            Stage::Browse => match code {
                KeyCode::Esc => return false,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.cursor = (self.cursor + 1).min(self.entries.len().saturating_sub(1))
                }
                KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Home => self.cursor = 0,
                KeyCode::End => self.cursor = self.entries.len().saturating_sub(1),
                KeyCode::Backspace | KeyCode::Left if !self.path.is_empty() => {
                    let mut path = self.path.clone();
                    path.pop();
                    self.navigate(Arc::clone(&app.client), path);
                }
                KeyCode::Char(' ') => {
                    if let Some(item) = self.entries.get(self.cursor)
                        && !self.marked.remove(&item.id)
                    {
                        self.marked.insert(item.id.clone());
                    }
                }
                KeyCode::Char('s') if !self.entries.is_empty() => {
                    self.destination = app.current_path_display();
                    app.text_cursor = usize::MAX;
                    self.stage = Stage::Destination;
                    self.notice.clear();
                }
                KeyCode::Enter => {
                    if let Some(item) = self.entries.get(self.cursor) {
                        let id = item.id.clone();
                        let client = Arc::clone(&app.client);
                        if item.is_folder() {
                            let mut path = self.path.clone();
                            path.push((id, item.name.clone()));
                            self.navigate(client, path);
                        } else {
                            let share = self.share.clone();
                            let token = self.token.clone();
                            let thumbnails =
                                app.config.thumbnail_mode != crate::config::ThumbnailMode::Off;
                            self.request("Loading file preview…", move || {
                                let preview = client.share_preview(&share, &id, &token)?;
                                let thumbnail = preview
                                    .thumbnail_link
                                    .as_deref()
                                    .filter(|_| thumbnails)
                                    .and_then(|url| {
                                        super::fetch_and_render_thumbnail(url, &client).ok()
                                    })
                                    .map(Arc::new);
                                Ok(Reply::Preview(preview, thumbnail))
                            });
                        }
                    }
                }
                _ => {}
            },
        }
        true
    }

    pub(super) fn draw(&self, app: &App, frame: &mut Frame) {
        let block = Block::bordered().title(" Open share ");
        let area = block.inner(frame.area());
        frame.render_widget(block, frame.area());
        let [body, status, help] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(if self.notice.is_empty() { 0 } else { 2 }),
            Constraint::Length(2),
        ])
        .areas(area);
        let width = usize::from(body.width);
        let mut lines = Vec::new();
        let hint = match &self.stage {
            Stage::Url | Stage::PassCode => {
                let on_url = matches!(self.stage, Stage::Url);
                let url = text_input_view(
                    &self.url,
                    app.text_cursor,
                    width.saturating_sub(7),
                    on_url && app.cursor_visible,
                );
                let mask = "•".repeat(self.pass_code.chars().count());
                let cursor = self
                    .pass_code
                    .get(..app.text_cursor.min(self.pass_code.len()))
                    .map_or(mask.len(), |s| s.chars().count() * '•'.len_utf8());
                let password = text_input_view(
                    &mask,
                    cursor,
                    width.saturating_sub(12),
                    !on_url && app.cursor_visible,
                );
                lines.push(Line::from(format!("URL    {url}")));
                lines.push(Line::from(format!("Code   {password}")));
                "Tab switch field   Enter open   Esc close"
            }
            Stage::Browse => {
                let names: Vec<_> = self
                    .path
                    .iter()
                    .map(|(_, n)| terminal_safe_text(n))
                    .collect();
                lines.push(Line::from(format!(
                    "/{}   ·   {} selected",
                    names.join("/"),
                    self.marked.len()
                )));
                let height = usize::from(body.height.saturating_sub(1));
                let start = self.cursor.saturating_add(1).saturating_sub(height);
                for (i, f) in self.entries.iter().enumerate().skip(start).take(height) {
                    let mark = if self.marked.contains(&f.id) {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    let mut name = terminal_safe_text(&f.name);
                    if f.is_folder() {
                        name.push('/');
                    }
                    let style = if i == self.cursor {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    lines.push(Line::styled(format!(" {mark} {name} "), style));
                }
                if self.entries.is_empty() {
                    lines.push(Line::from("(empty)"));
                }
                "↑/↓ move   Enter open/preview   Space mark   s save   ← parent   Esc close"
            }
            Stage::Preview(preview, _) => {
                lines.extend(
                    preview
                        .lines()
                        .iter()
                        .map(|s| Line::from(terminal_safe_text(s))),
                );
                "Esc back to share"
            }
            Stage::SaveReceipt(detail) => {
                lines.push(Line::from(detail.as_str()));
                "Esc back to share"
            }
            Stage::Destination => {
                lines.push(Line::from(format!(
                    "Save {} item(s) to drive folder",
                    self.selected_ids().len()
                )));
                lines.push(Line::from(text_input_view(
                    &self.destination,
                    app.text_cursor,
                    width,
                    app.cursor_visible,
                )));
                "Enter save   Esc back"
            }
        };
        let text_area = if let Stage::Preview(preview, Some(image)) = &self.stage {
            use super::image_render::ImageMode;
            use crate::config::ThumbnailRenderMode;
            let mode = match app.config.thumbnail_mode.should_use_color() {
                ThumbnailRenderMode::Grayscale => Some(ImageMode::Grayscale),
                ThumbnailRenderMode::ColoredHalf => Some(ImageMode::Colored),
                ThumbnailRenderMode::Auto => Some(
                    app.configured_image_picker()
                        .map_or(ImageMode::Colored, ImageMode::Native),
                ),
                ThumbnailRenderMode::Off => None,
            };
            if let Some(mode) = mode {
                let [text_area, image_area] = Layout::vertical([
                    Constraint::Length((lines.len() as u16).min(body.height / 2)),
                    Constraint::Min(0),
                ])
                .areas(body);
                app.draw_prepared_image(
                    frame,
                    image_area,
                    &format!("share:{}:{}", self.share, preview.id),
                    image,
                    mode,
                    2,
                );
                text_area
            } else {
                body
            }
        } else {
            body
        };
        let content = Paragraph::new(lines);
        frame.render_widget(
            if matches!(&self.stage, Stage::SaveReceipt(_)) {
                content.wrap(Wrap { trim: false })
            } else {
                content
            },
            text_area,
        );
        frame.render_widget(
            Paragraph::new(self.notice.as_str()).wrap(Wrap { trim: false }),
            status,
        );
        frame.render_widget(Paragraph::new(hint).wrap(Wrap { trim: true }), help);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partially_completed_save_removes_confirmed_items_from_the_selection() {
        let mut view = IncomingShare::new();
        view.marked = HashSet::from(["placed".to_string(), "unknown".to_string()]);
        let (tx, rx) = mpsc::channel();
        view.pending = Some(rx);
        tx.send(Ok(Reply::SaveFailed {
            detail: "check receipt".into(),
            accepted: true,
            completed: vec!["placed".into()],
        }))
        .unwrap();
        view.poll();
        assert!(matches!(view.stage, Stage::SaveReceipt(_)));
        assert_eq!(view.marked, HashSet::from(["unknown".to_string()]));
    }
}
