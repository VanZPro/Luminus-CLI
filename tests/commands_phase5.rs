use luminus::{
    command::{Command, help_text, parse_command},
    model::ModelRole,
};

#[test]
fn parses_bare_provider_as_list() {
    assert_eq!(parse_command("/provider"), Ok(Command::Provider(None)));
}

#[test]
fn parses_provider_with_name_as_switch() {
    assert_eq!(
        parse_command("/provider openai"),
        Ok(Command::Provider(Some("openai".into())))
    );
    assert_eq!(
        parse_command("/provider anthropic"),
        Ok(Command::Provider(Some("anthropic".into())))
    );
}

#[test]
fn provider_name_is_normalized_to_lowercase() {
    assert_eq!(
        parse_command("/provider OpenAI"),
        Ok(Command::Provider(Some("openai".into())))
    );
}

#[test]
fn parses_models_command() {
    assert_eq!(parse_command("/models"), Ok(Command::Models));
}

#[test]
fn command_names_remain_case_sensitive() {
    assert!(parse_command("/PROVIDER").is_err());
    assert!(parse_command("/Models").is_err());
    assert!(parse_command("/MODELS").is_err());
}

#[test]
fn rejects_malformed_provider_invocations() {
    assert!(parse_command("/provider ").is_err());
    assert!(parse_command("/provider two words").is_err());
    assert!(parse_command("/providers").is_err());
}

#[test]
fn rejects_junk_and_unknown_commands() {
    assert!(parse_command("/frobnicate").is_err());
    assert!(parse_command("/models extra").is_err());
    assert!(parse_command("models").is_err());
    assert!(parse_command("provider").is_err());
    assert!(parse_command("").is_err());
    assert!(parse_command("/").is_err());
}

#[test]
fn existing_commands_still_parse() {
    assert_eq!(parse_command("/help"), Ok(Command::Help));
    assert_eq!(parse_command("/about"), Ok(Command::About));
    assert_eq!(parse_command("/clear"), Ok(Command::Clear));
    assert_eq!(parse_command("/exit"), Ok(Command::Exit));
    assert_eq!(
        parse_command("/model deep"),
        Ok(Command::Model(ModelRole::Deep))
    );
}

#[test]
fn help_text_lists_new_commands() {
    let text = help_text();
    assert!(text.contains("/provider"));
    assert!(text.contains("/models"));
    assert!(text.contains("/model <role>"));
}
