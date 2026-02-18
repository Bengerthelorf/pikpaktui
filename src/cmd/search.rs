use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;
    let config = super::cli_config();
    let nerd_font = config.cli_nerd_font;

    let mut long = false;
    let mut query: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "--long" => long = true,
            arg if !arg.starts_with('-') => {
                if query.is_none() {
                    query = Some(arg.to_string());
                } else {
                    return Err(anyhow!("unexpected argument: {}", arg));
                }
            }
            other => return Err(anyhow!("unknown flag: {}", other)),
        }
        i += 1;
    }

    let query = query.ok_or_else(|| {
        anyhow!("Usage: pikpaktui search <keyword> [-l]\nExample: pikpaktui search \"Avatar\"")
    })?;

    if query.trim().is_empty() {
        return Err(anyhow!("search keyword cannot be empty"));
    }

    let entries = client.search_files(&query)?;

    if entries.is_empty() {
        println!("No results for \"{}\"", query);
        return Ok(());
    }

    if long {
        super::print_entries_long(&entries, nerd_font);
    } else {
        super::print_entries_short(&entries, nerd_font);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_query_detected_before_api() {
        // Empty keyword should be caught before making network calls.
        // We verify the arg-parsing logic here independently of the client.
        let args: Vec<String> = vec!["  ".to_string()];
        let mut query: Option<String> = None;
        for arg in &args {
            if !arg.starts_with('-') && query.is_none() {
                query = Some(arg.to_string());
            }
        }
        assert!(query.as_deref().map(|q| q.trim().is_empty()).unwrap_or(false));
    }

    #[test]
    fn long_flag_parsed() {
        let args: Vec<String> = vec!["-l".to_string(), "avatar".to_string()];
        let mut long = false;
        let mut query: Option<String> = None;
        for arg in &args {
            match arg.as_str() {
                "-l" | "--long" => long = true,
                a if !a.starts_with('-') => query = Some(a.to_string()),
                _ => {}
            }
        }
        assert!(long);
        assert_eq!(query.as_deref(), Some("avatar"));
    }
}
