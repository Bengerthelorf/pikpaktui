use super::*;
use crate::tui::browser::{LoadKey, PaneHit, PaneKind, RowHit, inline_thumbnail_url};
use crate::tui::thumbnail_placeholder::draw_thumbnail_placeholder;

impl App {
    pub(super) fn draw_browser_panes(&self, f: &mut Frame, area: Rect) {
        let plan = self.browser_pane_plan(area.width);
        let count = plan.panes.len();
        let chunks = Layout::horizontal(vec![Constraint::Fill(1); count]).split(area);
        let log_area = self.show_logs_overlay.then(|| {
            if count > plan.active + 1 {
                chunks[count - 1]
            } else {
                Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
                    .split(area)[1]
            }
        });
        self.logs_overlay_area.set(log_area.unwrap_or_default());
        self.browser.hits.borrow_mut().clear();
        self.browser.visible_images.borrow_mut().clear();
        self.browser.visible_directories.borrow_mut().clear();
        self.browser.visible_files.borrow_mut().clear();
        self.browser.visible_file_images.borrow_mut().clear();
        self.browser.thumbnail_entries.borrow_mut().clear();
        self.parent_pane_area.set(Rect::default());
        self.preview_pane_area.set(Rect::default());
        self.current_pane_area.set(chunks[plan.active]);
        for (kind, pane_area) in plan.panes.into_iter().zip(chunks.iter().copied()) {
            let mut rows = Vec::new();
            match kind {
                PaneKind::Active => {
                    self.browser
                        .visible_directories
                        .borrow_mut()
                        .insert(self.current_folder_id.clone());
                    let title = format!(
                        "{}{}",
                        if self.loading {
                            format!("{} ", SPINNER_FRAMES[self.spinner_idx])
                        } else {
                            String::new()
                        },
                        self.current_path_display()
                    );
                    let status = match plan.requested {
                        Some(requested) if requested != count => {
                            format!("{count}/{requested} cols · active {}", plan.active + 1)
                        }
                        Some(_) => format!("{count} cols · active {}", plan.active + 1),
                        None => format!("auto {count} · active {}", plan.active + 1),
                    };
                    let (offset, hits) = self.draw_browser_list(
                        f,
                        pane_area,
                        &title,
                        Some(&status),
                        &self.entries,
                        Some(self.selected),
                        self.scroll_offset.get(),
                        true,
                    );
                    self.scroll_offset.set(offset);
                    self.list_area_height
                        .set(pane_area.height.saturating_sub(2));
                    rows = hits;
                }
                PaneKind::Ancestor(depth) => {
                    if depth + 1 == self.breadcrumb.len() {
                        self.parent_pane_area.set(pane_area);
                    }
                    if let Some(id) = self.ancestor_id(depth) {
                        self.browser
                            .visible_directories
                            .borrow_mut()
                            .insert(id.to_owned());
                        let title = self.ancestor_path(depth);
                        if let Some(snapshot) = self.browser.directories.get(id) {
                            let child_id = if depth + 1 == self.breadcrumb.len() {
                                &self.current_folder_id
                            } else {
                                &self.breadcrumb[depth + 1].0
                            };
                            let selected = snapshot.entries.iter().position(|e| &e.id == child_id);
                            let (offset, hits) = self.draw_browser_list(
                                f,
                                pane_area,
                                &title,
                                None,
                                &snapshot.entries,
                                selected,
                                snapshot.scroll.get(),
                                false,
                            );
                            snapshot.scroll.set(offset);
                            if depth + 1 == self.breadcrumb.len() {
                                self.parent_scroll_offset.set(offset);
                            }
                            rows = hits;
                        } else {
                            self.draw_browser_placeholder(
                                f,
                                pane_area,
                                &title,
                                self.browser
                                    .failed
                                    .contains(&LoadKey::Directory(id.to_owned())),
                            );
                        }
                    }
                }
                PaneKind::Preview(_) | PaneKind::Descendant(_) => {
                    let Some(target) = self.preview_target(kind) else {
                        continue;
                    };
                    let entry = &target.entry;
                    let primary = target.path.is_empty()
                        && self.current_entry().is_some_and(|e| e.id == entry.id);
                    if primary {
                        self.preview_pane_area.set(pane_area);
                    }
                    if entry.kind == EntryKind::Folder {
                        self.browser
                            .visible_directories
                            .borrow_mut()
                            .insert(entry.id.clone());
                        if let Some(snapshot) = self.browser.directories.get(&entry.id) {
                            let title = format!("{} ({})", entry.name, snapshot.entries.len());
                            let (offset, hits) = self.draw_browser_list(
                                f,
                                pane_area,
                                &title,
                                None,
                                &snapshot.entries,
                                Some(snapshot.selected),
                                snapshot.scroll.get(),
                                false,
                            );
                            snapshot.scroll.set(offset);
                            rows = hits;
                        } else {
                            self.draw_browser_placeholder(
                                f,
                                pane_area,
                                &entry.name,
                                self.browser
                                    .failed
                                    .contains(&LoadKey::Directory(entry.id.clone())),
                            );
                        }
                    } else {
                        self.browser
                            .visible_files
                            .borrow_mut()
                            .insert(entry.id.clone());
                        if let Some(url) = inline_thumbnail_url(entry) {
                            self.browser
                                .visible_file_images
                                .borrow_mut()
                                .insert(url.to_owned());
                            self.browser
                                .thumbnail_entries
                                .borrow_mut()
                                .insert(url.to_owned(), entry.clone());
                        }
                        let state = self.file_preview_state_for_entry(entry);
                        let scroll = if primary {
                            self.preview_scroll
                        } else {
                            self.browser
                                .preview_scrolls
                                .get(&entry.id)
                                .copied()
                                .unwrap_or(0)
                        };
                        self.draw_entry_preview(f, pane_area, Some(entry), state, scroll);
                    }
                }
                PaneKind::Empty => {
                    let empty = Paragraph::new(Span::styled(
                        "No more items",
                        Style::default().fg(Color::DarkGray),
                    ))
                    .block(
                        self.styled_block()
                            .border_style(Style::default().fg(Color::DarkGray)),
                    );
                    f.render_widget(empty, pane_area);
                }
            }
            self.browser.hits.borrow_mut().push(PaneHit {
                area: pane_area,
                kind,
                rows,
            });
        }
        if let Some(log_area) = log_area {
            self.draw_log_overlay(f, log_area);
        }
    }

