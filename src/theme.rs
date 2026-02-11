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
        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "rmvb"
        | "rm" => FileCategory::Video,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "wma" | "m4a" | "opus" | "ape" => {
            FileCategory::Audio
        }
        "pdf" | "doc" | "docx" | "txt" | "md" | "rtf" | "odt" | "xls" | "xlsx" | "ppt"
        | "pptx" | "csv" | "epub" => FileCategory::Document,
        "rs" | "py" | "js" | "go" | "c" | "cpp" | "h" | "hpp" | "java" | "kt"
        | "swift" | "rb" | "php" | "sh" | "bash" | "zsh" | "lua" | "zig" | "toml" | "yaml"
        | "yml" | "json" | "xml" | "html" | "css" | "sql" | "r" | "dart" | "ex" | "exs"
        | "hs" | "ml" | "scala" | "clj" | "nim" | "v" | "vue" | "jsx" | "tsx" | "svelte" => {
            FileCategory::Code
        }
        _ => FileCategory::Default,
    }
}

pub fn icon(category: FileCategory, nerd_font: bool) -> &'static str {
    if nerd_font {
        match category {
            FileCategory::Folder => "\u{f07b} ",   //
            FileCategory::Archive => "\u{f1c6} ",   //
            FileCategory::Image => "\u{f1c5} ",     //
            FileCategory::Video => "\u{f03d} ",     //
            FileCategory::Audio => "\u{f001} ",     //
            FileCategory::Document => "\u{f15c} ",  //
            FileCategory::Code => "\u{f121} ",      //
            FileCategory::Default => "\u{f15b} ",   //
        }
    } else {
        match category {
            FileCategory::Folder => "[D]",
            _ => "[F]",
        }
    }
}

pub fn color(category: FileCategory) -> Color {
    match category {
        FileCategory::Folder => Color::Blue,
        FileCategory::Archive => Color::Red,
        FileCategory::Image => Color::Magenta,
        FileCategory::Video => Color::Cyan,
        FileCategory::Audio => Color::LightCyan,
        FileCategory::Document => Color::Green,
        FileCategory::Code => Color::Yellow,
        FileCategory::Default => Color::White,
    }
}
