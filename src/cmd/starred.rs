use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;
    let config = super::cli_config();
    let nerd_font = config.cli_nerd_font;

    let mut long = false;
    let mut limit = 100u32;

    for arg in args {
        match arg.as_str() {
            "-l" | "--long" => long = true,
            _ => {
                if let Ok(n) = arg.parse::<u32>() {
                    limit = n;
                }
            }
        }
    }

    let entries = client.starred_list(limit)?;

    if entries.is_empty() {
        println!("No starred items");
        return Ok(());
    }

    if long {
        super::print_entries_long(&entries, nerd_font);
    } else {
        super::print_entries_short(&entries, nerd_font);
    }

    Ok(())
}
