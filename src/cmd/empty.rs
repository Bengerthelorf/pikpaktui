use crate::pikpak::{Entry, PikPak};
use anyhow::{Result, anyhow};
use std::io::{self, Write};

pub fn run(args: &[String]) -> Result<()> {
    let mut all = false;
    let mut force = false;
    let mut dry_run = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--all" | "-r" | "--recursive" | "/" => all = true,
            "-f" | "--force" => force = true,
            "-n" | "--dry-run" => dry_run = true,
            other => names.push(other),
        }
    }

    if !all && names.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui empty [-n] <name...>   |   pikpaktui empty [-n] [-f] --all"
        ));
    }

    let client = super::cli_client()?;
    if all {
        empty_all(&client, dry_run, force)
    } else {
        empty_named(&client, &names, dry_run)
    }
}

fn empty_all(client: &PikPak, dry_run: bool, force: bool) -> Result<()> {
    let spinner = super::Spinner::new("Fetching trash...");
    let batch = client.ls_trash(500)?;
    drop(spinner);

    if batch.is_empty() {
        println!("Trash is already empty.");
        return Ok(());
    }

    if dry_run {
        println!("[dry-run] Would permanently delete all trash:");
        print_items(&batch);
        if batch.len() >= 500 {
            println!("  ... and more (showing first 500)");
        }
        return Ok(());
    }

    if !force && !confirm("Permanently delete ALL trash items? This cannot be undone. [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    // ls_trash is single-page, so drain it: delete a page, re-list, repeat. The
    // progress guard stops us if a stale listing keeps returning the same ids.
    let mut deleted = 0usize;
    let mut batch = batch;
    loop {
        let ids: Vec<&str> = batch.iter().map(|e| e.id.as_str()).collect();
        client.delete_permanent(&ids)?;
        deleted += ids.len();

        let next = client.ls_trash(500)?;
        if next.is_empty() {
            break;
        }
        if next.iter().map(|e| &e.id).eq(batch.iter().map(|e| &e.id)) {
            eprintln!(
                "warning: trash did not shrink after delete; stopping at {} item(s).",
                deleted
            );
            break;
        }
        batch = next;
    }
    println!("Permanently deleted {} item(s)", deleted);
    Ok(())
}

fn empty_named(client: &PikPak, names: &[&str], dry_run: bool) -> Result<()> {
    let spinner = super::Spinner::new("Fetching trash...");
    let trash = client.ls_trash(500)?;
    drop(spinner);

    for name in names {
        if !trash.iter().any(|e| &e.name == name) {
            eprintln!("warning: '{}' not found in trash", name);
        }
    }

    let targets: Vec<&Entry> = trash
        .iter()
        .filter(|e| names.contains(&e.name.as_str()))
        .collect();

    if targets.is_empty() {
        println!("No matching trash items.");
        return Ok(());
    }

    if dry_run {
        println!(
            "[dry-run] Would permanently delete {} item(s):",
            targets.len()
        );
        for e in &targets {
            println!("  {} (id: {})", e.name, e.id);
        }
        return Ok(());
    }

    let ids: Vec<&str> = targets.iter().map(|e| e.id.as_str()).collect();
    client.delete_permanent(&ids)?;
    println!("Permanently deleted {} item(s)", ids.len());
    Ok(())
}

fn print_items(entries: &[Entry]) {
    for e in entries {
        println!("  {} (id: {})", e.name, e.id);
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES"))
}
