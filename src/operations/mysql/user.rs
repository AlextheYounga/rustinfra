use std::collections::BTreeSet;
use std::error::Error;

use indexmap::IndexMap;

use crate::api::{Command, CommandBit, HiddenValue, StringCommand};
use crate::connectors::Connector;
use crate::facts::mysql::{
    MysqlConnection, MysqlUserGrants, MysqlUserGrantsInput, MysqlUserInfo, MysqlUsers, MysqlValue,
};
use crate::facts::{FactExecutor, FactOptions};

use super::privileges::{PrivilegesInput, privilege_commands};
use super::quoted_identifier;
use crate::operations::database::mysql_execute;
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Require {
    Ssl,
    X509,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserInput {
    pub user: String,
    pub present: bool,
    pub user_hostname: String,
    pub password: Option<String>,
    pub privileges: Option<BTreeSet<String>>,
    pub require: Option<Require>,
    pub require_cipher: Option<String>,
    pub require_issuer: Option<String>,
    pub require_subject: Option<String>,
    pub max_connections: Option<i64>,
    pub max_queries_per_hour: Option<i64>,
    pub max_updates_per_hour: Option<i64>,
    pub max_connections_per_hour: Option<i64>,
    pub connection: MysqlConnection,
}
impl UserInput {
    pub fn new(user: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            present: true,
            user_hostname: "localhost".to_owned(),
            password: None,
            privileges: None,
            require: None,
            require_cipher: None,
            require_issuer: None,
            require_subject: None,
            max_connections: None,
            max_queries_per_hour: None,
            max_updates_per_hour: None,
            max_connections_per_hour: None,
            connection: MysqlConnection::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct User;
impl StatefulOperation for User {
    type Input = UserInput;
    fn name(&self) -> &'static str {
        "mysql.user"
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
            validate(input)?;
            let facts = FactExecutor::new(connector, options);
            let users = facts.get(&MysqlUsers, &input.connection).await?;
            let mut commands = user_commands(input, &users)?;
            if input.present
                && let Some(privileges) = &input.privileges
            {
                let privilege_input = PrivilegesInput {
                    user: input.user.clone(),
                    privileges: privileges.clone(),
                    user_hostname: input.user_hostname.clone(),
                    database: "*".to_owned(),
                    table: "*".to_owned(),
                    flush: true,
                    with_grant_option: false,
                    connection: input.connection.clone(),
                };
                let grants_input = MysqlUserGrantsInput {
                    user: input.user.clone(),
                    hostname: input.user_hostname.clone(),
                    connection: input.connection.clone(),
                };
                let grants = facts.get(&MysqlUserGrants, &grants_input).await?;
                commands.extend(privilege_commands(&privilege_input, &grants)?);
            }
            Ok(commands)
        })
    }
}

fn user_commands(
    input: &UserInput,
    users: &IndexMap<String, MysqlUserInfo>,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    validate(input)?;
    let key = format!("{}@{}", input.user, input.user_hostname);
    let current = users.get(&key);
    if !input.present {
        return Ok(current.map_or_else(Vec::new, |_| {
            vec![execute(
                input,
                format!("DROP USER {}@{}", quoted_identifier(&input.user), quoted_identifier(&input.user_hostname))
                    .into(),
            )]
        }));
    }
    let Some(current) = current else {
        return Ok(vec![execute(input, create_query(input))]);
    };
    let mut clauses = require_clauses(input, Some(current));
    let resources = resource_clauses(input, Some(current));
    if !resources.is_empty() {
        clauses.push("WITH".to_owned());
        clauses.extend(resources);
    }
    if clauses.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!(
        "ALTER USER {}@{} {}",
        quoted_identifier(&input.user),
        quoted_identifier(&input.user_hostname),
        clauses.join(" ")
    );
    Ok(vec![execute(input, query.into())])
}

fn create_query(input: &UserInput) -> CommandBit {
    let mut bits = vec![CommandBit::from(format!(
        "CREATE USER {}@{}",
        quoted_identifier(&input.user),
        quoted_identifier(&input.user_hostname)
    ))];
    if let Some(password) = &input.password {
        let hidden = StringCommand::with_separator(
            [CommandBit::from("\""), HiddenValue::new(password.replace('"', "\"\"")).into(), CommandBit::from("\"")],
            "",
        );
        bits.push(StringCommand::new([CommandBit::from("IDENTIFIED BY"), hidden.into()]).into());
    }
    bits.extend(require_clauses(input, None).into_iter().map(CommandBit::from));
    let resources = resource_clauses(input, None);
    if !resources.is_empty() {
        bits.push(CommandBit::from("WITH"));
        bits.extend(resources.into_iter().map(CommandBit::from));
    }
    StringCommand::new(bits).into()
}

