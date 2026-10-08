use crossterm::event::KeyCode;

use crate::config::PlaybackQuality;

use super::{App, InputMode, PlayOption};

#[derive(Debug, Clone)]
pub(super) struct PlaybackDialog {
    pub name: String,
    pub medias: Vec<PlayOption>,
    pub selected: usize,
    /// A separate cursor allows Escape to cancel without changing the stream.
    pub quality_cursor: Option<usize>,
}

impl PlaybackDialog {
    pub fn new(name: String, medias: Vec<PlayOption>, preference: PlaybackQuality) -> Self {
        let selected = PlayOption::preferred_index(&medias, preference.height()).unwrap_or(0);
        Self {
            name,
            medias,
            selected,
            quality_cursor: None,
        }
    }

    pub fn option(&self) -> Option<&PlayOption> {
        self.medias.get(self.selected)
    }

    pub fn preference_note(&self, preference: PlaybackQuality) -> String {
        if PlayOption::preferred_index(&self.medias, preference.height())
            .is_some_and(|index| index != self.selected)
        {
            return format!("This playback only; saved default {}", preference.as_str());
        }
        let matched = self.option().is_some_and(|o| {
            o.available
                && match preference.height() {
                    Some(height) => o.height == Some(height),
                    None => o.is_original,
                }
        });
        if matched {
            String::new()
        } else {
            format!(
                "Preferred {}; using available selection",
                preference.as_str()
            )
        }
    }
}

pub(super) fn move_selection(medias: &[PlayOption], selected: usize, up: bool) -> usize {
    if up {
        (0..selected.min(medias.len()))
            .rev()
            .find(|&i| medias[i].available)
    } else {
        (selected.saturating_add(1)..medias.len()).find(|&i| medias[i].available)
    }
    .unwrap_or(selected)
}

impl App {
    pub(super) fn launch_play_option(&mut self, option: &PlayOption) -> bool {
        if !option.available || option.url.is_empty() {
            self.push_log(
                option
                    .unavailable_reason
                    .unwrap_or("Stream unavailable")
                    .into(),
            );
            return false;
        }
        if let Some(player) = self.config.player.clone() {
            self.spawn_player(&player, &option.url);
        } else {
            self.input = InputMode::PlayerInput {
                value: String::new(),
                pending_url: option.url.clone(),
            };
        }
        true
    }

