use anyhow::Result;

use crate::pikpak;

pub fn run(args: &[String]) -> Result<()> {
    let path = args.first().map(|s| s.as_str()).unwrap_or("/");
    let client = super::cli_client()?;
    let parent_id = client.resolve_path(path)?;
    let entries = client.ls(&parent_id)?;

    for e in &entries {
        let kind = match e.kind {
            pikpak::EntryKind::Folder => "DIR ",
            pikpak::EntryKind::File => "FILE",
        };
        println!("{} {:>12}  {}", kind, super::format_size(e.size), e.name);
    }
    if entries.is_empty() {
        println!("(empty)");
    }
    Ok(())
}
