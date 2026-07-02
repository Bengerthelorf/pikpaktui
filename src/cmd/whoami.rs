use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let json = args.iter().any(|a| a == "-J" || a == "--json");

    let client = super::cli_client()?;
    let spinner = super::Spinner::new("Fetching account info...");
    let me = client.user_info()?;
    drop(spinner);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&me).unwrap_or_else(|_| "{}".into())
        );
        return Ok(());
    }

    let field = |key: &str| me[key].as_str().unwrap_or("");
    let rows = [
        ("Name:    ", field("name")),
        ("Email:   ", field("email")),
        ("ID:      ", field("sub")),
        ("Status:  ", field("status")),
        ("Created: ", &super::format_date(field("created_at"))),
    ];
    for (label, value) in rows {
        if !value.is_empty() {
            println!("{}{}", label, value);
        }
    }
    // Third-party identities (google/apple/...) live in a nested list.
    if let Some(providers) = me["providers"].as_array() {
        let names: Vec<&str> = providers
            .iter()
            .filter_map(|p| p["provider_name"].as_str())
            .collect();
        if !names.is_empty() {
            println!("Login:   {}", names.join(", "));
        }
    }
    Ok(())
}