fn require_clauses(input: &UserInput, current: Option<&MysqlUserInfo>) -> Vec<String> {
    match input.require {
        Some(Require::Ssl) if current.is_none_or(|info| text(info, "ssl_type") != Some("ANY")) => {
            vec!["REQUIRE SSL".to_owned()]
        }
        Some(Require::X509) => {
            let mut clauses = Vec::new();
            for (keyword, desired, key) in [
                ("CIPHER", &input.require_cipher, "ssl_cipher"),
                ("ISSUER", &input.require_issuer, "x509_issuer"),
                ("SUBJECT", &input.require_subject, "x509_subject"),
            ] {
                if let Some(desired) = desired
                    && current.is_none_or(|info| text(info, key) != Some(desired))
                {
                    clauses.push(format!("{keyword} {}", quoted_identifier(desired)));
                }
            }
            if clauses.is_empty() && current.is_none_or(|info| text(info, "ssl_type") != Some("X509")) {
                clauses.push("X509".to_owned());
            }
            if clauses.is_empty() {
                clauses
            } else {
                let mut result = vec!["REQUIRE".to_owned()];
                result.extend(clauses);
                result
            }
        }
        _ => Vec::new(),
    }
}

fn resource_clauses(input: &UserInput, current: Option<&MysqlUserInfo>) -> Vec<String> {
    [
        ("MAX_USER_CONNECTIONS", input.max_connections, "max_user_connections"),
        ("MAX_QUERIES_PER_HOUR", input.max_queries_per_hour, "max_questions"),
        ("MAX_UPDATES_PER_HOUR", input.max_updates_per_hour, "max_updates"),
        ("MAX_CONNECTIONS_PER_HOUR", input.max_connections_per_hour, "max_connections"),
    ]
    .into_iter()
    .filter_map(|(keyword, desired, key)| {
        desired
            .filter(|desired| current.is_none_or(|info| integer(info, key) != Some(*desired)))
            .map(|value| format!("{keyword} {value}"))
    })
    .collect()
}

fn validate(input: &UserInput) -> Result<(), Box<dyn Error + Send + Sync>> {
    if input.require != Some(Require::X509) {
        if input.require_cipher.is_some() {
            return Err("Cannot set `require_cipher` if `require` is not \"X509\"".into());
        }
        if input.require_issuer.is_some() {
            return Err("Cannot set `require_issuer` if `require` is not \"X509\"".into());
        }
        if input.require_subject.is_some() {
            return Err("Cannot set `require_subject` if `require` is not \"X509\"".into());
        }
    }
    Ok(())
}
fn execute(input: &UserInput, query: CommandBit) -> Command {
    mysql_execute(&input.connection, None, query)
}
fn text<'a>(info: &'a MysqlUserInfo, key: &str) -> Option<&'a str> {
    match info.details.get(key) {
        Some(MysqlValue::Text(value)) => Some(value),
        _ => None,
    }
}
fn integer(info: &MysqlUserInfo, key: &str) -> Option<i64> {
    match info.details.get(key) {
        Some(MysqlValue::Integer(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw(commands: &[Command]) -> Vec<(String, String)> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some((command.raw_value(), command.masked_value())),
                _ => None,
            })
            .collect()
    }
    fn info(entries: impl IntoIterator<Item = (&'static str, MysqlValue)>) -> MysqlUserInfo {
        MysqlUserInfo {
            details: entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect(),
            privileges: Vec::new(),
        }
    }

    #[test]
    fn creates_user_with_masked_password_tls_and_resources() {
        let mut input = UserInput::new("someuser");
        input.password = Some("mypass".to_owned());
        input.require = Some(Require::X509);
        input.require_issuer = Some("issuer".to_owned());
        input.max_connections = Some(50);
        let command = &raw(&user_commands(&input, &IndexMap::new()).unwrap())[0];
        assert!(command.0.contains("IDENTIFIED BY \"mypass\" REQUIRE ISSUER \"issuer\" WITH MAX_USER_CONNECTIONS 50"));
        assert!(command.1.contains("IDENTIFIED BY \"*MASKED*\""));
    }

    #[test]
    fn updates_only_changed_tls_and_resource_values() {
        let mut input = UserInput::new("someuser");
        input.require = Some(Require::X509);
        input.require_cipher = Some("cipher".to_owned());
        input.require_issuer = Some("issuer".to_owned());
        input.max_connections = Some(50);
        let current = info([
            ("ssl_cipher", MysqlValue::Text("cipher".to_owned())),
            ("x509_issuer", MysqlValue::Null),
            ("max_user_connections", MysqlValue::Integer(5)),
        ]);
        let users = [("someuser@localhost".to_owned(), current)].into_iter().collect();
        assert_eq!(
            raw(&user_commands(&input, &users).unwrap())[0].0,
            "mysql -Be 'ALTER USER \"someuser\"@\"localhost\" REQUIRE ISSUER \"issuer\" WITH MAX_USER_CONNECTIONS 50'"
        );
    }

    #[test]
    fn rejects_x509_details_without_x509_mode() {
        let mut input = UserInput::new("someuser");
        input.require_cipher = Some("cipher".to_owned());
        assert!(user_commands(&input, &IndexMap::new()).is_err());
    }
}
