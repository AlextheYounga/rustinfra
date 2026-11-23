use crate::api::{Command, CommandBit, HiddenValue, StringCommand};
use crate::facts::mysql::MysqlConnection;
use crate::facts::postgresql::PostgresqlConnection;

pub fn mysql_execute(connection: &MysqlConnection, database: Option<&str>, query: CommandBit) -> Command {
    let mut bits = mysql_target(connection, database, "mysql");
    bits.extend([CommandBit::from("-Be"), CommandBit::quoted(query)]);
    Command::Shell(StringCommand::new(bits))
}

pub fn mysql_dump(connection: &MysqlConnection, database: Option<&str>, path: &str) -> Command {
    let mut bits = mysql_target(connection, database, "mysqldump");
    bits.extend([CommandBit::from(">"), CommandBit::quoted(path)]);
    Command::Shell(StringCommand::new(bits))
}

pub fn mysql_load(connection: &MysqlConnection, database: Option<&str>, path: &str) -> Command {
    let mut bits = mysql_target(connection, database, "mysql");
    bits.extend([CommandBit::from("<"), CommandBit::quoted(path)]);
    Command::Shell(StringCommand::new(bits))
}

fn mysql_target(connection: &MysqlConnection, database: Option<&str>, executable: &str) -> Vec<CommandBit> {
    let mut bits = vec![CommandBit::from(executable)];
    if let Some(database) = database {
        bits.push(CommandBit::quoted(database));
    }
    if let Some(user) = &connection.user {
        bits.push(joined("-u", CommandBit::quoted(user.clone())));
    }
    if let Some(password) = &connection.password {
        bits.push(joined("-p", CommandBit::quoted(HiddenValue::new(password.clone()))));
    }
    if let Some(host) = &connection.host {
        bits.push(joined("-h", CommandBit::quoted(host.clone())));
    }
    if let Some(port) = &connection.port {
        bits.push(joined("-P", CommandBit::quoted(port.clone())));
    }
    bits
}

pub fn postgres_execute(connection: &PostgresqlConnection, query: CommandBit) -> Command {
    let mut bits = postgres_target(connection, "psql");
    bits.extend([CommandBit::from("-Ac"), CommandBit::quoted(query)]);
    Command::Shell(StringCommand::new(bits))
}

pub fn postgres_redirect(connection: &PostgresqlConnection, executable: &str, operator: &str, path: &str) -> Command {
    let mut bits = postgres_target(connection, executable);
    bits.extend([CommandBit::from(operator), CommandBit::quoted(path)]);
    Command::Shell(StringCommand::new(bits))
}

fn postgres_target(connection: &PostgresqlConnection, executable: &str) -> Vec<CommandBit> {
    let mut bits = Vec::new();
    if let Some(password) = &connection.password {
        bits.push(joined("PGPASSWORD=", CommandBit::quoted(HiddenValue::new(password.clone()))));
    }
    bits.push(CommandBit::from(executable));
    for (option, value) in
        [("-d", &connection.database), ("-U", &connection.user), ("-h", &connection.host), ("-p", &connection.port)]
    {
        if let Some(value) = value {
            bits.extend([CommandBit::from(option), CommandBit::quoted(value.clone())]);
        }
    }
    bits
}

fn joined(prefix: &str, value: CommandBit) -> CommandBit {
    StringCommand::with_separator([CommandBit::from(prefix), value], "").into()
}
