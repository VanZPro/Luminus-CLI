use luminus::{
    command::{Command, parse_command},
    model::ModelRole,
};

#[test]
fn parses_model_role_command() {
    assert_eq!(
        parse_command("/model fast"),
        Ok(Command::Model(ModelRole::Fast))
    );
    assert!(parse_command("/model unknown").is_err());
}
