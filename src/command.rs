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
    /// Save the current conversation under a name.
    Save(String),
    /// List saved conversations.
    Sessions,
    /// Load a saved conversation by name.
    Load(String),
    /// /tools lists the available tools.
    Tools,
    /// /tool <name> <args...> runs an approved coding tool.
    Tool(String, Vec<String>),
    /// Spawn one independent child-agent request.
    Spawn(String),
    /// /diff opens the diff viewer overlay for current file edit history.
    Diff,
    /// /changes lists modified paths with +lines / -lines.
    Changes,
    /// /undo reverts the most recent file edit.
    Undo,
    /// /redo reapplies the most recently undone file edit.
    Redo,
    /// /revert-file <path> reverts a specific file to its initial state.
    RevertFile(String),
    /// /skills, /skills list — list available skills.
    Skills,
    /// /skills inspect <name> — show detailed skill info.
    SkillInspect(String),
    /// /skill <name> — activate and inject a skill.
    SkillUse(String),
    /// /env <key> <val> — write an env var to .env
    Env(String, String),
    /// /mcp — list mcp server connection status
    McpList,
    /// /mcp connect — connect to all configured mcp servers
    McpConnect,
    /// /context — inspect loaded project instructions and context files
    Context,
    /// /memory — inspect or manage memory
    Memory(Option<String>),
    /// /missions — list or manage long-running tasks
    Missions,
    /// /init — initialize project instructions file (.luminus/instructions.md)
    Init,
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
        "  /save <name>       save the current conversation",
        "  /sessions          list saved conversations",
        "  /load <name>       restore a saved conversation",
        "  /tools             list available coding tools",
        "  /tool <name> <args...> prepare a tool for approval",
        "  /provider          list/show providers",
        "  /provider <name>   switch to the named provider",
        "  /spawn <prompt>    run a child agent on the active provider",
        "  /diff              open the diff viewer overlay",
        "  /changes           list modified files with +lines / -lines",
        "  /undo              undo the most recent file edit",
        "  /redo              redo the most recently undone file edit",
        "  /revert-file <path> revert a file to its initial state",
        "  /skills            list available skills",
        "  /skills inspect <name> show detailed skill info",
        "  /skill <name>      activate and inject a skill",
        "  /env <key> <val>   write environment variable to .env",
        "  /mcp               list MCP server status",
        "  /context           inspect project context files",
        "  /memory [args]     manage soul memory",
        "  /missions          list long-running tasks",
        "  /init              initialize project instructions",
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
        "/sessions" => Ok(Command::Sessions),
        "/tools" => Ok(Command::Tools),
        "/diff" => Ok(Command::Diff),
        "/changes" => Ok(Command::Changes),
        "/undo" => Ok(Command::Undo),
        "/redo" => Ok(Command::Redo),
        command if command.starts_with("/save ") => {
            parse_named(command, "/save ").map(Command::Save)
        }
        command if command.starts_with("/load ") => {
            parse_named(command, "/load ").map(Command::Load)
        }
        command if command.starts_with("/tool ") => parse_tool_command(command),
        command if command.starts_with("/spawn ") => {
            let prompt = command.strip_prefix("/spawn ").unwrap().trim();
            if prompt.is_empty() {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::Spawn(prompt.to_owned()))
        }
        command if command.starts_with("/revert-file ") => {
            let path = command.strip_prefix("/revert-file ").unwrap().trim();
            if path.is_empty() {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::RevertFile(path.to_owned()))
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
        "/skills" | "/skills list" => Ok(Command::Skills),
        command if command.starts_with("/skills inspect ") => {
            let name = command.strip_prefix("/skills inspect ").unwrap().trim();
            if name.is_empty() || name.contains('/') || name.contains('\\') {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::SkillInspect(name.to_owned()))
        }
        command if command.starts_with("/skill ") => {
            let name = command.strip_prefix("/skill ").unwrap().trim();
            if name.is_empty() || name.contains('/') || name.contains('\\') {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::SkillUse(name.to_owned()))
        }
        command if command.starts_with("/env ") => {
            let parts: Vec<&str> = command
                .strip_prefix("/env ")
                .unwrap()
                .trim()
                .splitn(2, ' ')
                .collect();
            if parts.len() != 2 {
                return Err(ParseCommandError {
                    command: command.to_owned(),
                });
            }
            Ok(Command::Env(parts[0].to_owned(), parts[1].to_owned()))
        }
        "/mcp" => Ok(Command::McpList),
        "/mcp connect" => Ok(Command::McpConnect),
        "/context" => Ok(Command::Context),
        "/memory" => Ok(Command::Memory(None)),
        command if command.starts_with("/memory ") => {
            let arg = command.strip_prefix("/memory ").unwrap().trim();
            Ok(Command::Memory(Some(arg.to_owned())))
        }
        "/missions" => Ok(Command::Missions),
        "/init" => Ok(Command::Init),
        command => Err(ParseCommandError {
            command: command.to_owned(),
        }),
    }
}

