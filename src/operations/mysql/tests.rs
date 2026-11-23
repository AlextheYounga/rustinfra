use super::*;
use crate::api::Command;

fn descriptions(commands: &[Command]) -> Vec<(String, String)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(command) => Some((command.raw_value(), command.masked_value())),
            _ => None,
        })
        .collect()
}

#[test]
fn sql_quotes_connection_values_and_masks_password() {
    let input = SqlInput {
        sql: "SELECT 1".to_owned(),
        database: None,
        connection: MysqlConnection {
            user: Some("some user".to_owned()),
            password: Some("somepass".to_owned()),
            host: Some("127.0.0.1".to_owned()),
            port: Some("100".to_owned()),
        },
    };
    let command = &descriptions(&Sql.commands(&input))[0];
    assert_eq!(command.0, "mysql -u'some user' -psomepass -h127.0.0.1 -P100 -Be 'SELECT 1'");
    assert!(command.1.contains("-p'*MASKED*'"));
}

#[test]
fn dump_and_load_match_reference_redirections() {
    let mut input = FileInput::new("some file");
    input.database = Some("somedb".to_owned());
    input.connection.user = Some("root".to_owned());
    assert_eq!(descriptions(&Dump.commands(&input))[0].0, "mysqldump somedb -uroot > 'some file'");
    assert_eq!(descriptions(&Load.commands(&input))[0].0, "mysql somedb -uroot < 'some file'");
}