    pub(super) fn handle_play_dialog_key(&mut self, mut dialog: PlaybackDialog, code: KeyCode) {
        if let Some(cursor) = dialog.quality_cursor {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    dialog.quality_cursor = Some(move_selection(&dialog.medias, cursor, true));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    dialog.quality_cursor = Some(move_selection(&dialog.medias, cursor, false));
                }
                KeyCode::Enter => {
                    if dialog.medias.get(cursor).is_some_and(|o| o.available) {
                        dialog.selected = cursor;
                        dialog.quality_cursor = None;
                    } else if let Some(option) = dialog.medias.get(cursor) {
                        self.push_log(
                            option
                                .unavailable_reason
                                .unwrap_or("Stream unavailable")
                                .into(),
                        );
                    }
                }
                KeyCode::Esc | KeyCode::Char('n' | 'q' | ' ') | KeyCode::Tab => {
                    dialog.quality_cursor = None;
                }
                _ => {}
            }
        } else {
            match code {
                KeyCode::Enter | KeyCode::Char('y') => {
                    if let Some(option) = dialog.option()
                        && self.launch_play_option(option)
                    {
                        return;
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') => return,
                KeyCode::Char('q' | ' ') | KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                    dialog.quality_cursor = Some(dialog.selected);
                }
                _ => {}
            }
        }
        self.input = InputMode::ConfirmPlay { dialog };
    }

    pub(super) fn handle_play_dialog_click(&mut self, col: u16, row: u16) {
        let InputMode::ConfirmPlay { mut dialog } =
            std::mem::replace(&mut self.input, InputMode::Normal)
        else {
            return;
        };
        if self.play_quality_area.get().contains((col, row).into()) {
            dialog.quality_cursor = if dialog.quality_cursor.is_some() {
                None
            } else {
                Some(dialog.selected)
            };
        } else if dialog.quality_cursor.is_some()
            && self.mouse_list_area.get().contains((col, row).into())
            && row >= self.mouse_list_first_row.get()
        {
            let relative = (row - self.mouse_list_first_row.get()) as usize;
            if relative < self.mouse_list_visible.get() {
                let index = self.mouse_list_offset.get() + relative;
                if let Some(option) = dialog.medias.get(index) {
                    if option.available {
                        dialog.selected = index;
                        dialog.quality_cursor = None;
                    } else {
                        self.push_log(
                            option
                                .unavailable_reason
                                .unwrap_or("Stream unavailable")
                                .into(),
                        );
                    }
                }
            }
        }
        self.input = InputMode::ConfirmPlay { dialog };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::TuiConfig,
        pikpak::{FileInfoResponse, PikPak},
    };
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::{Terminal, backend::TestBackend};

    fn options() -> Vec<PlayOption> {
        serde_json::from_value::<FileInfoResponse>(serde_json::json!({
            "name":"sample-video.mp4",
            "medias":[
                {"is_origin":true,"video":{"height":2160},"link":{"url":"https://example.invalid/original"}},
                {"media_name":"1080p","need_more_quota":true,"link":{"url":"https://example.invalid/restricted"}},
                {"media_name":"720p","link":{"url":"https://example.invalid/720"}},
                {"media_name":"480p"},
                {"media_name":"360p","link":{"url":"https://example.invalid/360"}}
            ]
        })).unwrap().play_options()
    }

    fn app() -> App {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        app.input = InputMode::ConfirmPlay {
            dialog: PlaybackDialog::new(
                "sample-video.mp4".into(),
                options(),
                PlaybackQuality::Original,
            ),
        };
        app
    }

    fn key(app: &mut App, code: KeyCode) {
        app.handle_key(code, KeyModifiers::NONE).unwrap();
    }

    fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
        app.handle_mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn dialog(app: &App) -> &PlaybackDialog {
        let InputMode::ConfirmPlay { dialog } = &app.input else {
            panic!("confirmation closed")
        };
        dialog
    }

    #[test]
    fn selecting_quality_requires_separate_play_confirmation() {
        let mut app = app();
        key(&mut app, KeyCode::Char('q'));
        key(&mut app, KeyCode::Down);
        assert_eq!(dialog(&app).quality_cursor, Some(2));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(dialog(&app).selected, 0);
        key(&mut app, KeyCode::Enter);
        assert_eq!(dialog(&app).selected, 2);
        assert!(dialog(&app).quality_cursor.is_none());
        assert!(
            dialog(&app)
                .preference_note(app.config.playback_quality)
                .contains("This playback only")
        );
        key(&mut app, KeyCode::Enter);
        assert!(
            matches!(&app.input, InputMode::PlayerInput { pending_url, .. } if pending_url == "https://example.invalid/720")
        );
        assert_eq!(app.config.playback_quality, PlaybackQuality::Original);
    }

    #[test]
    fn escape_cancels_dropdown_then_closes_confirmation() {
        let mut app = app();
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Char('j'));
        key(&mut app, KeyCode::Esc);
        assert_eq!(dialog(&app).selected, 0);
        assert!(dialog(&app).quality_cursor.is_none());
        key(&mut app, KeyCode::Esc);
        assert!(matches!(app.input, InputMode::Normal));
    }

    #[test]
    fn unavailable_stream_cannot_be_committed_or_launched() {
        let mut app = app();
        if let InputMode::ConfirmPlay { dialog } = &mut app.input {
            dialog.quality_cursor = Some(1);
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(dialog(&app).selected, 0);
        assert_eq!(dialog(&app).quality_cursor, Some(1));
        if let InputMode::ConfirmPlay { dialog } = &mut app.input {
            for option in &mut dialog.medias {
                option.available = false;
            }
            dialog.quality_cursor = None;
        }
        key(&mut app, KeyCode::Char('y'));
        assert!(matches!(app.input, InputMode::ConfirmPlay { .. }));
    }

    #[test]
    fn quality_mouse_field_and_options_never_autoplay() {
        let mut app = app();
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let field = app.play_quality_area.get();
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            field.x,
            field.y,
        );
        assert_eq!(dialog(&app).quality_cursor, Some(0));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let list = app.mouse_list_area.get();
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            list.x + 1,
            list.y + 1,
        );
        assert_eq!(dialog(&app).selected, 0);
        assert!(dialog(&app).quality_cursor.is_some());
        mouse(&mut app, MouseEventKind::ScrollDown, list.x, list.y);
        assert_eq!(dialog(&app).quality_cursor, Some(2));
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            list.x + 1,
            list.y + 2,
        );
        assert_eq!(dialog(&app).selected, 2);
        assert!(dialog(&app).quality_cursor.is_none());
        // A second click at the same location does not double-click through to playback.
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            list.x + 1,
            list.y + 2,
        );
        assert!(matches!(app.input, InputMode::ConfirmPlay { .. }));
    }

    #[test]
    fn long_quality_lists_scroll_with_correct_click_mapping_and_clip_small_terminals() {
        let mut app = app();
        if let InputMode::ConfirmPlay { dialog } = &mut app.input {
            let mut option = dialog.medias[2].clone();
            for index in 5..20 {
                option.label = format!("Variant {index}");
                dialog.medias.push(option.clone());
            }
            dialog.quality_cursor = Some(19);
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let list = app.mouse_list_area.get();
        assert!(app.mouse_list_offset.get() > 0);
        let row = list.y + (19 - app.mouse_list_offset.get()) as u16;
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .chunks(80)
                .nth(row as usize)
                .unwrap()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("Variant 19")
        );
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            list.x,
            row,
        );
        assert_eq!(dialog(&app).selected, 19);
        for (width, height) in [(1, 1), (12, 5), (40, 8)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert_eq!(app.play_quality_area.get().height, 0);
            assert_eq!(app.mouse_list_visible.get(), 0);
        }
    }

    #[test]
    fn saved_preference_seeds_both_playback_entry_points_and_reports_fallback() {
        let mut app = app();
        app.config.playback_quality = PlaybackQuality::P1080;
        let info: FileInfoResponse = serde_json::from_value(serde_json::json!({
            "name":"sample-video.mp4",
            "medias":[
                {"is_origin":true,"link":{"url":"https://example.invalid/original"}},
                {"media_name":"720p","link":{"url":"https://example.invalid/720"}}
            ]
        }))
        .unwrap();
        app.input = InputMode::Normal;
        let request = app.begin_modal_request(super::super::AsyncRequestKind::Play, "fixture");
        app.result_tx
            .send(super::super::OpResult::PlayInfo(request, Ok(info.clone())))
            .unwrap();
        app.poll_results();
        assert_eq!(dialog(&app).selected, 1);
        assert!(
            dialog(&app)
                .preference_note(app.config.playback_quality)
                .contains("1080p")
        );
        app.input = InputMode::Normal;
        let request =
            app.begin_modal_request(super::super::AsyncRequestKind::PlayPicker, "fixture");
        let options = info.play_options();
        app.result_tx
            .send(super::super::OpResult::PlayPickerInfo(
                request,
                Ok((info, options)),
            ))
            .unwrap();
        app.poll_results();
        assert!(matches!(
            app.input,
            InputMode::PlayPicker { selected: 1, .. }
        ));
    }

    #[test]
    fn quality_setting_is_correctly_indexed_and_applies_after_save() {
        let mut app = app();
        let mut draft = TuiConfig::default();
        draft.playback_quality = PlaybackQuality::P1080;
        let items = App::settings_items(&draft);
        let row = items.iter().flat_map(|(_, rows)| rows).nth(15).unwrap();
        assert_eq!(row.0, "Default Playback Quality");
        assert_eq!(row.2, "1080p");
        app.input = InputMode::Settings {
            selected: 15,
            editing: false,
            draft,
            modified: false,
        };
        for code in [KeyCode::Enter, KeyCode::Right, KeyCode::Enter] {
            key(&mut app, code);
        }
        let InputMode::Settings {
            draft,
            modified: true,
            ..
        } = &app.input
        else {
            panic!("settings lost")
        };
        assert_eq!(draft.playback_quality, PlaybackQuality::P720);
        assert_eq!(app.config.playback_quality, PlaybackQuality::Original);
        assert!(!app.apply_settings(draft.clone()));
        assert_eq!(app.config.playback_quality, PlaybackQuality::P720);
    }

    #[test]
    fn export_playback_dialogs_for_visual_review() {
        let Ok(path) = std::env::var("PIKPAKTUI_PLAYBACK_SNAPSHOT") else {
            return;
        };
        let mut app = app();
        app.config.player = Some("open -a IINA".into());
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        let mut frames = Vec::new();
        for expanded in [false, true] {
            if let InputMode::ConfirmPlay { dialog } = &mut app.input {
                dialog.quality_cursor = expanded.then_some(2);
            }
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            frames.push(serde_json::json!({"expanded":expanded,"width":100,"height":32,"cells":buffer.content.iter().map(|cell|serde_json::json!({"s":cell.symbol(),"fg":format!("{:?}",cell.fg),"bg":format!("{:?}",cell.bg)})).collect::<Vec<_>>() }));
        }
        std::fs::write(path, serde_json::to_vec(&frames).unwrap()).unwrap();
    }
}