fn parse_named(command: &str, prefix: &str) -> Result<String, ParseCommandError> {
    let value = command.strip_prefix(prefix).unwrap_or_default().trim();
    if value.is_empty() || value.contains('/') || value.contains('\\') {
        Err(ParseCommandError {
            command: command.to_owned(),
        })
    } else {
        Ok(value.to_owned())
    }
}

fn parse_tool_command(command: &str) -> Result<Command, ParseCommandError> {
    let rest = command.strip_prefix("/tool ").unwrap_or_default().trim();
    let mut parts = rest.split_whitespace();
    let Some(name) = parts.next() else {
        return Err(ParseCommandError {
            command: command.to_owned(),
        });
    };
    if name.contains('/') || name.contains('\\') {
        return Err(ParseCommandError {
            command: command.to_owned(),
        });
    }
    let args = parts.map(ToOwned::to_owned).collect::<Vec<_>>();
    Ok(Command::Tool(name.to_owned(), args))
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
    fn parses_session_commands_and_rejects_path_traversal() {
        assert_eq!(
            parse_command("/save morning"),
            Ok(Command::Save("morning".into()))
        );
        assert_eq!(
            parse_command("/load morning"),
            Ok(Command::Load("morning".into()))
        );
        assert_eq!(parse_command("/sessions"), Ok(Command::Sessions));
        assert!(parse_command("/save ../secret").is_err());
        assert!(parse_command("/load ").is_err());
    }

    #[test]
    fn parses_tool_commands_and_supports_zero_args() {
        assert_eq!(parse_command("/tools"), Ok(Command::Tools));
        assert_eq!(
            parse_command("/tool read_file README.md"),
            Ok(Command::Tool("read_file".into(), vec!["README.md".into()]))
        );
        assert_eq!(
            parse_command("/tool list_dir ."),
            Ok(Command::Tool("list_dir".into(), vec![".".into()]))
        );
        assert_eq!(
            parse_command("/tool write_file a.txt hello"),
            Ok(Command::Tool(
                "write_file".into(),
                vec!["a.txt".into(), "hello".into()]
            ))
        );
        assert!(parse_command("/tool ").is_err());
        assert!(parse_command("/tool").is_err());
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
            "/discover",
            "/save",
            "/sessions",
            "/load",
            "/tools",
            "/tool",
            "/provider",
            "/spawn",
            "/diff",
            "/changes",
            "/undo",
            "/redo",
            "/revert-file",
            "/skills",
            "/skill",
        ] {
            assert!(text.contains(command), "help text missing {command}");
        }
    }

    #[test]
    fn parses_skills_commands() {
        assert_eq!(parse_command("/skills"), Ok(Command::Skills));
        assert_eq!(parse_command("/skills list"), Ok(Command::Skills));
        assert_eq!(
            parse_command("/skills inspect fix-tests"),
            Ok(Command::SkillInspect("fix-tests".into()))
        );
        assert_eq!(
            parse_command("/skill code-review"),
            Ok(Command::SkillUse("code-review".into()))
        );
        // Empty names rejected
        assert!(parse_command("/skills inspect ").is_err());
        assert!(parse_command("/skill ").is_err());
        // Path traversal rejected
        assert!(parse_command("/skills inspect ../secret").is_err());
        assert!(parse_command("/skill /etc/passwd").is_err());
    }
}
