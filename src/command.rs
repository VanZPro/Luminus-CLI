use std::{error::Error, fmt};

use crate::model::ModelRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,
    About,
    Clear,
    Exit,
    Model(ModelRole),
    Provider,
    Models,
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

/// Parses one of the commands supported by Milestone 1.
pub fn parse_command(input: &str) -> Result<Command, ParseCommandError> {
    match input {
        "/help" => Ok(Command::Help),
        "/about" => Ok(Command::About),
        "/clear" => Ok(Command::Clear),
        "/exit" => Ok(Command::Exit),
        "/provider" => Ok(Command::Provider),
        "/models" => Ok(Command::Models),
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
    use super::{Command, parse_command};

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
        assert!(parse_command("help").is_err());
        assert!(parse_command("/HELP").is_err());
        assert!(parse_command("/help now").is_err());
    }
}
