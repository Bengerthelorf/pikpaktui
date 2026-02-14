use crate::config::ColorScheme;
use crate::pikpak::{Entry, EntryKind};
use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Folder,
    Archive,
    Image,
    Video,
    Audio,
    Document,
    Code,
    Default,
}

pub fn categorize(entry: &Entry) -> FileCategory {
    if entry.kind == EntryKind::Folder {
        return FileCategory::Folder;
    }

    let ext = entry
        .name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "zip" | "tar" | "gz" | "bz2" | "xz" | "rar" | "7z" | "zst" | "lz4" | "tgz" => {
            FileCategory::Archive
        }
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "bmp" | "ico" | "tiff" | "tif"
        | "heic" | "heif" | "avif" => FileCategory::Image,
        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "rmvb" | "rm" => {
            FileCategory::Video
        }
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "wma" | "m4a" | "opus" | "ape" => {
            FileCategory::Audio
        }
        "pdf" | "doc" | "docx" | "txt" | "md" | "rtf" | "odt" | "xls" | "xlsx" | "ppt" | "pptx"
        | "csv" | "epub" => FileCategory::Document,
        "rs" | "py" | "js" | "go" | "c" | "cpp" | "h" | "hpp" | "java" | "kt" | "swift" | "rb"
        | "php" | "sh" | "bash" | "zsh" | "lua" | "zig" | "toml" | "yaml" | "yml" | "json"
        | "xml" | "html" | "css" | "sql" | "r" | "dart" | "ex" | "exs" | "hs" | "ml" | "scala"
        | "clj" | "nim" | "v" | "vue" | "jsx" | "tsx" | "svelte" => FileCategory::Code,
        _ => FileCategory::Default,
    }
}

pub fn icon(category: FileCategory, nerd_font: bool) -> &'static str {
    if nerd_font {
        match category {
            FileCategory::Folder => "\u{f07b} ",   //
            FileCategory::Archive => "\u{f1c6} ",  //
            FileCategory::Image => "\u{f1c5} ",    //
            FileCategory::Video => "\u{f03d} ",    //
            FileCategory::Audio => "\u{f001} ",    //
            FileCategory::Document => "\u{f15c} ", //
            FileCategory::Code => "\u{f121} ",     //
            FileCategory::Default => "\u{f15b} ",  //
        }
    } else {
        match category {
            FileCategory::Folder => "[D]",
            _ => "[F]",
        }
    }
}

pub fn color_for_scheme(category: FileCategory, scheme: ColorScheme) -> Color {
    match scheme {
        ColorScheme::Classic => match category {
            FileCategory::Folder => Color::Blue,
            FileCategory::Archive => Color::Red,
            FileCategory::Image => Color::Magenta,
            FileCategory::Video => Color::Cyan,
            FileCategory::Audio => Color::LightCyan,
            FileCategory::Document => Color::Green,
            FileCategory::Code => Color::Yellow,
            FileCategory::Default => Color::White,
        },
        ColorScheme::Vibrant => match category {
            FileCategory::Folder => Color::LightBlue,
            FileCategory::Archive => Color::LightRed,
            FileCategory::Image => Color::LightMagenta,
            FileCategory::Video => Color::LightCyan,
            FileCategory::Audio => Color::Cyan,
            FileCategory::Document => Color::LightGreen,
            FileCategory::Code => Color::LightYellow,
            FileCategory::Default => Color::White,
        },
        ColorScheme::Custom => {
            // Custom colors should be handled by TuiConfig::get_color()
            // This is a fallback to Vibrant
            match category {
                FileCategory::Folder => Color::LightBlue,
                FileCategory::Archive => Color::LightRed,
                FileCategory::Image => Color::LightMagenta,
                FileCategory::Video => Color::LightCyan,
                FileCategory::Audio => Color::Cyan,
                FileCategory::Document => Color::LightGreen,
                FileCategory::Code => Color::LightYellow,
                FileCategory::Default => Color::White,
            }
        }
    }
}

/// Icon for CLI output. Returns "" when nerd_font is off (colors are enough).
pub fn cli_icon(category: FileCategory, nerd_font: bool) -> &'static str {
    if nerd_font { icon(category, true) } else { "" }
}

/// Returns true if the file extension suggests a text file that can be previewed.
pub fn is_text_previewable(entry: &Entry) -> bool {
    if entry.kind == EntryKind::Folder {
        return false;
    }

    let ext = entry
        .name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    matches!(
        ext.as_str(),
        // Document (text)
        "txt" | "md" | "csv" | "rtf"
        // Code
        | "rs" | "py" | "js" | "go" | "c" | "cpp" | "h" | "hpp" | "java" | "kt"
        | "swift" | "rb" | "php" | "sh" | "bash" | "zsh" | "lua" | "zig" | "toml" | "yaml"
        | "yml" | "json" | "xml" | "html" | "css" | "sql" | "r" | "dart" | "ex" | "exs"
        | "hs" | "ml" | "scala" | "clj" | "nim" | "v" | "vue" | "jsx" | "tsx" | "svelte"
        // Subtitle / config
        | "srt" | "ass" | "ssa" | "vtt" | "sub" | "log" | "ini" | "cfg" | "conf" | "env"
        | "properties" | "nfo"
    )
}

/// ANSI colored text for CLI output, using eza-style colors.
pub fn cli_colored(text: &str, category: FileCategory) -> String {
    let code = match category {
        FileCategory::Folder => "1;34",   // bold blue
        FileCategory::Archive => "1;31",  // bold red
        FileCategory::Image => "35",      // magenta
        FileCategory::Video => "1;35",    // bold magenta
        FileCategory::Audio => "36",      // cyan
        FileCategory::Document => "1;33", // bold yellow
        FileCategory::Code => "1;32",     // bold green
        FileCategory::Default => "0",     // reset
    };
    format!("\x1b[{}m{}\x1b[0m", code, text)
}
