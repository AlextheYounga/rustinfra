use std::collections::BTreeSet;
use std::error::Error;

use indexmap::IndexMap;

use crate::api::{Command, CommandBit};
use crate::connectors::Connector;
use crate::facts::mysql::{MysqlConnection, MysqlDatabaseInfo, MysqlDatabases, MysqlUserGrants, MysqlUserGrantsInput};
use crate::facts::{FactExecutor, FactOptions};

use super::{quoted_identifier, quoted_mysql_name};
use crate::operations::database::mysql_execute;
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivilegesInput {
    pub user: String,
    pub privileges: BTreeSet<String>,
    pub user_hostname: String,
    pub database: String,
    pub table: String,
    pub flush: bool,
    pub with_grant_option: bool,
    pub connection: MysqlConnection,
}

impl PrivilegesInput {
    pub fn new(user: impl Into<String>, privileges: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            user: user.into(),
            privileges: privileges.into_iter().map(Into::into).collect(),
            user_hostname: "localhost".to_owned(),
            database: "*".to_owned(),
            table: "*".to_owned(),
            flush: true,
            with_grant_option: false,
            connection: MysqlConnection::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Privileges;
impl StatefulOperation for Privileges {
    type Input = PrivilegesInput;
    fn name(&self) -> &'static str {
        "mysql.privileges"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            validate_target(input)?;
            let grants_input = MysqlUserGrantsInput {
                user: input.user.clone(),
                hostname: input.user_hostname.clone(),
                connection: input.connection.clone(),
            };
            let grants = FactExecutor::new(connector, options).get(&MysqlUserGrants, &grants_input).await?;
            privilege_commands(input, &grants)
        })
    }
}

pub(super) fn privilege_commands(
    input: &PrivilegesInput,
    grants: &IndexMap<String, Vec<String>>,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    validate_target(input)?;
    let target = target(input);
    let existing = grants
        .get(&target)
        .into_iter()
        .flatten()
        .map(|privilege| if privilege == "ALL PRIVILEGES" { "ALL".to_owned() } else { privilege.clone() })
        .collect::<BTreeSet<_>>();
    let mut desired = input.privileges.clone();
    if input.with_grant_option {
        desired.insert("GRANT OPTION".to_owned());
    }
    let mut revoke = existing.difference(&desired).cloned().collect::<BTreeSet<_>>();
    let grant = desired.difference(&existing).cloned().collect::<BTreeSet<_>>();
    if !grant.is_empty() {
        revoke.remove("USAGE");
    }

    let mut commands = Vec::new();
    if revoke == BTreeSet::from(["ALL".to_owned(), "GRANT OPTION".to_owned()]) {
        commands.push(privilege_command(input, PrivilegeAction::Revoke, ["GRANT OPTION"], None));
        commands.push(privilege_command(input, PrivilegeAction::Revoke, ["ALL PRIVILEGES"], None));
    } else if !revoke.is_empty() {
        commands.push(privilege_command(input, PrivilegeAction::Revoke, revoke.iter().map(String::as_str), None));
    }
    if grant == BTreeSet::from(["ALL".to_owned(), "GRANT OPTION".to_owned()]) {
        commands.push(privilege_command(input, PrivilegeAction::Grant, ["ALL PRIVILEGES"], Some("GRANT OPTION")));
    } else if !grant.is_empty() {
        commands.push(privilege_command(input, PrivilegeAction::Grant, grant.iter().map(String::as_str), None));
    }
    if input.flush && (!grant.is_empty() || !revoke.is_empty()) {
        commands.push(mysql_execute(&input.connection, None, CommandBit::from("FLUSH PRIVILEGES")));
    }
    Ok(commands)
}

#[derive(Clone, Copy)]
enum PrivilegeAction {
    Grant,
    Revoke,
}

fn privilege_command<'a>(
    input: &PrivilegesInput,
    action: PrivilegeAction,
    privileges: impl IntoIterator<Item = &'a str>,
    with_statement: Option<&str>,
) -> Command {
    let privileges = privileges.into_iter().collect::<Vec<_>>().join(", ");
    let (action, direction) = match action {
        PrivilegeAction::Grant => ("GRANT", "TO"),
        PrivilegeAction::Revoke => ("REVOKE", "FROM"),
    };
    let mut query = format!(
        "{action} {privileges} ON {} {direction} {}@{}",
        target(input),
        quoted_identifier(&input.user),
        quoted_identifier(&input.user_hostname),
    );
    if let Some(statement) = with_statement {
        query.push_str(&format!(" WITH {statement}"));
    }
    mysql_execute(&input.connection, None, CommandBit::from(query))
}

fn validate_target(input: &PrivilegesInput) -> Result<(), Box<dyn Error + Send + Sync>> {
    if input.database == "*" && input.table != "*" {
        Err(format!("Cannot apply MySQL privileges on *.{}, no database provided", quoted_mysql_name(&input.table))
            .into())
    } else {
        Ok(())
    }
}

