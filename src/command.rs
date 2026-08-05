use std::{error::Error, fmt};

use crate::model::ModelRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    About,
    Clear,
    Exit,
    Model(ModelRole),
    /// `/provider` lists/shows providers (`None`); `/provider <name>` switches (`Some(name)`).
    Provider(Option<String>),
    /// `/models` lists the configured roles and their models.
    Models,
    /// `/discover` asks the active provider for its model ids.
    Discover,
    /// Spawn one independent child-agent request.
    Spawn(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCommandError {
    command: String,
}

impl fmt::Display for ParseCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unsupported command: {}", self.command)
    }
}

impl Error for ParseCommandError {}

/// Rich help text listing every supported slash command.
pub fn help_text() -> String {
    [
        "Commands:",
        "  /help              show this help",
        "  /about             about LUMINUS",
        "  /clear             clear the conversation",
        "  /exit              quit",
        "  /model <role>      switch the active model role",
        "  /models            list configured roles and their models",
        "  /discover          discover models from the active provider",
        "  /provider          list/show providers",
        "  /provider <name>   switch to the named provider",
        "  /spawn <prompt>    run a child agent on the active provider",
    ]
    .join("\n")
}

/// Parses one of the supported slash commands.
pub fn parse_command(input: &str) -> Result<Command, ParseCommandError> {
    match input {
        "/help" => Ok(Command::Help),
        "/about" => Ok(Command::About),
        "/clear" => Ok(Command::Clear),
        "/exit" => Ok(Command::Exit),
        "/provider" => Ok(Command::Provider(None)),
        "/models" => Ok(Command::Models),
        "/discover" => Ok(Command::Discover),
        command if command.starts_with("/spawn ") => {
            let prompt = command.strip_prefix("/spawn ").unwrap().trim();
            if prompt.is_empty() {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::Spawn(prompt.to_owned()))
        }
        command if command.starts_with("/provider ") => {
            let name = command.strip_prefix("/provider ").unwrap();
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::Provider(Some(name.to_ascii_lowercase())))
        }
        command if command.starts_with("/model ") => {
            let role = command.strip_prefix("/model ").unwrap();
            if role.is_empty() || role.contains(char::is_whitespace) {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            role.parse::<ModelRole>()
                .map(Command::Model)
                .map_err(|_| ParseCommandError {
                    command: command.to_owned(),
                })
        }
        command => Err(ParseCommandError {
            command: command.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, help_text, parse_command};

    #[test]
    fn parses_supported_commands() {
        assert_eq!(parse_command("/help"), Ok(Command::Help));
        assert_eq!(parse_command("/about"), Ok(Command::About));
        assert_eq!(parse_command("/clear"), Ok(Command::Clear));
        assert_eq!(parse_command("/exit"), Ok(Command::Exit));
    }

    #[test]
    fn rejects_everything_else() {
        assert!(parse_command("/model").is_err());
        assert!(parse_command("/spawn").is_err());
        assert!(parse_command("/spawn   ").is_err());
        assert!(parse_command("help").is_err());
        assert!(parse_command("/HELP").is_err());
        assert!(parse_command("/help now").is_err());
    }

    #[test]
    fn parses_spawn_prompt_and_help_mentions_it() {
        assert_eq!(
            parse_command("/spawn  inspect the architecture  "),
            Ok(Command::Spawn("inspect the architecture".into()))
        );
        assert!(help_text().contains("/spawn"));
    }

    #[test]
    fn help_text_mentions_every_command() {
        let text = help_text();
        for command in [
            "/help",
            "/about",
            "/clear",
            "/exit",
            "/model",
            "/models",
            "/provider",
        ] {
            assert!(text.contains(command), "help text missing {command}");
        }
    }
}