    fn draw_browser_placeholder(&self, f: &mut Frame, area: Rect, title: &str, failed: bool) {
        let message = if failed {
            "Preview unavailable · r to retry".to_owned()
        } else {
            format!("{} Loading...", SPINNER_FRAMES[self.spinner_idx])
        };
        f.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: false })
                .block(
                    self.styled_block()
                        .title(format!(
                            " {} ",
                            truncate_name(title, area.width.saturating_sub(4) as usize)
                        ))
                        .border_style(Style::default().fg(Color::DarkGray)),
                ),
            area,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_browser_list(
        &self,
        f: &mut Frame,
        area: Rect,
        title: &str,
        status: Option<&str>,
        entries: &[Entry],
        selected: Option<usize>,
        offset: usize,
        active: bool,
    ) -> (usize, Vec<RowHit>) {
        let marker_width = if selected.is_some() { 2 } else { 0 };
        let visible_height = area.height.saturating_sub(2);
        let max_height = visible_height.max(1);
        let selected = selected.filter(|index| *index < entries.len());
        let height_at = |index: usize| self.browser_row_height(&entries[index]).min(max_height);
        let mut offset = offset.min(entries.len().saturating_sub(1));
        if let Some(selected) = selected {
            if selected < offset {
                offset = selected;
            } else if selected - offset >= visible_height as usize
                || (offset..=selected)
                    .map(|i| height_at(i) as usize)
                    .sum::<usize>()
                    > visible_height as usize
            {
                offset = selected;
                let mut used = height_at(selected);
                while offset > 0 && used.saturating_add(height_at(offset - 1)) <= visible_height {
                    offset -= 1;
                    used += height_at(offset);
                }
            }
        }
        let mut end = offset;
        let mut used = 0u16;
        while end < entries.len() && used.saturating_add(height_at(end)) <= visible_height {
            used += height_at(end);
            end += 1;
        }
        // Build text and thumbnail cells only for the visible window. Folder
        // previews may contain thousands of entries, even with many panes.
        let items: Vec<ListItem> = entries[offset..end]
            .iter()
            .map(|entry| {
                self.browser_list_item(
                    entry,
                    area.width.saturating_sub(2 + marker_width),
                    max_height,
                )
            })
            .collect();
        let mut state = ListState::default().with_selected(
            selected
                .filter(|i| *i >= offset && *i < end)
                .map(|i| i - offset),
        );
        let border_color = if active {
            if self.is_vibrant() {
                Color::LightBlue
            } else {
                Color::Cyan
            }
        } else {
            Color::DarkGray
        };
        let mut block = self
            .styled_block()
            .title(format!(
                " {} ",
                truncate_name(title, area.width.saturating_sub(4) as usize)
            ))
            .title_style(Style::default().fg(border_color).add_modifier(if active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }))
            .border_style(Style::default().fg(border_color));
        if let Some(status) = status {
            block = block.title_bottom(format!(
                " {} ",
                truncate_name(status, area.width.saturating_sub(4) as usize)
            ));
        }
        let inner = block.inner(area);
        let list = List::new(items)
            .block(block)
            .highlight_style(if active {
                self.highlight_style()
            } else {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            })
            .highlight_symbol(if marker_width > 0 { "› " } else { "" });
        f.render_stateful_widget(list, area, &mut state);
        let mut rows = Vec::new();
        let mut y = inner.y;
        for (index, entry) in entries.iter().enumerate().take(end).skip(offset) {
            let height = self.browser_row_height(entry).min(max_height);
            if height == 0 || height > inner.bottom().saturating_sub(y) {
                break;
            }
            rows.push(RowHit {
                top: y,
                height,
                index,
            });
            if self.has_inline_thumbnail_slot(entry) && inner.width > marker_width {
                let (width, _) = self.config.inline_thumbnail_size.dimensions();
                let thumbnail_area = Rect::new(
                    inner.x + marker_width,
                    y,
                    width.min(inner.width.saturating_sub(marker_width)),
                    height,
                );
                let url = inline_thumbnail_url(entry);
                if let Some(url) = url {
                    self.browser
                        .thumbnail_entries
                        .borrow_mut()
                        .insert(url.to_owned(), entry.clone());
                    self.browser.visible_images.borrow_mut().push((
                        url.to_owned(),
                        entry.name.clone(),
                        thumbnail_area,
                    ));
                }
                if let Some((url, cache)) = url.and_then(|url| {
                    self.browser
                        .inline_images
                        .get(url)
                        .map(|cache| (url, cache))
                }) {
                    self.draw_inline_thumbnail(f, thumbnail_area, url, &cache.value);
                } else if self.config.thumbnail_placeholders {
                    draw_thumbnail_placeholder(
                        f,
                        thumbnail_area,
                        self.thumbnail_placeholder_state(entry),
                        self.spinner_idx,
                    );
                }
            }
            y += height;
        }
        (offset, rows)
    }

    fn draw_inline_thumbnail(
        &self,
        f: &mut Frame,
        area: Rect,
        url: &str,
        image: &image::DynamicImage,
    ) {
        use crate::tui::image_render::InlineImageProtocol;
        use ratatui_image::{StatefulImage, picker::ProtocolType};

        if !self.has_overlay()
            && !self.show_help_sheet
            && !(self.show_logs_overlay && area.intersects(self.logs_overlay_area.get()))
            && let Some(picker) = self
                .configured_image_picker()
                .filter(|picker| picker.protocol_type() != ProtocolType::Halfblocks)
        {
            let cells = (area.width, area.height);
            let mut protocols = self.browser.inline_protocols.borrow_mut();
            let rebuild = protocols.get(url).is_none_or(|cached| {
                cached.cells != cells
                    || cached.font_size != picker.font_size()
                    || cached.protocol_type != picker.protocol_type()
            });
            if rebuild {
                protocols.insert(
                    url.to_owned(),
                    InlineImageProtocol {
                        cells,
                        font_size: picker.font_size(),
                        protocol_type: picker.protocol_type(),
                        protocol: picker.new_resize_protocol(upscale_for_rect(
                            image,
                            area,
                            picker.font_size(),
                        )),
                    },
                );
            }
            if let Some(cached) = protocols.get_mut(url) {
                f.render_stateful_widget(StatefulImage::default(), area, &mut cached.protocol);
                return;
            }
        }
        // Draw after List applies the selection style, so selecting a row
        // preserves thumbnail colors. Half blocks work on ordinary terminals.
        let lines = render_image_to_colored_lines(image, area.width as u32, area.height as u32);
        f.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    pub(super) fn draw_cached_preview_image(
        &self,
        f: &mut Frame,
        area: Rect,
        entry: &Entry,
        image: &image::DynamicImage,
        picker: &ratatui_image::picker::Picker,
    ) -> bool {
        use crate::tui::image_render::InlineImageProtocol;
        if self.show_help_sheet
            || (self.show_logs_overlay && area.intersects(self.logs_overlay_area.get()))
        {
            return false;
        }
        let cells = (area.width, area.height);
        let mut protocols = self.browser.preview_protocols.borrow_mut();
        if protocols.get(&entry.id).is_none_or(|cached| {
            cached.cells != cells
                || cached.font_size != picker.font_size()
                || cached.protocol_type != picker.protocol_type()
        }) {
            protocols.insert(
                entry.id.clone(),
                InlineImageProtocol {
                    cells,
                    font_size: picker.font_size(),
                    protocol_type: picker.protocol_type(),
                    protocol: picker.new_resize_protocol(upscale_for_rect(
                        image,
                        area,
                        picker.font_size(),
                    )),
                },
            );
        }
        let cached = protocols.get_mut(&entry.id).unwrap();
        f.render_stateful_widget(
            ratatui_image::StatefulImage::default(),
            area,
            &mut cached.protocol,
        );
        true
    }

    pub(super) fn has_thumbnail_slot(entry: &Entry) -> bool {
        inline_thumbnail_url(entry).is_some()
            || matches!(
                theme::categorize(entry),
                theme::FileCategory::Image | theme::FileCategory::Video
            )
    }

    fn has_inline_thumbnail_slot(&self, entry: &Entry) -> bool {
        self.config.inline_thumbnails
            && (inline_thumbnail_url(entry).is_some()
                || (self.config.thumbnail_placeholders && Self::has_thumbnail_slot(entry)))
    }

    fn browser_row_height(&self, entry: &Entry) -> u16 {
        if self.has_inline_thumbnail_slot(entry)
            && (self.config.thumbnail_placeholders
                || inline_thumbnail_url(entry).is_some_and(|url| {
                    !self
                        .browser
                        .failed
                        .contains(&LoadKey::InlineImage(url.to_owned()))
                }))
        {
            self.config.inline_thumbnail_size.dimensions().1
        } else {
            1
        }
    }

    fn browser_list_item(&self, entry: &Entry, width: u16, max_height: u16) -> ListItem<'static> {
        let category = theme::categorize(entry);
        let color = self.file_color(category);
        let icon = theme::icon(category, self.config.nerd_font);
        let thumbnail = self
            .config
            .inline_thumbnails
            .then(|| inline_thumbnail_url(entry))
            .flatten();
        let thumbnail_slot = self.has_inline_thumbnail_slot(entry);
        let thumb_width = if thumbnail_slot {
            self.config.inline_thumbnail_size.dimensions().0
        } else {
            0
        };
        let height = self.browser_row_height(entry).min(max_height);
        let has_image = thumbnail.is_some_and(|url| self.browser.inline_images.contains_key(url));
        let star = if entry.starred { "★ " } else { "" };
        let cart = if self.cart_ids.contains(&entry.id) {
            "☆ "
        } else {
            ""
        };
        let prefix_width = if thumbnail_slot {
            thumb_width as usize + 1
        } else {
            unicode_width::UnicodeWidthStr::width(icon) + 1
        };
        let marker_width = unicode_width::UnicodeWidthStr::width(star)
            + unicode_width::UnicodeWidthStr::width(cart);
        let size = if entry.kind == EntryKind::File {
            format!("  {}", format_size(entry.size))
        } else {
            String::new()
        };
        let text_width = (width as usize).saturating_sub(prefix_width + marker_width);
        let reserve_size = height == 1 && text_width > size.len() + 6;
        let name_width = text_width.saturating_sub(if reserve_size { size.len() } else { 0 });
        let names = wrap_thumbnail_name(&entry.name, name_width, height as usize);
        let size_row = if reserve_size {
            Some(0)
        } else if names.len() < height as usize && size.len() <= text_width {
            Some(names.len())
        } else {
            None
        };
        let mut lines = Vec::new();
        for row in 0..height {
            let mut spans = if thumbnail_slot {
                let mut line = if !has_image && row == 0 {
                    let placeholder =
                        if unicode_width::UnicodeWidthStr::width(icon) > thumb_width as usize {
                            "·"
                        } else {
                            icon
                        };
                    Line::from(Span::styled(placeholder, Style::default().fg(color)))
                } else {
                    Line::default()
                };
                let padding = thumb_width as usize - line.width().min(thumb_width as usize);
                line.spans.push(Span::raw(" ".repeat(padding + 1)));
                line.spans
            } else {
                vec![
                    Span::styled(icon, Style::default().fg(color)),
                    Span::raw(" "),
                ]
            };
            if row == 0 {
                spans.extend([
                    Span::styled(star, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        cart,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::DIM),
                    ),
                ]);
            } else {
                spans.push(Span::raw(" ".repeat(marker_width)));
            }
            if let Some(name) = names.get(row as usize) {
                spans.push(Span::styled(name.clone(), Style::default().fg(color)));
            }
            if size_row == Some(row as usize) {
                spans.push(Span::styled(
                    size.clone(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(spans));
        }
        ListItem::new(Text::from(lines))
    }
}

/// Thumbnail rows can use their image height for text instead of leaving it blank.
fn wrap_thumbnail_name(name: &str, width: usize, max_lines: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines = vec![String::new()];
    let mut line_width = 0;
    for (index, ch) in name.char_indices() {
        let char_width = ch.width().unwrap_or(0);
        if line_width + char_width > width {
            if lines.len() == max_lines {
                let last = lines.last_mut().unwrap();
                last.push_str(&name[index..]);
                *last = truncate_name(last, width);
                break;
            }
            lines.push(String::new());
            line_width = 0;
        }
        if char_width > width {
            lines.last_mut().unwrap().push('.');
            line_width += 1;
        } else {
            lines.last_mut().unwrap().push(ch);
            line_width += char_width;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColumnCount, InlineThumbnailSize, TuiConfig};
    use crate::pikpak::PikPak;
    use crate::tui::browser::Cached;
    use ratatui::{Terminal, backend::TestBackend};
    use std::time::Instant;

    fn entry(id: &str, kind: EntryKind, name: &str) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            kind,
            size: 1024,
            created_time: String::new(),
            modified_time: String::new(),
            starred: false,
            thumbnail_link: None,
            phase: None,
            audit: None,
        }
    }

    fn photo(id: &str) -> Entry {
        let mut entry = entry(id, EntryKind::File, &format!("{id}.jpg"));
        entry.thumbnail_link = Some(format!("https://example.invalid/{id}"));
        entry
    }

    fn fixture() -> App {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        app.input = InputMode::Normal;
        app.config.columns = ColumnCount::Fixed(4);
        app.config.show_help_bar = false;
        app.current_folder_id = "pack".into();
        app.breadcrumb = vec![(String::new(), "My Pack".into())];
        app.entries = vec![
            entry("photos", EntryKind::Folder, "Photos"),
            entry("research", EntryKind::Folder, "Research"),
            photo("beach"),
            photo("cat"),
            entry("notes", EntryKind::File, "notes.txt"),
        ];
        app.browser.cache_directory(
            String::new(),
            vec![
                entry("pack", EntryKind::Folder, "My Pack"),
                entry("shared", EntryKind::Folder, "Shared"),
            ],
        );
        app.browser.cache_directory(
            "photos".into(),
            (0..15).map(|i| photo(&format!("photo{i}"))).collect(),
        );
        app.browser.cache_directory(
            "research".into(),
            vec![
                entry("papers", EntryKind::Folder, "Papers"),
                entry("readme", EntryKind::File, "README.md"),
            ],
        );
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(24, 24, |x, y| {
            image::Rgba([(x * 8) as u8, (y * 8) as u8, 180, 255])
        }));
        for id in [
            "beach", "cat", "photo0", "photo1", "photo2", "photo3", "photo4", "photo5",
        ] {
            app.browser.inline_images.insert(
                format!("https://example.invalid/{id}"),
                Cached {
                    value: image.clone(),
                    touched: Instant::now(),
                },
            );
            app.browser.thumbnail_previews.insert(
                format!("https://example.invalid/{id}"),
                Cached {
                    value: PreviewState::ThumbnailImage {
                        image: image.clone(),
                    },
                    touched: Instant::now(),
                },
            );
        }
        app.on_cursor_move();
        app
    }

    fn render(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal
    }

    #[test]
    fn changing_selection_updates_the_selected_descendant_path() {
        let mut app = fixture();
        render(&app, 120, 16);
        let hits = app.browser.hits.borrow().clone();
        assert_eq!(hits[2].kind, PaneKind::Preview(0));
        assert_eq!(app.preview_target(hits[3].kind).unwrap().entry.id, "photo0");
        app.selected = 1;
        app.on_cursor_move();
        render(&app, 120, 16);
        let hits = app.browser.hits.borrow().clone();
        assert_eq!(hits[2].kind, PaneKind::Preview(1));
        assert_eq!(app.preview_target(hits[3].kind).unwrap().entry.id, "papers");
        app.selected = 2;
        app.on_cursor_move();
        render(&app, 120, 16);
        let hits = app.browser.hits.borrow().clone();
        assert_eq!(hits[2].kind, PaneKind::Preview(2));
        assert_eq!(
            hits[3].kind,
            PaneKind::Preview(3),
            "file previews still follow selected and subsequent siblings"
        );
    }

    fn handdraw_fixture() -> App {
        let mut app = fixture();
        app.config.columns = ColumnCount::Auto;
        app.current_folder_id = "snaix".into();
        app.breadcrumb = vec![(String::new(), "Snaix".into())];
        app.entries = vec![
            entry("an", EntryKind::Folder, "AN"),
            entry("po", EntryKind::Folder, "PO"),
        ];
        app.selected = 1;
        app.browser.cache_directory(
            String::new(),
            vec![entry("snaix", EntryKind::Folder, "Snaix")],
        );
        app.browser.cache_directory(
            "po".into(),
            vec![
                entry(
                    "alice",
                    EntryKind::Folder,
                    "Long directory name with Chinese 中文",
                ),
                entry("bob", EntryKind::Folder, "Another directory"),
            ],
        );
        let mut video = photo("clip1");
        video.name = "clip1.mp4".into();
        let mut video2 = photo("clip2");
        video2.name = "clip2.mp4".into();
        app.browser
            .cache_directory("alice".into(), vec![photo("cover"), video, video2]);
        for id in ["cover", "clip1", "clip2"] {
            let image = app.browser.inline_images["https://example.invalid/beach"]
                .value
                .clone();
            app.browser.inline_images.insert(
                format!("https://example.invalid/{id}"),
                Cached {
                    value: image.clone(),
                    touched: Instant::now(),
                },
            );
            app.browser.thumbnail_previews.insert(
                format!("https://example.invalid/{id}"),
                Cached {
                    value: PreviewState::ThumbnailImage { image },
                    touched: Instant::now(),
                },
            );
        }
        app.cache_main_directory();
        app
    }

    #[test]
    fn last_sibling_expands_descendants_and_clicking_promotes_the_full_path() {
        let mut app = handdraw_fixture();
        render(&app, 180, 20);
        let hits = app.browser.hits.borrow().clone();
        assert_eq!(hits.len(), 6);
        let sources: Vec<_> = hits
            .iter()
            .filter_map(|hit| app.preview_target(hit.kind))
            .map(|target| target.entry.id)
            .collect();
        assert_eq!(sources, ["po", "alice", "cover", "clip1"]);
        let alice = &hits[3];
        app.handle_browser_click(alice.area.x + 5, alice.rows[1].top, false);
        assert_eq!(app.current_folder_id, "alice");
        assert_eq!(app.entries[app.selected].id, "clip1");
        assert_eq!(
            app.current_path_display(),
            "/Snaix/PO/Long directory name with Chinese 中文"
        );
        render(&app, 180, 20);
        assert_eq!(app.browser.hits.borrow().len(), 6);
        app.leave_directory();
        assert_eq!(app.current_folder_id, "po");
        assert_eq!(app.entries[app.selected].id, "alice");
        app.leave_directory();
        assert_eq!(app.current_folder_id, "snaix");
        assert_eq!(app.entries[app.selected].id, "po");
    }

    #[test]
    fn auto_count_remains_stable_when_entering_long_names_and_reacts_to_resize() {
        let mut app = handdraw_fixture();
        render(&app, 180, 20);
        assert_eq!(app.browser.hits.borrow().len(), 6);
        let po = app.current_entry().unwrap().clone();
        app.enter_directory(po);
        render(&app, 180, 20);
        assert_eq!(
            app.browser.hits.borrow().len(),
            6,
            "directory names must not change an established layout"
        );
        app.config.inline_thumbnail_size = InlineThumbnailSize::Large;
        render(&app, 180, 20);
        assert_eq!(
            app.browser.hits.borrow().len(),
            6,
            "thumbnail size shortcuts must not collapse the column layout"
        );
        render(&app, 80, 20);
        assert!(app.browser.hits.borrow().len() < 6);
        assert_eq!(app.config.columns, ColumnCount::Auto);
    }

    #[test]
    fn video_thumbnail_failures_keep_names_and_row_geometry_stable() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.config.inline_thumbnail_size = InlineThumbnailSize::Large;
        let mut video = photo("beach");
        video.name = "Long video file name 中文标题 which should wrap.mp4".into();
        app.entries = vec![video];
        let terminal = render(&app, 40, 10);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 1)].symbol(), "▀");
        assert_eq!(app.browser.hits.borrow()[0].rows[0].height, 4);
        assert_eq!(buffer[(12, 1)].symbol(), "L");
        assert_ne!(
            buffer[(12, 2)].symbol(),
            " ",
            "continuation text belongs beside the image"
        );
        app.browser.inline_images.clear();
        app.browser
            .failed
            .insert(LoadKey::InlineImage("https://example.invalid/beach".into()));
        render(&app, 40, 10);
        assert_eq!(app.browser.hits.borrow()[0].rows[0].height, 4);
        app.config.thumbnail_placeholders = false;
        render(&app, 40, 10);
        assert_eq!(app.browser.hits.borrow()[0].rows[0].height, 1);
    }

    #[test]
    fn pending_loading_failed_and_missing_thumbnails_keep_hit_targets_and_text_aligned() {
        use crate::tui::thumbnail_placeholder::ThumbnailPlaceholder;
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.config.inline_thumbnail_size = InlineThumbnailSize::Large;
        app.entries = vec![photo("first"), photo("second")];
        let first = app.entries[0].clone();
        app.browser.inline_images.clear();
        app.preview_state = PreviewState::Empty;
        app.preview_target_id = None;
        let before = render(&app, 40, 12);
        let expected: Vec<_> = app.browser.hits.borrow()[0]
            .rows
            .iter()
            .map(|r| (r.top, r.height, r.index))
            .collect();
        assert_eq!(
            app.thumbnail_placeholder_state(&first),
            ThumbnailPlaceholder::Pending
        );
        assert_eq!(before.backend().buffer()[(12, 1)].symbol(), "f");
        for state in [
            ThumbnailPlaceholder::Loading,
            ThumbnailPlaceholder::Unavailable,
        ] {
            app.preview_target_id = Some(first.id.clone());
            app.preview_state = if state == ThumbnailPlaceholder::Loading {
                PreviewState::Loading
            } else {
                PreviewState::Empty
            };
            if state == ThumbnailPlaceholder::Unavailable {
                app.browser
                    .failed
                    .insert(LoadKey::InlineImage(first.thumbnail_link.clone().unwrap()));
            }
            let terminal = render(&app, 40, 12);
            assert_eq!(app.thumbnail_placeholder_state(&first), state);
            let hits: Vec<_> = app.browser.hits.borrow()[0]
                .rows
                .iter()
                .map(|r| (r.top, r.height, r.index))
                .collect();
            assert_eq!(hits, expected);
            assert_eq!(terminal.backend().buffer()[(12, 1)].symbol(), "f");
        }
        app.entries[0].thumbnail_link = None;
        let missing = render(&app, 40, 12);
        assert_eq!(app.browser.hits.borrow()[0].rows[0].height, 4);
        assert_eq!(missing.backend().buffer()[(12, 1)].symbol(), "f");
        app.handle_browser_click(14, expected[1].0 + 2, false);
        assert_eq!(app.selected, 1);
        assert!(
            !app.browser
                .visible_images
                .borrow()
                .iter()
                .any(|(url, _, _)| url.ends_with("first"))
        );
    }

    #[test]
    fn disabling_placeholder_restores_icons_without_changing_loaded_images() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.entries = vec![photo("beach")];
        let with = render(&app, 40, 10);
        app.config.thumbnail_placeholders = false;
        let without = render(&app, 40, 10);
        assert_eq!(with.backend().buffer(), without.backend().buffer());
        app.browser.inline_images.clear();
        let icon = render(&app, 40, 10);
        assert!(
            icon.backend()
                .buffer()
                .content
                .iter()
                .all(|cell| cell.bg != Color::Rgb(43, 47, 54))
        );
    }

    #[test]
    fn large_preview_metadata_stays_in_place_when_placeholder_becomes_an_image() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(2);
        app.config.thumbnail_mode = crate::config::ThumbnailMode::ForceColor;
        app.entries = vec![photo("pending")];
        app.preview_state = PreviewState::Empty;
        app.preview_target_id = None;
        let before = render(&app, 80, 20);
        let info_row = |terminal: &Terminal<TestBackend>| {
            terminal
                .backend()
                .buffer()
                .content
                .chunks(80)
                .position(|row| {
                    row.iter()
                        .map(|c| c.symbol())
                        .collect::<String>()
                        .contains("Name:")
                })
        };
        let position = info_row(&before).expect("placeholder keeps file metadata");
        assert!(
            before
                .backend()
                .buffer()
                .content
                .iter()
                .any(|c| c.bg == Color::Rgb(43, 47, 54))
        );
        app.browser.thumbnail_previews.insert(
            "https://example.invalid/pending".into(),
            Cached {
                value: PreviewState::ThumbnailImage {
                    image: image::DynamicImage::new_rgb8(16, 9),
                },
                touched: Instant::now(),
            },
        );
        let after = render(&app, 80, 20);
        assert_eq!(info_row(&after), Some(position));
        app.browser.thumbnail_previews.clear();
        app.browser.failed.insert(LoadKey::InlineImage(
            "https://example.invalid/pending".into(),
        ));
        let failed = render(&app, 80, 20);
        assert_eq!(info_row(&failed), Some(position));
    }

    #[test]
    fn selected_inline_image_keeps_rgb_colors_and_disabling_restores_text_rows() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.entries = vec![
            photo("beach"),
            entry("text", EntryKind::File, "notes.txt"),
            photo("cat"),
        ];
        app.config.inline_thumbnail_size = InlineThumbnailSize::Small;
        let terminal = render(&app, 40, 10);
        let pixel = &terminal.backend().buffer()[(3, 1)];
        assert_eq!(pixel.symbol(), "▀");
        assert!(matches!(pixel.fg, Color::Rgb(..)));
        assert!(matches!(pixel.bg, Color::Rgb(..)));
        let rows = app.browser.hits.borrow()[0].rows.clone();
        assert_eq!(
            rows.iter()
                .map(|r| (r.index, r.top, r.height))
                .collect::<Vec<_>>(),
            vec![(0, 1, 2), (1, 3, 1), (2, 4, 2)]
        );
        assert_eq!(app.browser_page_size(), 3);
        app.handle_browser_click(8, 5, false);
        assert_eq!(
            app.selected, 2,
            "second line of a thumbnail must select that image"
        );
        app.handle_browser_click(0, 1, false);
        assert_eq!(app.selected, 2, "border must not select an entry");
        app.config.inline_thumbnails = false;
        let terminal = render(&app, 40, 10);
        assert!(
            app.browser.hits.borrow()[0]
                .rows
                .iter()
                .all(|r| r.height == 1)
        );
        assert!(app.browser.visible_images.borrow().is_empty());
        assert!(
            !terminal
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol() == "▀")
        );
    }

    #[test]
    fn folder_preview_scroll_and_click_promote_the_correct_directory_and_row() {
        let mut app = fixture();
        app.config.inline_thumbnail_size = InlineThumbnailSize::Small;
        render(&app, 120, 10);
        app.handle_browser_scroll(65, 5, false);
        render(&app, 120, 10);
        let hit = app.browser.hits.borrow()[2].clone();
        assert_eq!(hit.rows[0].index, 3);
        app.handle_browser_click(65, hit.rows[0].top + 1, false);
        assert_eq!(app.current_folder_id, "photos");
        assert_eq!(app.entries[app.selected].id, "photo3");
        render(&app, 120, 10);
        assert_eq!(app.browser.hits.borrow()[2].kind, PaneKind::Active);
        app.leave_directory();
        assert_eq!(app.current_folder_id, "pack");
        assert_eq!(app.entries[app.selected].id, "photos");
    }

    #[test]
    fn thumbnails_paginate_by_entries_and_ignore_unrendered_last_rows() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.config.inline_thumbnail_size = InlineThumbnailSize::Medium;
        app.entries = (0..20).map(|i| photo(&format!("photo{i}"))).collect();
        render(&app, 40, 10);
        assert_eq!(app.browser_page_size(), 2);
        assert_eq!(app.browser.visible_images.borrow().len(), 2);
        app.handle_key(
            crossterm::event::KeyCode::PageDown,
            crossterm::event::KeyModifiers::NONE,
        )
        .unwrap();
        assert_eq!(app.selected, 2);
        render(&app, 40, 10);
        let hit = app.browser.hits.borrow()[0].clone();
        assert_eq!(hit.rows.len(), 2);
        assert!(hit.rows.iter().any(|r| r.index == 2));
        app.handle_browser_click(5, hit.rows[0].top, false);
        assert_eq!(app.selected, hit.rows[0].index);
    }

    #[test]
    fn tiny_thumbnail_loading_keeps_the_filename_in_the_same_column() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.entries = vec![photo("beach")];
        let cached = app
            .browser
            .inline_images
            .remove("https://example.invalid/beach")
            .unwrap();
        let loading = render(&app, 40, 10);
        assert_eq!(loading.backend().buffer()[(6, 1)].symbol(), "b");
        app.browser
            .inline_images
            .insert("https://example.invalid/beach".into(), cached);
        let ready = render(&app, 40, 10);
        assert_eq!(ready.backend().buffer()[(6, 1)].symbol(), "b");
        assert_eq!(ready.backend().buffer()[(3, 1)].symbol(), "▀");
    }

    #[test]
    fn native_inline_images_reuse_encoding_and_resize_without_covering_text() {
        use crate::config::ImageProtocol;
        use ratatui_image::picker::{Picker, ProtocolType};
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(1);
        app.entries = vec![photo("beach")];
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        app.image_picker = Some(picker);
        app.config
            .image_protocols
            .insert(TuiConfig::detect_terminal(), ImageProtocol::Kitty);
        let first = render(&app, 40, 10);
        let second = render(&app, 40, 10);
        let third = render(&app, 40, 10);
        assert_ne!(
            first.backend().buffer(),
            second.backend().buffer(),
            "the first frame transmits image data; later frames reuse it"
        );
        assert_eq!(
            second.backend().buffer(),
            third.backend().buffer(),
            "native image id and encoding must remain stable between frames"
        );
        assert_eq!(second.backend().buffer()[(6, 1)].symbol(), "b");
        assert_eq!(app.browser.inline_protocols.borrow().len(), 1);
        app.config.inline_thumbnail_size = InlineThumbnailSize::Small;
        let resized = render(&app, 40, 10);
        assert_eq!(resized.backend().buffer()[(8, 1)].symbol(), "b");
        assert_eq!(
            app.browser.inline_protocols.borrow()["https://example.invalid/beach"].cells,
            (4, 2)
        );
        app.input = InputMode::ConfirmDelete;
        let overlay = render(&app, 40, 10);
        assert!(
            !overlay
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol().contains('\u{1b}')),
            "popups must suppress native image escape sequences"
        );
    }

    #[test]
    fn native_full_preview_reuses_encoding_and_log_overlay_does_not_hide_a_transmission() {
        use crate::config::ImageProtocol;
        use ratatui_image::picker::{Picker, ProtocolType};
        let mut app = fixture();
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        app.image_picker = Some(picker);
        app.config
            .image_protocols
            .insert(TuiConfig::detect_terminal(), ImageProtocol::Kitty);
        app.show_logs_overlay = true;
        render(&app, 120, 20);
        assert!(
            app.browser.preview_protocols.borrow().is_empty(),
            "a covered pane must not mark an image as transmitted"
        );
        app.show_logs_overlay = false;
        let first = render(&app, 120, 20);
        let second = render(&app, 120, 20);
        let third = render(&app, 120, 20);
        assert_ne!(first.backend().buffer(), second.backend().buffer());
        assert_eq!(second.backend().buffer(), third.backend().buffer());
        assert!(
            app.browser
                .preview_protocols
                .borrow()
                .contains_key("photo0")
        );
    }

    #[test]
    fn fixed_count_fallback_is_visible_and_small_windows_render_safely() {
        let mut app = fixture();
        app.config.columns = ColumnCount::Fixed(6);
        app.config.inline_thumbnail_size = InlineThumbnailSize::Large;
        for (width, height) in [(1, 1), (8, 3), (50, 3), (80, 10), (180, 24), (240, 32)] {
            render(&app, width, height);
            assert_eq!(app.config.columns, ColumnCount::Fixed(6));
            assert!(
                app.browser
                    .hits
                    .borrow()
                    .iter()
                    .any(|p| p.kind == PaneKind::Active)
            );
        }
        let terminal = render(&app, 80, 10);
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("2/6 cols"), "{text}");
    }

    #[test]
    fn export_browser_fixture_for_visual_review() {
        // Optional output contains TestBackend cells, exactly as Rust draws
        // them, with synthetic image data and no account/network access.
        let Ok(path) = std::env::var("PIKPAKTUI_BROWSER_SNAPSHOT") else {
            return;
        };
        let mut app = fixture();
        let mut snapshots = Vec::new();
        for (label, count, size, width) in [
            ("four-tiny", 4, InlineThumbnailSize::Tiny, 120),
            ("four-small", 4, InlineThumbnailSize::Small, 120),
            ("six-small", 6, InlineThumbnailSize::Small, 180),
            ("narrow-six", 6, InlineThumbnailSize::Tiny, 80),
        ] {
            app.config.columns = ColumnCount::Fixed(count);
            app.config.inline_thumbnail_size = size;
            let terminal = render(&app, width, 20);
            snapshots.push(serde_json::json!({"label": label, "width": width, "height": 20, "cells": terminal.backend().buffer().content.iter().map(|c| serde_json::json!({"s": c.symbol(), "fg": format!("{:?}", c.fg), "bg": format!("{:?}", c.bg)})).collect::<Vec<_>>() }));
        }
        let mut app = handdraw_fixture();
        for label in ["po-continuous-preview", "po-entered", "media-multiline"] {
            if label != "po-continuous-preview" {
                let selected = app.current_entry().unwrap().clone();
                app.enter_directory(selected);
            }
            if label == "media-multiline" {
                app.config.inline_thumbnail_size = InlineThumbnailSize::Large;
                app.config.columns = ColumnCount::Fixed(6);
                app.entries[1].name = "Long video file name 中文标题 which wraps.mp4".into();
            }
            let terminal = render(&app, 180, 20);
            snapshots.push(serde_json::json!({"label": label, "width": 180, "height": 20, "cells": terminal.backend().buffer().content.iter().map(|c| serde_json::json!({"s": c.symbol(), "fg": format!("{:?}", c.fg), "bg": format!("{:?}", c.bg)})).collect::<Vec<_>>() }));
        }
        std::fs::write(path, serde_json::to_vec(&snapshots).unwrap()).unwrap();
    }
}
