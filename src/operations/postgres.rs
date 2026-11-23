use indexmap::IndexMap;

use crate::api::{Command, CommandBit, HiddenValue, StringCommand};
use crate::connectors::Connector;
use crate::facts::postgresql::{PostgresqlConnection, PostgresqlDatabases, PostgresqlRoles, PostgresqlValue};
use crate::facts::{FactExecutor, FactOptions};

use super::database::{postgres_execute, postgres_redirect};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlInput {
    pub sql: String,
    pub connection: PostgresqlConnection,
}

impl SqlInput {
    pub fn new(sql: impl Into<String>) -> Self {
        Self { sql: sql.into(), connection: PostgresqlConnection::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sql;

impl Operation for Sql {
    type Input = SqlInput;
    fn name(&self) -> &'static str {
        "postgres.sql"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![postgres_execute(&input.connection, input.sql.clone().into())]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleInput {
    pub role: String,
    pub present: bool,
    pub password: Option<String>,
    pub login: bool,
    pub superuser: bool,
    pub inherit: bool,
    pub createdb: bool,
    pub createrole: bool,
    pub replication: bool,
    pub connection_limit: Option<i64>,
    pub connection: PostgresqlConnection,
}

impl RoleInput {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            present: true,
            password: None,
            login: true,
            superuser: false,
            inherit: false,
            createdb: false,
            createrole: false,
            replication: false,
            connection_limit: None,
            connection: PostgresqlConnection::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Role;

impl StatefulOperation for Role {
    type Input = RoleInput;
    fn name(&self) -> &'static str {
        "postgres.role"
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
            let roles = FactExecutor::new(connector, options).get(&PostgresqlRoles, &input.connection).await?;
            Ok(role_commands(input, &roles))
        })
    }
}

fn role_commands(input: &RoleInput, roles: &IndexMap<String, IndexMap<String, PostgresqlValue>>) -> Vec<Command> {
    let current = roles.get(&input.role);
    if !input.present {
        return current.map_or_else(Vec::new, |_| {
            vec![postgres_execute(&input.connection, format!("DROP ROLE {}", identifier(&input.role)).into())]
        });
    }
    let Some(current) = current else {
        return vec![postgres_execute(&input.connection, role_create_query(input))];
    };
    let mut clauses = Vec::new();
    for (enabled, name, keys) in [
        (input.login, "LOGIN", &["login", "canlogin"][..]),
        (input.superuser, "SUPERUSER", &["superuser", "super"][..]),
        (input.inherit, "INHERIT", &["inherit"][..]),
        (input.createdb, "CREATEDB", &["createdb"][..]),
        (input.createrole, "CREATEROLE", &["createrole"][..]),
        (input.replication, "REPLICATION", &["replication"][..]),
    ] {
        if enabled && boolean_value(current, keys).is_some_and(|value| !value) {
            clauses.push(CommandBit::from(name));
        }
    }
    if let Some(limit) = input.connection_limit
        && integer_value(current, &["connection_limit", "connlimit"]).is_some_and(|value| value != limit)
    {
        clauses.push(CommandBit::from(format!("CONNECTION LIMIT {limit}")));
    }
    if let Some(password) = &input.password {
        clauses.push(password_clause(password));
    }
    if clauses.is_empty() {
        return Vec::new();
    }
    let mut bits = vec![CommandBit::from(format!("ALTER ROLE {}", identifier(&input.role)))];
    bits.extend(clauses);
    vec![postgres_execute(&input.connection, StringCommand::new(bits).into())]
}

fn role_create_query(input: &RoleInput) -> CommandBit {
    let mut bits = vec![CommandBit::from(format!("CREATE ROLE {}", identifier(&input.role)))];
    for (enabled, name) in [
        (input.login, "LOGIN"),
        (input.superuser, "SUPERUSER"),
        (input.inherit, "INHERIT"),
        (input.createdb, "CREATEDB"),
        (input.createrole, "CREATEROLE"),
        (input.replication, "REPLICATION"),
    ] {
        if enabled {
            bits.push(CommandBit::from(name));
        }
    }
    if let Some(limit) = input.connection_limit {
        bits.push(CommandBit::from(format!("CONNECTION LIMIT {limit}")));
    }
    if let Some(password) = &input.password {
        bits.push(password_clause(password));
    }
    StringCommand::new(bits).into()
}

fn password_clause(password: &str) -> CommandBit {
    let quoted = StringCommand::with_separator(
        [
            CommandBit::from("'"),
            CommandBit::from(HiddenValue::new(password.replace('\'', "''"))),
            CommandBit::from("'"),
        ],
        "",
    );
    StringCommand::new([CommandBit::from("PASSWORD"), quoted.into()]).into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseInput {
    pub database: String,
    pub present: bool,
    pub owner: Option<String>,
    pub template: Option<String>,
    pub encoding: Option<String>,
    pub lc_collate: Option<String>,
    pub lc_ctype: Option<String>,
    pub tablespace: Option<String>,
    pub connection_limit: Option<i64>,
    pub connection: PostgresqlConnection,
}

impl DatabaseInput {
    pub fn new(database: impl Into<String>) -> Self {
        Self {
            database: database.into(),
            present: true,
            owner: None,
            template: None,
            encoding: None,
            lc_collate: None,
            lc_ctype: None,
            tablespace: None,
            connection_limit: None,
            connection: PostgresqlConnection::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Database;

impl StatefulOperation for Database {
    type Input = DatabaseInput;
    fn name(&self) -> &'static str {
        "postgres.database"
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
            let databases = FactExecutor::new(connector, options).get(&PostgresqlDatabases, &input.connection).await?;
            Ok(database_commands(input, &databases))
        })
    }
}

fn database_commands(
    input: &DatabaseInput,
    databases: &IndexMap<String, IndexMap<String, PostgresqlValue>>,
) -> Vec<Command> {
    let current = databases.get(&input.database);
    if !input.present {
        return current.map_or_else(Vec::new, |_| {
            vec![postgres_execute(&input.connection, format!("DROP DATABASE {}", identifier(&input.database)).into())]
        });
    }
    let Some(current) = current else {
        return vec![postgres_execute(&input.connection, database_create_query(input).into())];
    };
    let mut statements = Vec::new();
    if input.owner.as_ref().is_some_and(|owner| text_value(current, "owner").is_some_and(|value| value != owner)) {
        statements.push(format!(
            "ALTER DATABASE {} OWNER TO {};",
            identifier(&input.database),
            identifier(input.owner.as_deref().unwrap_or_default())
        ));
    }
    if input
        .tablespace
        .as_ref()
        .is_some_and(|tablespace| text_value(current, "tablespace").is_some_and(|value| value != tablespace))
    {
        statements.push(format!(
            "ALTER DATABASE {} SET TABLESPACE {};",
            identifier(&input.database),
            identifier(input.tablespace.as_deref().unwrap_or_default())
        ));
    }
    if let Some(limit) = input.connection_limit
        && integer_value(current, &["connection_limit", "connlimit"]).is_some_and(|value| value != limit)
    {
        statements.push(format!("ALTER DATABASE {} CONNECTION LIMIT {limit};", identifier(&input.database)));
    }
    if statements.is_empty() {
        Vec::new()
    } else {
        vec![postgres_execute(&input.connection, statements.join(" ").into())]
    }
}

fn database_create_query(input: &DatabaseInput) -> String {
    let mut bits = vec![format!("CREATE DATABASE {}", identifier(&input.database))];
    if let Some(value) = &input.owner {
        bits.push(format!("OWNER {}", identifier(value)));
    }
    if let Some(value) = &input.template {
        bits.push(format!("TEMPLATE {value}"));
    }
    if let Some(value) = &input.encoding {
        bits.push(format!("ENCODING {value}"));
    }
    if let Some(value) = &input.lc_collate {
        bits.push(format!("LC_COLLATE {}", literal(value)));
    }
    if let Some(value) = &input.lc_ctype {
        bits.push(format!("LC_CTYPE {}", literal(value)));
    }
    if let Some(value) = &input.tablespace {
        bits.push(format!("TABLESPACE {value}"));
    }
    if let Some(value) = input.connection_limit {
        bits.push(format!("CONNECTION LIMIT {value}"));
    }
    bits.join(" ")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInput {
    pub path: String,
    pub connection: PostgresqlConnection,
}
impl FileInput {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), connection: PostgresqlConnection::default() }
    }
}

macro_rules! file_operation {
    ($name:ident, $operation_name:literal, $executable:literal, $operator:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Operation for $name {
            type Input = FileInput;
            fn name(&self) -> &'static str {
                $operation_name
            }
            fn is_idempotent(&self) -> bool {
                false
            }
            fn commands(&self, input: &Self::Input) -> Vec<Command> {
                vec![postgres_redirect(&input.connection, $executable, $operator, &input.path)]
            }
        }
    };
}
file_operation!(Dump, "postgres.dump", "pg_dump", ">");
file_operation!(Load, "postgres.load", "psql", "<");

fn identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn boolean_value(values: &IndexMap<String, PostgresqlValue>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| match values.get(*key) {
        Some(PostgresqlValue::Boolean(value)) => Some(*value),
        _ => None,
    })
}
fn integer_value(values: &IndexMap<String, PostgresqlValue>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| match values.get(*key) {
        Some(PostgresqlValue::Integer(value)) => Some(*value),
        _ => None,
    })
}
fn text_value<'a>(values: &'a IndexMap<String, PostgresqlValue>, key: &str) -> Option<&'a str> {
    match values.get(key) {
        Some(PostgresqlValue::Text(value)) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
