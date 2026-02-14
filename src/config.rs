use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let cfg: AppConfig =
            serde_yaml::from_str(&raw).with_context(|| "failed to parse login.yaml")?;
        Ok(cfg)
    }

    pub fn save_credentials(username: &str, password: &str) -> Result<()> {
        let path = config_path()?;
        let mut cfg = if path.exists() {
            let raw = fs::read_to_string(&path).unwrap_or_default();
            serde_yaml::from_str(&raw).unwrap_or_default()
        } else {
            AppConfig::default()
        };

        cfg.username = Some(username.to_string());
        cfg.password = Some(password.to_string());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }

        let raw = serde_yaml::to_string(&cfg).context("failed to serialize config")?;
        fs::write(&path, raw)
            .with_context(|| format!("failed to write config {}", path.display()))?;
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = home_config_dir().ok_or_else(|| anyhow::anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("login.yaml"))
}

/// Returns ~/.config on all platforms instead of platform-specific config dirs.
fn home_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BorderStyle {
    Rounded,
    Thick,
    ThickRounded,
    Double,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self::Thick
    }
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
pub enum ColorScheme {
    Vibrant,
    Classic,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbnailMode {
    Auto,
    Off,
    ForceColor,
    ForceGrayscale,
}

impl Default for ThumbnailMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl ThumbnailMode {
    pub fn all() -> &'static [Self] {
        &[Self::Auto, Self::Off, Self::ForceColor, Self::ForceGrayscale]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::ForceColor => "force-color",
            Self::ForceGrayscale => "force-grayscale",
        }
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
    if let Ok(ct) = env::var("COLORTERM") {
        if ct.contains("truecolor") || ct.contains("24bit") {
            return true;
        }
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

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Vibrant
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TuiConfig {
    #[serde(default)]
    pub nerd_font: bool,
    #[serde(default = "default_move_mode")]
    pub move_mode: String, // "picker" or "input"
    #[serde(default = "default_true")]
    pub show_help_bar: bool,
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
}

fn default_preview_max_size() -> u64 {
    65536
}

fn default_true() -> bool {
    true
}

fn default_move_mode() -> String {
    "picker".to_string()
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            nerd_font: false,
            move_mode: "picker".to_string(),
            show_help_bar: true,
            cli_nerd_font: false,
            border_style: BorderStyle::default(),
            color_scheme: ColorScheme::default(),
            show_preview: true,
            lazy_preview: false,
            preview_max_size: default_preview_max_size(),
            custom_colors: CustomColors::default(),
            thumbnail_mode: ThumbnailMode::default(),
        }
    }
}

impl TuiConfig {
    pub fn use_picker(&self) -> bool {
        self.move_mode != "input"
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
        toml::from_str(&raw).unwrap_or_default()
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
        fs::write(&path, raw)
            .with_context(|| format!("failed to write config {}", path.display()))?;
        Ok(())
    }
}
