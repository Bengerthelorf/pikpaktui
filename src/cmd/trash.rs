use crate::pikpak::EntryKind;
use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    let limit = args
        .first()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(100);

    let entries = client.ls_trash(limit)?;

    if entries.is_empty() {
        println!("Trash is empty");
        return Ok(());
    }

    for e in &entries {
        let icon = match e.kind {
            EntryKind::Folder => "\u{1f4c1}",
            EntryKind::File => "\u{1f4c4}",
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
