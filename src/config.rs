use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            let cfg: AppConfig =
                toml::from_str(&raw).with_context(|| "failed to parse login.toml")?;
            return Ok(cfg);
        }
        // Backward compat: try legacy login.yaml
        let legacy = path.with_file_name("login.yaml");
        if legacy.exists() {
            let raw = fs::read_to_string(&legacy)
                .with_context(|| format!("failed to read legacy config {}", legacy.display()))?;
            // Parse simple key: value YAML manually to avoid serde_yaml dependency
            let cfg = Self::parse_legacy_yaml(&raw);
            return Ok(cfg);
        }
        Ok(Self::default())
    }

    /// Minimal parser for legacy login.yaml (simple key: value format).
    fn parse_legacy_yaml(raw: &str) -> Self {
        let mut cfg = Self::default();
        for line in raw.lines() {
            let line = line.trim();
            if let Some((key, val)) = line.split_once(':') {
                let key = key.trim();
                let val = val.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "username" => cfg.username = Some(val.to_string()),
                    "password" => cfg.password = Some(val.to_string()),
                    _ => {}
                }
            }
        }
        cfg
    }

    pub fn save_credentials(username: &str, password: &str) -> Result<()> {
        let path = config_path()?;
        let mut cfg = if path.exists() {
            let raw = fs::read_to_string(&path).unwrap_or_default();
            toml::from_str(&raw).unwrap_or_default()
        } else {
            AppConfig::default()
        };

        cfg.username = Some(username.to_string());
        cfg.password = Some(password.to_string());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }

        let raw = toml::to_string_pretty(&cfg).context("failed to serialize config")?;
        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, &raw)
            .with_context(|| format!("failed to write config {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &path)
            .with_context(|| format!("failed to rename config {}", path.display()))?;
        set_file_owner_only(&path);
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = home_config_dir().ok_or_else(|| anyhow::anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("login.toml"))
}

/// Restrict file permissions to owner-only (0600) on Unix.
#[cfg(unix)]
fn set_file_owner_only(path: &PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_file_owner_only(_path: &PathBuf) {}

/// Returns ~/.config on all platforms instead of platform-specific config dirs.
fn home_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum QuotaBarStyle {
    #[default]
    Bar,
    Percent,
}


impl QuotaBarStyle {
    pub fn all() -> &'static [Self] {
        &[Self::Bar, Self::Percent]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Percent => "percent",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum BorderStyle {
    Rounded,
    #[default]
    Thick,
    ThickRounded,
    Double,
}


impl BorderStyle {
    pub fn all() -> &'static [Self] {
        &[Self::Rounded, Self::Thick, Self::ThickRounded, Self::Double]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rounded => "rounded",
            Self::Thick => "thick",
            Self::ThickRounded => "thick-rounded",
            Self::Double => "double",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ColorScheme {
    #[default]
    Vibrant,
    Classic,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ImageProtocol {
    #[default]
    Auto,
    Kitty,
    Iterm2,
    Sixel,
}


impl ImageProtocol {
    pub fn all() -> &'static [Self] {
        &[Self::Auto, Self::Kitty, Self::Iterm2, Self::Sixel]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Kitty => "Kitty",
            Self::Iterm2 => "iTerm2",
            Self::Sixel => "Sixel",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ThumbnailSize {
    Small,
    #[default]
    Medium,
    Large,
}


impl ThumbnailSize {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Small => "SIZE_SMALL",
            Self::Medium => "SIZE_MEDIUM",
            Self::Large => "SIZE_LARGE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ThumbnailMode {
    #[default]
    Auto,
    Off,
    ForceColor,
    ForceGrayscale,
}


impl ThumbnailMode {
    pub fn all() -> &'static [Self] {
        &[Self::Auto, Self::Off, Self::ForceColor, Self::ForceGrayscale]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Off => "Off",
            Self::ForceColor => "Force: Color",
            Self::ForceGrayscale => "Force: Grayscale",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }

    pub fn should_use_color(&self) -> ThumbnailRenderMode {
        match self {
            Self::Auto => {
                if detect_truecolor_support() {
                    ThumbnailRenderMode::Auto
                } else {
                    ThumbnailRenderMode::Grayscale
                }
            }
            Self::Off => ThumbnailRenderMode::Off,
            Self::ForceColor => ThumbnailRenderMode::ColoredHalf,
            Self::ForceGrayscale => ThumbnailRenderMode::Grayscale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailRenderMode {
    Auto,
    ColoredHalf,
    Grayscale,
    Off,
}

pub fn detect_truecolor_support() -> bool {
    if let Ok(ct) = env::var("COLORTERM")
        && (ct.contains("truecolor") || ct.contains("24bit")) {
            return true;
        }

    if let Ok(term) = env::var("TERM") {
        if term.contains("truecolor") || term.contains("24bit") {
            return true;
        }
        if term.starts_with("xterm-256color") {
            return true;
        }
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum SortField {
    #[default]
    Name,
    Size,
    Created,
    Type,
    Extension,
    None,
}


impl SortField {
    pub fn all() -> &'static [Self] {
        &[
            Self::Name,
            Self::Size,
            Self::Created,
            Self::Type,
            Self::Extension,
            Self::None,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Created => "created",
            Self::Type => "type",
            Self::Extension => "extension",
            Self::None => "none",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }
}


impl ColorScheme {
    pub fn all() -> &'static [Self] {
        &[Self::Vibrant, Self::Classic, Self::Custom]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vibrant => "vibrant",
            Self::Classic => "classic",
            Self::Custom => "custom",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }
}

/// Custom color configuration for file categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct CustomColors {
    #[serde(default = "default_folder_color")]
    pub folder: (u8, u8, u8),
    #[serde(default = "default_archive_color")]
    pub archive: (u8, u8, u8),
    #[serde(default = "default_image_color")]
    pub image: (u8, u8, u8),
    #[serde(default = "default_video_color")]
    pub video: (u8, u8, u8),
    #[serde(default = "default_audio_color")]
    pub audio: (u8, u8, u8),
    #[serde(default = "default_document_color")]
    pub document: (u8, u8, u8),
    #[serde(default = "default_code_color")]
    pub code: (u8, u8, u8),
    #[serde(default = "default_default_color")]
    pub default: (u8, u8, u8),
}

fn default_folder_color() -> (u8, u8, u8) {
    (92, 176, 255) // Light Blue
}
fn default_archive_color() -> (u8, u8, u8) {
    (255, 102, 102) // Light Red
}
fn default_image_color() -> (u8, u8, u8) {
    (255, 102, 255) // Light Magenta
}
fn default_video_color() -> (u8, u8, u8) {
    (102, 255, 255) // Light Cyan
}
fn default_audio_color() -> (u8, u8, u8) {
    (0, 255, 255) // Cyan
}
fn default_document_color() -> (u8, u8, u8) {
    (102, 255, 102) // Light Green
}
fn default_code_color() -> (u8, u8, u8) {
    (255, 255, 102) // Light Yellow
}
fn default_default_color() -> (u8, u8, u8) {
    (255, 255, 255) // White
}

impl Default for CustomColors {
    fn default() -> Self {
        Self {
            folder: default_folder_color(),
            archive: default_archive_color(),
            image: default_image_color(),
            video: default_video_color(),
            audio: default_audio_color(),
            document: default_document_color(),
            code: default_code_color(),
            default: default_default_color(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum MoveMode {
    #[default]
    Picker,
    Input,
}

impl MoveMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Picker => Self::Input,
            Self::Input => Self::Picker,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Picker => "picker",
            Self::Input => "input",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TuiConfig {
    #[serde(default)]
    pub nerd_font: bool,
    #[serde(default)]
    pub move_mode: MoveMode,
    #[serde(default = "default_true")]
    pub show_help_bar: bool,
    #[serde(default)]
    pub quota_bar_style: QuotaBarStyle,
    #[serde(default)]
    pub cli_nerd_font: bool,
    #[serde(default)]
    pub border_style: BorderStyle,
    #[serde(default)]
    pub color_scheme: ColorScheme,
    #[serde(default = "default_true")]
    pub show_preview: bool,
    #[serde(default)]
    pub lazy_preview: bool,
    #[serde(default = "default_preview_max_size")]
    pub preview_max_size: u64,
    #[serde(default)]
    pub custom_colors: CustomColors,
    #[serde(default)]
    pub thumbnail_mode: ThumbnailMode,
    #[serde(default)]
    pub thumbnail_size: ThumbnailSize,
    #[serde(default)]
    pub sort_field: SortField,
    #[serde(default)]
    pub sort_reverse: bool,
    #[serde(default)]
    pub image_protocols: BTreeMap<String, ImageProtocol>,
    /// Legacy single-value field kept for backward-compatible deserialization.
    #[serde(default, skip_serializing)]
    image_protocol: Option<ImageProtocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    #[serde(default = "default_download_jobs")]
    pub download_jobs: usize,
    #[serde(default)]
    pub update_check: UpdateCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateCheck {
    #[default]
    Notify,
    Quiet,
    Off,
}

impl UpdateCheck {
    pub fn all() -> &'static [Self] {
        &[Self::Notify, Self::Quiet, Self::Off]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Notify => "Notify",
            Self::Quiet => "Quiet",
            Self::Off => "Off",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Notify => "Check & show in status bar + CLI",
            Self::Quiet => "Check silently, log only",
            Self::Off => "No update checking",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap();
        all[(idx + all.len() - 1) % all.len()]
    }
}

fn default_download_jobs() -> usize { 1 }

fn default_preview_max_size() -> u64 {
    65536
}

fn default_true() -> bool {
    true
}


impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            nerd_font: false,
            move_mode: MoveMode::default(),
            show_help_bar: true,
            quota_bar_style: QuotaBarStyle::default(),
            cli_nerd_font: false,
            border_style: BorderStyle::default(),
            color_scheme: ColorScheme::default(),
            show_preview: true,
            lazy_preview: false,
            preview_max_size: default_preview_max_size(),
            custom_colors: CustomColors::default(),
            thumbnail_mode: ThumbnailMode::default(),
            thumbnail_size: ThumbnailSize::default(),
            sort_field: SortField::default(),
            sort_reverse: false,
            image_protocols: BTreeMap::new(),
            image_protocol: None,
            player: None,
            download_jobs: 1,
            update_check: UpdateCheck::default(),
        }
    }
}

impl TuiConfig {
    pub fn use_picker(&self) -> bool {
        self.move_mode != MoveMode::Input
    }

    /// Detect the current terminal emulator name via `TERM_PROGRAM`.
    pub fn detect_terminal() -> String {
        env::var("TERM_PROGRAM").unwrap_or_else(|_| "unknown".to_string())
    }

    /// Return the image protocol configured for the current terminal,
    /// falling back to `Auto`.
    pub fn current_image_protocol(&self) -> ImageProtocol {
        let term = Self::detect_terminal();
        self.image_protocols
            .get(&term)
            .copied()
            .unwrap_or(ImageProtocol::Auto)
    }

    /// Ensure the current terminal has an entry in the map (defaulting to `Auto`)
    /// and return its name.
    pub fn ensure_current_terminal(&mut self) -> String {
        let term = Self::detect_terminal();
        self.image_protocols
            .entry(term.clone())
            .or_insert(ImageProtocol::Auto);
        term
    }

    pub fn get_color(&self, category: crate::theme::FileCategory) -> ratatui::style::Color {
        use ratatui::style::Color;
        if self.color_scheme == ColorScheme::Custom {
            let rgb = match category {
                crate::theme::FileCategory::Folder => self.custom_colors.folder,
                crate::theme::FileCategory::Archive => self.custom_colors.archive,
                crate::theme::FileCategory::Image => self.custom_colors.image,
                crate::theme::FileCategory::Video => self.custom_colors.video,
                crate::theme::FileCategory::Audio => self.custom_colors.audio,
                crate::theme::FileCategory::Document => self.custom_colors.document,
                crate::theme::FileCategory::Code => self.custom_colors.code,
                crate::theme::FileCategory::Default => self.custom_colors.default,
            };
            Color::Rgb(rgb.0, rgb.1, rgb.2)
        } else {
            crate::theme::color_for_scheme(category, self.color_scheme)
        }
    }
}

impl TuiConfig {
    pub fn load() -> Self {
        let path = match home_config_dir() {
            Some(base) => base.join("pikpaktui").join("config.toml"),
            None => return Self::default(),
        };
        if !path.exists() {
            return Self::default();
        }
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return Self::default(),
        };
        let mut cfg: TuiConfig = match toml::from_str(&raw) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to parse config.toml, using defaults: {e}");
                Self::default()
            }
        };
        if let Some(proto) = cfg.image_protocol.take()
            && cfg.image_protocols.is_empty() {
                let term = Self::detect_terminal();
                cfg.image_protocols.insert(term, proto);
            }
        cfg
    }

    pub fn save(&self) -> Result<()> {
        let path = match home_config_dir() {
            Some(base) => base.join("pikpaktui").join("config.toml"),
            None => return Err(anyhow::anyhow!("unable to locate config dir")),
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }

        let raw = toml::to_string_pretty(self)
            .context("failed to serialize config")?;
        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, &raw)
            .with_context(|| format!("failed to write config {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &path)
            .with_context(|| format!("failed to rename config {}", path.display()))?;
        Ok(())
    }
}

/// Sort a list of entries in-place based on the given sort field and direction.
/// For all sort modes except `None`, folders are always sorted before files.
pub fn sort_entries(entries: &mut [crate::pikpak::Entry], field: SortField, reverse: bool) {
    use crate::pikpak::EntryKind;

    match field {
        SortField::None => return,
        SortField::Name => {
            entries.sort_by(|a, b| {
                let kind_ord = kind_order(&a.kind).cmp(&kind_order(&b.kind));
                kind_ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
        }
        SortField::Size => {
            entries.sort_by(|a, b| {
                let kind_ord = kind_order(&a.kind).cmp(&kind_order(&b.kind));
                kind_ord.then_with(|| b.size.cmp(&a.size))
            });
        }
        SortField::Created => {
            entries.sort_by(|a, b| {
                let kind_ord = kind_order(&a.kind).cmp(&kind_order(&b.kind));
                kind_ord.then_with(|| b.created_time.cmp(&a.created_time))
            });
        }
        SortField::Type => {
            entries.sort_by(|a, b| {
                let kind_ord = kind_order(&a.kind).cmp(&kind_order(&b.kind));
                kind_ord.then_with(|| {
                    let cat_a = category_order(crate::theme::categorize(a));
                    let cat_b = category_order(crate::theme::categorize(b));
                    cat_a.cmp(&cat_b).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                })
            });
        }
        SortField::Extension => {
            entries.sort_by(|a, b| {
                let kind_ord = kind_order(&a.kind).cmp(&kind_order(&b.kind));
                kind_ord.then_with(|| {
                    let ext_a = std::path::Path::new(&a.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let ext_b = std::path::Path::new(&b.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    ext_a.cmp(&ext_b).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                })
            });
        }
    }

    if reverse {
        let folder_end = entries.iter().position(|e| e.kind == EntryKind::File).unwrap_or(entries.len());
        entries[..folder_end].reverse();
        entries[folder_end..].reverse();
    }
}

fn kind_order(kind: &crate::pikpak::EntryKind) -> u8 {
    match kind {
        crate::pikpak::EntryKind::Folder => 0,
        crate::pikpak::EntryKind::File => 1,
    }
}

fn category_order(cat: crate::theme::FileCategory) -> u8 {
    use crate::theme::FileCategory;
    match cat {
        FileCategory::Folder => 0,
        FileCategory::Archive => 1,
        FileCategory::Image => 2,
        FileCategory::Video => 3,
        FileCategory::Audio => 4,
        FileCategory::Document => 5,
        FileCategory::Code => 6,
        FileCategory::Default => 7,
    }
}
