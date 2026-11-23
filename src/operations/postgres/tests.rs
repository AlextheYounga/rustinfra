use super::*;
use crate::api::Command;

fn values(entries: impl IntoIterator<Item = (&'static str, PostgresqlValue)>) -> IndexMap<String, PostgresqlValue> {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}
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
fn role_matches_create_update_remove_and_masks_password() {
    let mut input = RoleInput::new("testuser");
    input.connection_limit = Some(15);
    input.password = Some("abc".to_owned());
    let created = descriptions(&role_commands(&input, &IndexMap::new()));
    assert_eq!(
        created[0].0,
        "psql -Ac 'CREATE ROLE \"testuser\" LOGIN CONNECTION LIMIT 15 PASSWORD '\"'\"'abc'\"'\"''"
    );
    assert!(created[0].1.contains("*MASKED*"));

    let roles = [("testuser".to_owned(), values([("connlimit", PostgresqlValue::Integer(10))]))].into_iter().collect();
    assert!(descriptions(&role_commands(&input, &roles))[0].0.contains("ALTER ROLE"));
    input.present = false;
    assert_eq!(descriptions(&role_commands(&input, &roles))[0].0, "psql -Ac 'DROP ROLE \"testuser\"'");
}

#[test]
fn database_matches_create_update_and_noop_cases() {
    let mut input = DatabaseInput::new("somedb");
    input.owner = Some("someowner".to_owned());
    assert_eq!(
        descriptions(&database_commands(&input, &IndexMap::new()))[0].0,
        "psql -Ac 'CREATE DATABASE \"somedb\" OWNER \"someowner\"'"
    );
    let databases = [(
        "somedb".to_owned(),
        values([
            ("owner", PostgresqlValue::Text("old".to_owned())),
            ("tablespace", PostgresqlValue::Text("oldspace".to_owned())),
        ]),
    )]
    .into_iter()
    .collect();
    input.tablespace = Some("newspace".to_owned());
    assert_eq!(
        descriptions(&database_commands(&input, &databases))[0].0,
        "psql -Ac 'ALTER DATABASE \"somedb\" OWNER TO \"someowner\"; ALTER DATABASE \"somedb\" SET TABLESPACE \"newspace\";'"
    );
}

#[test]
fn connection_options_redirections_and_sql_are_quoted() {
    let connection = PostgresqlConnection {
        user: Some("user name".to_owned()),
        password: Some("secret".to_owned()),
        host: Some("host;bad".to_owned()),
        port: Some("5432".to_owned()),
        database: Some("the db".to_owned()),
    };
    let sql = SqlInput { sql: "SELECT 1".to_owned(), connection: connection.clone() };
    let command = descriptions(&Sql.commands(&sql))[0].clone();
    assert!(command.0.starts_with("PGPASSWORD=secret psql -d 'the db' -U 'user name' -h 'host;bad' -p 5432 -Ac"));
    assert!(command.1.starts_with("PGPASSWORD='*MASKED*'"));
    let dump = FileInput { path: "some file".to_owned(), connection };
    assert!(
        descriptions(&Dump.commands(&dump))[0]
            .0
            .ends_with("pg_dump -d 'the db' -U 'user name' -h 'host;bad' -p 5432 > 'some file'")
    );
}
