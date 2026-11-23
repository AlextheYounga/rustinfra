use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, HiddenValue, StringCommand};

use super::Fact;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MysqlConnection {
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MysqlDatabaseInfo {
    pub character_set: String,
    pub collation_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MysqlValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MysqlUserInfo {
    pub details: IndexMap<String, MysqlValue>,
    pub privileges: Vec<String>,
}

fn command(connection: &MysqlConnection, query: &str, ignore_errors: bool) -> StringCommand {
    let mut bits = vec![CommandBit::from("mysql")];
    if let Some(user) = &connection.user {
        bits.push(StringCommand::with_separator([CommandBit::from("-u"), CommandBit::quoted(user.clone())], "").into());
    }
    if let Some(password) = &connection.password {
        bits.push(
            StringCommand::with_separator(
                [CommandBit::from("-p"), CommandBit::quoted(CommandBit::from(HiddenValue::new(password.clone())))],
                "",
            )
            .into(),
        );
    }
    if let Some(host) = &connection.host {
        bits.push(StringCommand::with_separator([CommandBit::from("-h"), CommandBit::quoted(host.clone())], "").into());
    }
    if let Some(port) = &connection.port {
        bits.push(StringCommand::with_separator([CommandBit::from("-P"), CommandBit::quoted(port.clone())], "").into());
    }
    bits.push(CommandBit::from("-Be"));
    bits.push(CommandBit::quoted(query));
    if ignore_errors {
        bits.extend([CommandBit::from("||"), CommandBit::from("true")]);
    }
    StringCommand::new(bits)
}

fn rows(output: &[String]) -> Vec<IndexMap<String, String>> {
    let Some(header) = output.first() else { return Vec::new() };
    let columns = header.split('\t').collect::<Vec<_>>();
    output
        .iter()
        .skip(1)
        .map(|line| {
            columns
                .iter()
                .zip(line.split('\t'))
                .map(|(column, value)| ((*column).to_owned(), value.to_owned()))
                .collect()
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MysqlDatabases;
impl Fact for MysqlDatabases {
    type Input = MysqlConnection;
    type Output = IndexMap<String, MysqlDatabaseInfo>;
    fn name(&self) -> &'static str {
        "mysql.MysqlDatabases"
    }
    fn command(&self, input: &MysqlConnection) -> StringCommand {
        command(input, "SELECT * FROM information_schema.SCHEMATA", false)
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &MysqlConnection) -> Option<&str> {
        Some("mysql")
    }
    fn process(&self, output: &[String], _: &MysqlConnection) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(rows(output)
            .into_iter()
            .filter_map(|row| {
                Some((
                    row.get("SCHEMA_NAME")?.clone(),
                    MysqlDatabaseInfo {
                        character_set: row.get("DEFAULT_CHARACTER_SET_NAME")?.clone(),
                        collation_name: row.get("DEFAULT_COLLATION_NAME")?.clone(),
                    },
                ))
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MysqlUsers;
impl Fact for MysqlUsers {
    type Input = MysqlConnection;
    type Output = IndexMap<String, MysqlUserInfo>;
    fn name(&self) -> &'static str {
        "mysql.MysqlUsers"
    }
    fn command(&self, input: &MysqlConnection) -> StringCommand {
        command(input, "SELECT * FROM mysql.user", false)
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &MysqlConnection) -> Option<&str> {
        Some("mysql")
    }
    fn process(&self, output: &[String], _: &MysqlConnection) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut users = IndexMap::new();
        for mut row in rows(output) {
            let (Some(user), Some(host)) = (row.shift_remove("User"), row.shift_remove("Host")) else { continue };
            if user.is_empty() || host.is_empty() {
                continue;
            }
            let mut details = IndexMap::new();
            let mut privileges = Vec::new();
            for (key, value) in row {
                if let Some(privilege) = key.strip_suffix("_priv") {
                    if value == "Y" {
                        privileges.push(privilege.to_owned());
                    }
                } else {
                    let value = if key.starts_with("max_") {
                        value.parse().map_or(MysqlValue::Text(value), MysqlValue::Integer)
                    } else if matches!(key.as_str(), "password_expired" | "is_role") {
                        MysqlValue::Boolean(value == "Y")
                    } else if value.is_empty() {
                        MysqlValue::Null
                    } else {
                        MysqlValue::Text(value)
                    };
                    details.insert(key, value);
                }
            }
            privileges.sort();
            users.insert(format!("{user}@{host}"), MysqlUserInfo { details, privileges });
        }
        Ok(users)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MysqlUserGrantsInput {
    pub user: String,
    pub hostname: String,
    pub connection: MysqlConnection,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MysqlUserGrants;
impl Fact for MysqlUserGrants {
    type Input = MysqlUserGrantsInput;
    type Output = IndexMap<String, Vec<String>>;
    fn name(&self) -> &'static str {
        "mysql.MysqlUserGrants"
    }
    fn command(&self, input: &MysqlUserGrantsInput) -> StringCommand {
        command(&input.connection, &format!("SHOW GRANTS FOR \"{}\"@\"{}\"", input.user, input.hostname), true)
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &MysqlUserGrantsInput) -> Option<&str> {
        Some("mysql")
    }
    fn process(
        &self,
        output: &[String],
        _: &MysqlUserGrantsInput,
    ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut grants = IndexMap::<String, Vec<String>>::new();
        for line in output {
            let Some(grant) = line.strip_prefix("GRANT ") else { continue };
            let Some((privileges, target_and_user)) = grant.split_once(" ON ") else { continue };
            let Some((target, extras)) = target_and_user.split_once(" TO ") else { continue };
            let values = grants.entry(target.replace("\\\\", "\\")).or_default();
            for privilege in privileges.split(',').map(str::trim) {
                if !values.iter().any(|value| value == privilege) {
                    values.push(privilege.to_owned());
                }
            }
            if extras.contains("WITH GRANT OPTION") {
                values.push("GRANT OPTION".to_owned());
            }
        }
        Ok(grants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_mask_passwords_quote_values_and_parse_results() {
        let connection = MysqlConnection {
            user: Some("x$(reboot)".to_owned()),
            password: Some("p".to_owned()),
            host: Some("h; rm".to_owned()),
            port: Some("3306".to_owned()),
        };
        let built = MysqlDatabases.command(&connection);
        assert_eq!(
            built.raw_value(),
            "mysql -u'x$(reboot)' -pp -h'h; rm' -P3306 -Be 'SELECT * FROM information_schema.SCHEMATA'"
        );
        assert!(built.masked_value().contains("-p'*MASKED*'"));
        let databases = MysqlDatabases
            .process(
                &[
                    "CATALOG_NAME\tSCHEMA_NAME\tDEFAULT_CHARACTER_SET_NAME\tDEFAULT_COLLATION_NAME".to_owned(),
                    "def\tmysql\tutf8mb4\tutf8mb4_general_ci".to_owned(),
                ],
                &connection,
            )
            .unwrap();
        assert_eq!(databases["mysql"].character_set, "utf8mb4");
    }

    #[test]
    fn parses_users_and_grants() {
        let users = MysqlUsers
            .process(
                &[
                    "Host\tUser\tSelect_priv\tmax_connections\tpassword_expired".to_owned(),
                    "localhost\troot\tY\t5\tN".to_owned(),
                ],
                &MysqlConnection::default(),
            )
            .unwrap();
        assert_eq!(users["root@localhost"].privileges, ["Select"]);
        assert_eq!(users["root@localhost"].details["max_connections"], MysqlValue::Integer(5));
        let input = MysqlUserGrantsInput {
            user: "someuser".to_owned(),
            hostname: "localhost".to_owned(),
            connection: MysqlConnection::default(),
        };
        let grants = MysqlUserGrants
            .process(
                &[
                    "GRANT USAGE ON *.* TO `someuser`@`localhost`".to_owned(),
                    "GRANT ALL PRIVILEGES ON `database`.* TO `someuser`@`localhost` WITH GRANT OPTION".to_owned(),
                ],
                &input,
            )
            .unwrap();
        assert_eq!(grants["`database`.*"], ["ALL PRIVILEGES", "GRANT OPTION"]);
    }
}
