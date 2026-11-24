use anyhow::{Result, anyhow};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlayContext {
    pub name: String,
    pub path: String,
    pub id: String,
}

pub(crate) fn build_command(
    player: &str,
    extra_args: &[String],
    url: &str,
    context: &PlayContext,
) -> Result<Command> {
    let mut parts = player.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow!("player command is empty"))?;
    let mut command = Command::new(program);
    command.args(parts);
    for arg in extra_args {
        let mut expanded = String::new();
        let mut rest = arg.as_str();
        while !rest.is_empty() {
            if let Some((token, value)) = [
                ("{name}", context.name.as_str()),
                ("{path}", context.path.as_str()),
                ("{id}", context.id.as_str()),
            ]
            .into_iter()
            .find(|(token, _)| rest.starts_with(token))
            {
                expanded.push_str(value);
                rest = &rest[token.len()..];
            } else {
                let ch = rest.chars().next().unwrap();
                expanded.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
        command.arg(expanded);
    }
    command
        .arg("--")
        .arg(url)
        .env("PIKPAKTUI_PLAY_NAME", &context.name)
        .env("PIKPAKTUI_PLAY_PATH", &context.path)
        .env("PIKPAKTUI_PLAY_ID", &context.id);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, ffi::OsStr};

    #[test]
    fn safe_arguments_preserve_boundaries_and_substitute_context_once() {
        let context = PlayContext {
            name: "--movie 'one' {path}.mkv".into(),
            path: "/TV Shows/--movie 'one' {path}.mkv".into(),
            id: "file-{name}".into(),
        };
        let command = build_command(
            "wrapper --legacy",
            &[
                "--title={name}".into(),
                "{path}".into(),
                "{id}".into(),
                "quote free two words".into(),
                "{unknown}".into(),
                "$(touch ignored)".into(),
            ],
            "https://example.invalid/video?x=1&y=2",
            &context,
        )
        .unwrap();
        assert_eq!(command.get_program(), OsStr::new("wrapper"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                "--legacy",
                "--title=--movie 'one' {path}.mkv",
                "/TV Shows/--movie 'one' {path}.mkv",
                "file-{name}",
                "quote free two words",
                "{unknown}",
                "$(touch ignored)",
                "--",
                "https://example.invalid/video?x=1&y=2",
            ]
        );
    }

    #[test]
    fn legacy_player_splitting_and_separator_remain_unchanged() {
        let context = PlayContext {
            name: String::new(),
            path: String::new(),
            id: "id".into(),
        };
        let command = build_command("  open -a IINA  ", &[], "url", &context).unwrap();
        assert_eq!(command.get_program(), OsStr::new("open"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec!["-a", "IINA", "--", "url"]
        );
        assert!(build_command("  ", &[], "url", &context).is_err());
    }

    #[test]
    fn context_is_set_only_on_the_child_command() {
        let context = PlayContext {
            name: "film\".mkv".into(),
            path: "/film\".mkv".into(),
            id: "id".into(),
        };
        let before = std::env::var_os("PIKPAKTUI_PLAY_NAME");
        let command = build_command("mpv", &[], "url", &context).unwrap();
        let env: BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env.get(OsStr::new("PIKPAKTUI_PLAY_NAME")),
            Some(&Some(OsStr::new("film\".mkv")))
        );
        assert_eq!(
            env.get(OsStr::new("PIKPAKTUI_PLAY_PATH")),
            Some(&Some(OsStr::new("/film\".mkv")))
        );
        assert_eq!(
            env.get(OsStr::new("PIKPAKTUI_PLAY_ID")),
            Some(&Some(OsStr::new("id")))
        );
        assert_eq!(std::env::var_os("PIKPAKTUI_PLAY_NAME"), before);
    }

    #[cfg(unix)]
    #[test]
    fn child_process_receives_literal_arguments_and_context() {
        let context = PlayContext {
            name: "--movie 'one' \"two\" $(printf injected) {path}.mkv".into(),
            path: "/TV Shows/数据 'one' \"two\" $(printf injected).mkv".into(),
            id: "file 'one' \"two\" $(printf injected) {name}".into(),
        };
        let url = "https://example.invalid/video path?name='one'&literal=$(printf injected)";
        // The shell program is fixed; all player data reaches it as positional
        // arguments or environment values, never as shell source. NUL records
        // let the assertion distinguish separate arguments from joined text.
        let script = r#"printf '%s\0' "$#" "$0" "$@" "$PIKPAKTUI_PLAY_NAME" "$PIKPAKTUI_PLAY_PATH" "$PIKPAKTUI_PLAY_ID""#;
        let output = build_command(
            "/bin/sh",
            &[
                "-c".into(),
                script.into(),
                "player-context-probe".into(),
                "{name}".into(),
                "{path}".into(),
                "{id}".into(),
            ],
            url,
            &context,
        )
        .unwrap()
        .env_remove("ENV")
        .env_remove("BASH_ENV")
        .output()
        .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let expected = [
            "5",
            "player-context-probe",
            &context.name,
            &context.path,
            &context.id,
            "--",
            url,
            &context.name,
            &context.path,
            &context.id,
        ]
        .into_iter()
        .flat_map(|value| value.bytes().chain(std::iter::once(0)))
        .collect::<Vec<_>>();
        assert_eq!(output.stdout, expected);
    }
}
