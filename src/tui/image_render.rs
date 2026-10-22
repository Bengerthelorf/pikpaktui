use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};
use std::time::Instant;

const WORKERS: usize = 2;
const MAX_QUEUED: usize = 32;
const MAX_CACHED: usize = 64;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub(super) enum ImageMode {
    Native(ratatui_image::picker::Picker),
    Colored,
    Grayscale,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderKey {
    id: String,
    // Sharing the source avoids copying pixels in draw and distinguishes a
    // replacement image even when its remote ID/URL is unchanged.
    source: usize,
    cells: (u16, u16),
    mode: (u8, u16, u16),
}

struct Request {
    key: RenderKey,
    id: u64,
    image: Arc<image::DynamicImage>,
    mode: ImageMode,
    priority: u8,
}

enum Asset {
    Native(ratatui_image::protocol::Protocol),
    Lines(Vec<Line<'static>>),
}
struct CachedAsset {
    request: u64,
    source: Arc<image::DynamicImage>,
    asset: Option<Result<Asset, String>>,
    touched: Instant,
    bytes: usize,
}
struct Response {
    worker: usize,
    key: RenderKey,
    id: u64,
    asset: Result<Asset, String>,
}
struct Worker {
    tx: mpsc::SyncSender<Request>,
    busy: bool,
}

pub(super) struct RenderCache {
    entries: HashMap<RenderKey, CachedAsset>,
    queued: HashMap<RenderKey, Request>,
    wanted: HashSet<RenderKey>,
    workers: Vec<Worker>,
    responses: mpsc::Receiver<Response>,
    next_request: u64,
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::with_preparer(prepare)
    }
}

impl RenderCache {
    fn with_preparer(
        prepare: impl Fn(&Request) -> Result<Asset, String> + Send + Sync + 'static,
    ) -> Self {
        let prepare = Arc::new(prepare);
        let (tx, responses) = mpsc::sync_channel(WORKERS);
        let workers = (0..WORKERS)
            .map(|worker| {
                let (requests, rx) = mpsc::sync_channel::<Request>(1);
                let results = tx.clone();
                let prepare = Arc::clone(&prepare);
                std::thread::spawn(move || {
                    while let Ok(request) = rx.recv() {
                        let asset = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            prepare(&request)
                        }))
                        .unwrap_or_else(|_| Err("Image preparation failed".into()));
                        if results
                            .send(Response {
                                worker,
                                key: request.key,
                                id: request.id,
                                asset,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                Worker {
                    tx: requests,
                    busy: false,
                }
            })
            .collect();
        Self {
            entries: HashMap::new(),
            queued: HashMap::new(),
            wanted: HashSet::new(),
            workers,
            responses,
            next_request: 0,
        }
    }
}

impl RenderCache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.queued.clear();
        self.wanted.clear();
        // Running workers remain busy until they return. Refresh and resize
        // never release fictitious slots or create another encoding pool.
    }

    pub fn poll(&mut self) {
        for _ in 0..WORKERS {
            let Ok(response) = self.responses.try_recv() else {
                break;
            };
            self.workers[response.worker].busy = false;
            if let Some(entry) = self.entries.get_mut(&response.key)
                && entry.request == response.id
            {
                entry.asset = Some(response.asset);
            }
        }
    }

    pub fn begin_frame(&mut self) {
        self.poll();
        self.wanted.clear();
    }

    #[cfg(test)]
    pub fn pending_visible(&self) -> bool {
        self.wanted.iter().any(|key| {
            self.entries
                .get(key)
                .is_none_or(|entry| entry.asset.is_none())
        })
    }

    #[cfg(test)]
    pub fn has_native(&self, id: &str, cells: Option<(u16, u16)>) -> bool {
        self.entries.iter().any(|(key, entry)| {
            key.id == id
                && cells.is_none_or(|cells| cells == key.cells)
                && matches!(entry.asset, Some(Ok(Asset::Native(_))))
        })
    }

    pub fn finish_frame(&mut self) {
        // Navigation/resize replaces queued work with the current frame's
        // working set. Slow old encodes cannot create an input-sized backlog.
        self.queued.retain(|key, _| self.wanted.contains(key));
        while self.queued.len() > MAX_QUEUED {
            let key = self
                .queued
                .iter()
                .min_by_key(|(_, request)| (request.priority, std::cmp::Reverse(request.id)))
                .map(|(key, _)| key.clone())
                .unwrap();
            self.queued.remove(&key);
            self.entries.remove(&key);
        }
        for worker in &mut self.workers {
            if worker.busy {
                continue;
            }
            let key = self
                .queued
                .iter()
                .max_by_key(|(_, request)| (request.priority, std::cmp::Reverse(request.id)))
                .map(|(key, _)| key.clone());
            let Some(key) = key else { break };
            let request = self.queued.remove(&key).unwrap();
            match worker.tx.try_send(request) {
                Ok(()) => worker.busy = true,
                Err(error) => {
                    let request = match error {
                        mpsc::TrySendError::Full(request)
                        | mpsc::TrySendError::Disconnected(request) => request,
                    };
                    if let Some(entry) = self.entries.get_mut(&request.key) {
                        entry.asset = Some(Err("Image worker unavailable".into()));
                    }
                }
            }
        }
        self.entries
            .retain(|key, entry| entry.asset.is_some() || self.wanted.contains(key));
        let mut bytes: usize = self.entries.values().map(|entry| entry.bytes).sum();
        while self.entries.len() > MAX_CACHED || bytes > MAX_CACHE_BYTES {
            let key = self
                .entries
                .iter()
                .filter(|(key, _)| !self.wanted.contains(*key))
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone());
            let Some(key) = key else { break };
            if let Some(entry) = self.entries.remove(&key) {
                bytes = bytes.saturating_sub(entry.bytes);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        frame: &mut ratatui::Frame,
        area: Rect,
        id: &str,
        image: &Arc<image::DynamicImage>,
        mode: ImageMode,
        priority: u8,
        spinner: usize,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mode_key = match &mode {
            ImageMode::Native(picker) => (
                picker.protocol_type() as u8,
                picker.font_size().0,
                picker.font_size().1,
            ),
            ImageMode::Colored => (254, 0, 0),
            ImageMode::Grayscale => (255, 0, 0),
        };
        let key = RenderKey {
            id: id.into(),
            source: Arc::as_ptr(image) as usize,
            cells: (area.width, area.height),
            mode: mode_key,
        };
        self.wanted.insert(key.clone());
        if !self.entries.contains_key(&key) {
            self.next_request = self.next_request.wrapping_add(1);
            let bytes = if mode_key.1 > 0 {
                (usize::from(area.width) * usize::from(mode_key.1)).min(2048)
                    * (usize::from(area.height) * usize::from(mode_key.2)).min(2048)
                    * 8
            } else {
                usize::from(area.width) * usize::from(area.height) * 96
            };
            self.entries.insert(
                key.clone(),
                CachedAsset {
                    request: self.next_request,
                    source: Arc::clone(image),
                    asset: None,
                    touched: Instant::now(),
                    bytes,
                },
            );
            self.queued.insert(
                key.clone(),
                Request {
                    key: key.clone(),
                    id: self.next_request,
                    image: Arc::clone(image),
                    mode,
                    priority,
                },
            );
        }
        let entry = self.entries.get_mut(&key).unwrap();
        entry.touched = Instant::now();
        // Keep the original allocation alive while its pointer is a cache key.
        debug_assert!(Arc::ptr_eq(image, &entry.source));
        let state = match &entry.asset {
            Some(Ok(Asset::Native(protocol))) => {
                frame.render_widget(ratatui_image::Image::new(protocol), area);
                return;
            }
            Some(Ok(Asset::Lines(lines))) => {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(ratatui::text::Text::from(lines.clone())),
                    area,
                );
                return;
            }
            Some(Err(_)) => super::thumbnail_placeholder::ThumbnailPlaceholder::Unavailable,
            None => super::thumbnail_placeholder::ThumbnailPlaceholder::Loading,
        };
        super::thumbnail_placeholder::draw_thumbnail_placeholder(frame, area, state, spinner);
    }
}

fn prepare(request: &Request) -> Result<Asset, String> {
    let area = Rect::new(0, 0, request.key.cells.0, request.key.cells.1);
    match &request.mode {
        ImageMode::Native(picker) => {
            let image = upscale_for_rect(&request.image, area, picker.font_size());
            picker
                .new_protocol(image, area, ratatui_image::Resize::Fit(None))
                .map(Asset::Native)
                .map_err(|error| error.to_string())
        }
        ImageMode::Colored => Ok(Asset::Lines(render_image_to_colored_lines(
            &request.image,
            area.width.into(),
            area.height.into(),
        ))),
        ImageMode::Grayscale => {
            let mut lines = render_image_to_grayscale_lines(
                &request.image,
                area.width.into(),
                area.height.into(),
            );
            for line in &mut lines {
                line.style = Style::default().fg(Color::DarkGray);
            }
            Ok(Asset::Lines(lines))
        }
    }
}

impl super::App {
    pub(super) fn draw_prepared_image(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        id: &str,
        image: &Arc<image::DynamicImage>,
        mode: ImageMode,
        priority: u8,
    ) {
        self.image_render_cache.borrow_mut().draw(
            frame,
            area,
            id,
            image,
            mode,
            priority,
            self.spinner_idx,
        );
    }
}

/// Upscale `img` so it fills at least `area` terminal cells (using `font_size` px/cell).
/// If the image is already large enough, returns a clone unchanged.
/// This ensures protocol renderers (Kitty/iTerm2) don't render at native pixel size
/// when the thumbnail API returned a low-resolution image.
pub(super) fn upscale_for_rect(
    img: &image::DynamicImage,
    area: Rect,
    font_size: (u16, u16),
) -> image::DynamicImage {
    let target_px_w = (area.width as u32 * font_size.0 as u32).min(2048);
    let target_px_h = (area.height as u32 * font_size.1 as u32).min(2048);
    if target_px_w == 0 || target_px_h == 0 {
        return img.clone();
    }
    if img.width() < target_px_w || img.height() < target_px_h {
        img.resize(
            target_px_w,
            target_px_h,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img.clone()
    }
}

/// Compute a horizontally-centered sub-rect for a protocol image inside `area`.
/// When the image is portrait (height-constrained), it renders narrower than
/// `area.width`; this centers it so the text below is unaffected.
pub(super) fn center_image_rect(img: &image::DynamicImage, area: Rect) -> Rect {
    use image::GenericImageView;
    let (orig_w, orig_h) = img.dimensions();
    if orig_h == 0 || area.height == 0 {
        return area;
    }
    let orig_aspect = orig_w as f32 / orig_h as f32;
    let natural_w = (area.height as f32 * 2.0 * orig_aspect) as u16;
    if natural_w >= area.width {
        // Landscape / square: already fills full width, no shift needed
        return area;
    }
    // Portrait: narrower than the panel — center horizontally
    Rect {
        x: area.x + (area.width - natural_w) / 2,
        y: area.y,
        width: natural_w,
        height: area.height,
    }
}

/// Render image to colored halfblock lines.
pub(super) fn render_image_to_colored_lines(
    img: &image::DynamicImage,
    max_width: u32,
    max_height: u32,
) -> Vec<Line<'static>> {
    use image::GenericImageView;

    let (orig_w, orig_h) = img.dimensions();
    if orig_w == 0 || orig_h == 0 || max_width == 0 || max_height == 0 {
        return vec![];
    }
    let orig_aspect = orig_w as f32 / orig_h as f32;

    // Terminal characters are ~2x taller than wide
    let target_width = max_width;
    let target_height_chars = ((target_width as f32 / orig_aspect) / 2.0).max(1.0) as u32;

    let (final_width, final_height_chars) = if target_height_chars > max_height {
        let h = max_height;
        let w = ((h as f32 * 2.0 * orig_aspect) as u32).max(1);
        (w, h)
    } else {
        (target_width, target_height_chars)
    };

    let left_pad = (max_width.saturating_sub(final_width) / 2) as usize;

    // Resize to double height (each char shows 2 pixels vertically)
    let img = img.resize(
        final_width,
        final_height_chars * 2,
        image::imageops::FilterType::Lanczos3,
    );

    let (w, h) = img.dimensions();
    let mut lines = Vec::new();

    for y in 0..final_height_chars {
        let mut spans = Vec::new();
        if left_pad > 0 {
            spans.push(Span::raw(" ".repeat(left_pad)));
        }
        for x in 0..w {
            let y_top = (y * 2).min(h - 1);
            let y_bottom = (y * 2 + 1).min(h - 1);

            let top_pixel = img.get_pixel(x, y_top);
            let bottom_pixel = img.get_pixel(x, y_bottom);

            let span = Span::styled(
                "▀",
                Style::default()
                    .fg(Color::Rgb(top_pixel[0], top_pixel[1], top_pixel[2]))
                    .bg(Color::Rgb(
                        bottom_pixel[0],
                        bottom_pixel[1],
                        bottom_pixel[2],
                    )),
            );
            spans.push(span);
        }
        lines.push(Line::from(spans));
    }

    lines
}

/// Render image to grayscale ASCII art lines.
pub(super) fn render_image_to_grayscale_lines(
    img: &image::DynamicImage,
    max_width: u32,
    max_height: u32,
) -> Vec<Line<'static>> {
    use image::GenericImageView;

    const ASCII_CHARS: &[char] = &[' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];

    let (orig_w, orig_h) = img.dimensions();
    if orig_w == 0 || orig_h == 0 || max_width == 0 || max_height == 0 {
        return vec![];
    }
    let orig_aspect = orig_w as f32 / orig_h as f32;

    // Terminal characters are ~2x taller than wide
    let target_width = max_width;
    let target_height_chars = ((target_width as f32 / orig_aspect) / 2.0).max(1.0) as u32;

    let (final_width, final_height_chars) = if target_height_chars > max_height {
        let h = max_height;
        let w = ((h as f32 * 2.0 * orig_aspect) as u32).max(1);
        (w, h)
    } else {
        (target_width, target_height_chars)
    };

    let left_pad = (max_width.saturating_sub(final_width) / 2) as usize;

    // Resize to double height (sample every 2 rows)
    let img = img.resize(
        final_width,
        final_height_chars * 2,
        image::imageops::FilterType::Lanczos3,
    );

    let (w, h) = img.dimensions();
    let mut lines = Vec::new();

    for y in 0..final_height_chars {
        let mut line_str = if left_pad > 0 {
            " ".repeat(left_pad)
        } else {
            String::new()
        };
        for x in 0..w {
            let y1 = (y * 2).min(h - 1);
            let y2 = (y * 2 + 1).min(h - 1);

            let pixel1 = img.get_pixel(x, y1);
            let pixel2 = img.get_pixel(x, y2);

            let brightness = ((pixel1[0] as u32 + pixel2[0] as u32) / 2
                + (pixel1[1] as u32 + pixel2[1] as u32) / 2
                + (pixel1[2] as u32 + pixel2[2] as u32) / 2)
                / 3;

            let idx = (brightness as usize * ASCII_CHARS.len()) / 256;
            line_str.push(ASCII_CHARS[idx.min(ASCII_CHARS.len() - 1)]);
        }
        lines.push(Line::from(line_str));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }
    fn stalled_cache() -> (RenderCache, mpsc::Receiver<()>, Release) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release = Release(Arc::clone(&gate));
        let (tx, started) = mpsc::channel();
        let cache = RenderCache::with_preparer(move |request| {
            tx.send(()).unwrap();
            let (mutex, condition) = &*gate;
            let mut released = mutex.lock().unwrap();
            while !*released {
                let (next, timeout) = condition
                    .wait_timeout(released, Duration::from_secs(3))
                    .unwrap();
                released = next;
                if timeout.timed_out() {
                    return Err("test gate timed out".into());
                }
            }
            prepare(request)
        });
        (cache, started, release)
    }
    fn draw_requests(
        cache: &mut RenderCache,
        image: &Arc<image::DynamicImage>,
        names: &[String],
        width: u16,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        cache.begin_frame();
        terminal
            .draw(|frame| {
                for (index, name) in names.iter().enumerate() {
                    cache.draw(
                        frame,
                        Rect::new(0, 0, width, 8),
                        name,
                        image,
                        ImageMode::Colored,
                        u8::from(index == 0),
                        0,
                    );
                }
            })
            .unwrap();
        cache.finish_frame();
        terminal
    }
    fn settle(cache: &mut RenderCache) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while cache.pending_visible() {
            assert!(Instant::now() < deadline);
            cache.poll();
            cache.finish_frame();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn stalled_encoders_never_block_draw_or_input_and_queue_stays_bounded() {
        let (mut cache, started, release) = stalled_cache();
        let image = Arc::new(image::DynamicImage::new_rgb8(4096, 2304));
        let names: Vec<_> = (0..100).map(|i| format!("first-{i}")).collect();
        let loading = draw_requests(&mut cache, &image, &names, 30);
        for _ in 0..WORKERS {
            started.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        assert_eq!(
            cache.workers.iter().filter(|worker| worker.busy).count(),
            WORKERS
        );
        assert!(cache.queued.len() <= MAX_QUEUED);
        assert!(
            loading
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol() == super::super::SPINNER_FRAMES[0])
        );
        let input_started = Instant::now();
        let mut app = super::super::App::new_login(
            crate::pikpak::PikPak::new().unwrap(),
            None,
            crate::config::TuiConfig::default(),
        );
        app.input = super::super::InputMode::Normal;
        app.image_render_cache = std::cell::RefCell::new(cache);
        // The heavyweight 4K image is already shared; draw only clones Arcs.
        for _ in 0..100 {
            app.handle_key(
                crossterm::event::KeyCode::Char('h'),
                crossterm::event::KeyModifiers::NONE,
            )
            .unwrap();
        }
        let mut cache = app.image_render_cache.borrow_mut();
        let latest: Vec<_> = (0..100).map(|i| format!("latest-{i}")).collect();
        draw_requests(&mut cache, &image, &latest, 20);
        assert!(cache.queued.keys().all(|key| key.id.starts_with("latest-")));
        assert!(
            input_started.elapsed() < Duration::from_secs(1),
            "input/draw waited for the encoder gate"
        );
        cache.clear();
        assert!(
            cache.workers.iter().all(|worker| worker.busy),
            "refresh must retain actual worker ownership"
        );
        drop(release);
    }

    #[test]
    fn visible_images_beyond_queue_capacity_finish_over_subsequent_frames() {
        let mut cache = RenderCache::default();
        let image = Arc::new(image::DynamicImage::new_rgb8(32, 32));
        let names: Vec<_> = (0..MAX_QUEUED + 5)
            .map(|i| format!("visible-{i}"))
            .collect();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            draw_requests(&mut cache, &image, &names, 20);
            assert!(cache.queued.len() <= MAX_QUEUED);
            assert!(cache.workers.iter().filter(|worker| worker.busy).count() <= WORKERS);
            if !cache.pending_visible() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "visible images starved behind the bounded queue"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn grayscale_preparation_preserves_the_existing_muted_style() {
        let mut cache = RenderCache::default();
        let image = Arc::new(image::DynamicImage::new_rgb8(32, 32));
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        cache.begin_frame();
        terminal
            .draw(|frame| {
                cache.draw(
                    frame,
                    Rect::new(0, 0, 20, 8),
                    "gray",
                    &image,
                    ImageMode::Grayscale,
                    1,
                    0,
                )
            })
            .unwrap();
        cache.finish_frame();
        settle(&mut cache);
        let asset = cache
            .entries
            .values()
            .next()
            .unwrap()
            .asset
            .as_ref()
            .unwrap();
        let Ok(Asset::Lines(lines)) = asset else {
            panic!("expected grayscale text");
        };
        assert!(
            lines
                .iter()
                .all(|line| line.style.fg == Some(Color::DarkGray))
        );
    }

    #[test]
    fn resize_and_refresh_ignore_late_results_and_replace_image_by_shared_identity() {
        let (mut cache, started, release) = stalled_cache();
        let image = Arc::new(image::DynamicImage::new_rgb8(128, 128));
        let name = vec!["selected".into()];
        draw_requests(&mut cache, &image, &name, 20);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        cache.clear();
        draw_requests(&mut cache, &image, &name, 30);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            cache.workers.iter().filter(|worker| worker.busy).count(),
            WORKERS
        );
        drop(release);
        settle(&mut cache);
        assert!(cache.entries.keys().all(|key| key.cells == (30, 8)));
        let replacement = Arc::new(image::DynamicImage::new_rgb8(128, 128));
        draw_requests(&mut cache, &replacement, &name, 30);
        assert!(cache.entries.iter().any(|(key, entry)| key.source
            == Arc::as_ptr(&replacement) as usize
            && entry.asset.is_none()));
        settle(&mut cache);
        assert!(cache.entries.iter().any(|(key, entry)| key.source
            == Arc::as_ptr(&replacement) as usize
            && entry.asset.is_some()));
    }
}
