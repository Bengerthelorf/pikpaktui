use anyhow::{Context, Result, anyhow};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

pub struct Pikpak;

impl Pikpak {
    pub fn ls(path: &str) -> Result<Vec<Entry>> {
        let out = Self::run(["ls", "-l", "-p", path])?;
        Ok(parse_ls(&out))
    }

    pub fn mv(current_path: &str, name: &str, target_path: &str) -> Result<String> {
        let attempts: [Vec<&str>; 2] = [
            vec!["mv", "-p", current_path, "--name", name, "--to", target_path],
            vec!["move", "-p", current_path, "--name", name, "--to", target_path],
        ];
        Self::try_attempts(&attempts)
    }

    pub fn cp(current_path: &str, name: &str, target_path: &str) -> Result<String> {
        let attempts: [Vec<&str>; 2] = [
            vec!["cp", "-p", current_path, "--name", name, "--to", target_path],
            vec!["copy", "-p", current_path, "--name", name, "--to", target_path],
        ];
        Self::try_attempts(&attempts)
    }

    pub fn rename(current_path: &str, old_name: &str, new_name: &str) -> Result<String> {
        let attempts: [Vec<&str>; 2] = [
            vec![
                "rename",
                "-p",
                current_path,
                "--name",
                old_name,
                "--new-name",
                new_name,
            ],
            vec!["mv", "-p", current_path, "--name", old_name, "--new-name", new_name],
        ];
        Self::try_attempts(&attempts)
    }

    pub fn remove(current_path: &str, name: &str) -> Result<String> {
        let attempts: [Vec<&str>; 2] = [
            vec!["rm", "-p", current_path, "--name", name],
            vec!["remove", "-p", current_path, "--name", name],
        ];
        Self::try_attempts(&attempts)
    }

    fn try_attempts(attempts: &[Vec<&str>]) -> Result<String> {
        let mut errors = Vec::new();
        for args in attempts {
            match Self::run(args.iter().copied()) {
                Ok(o) => return Ok(o),
                Err(e) => errors.push(e.to_string()),
            }
        }
        Err(anyhow!(
            "all command variants failed:\n{}",
            errors
                .into_iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }

    fn run<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<String> {
        let out = Command::new("pikpakcli")
            .args(args)
            .output()
            .context("failed to spawn pikpakcli")?;

        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            Err(anyhow!(
                "pikpakcli failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }
}

fn parse_ls(raw: &str) -> Vec<Entry> {
    raw.lines()
        .filter_map(|line| parse_ls_line(line.trim()))
        .collect()
}

fn parse_ls_line(line: &str) -> Option<Entry> {
    if line.is_empty()
        || line.starts_with("total")
        || line.starts_with("Name")
        || line.starts_with("ID")
    {
        return None;
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let size_idx = parts.iter().position(|p| p.parse::<u64>().is_ok())?;
    let size = parts[size_idx].parse::<u64>().ok()?;
    let name = parts.get(size_idx + 1..)?.join(" ");

    if name.is_empty() {
        return None;
    }

    Some(Entry { name, size })
}
