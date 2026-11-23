use crate::api::{Command, CommandBit};
use crate::facts::mysql::MysqlConnection;

use super::Operation;
use super::database::{mysql_dump, mysql_execute, mysql_load};

mod privileges;
mod user;
pub use privileges::{Database, DatabaseInput, Privileges, PrivilegesInput};
pub use user::{Require, User, UserInput};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlInput {
    pub sql: String,
    pub database: Option<String>,
    pub connection: MysqlConnection,
}

impl SqlInput {
    pub fn new(sql: impl Into<String>) -> Self {
        Self { sql: sql.into(), database: None, connection: MysqlConnection::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sql;
impl Operation for Sql {
    type Input = SqlInput;
    fn name(&self) -> &'static str {
        "mysql.sql"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![mysql_execute(&input.connection, input.database.as_deref(), CommandBit::from(input.sql.clone()))]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInput {
    pub path: String,
    pub database: Option<String>,
    pub connection: MysqlConnection,
}
impl FileInput {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), database: None, connection: MysqlConnection::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Dump;
impl Operation for Dump {
    type Input = FileInput;
    fn name(&self) -> &'static str {
        "mysql.dump"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![mysql_dump(&input.connection, input.database.as_deref(), &input.path)]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Load;
impl Operation for Load {
    type Input = FileInput;
    fn name(&self) -> &'static str {
        "mysql.load"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![mysql_load(&input.connection, input.database.as_deref(), &input.path)]
    }
}

pub(super) fn quoted_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
pub(super) fn quoted_mysql_name(value: &str) -> String {
    format!("`{}`", value.replace('`', "``"))
}

#[cfg(test)]
mod tests;
