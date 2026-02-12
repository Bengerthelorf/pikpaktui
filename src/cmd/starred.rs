use crate::pikpak::EntryKind;
use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    let limit = args
        .first()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(100);

    let entries = client.starred_list(limit)?;

    if entries.is_empty() {
        println!("No starred items");
        return Ok(());
    }

    for e in &entries {
        let icon = match e.kind {
            EntryKind::Folder => "📁",
            EntryKind::File => "📄",
        };
        let size = if e.kind == EntryKind::File {
            super::format_size(e.size)
        } else {
            String::new()
        };
        println!("{} {:>10}  {}", icon, size, e.name);
    }

    Ok(())
}
