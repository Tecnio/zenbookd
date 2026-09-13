use std::io::{self, Write};

use clap::{Arg, Command, CommandFactory};

use crate::Cli;

pub fn write_fish(out: &mut dyn Write) -> io::Result<()> {
    let mut cmd = Cli::command();
    cmd.build();

    writeln!(out, "complete -c zenbookd -f")?;

    emit_named(out, "__fish_use_subcommand", &cmd)?;

    for sub in visible_subs(&cmd) {
        let name = sub.get_name();
        let mut spec = format!("complete -c zenbookd -n '__fish_use_subcommand' -a {name}");

        if let Some(about) = sub.get_about() {
            spec.push_str(&format!(" -d '{}'", escape(about)));
        }

        writeln!(out, "{spec}")?;
    }

    for sub in visible_subs(&cmd) {
        let name = sub.get_name();
        let condition = format!("__fish_seen_subcommand_from {name}");

        emit_named(out, &condition, sub)?;
        emit_positionals(out, &condition, sub)?;
    }

    for sub in visible_subs(&cmd).filter(|sub| sub.get_name() != "help") {
        let name = sub.get_name();
        let mut spec =
            format!("complete -c zenbookd -n '__fish_seen_subcommand_from help' -a {name}");

        if let Some(about) = sub.get_about() {
            spec.push_str(&format!(" -d '{}'", escape(about)));
        }

        writeln!(out, "{spec}")?;
    }

    Ok(())
}

fn visible_subs(cmd: &Command) -> impl Iterator<Item = &Command> {
    cmd.get_subcommands().filter(|sub| !sub.is_hide_set())
}

fn emit_named(out: &mut dyn Write, condition: &str, cmd: &Command) -> io::Result<()> {
    for arg in cmd.get_arguments() {
        emit_named_arg(out, condition, arg)?;
    }

    Ok(())
}

fn emit_named_arg(out: &mut dyn Write, condition: &str, arg: &Arg) -> io::Result<()> {
    if arg.is_positional() || arg.is_hide_set() {
        return Ok(());
    }

    let mut spec = format!("complete -c zenbookd -n '{condition}'");
    let mut named = false;

    if let Some(shorts) = arg.get_short_and_visible_aliases() {
        for short in shorts {
            spec.push_str(&format!(" -s {short}"));
            named = true;
        }
    }

    if let Some(longs) = arg.get_long_and_visible_aliases() {
        for long in longs {
            spec.push_str(&format!(" -l {long}"));
            named = true;
        }
    }

    if !named {
        return Ok(());
    }

    if let Some(help) = arg.get_help() {
        spec.push_str(&format!(" -d '{}'", escape(help)));
    }

    writeln!(out, "{spec}")
}

fn emit_positionals(out: &mut dyn Write, condition: &str, cmd: &Command) -> io::Result<()> {
    for arg in cmd.get_positionals() {
        if arg.is_hide_set() {
            continue;
        }

        for value in arg.get_possible_values() {
            if value.is_hide_set() {
                continue;
            }

            let mut spec = format!(
                "complete -c zenbookd -n '{condition}' -a {}",
                value.get_name()
            );

            if let Some(help) = value.get_help() {
                spec.push_str(&format!(" -d '{}'", escape(help)));
            }

            writeln!(out, "{spec}")?;
        }
    }

    Ok(())
}

fn escape(text: impl std::fmt::Display) -> String {
    text.to_string().replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script() -> String {
        let mut buf = Vec::new();
        write_fish(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn lists_each_visible_subcommand_with_its_about() {
        let script = script();
        let cmd = Cli::command();

        for sub in visible_subs(&cmd) {
            let name = sub.get_name();

            assert!(
                script.contains(&format!("-a {name}")),
                "missing subcommand {name} in:\n{script}"
            );

            if let Some(about) = sub.get_about() {
                let about = about.to_string();

                assert!(
                    script.contains(&about),
                    "missing description `{about}` in:\n{script}"
                );
            }
        }
    }

    #[test]
    fn includes_flags_and_toggle_values() {
        let script = script();

        assert!(script.contains("-l stop"));
        assert!(script.contains("-l help"));
        assert!(script.contains("-l version"));
        assert!(script.contains("-a on"));
        assert!(script.contains("-a off"));
        assert!(script.contains("-a help"));
        assert!(!script.contains("-a completions"));
        assert!(
            !script
                .lines()
                .any(|line| line.trim_start().starts_with('#'))
        );
    }
}