fn target(input: &PrivilegesInput) -> String {
    let database = if input.database == "*" { "*".to_owned() } else { quoted_mysql_name(&input.database) };
    let table = if input.table == "*" { "*".to_owned() } else { quoted_mysql_name(&input.table) };
    format!("{database}.{table}")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseInput {
    pub database: String,
    pub present: bool,
    pub collate: Option<String>,
    pub charset: Option<String>,
    pub user: Option<String>,
    pub user_hostname: String,
    pub user_privileges: BTreeSet<String>,
    pub connection: MysqlConnection,
}
impl DatabaseInput {
    pub fn new(database: impl Into<String>) -> Self {
        Self {
            database: database.into(),
            present: true,
            collate: None,
            charset: None,
            user: None,
            user_hostname: "localhost".to_owned(),
            user_privileges: BTreeSet::from(["ALL".to_owned()]),
            connection: MysqlConnection::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Database;
impl StatefulOperation for Database {
    type Input = DatabaseInput;
    fn name(&self) -> &'static str {
        "mysql.database"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, options);
            let databases = facts.get(&MysqlDatabases, &input.connection).await?;
            let mut commands = database_commands(input, &databases);
            if input.present
                && let Some(user) = &input.user
                && !input.user_privileges.is_empty()
            {
                let privileges = PrivilegesInput {
                    user: user.clone(),
                    privileges: input.user_privileges.clone(),
                    user_hostname: input.user_hostname.clone(),
                    database: input.database.clone(),
                    table: "*".to_owned(),
                    flush: true,
                    with_grant_option: false,
                    connection: input.connection.clone(),
                };
                let grants_input = MysqlUserGrantsInput {
                    user: user.clone(),
                    hostname: input.user_hostname.clone(),
                    connection: input.connection.clone(),
                };
                let grants = facts.get(&MysqlUserGrants, &grants_input).await?;
                commands.extend(privilege_commands(&privileges, &grants)?);
            }
            Ok(commands)
        })
    }
}

fn database_commands(input: &DatabaseInput, databases: &IndexMap<String, MysqlDatabaseInfo>) -> Vec<Command> {
    let is_present = databases.contains_key(&input.database);
    if !input.present {
        return if is_present {
            vec![mysql_execute(
                &input.connection,
                None,
                format!("DROP DATABASE {}", quoted_mysql_name(&input.database)).into(),
            )]
        } else {
            Vec::new()
        };
    }
    if is_present {
        return Vec::new();
    }
    let mut query = format!("CREATE DATABASE {}", quoted_mysql_name(&input.database));
    if let Some(collate) = &input.collate {
        query.push_str(&format!(" COLLATE {collate}"));
    }
    if let Some(charset) = &input.charset {
        query.push_str(&format!(" CHARSET {charset}"));
    }
    vec![mysql_execute(&input.connection, None, query.into())]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn grants_and_revokes_differences_in_sorted_order() {
        let input = PrivilegesInput::new("someuser", ["SELECT", "INSERT", "DELETE"]);
        let grants = [("*.*".to_owned(), vec!["DELETE".to_owned(), "UPDATE".to_owned()])].into_iter().collect();
        assert_eq!(
            raw(&privilege_commands(&input, &grants).unwrap()),
            [
                "mysql -Be 'REVOKE UPDATE ON *.* FROM \"someuser\"@\"localhost\"'",
                "mysql -Be 'GRANT INSERT, SELECT ON *.* TO \"someuser\"@\"localhost\"'",
                "mysql -Be 'FLUSH PRIVILEGES'",
            ]
        );
    }

    #[test]
    fn all_and_grant_option_use_special_mysql_syntax() {
        let mut input = PrivilegesInput::new("someuser", ["ALL"]);
        input.with_grant_option = true;
        assert!(
            raw(&privilege_commands(&input, &IndexMap::new()).unwrap())[0]
                .contains("GRANT ALL PRIVILEGES ON *.* TO \"someuser\"@\"localhost\" WITH GRANT OPTION")
        );
    }

    #[test]
    fn creates_and_drops_database_with_reference_options() {
        let mut input = DatabaseInput::new("some`db");
        input.collate = Some("utf8_bin".to_owned());
        input.charset = Some("UTF-8".to_owned());
        assert_eq!(
            raw(&database_commands(&input, &IndexMap::new())),
            ["mysql -Be 'CREATE DATABASE `some``db` COLLATE utf8_bin CHARSET UTF-8'"]
        );
        input.present = false;
        let databases = [(
            input.database.clone(),
            MysqlDatabaseInfo { character_set: String::new(), collation_name: String::new() },
        )]
        .into_iter()
        .collect();
        assert_eq!(raw(&database_commands(&input, &databases)), ["mysql -Be 'DROP DATABASE `some``db`'"]);
    }
}
