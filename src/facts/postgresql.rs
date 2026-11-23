use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, HiddenValue, StringCommand};

use super::Fact;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PostgresqlConnection {
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
    pub database: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PostgresqlValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Null,
}

fn command(connection: &PostgresqlConnection, query: &str) -> StringCommand {
    let mut bits = Vec::<CommandBit>::new();
    if let Some(password) = &connection.password {
        bits.push(
            StringCommand::with_separator(
                [
                    CommandBit::from("PGPASSWORD"),
                    CommandBit::quoted(CommandBit::from(HiddenValue::new(password.clone()))),
                ],
                "=",
            )
            .into(),
        );
    }
    bits.push(CommandBit::from("psql"));
    for (option, value) in
        [("-d", &connection.database), ("-U", &connection.user), ("-h", &connection.host), ("-p", &connection.port)]
    {
        if let Some(value) = value {
            bits.extend([CommandBit::from(option), CommandBit::quoted(value.clone())]);
        }
    }
    bits.extend([CommandBit::from("-Ac"), CommandBit::quoted(query)]);
    StringCommand::new(bits)
}

fn rows(output: &[String], prefix: &str) -> Vec<IndexMap<String, String>> {
    let Some(header) = output.first() else { return Vec::new() };
    let columns = header.split('|').map(|column| column.strip_prefix(prefix).unwrap_or(column)).collect::<Vec<_>>();
    output
        .iter()
        .skip(1)
        .filter(|line| !line.starts_with('('))
        .map(|line| {
            columns
                .iter()
                .zip(line.split('|'))
                .map(|(column, value)| ((*column).to_owned(), value.to_owned()))
                .collect()
        })
        .collect()
}

fn value(key: &str, raw: String, integers: &[&str], booleans: &[&str]) -> PostgresqlValue {
    if integers.contains(&key) {
        raw.parse().map_or(PostgresqlValue::Text(raw), PostgresqlValue::Integer)
    } else if booleans.contains(&key) {
        PostgresqlValue::Boolean(raw == "t")
    } else if raw.is_empty() {
        PostgresqlValue::Null
    } else {
        PostgresqlValue::Text(raw)
    }
}

macro_rules! postgresql_fact {
    ($name:ident, $fact_name:literal, $query:expr, $prefix:literal, $key:literal, $integers:expr, $booleans:expr, $adjust:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = PostgresqlConnection;
            type Output = IndexMap<String, IndexMap<String, PostgresqlValue>>;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, input: &PostgresqlConnection) -> StringCommand {
                command(input, $query)
            }
            fn default(&self) -> Self::Output {
                IndexMap::new()
            }
            fn requires_command(&self, _: &PostgresqlConnection) -> Option<&str> {
                Some("psql")
            }
            fn process(
                &self,
                output: &[String],
                _: &PostgresqlConnection,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(rows(output, $prefix)
                    .into_iter()
                    .filter_map(|mut row| {
                        ($adjust)(&mut row);
                        let name = row.shift_remove($key)?;
                        let details = row
                            .into_iter()
                            .map(|(key, raw)| {
                                let parsed = value(&key, raw, $integers, $booleans);
                                (key, parsed)
                            })
                            .collect();
                        Some((name, details))
                    })
                    .collect())
            }
        }
    };
}

postgresql_fact!(
    PostgresqlRoles,
    "postgresql.PostgresqlRoles",
    "SELECT * FROM pg_catalog.pg_roles",
    "rol",
    "name",
    &["oid", "connlimit"],
    &["super", "inherit", "createrole", "createdb", "canlogin", "replication", "bypassrls"],
    |_row: &mut IndexMap<String, String>| {}
);

postgresql_fact!(
    PostgresqlDatabases,
    "postgresql.PostgresqlDatabases",
    "SELECT pg_catalog.pg_encoding_to_char(encoding), *, pg_catalog.pg_get_userbyid(datdba) AS owner FROM pg_catalog.pg_database",
    "dat",
    "name",
    &["dba", "tablespace", "connlimit", "lastsysoid", "frozenxid", "minmxid"],
    &["istemplate", "allowconn"],
    |row: &mut IndexMap<String, String>| {
        if let Some(encoding) = row.shift_remove("pg_encoding_to_char") {
            row.insert("encoding".to_owned(), encoding);
        }
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_role_and_database_reference_rows() {
        let roles = PostgresqlRoles
            .process(
                &[
                    "rolname|rolsuper|rolinherit|rolcreaterole|rolcreatedb|rolcanlogin|rolreplication|rolconnlimit|rolpassword|rolvaliduntil|rolbypassrls|rolconfig|oid".to_owned(),
                    "postgres|t|t|t|t|t|t|-1|********||t||10".to_owned(),
                    "pyinfra|t|t|f|f|t|f|-1|********||f||16384".to_owned(),
                    "(7 rows)".to_owned(),
                ],
                &PostgresqlConnection::default(),
            )
            .unwrap();
        assert_eq!(roles.len(), 2);
        assert_eq!(roles["postgres"]["super"], PostgresqlValue::Boolean(true));
        assert_eq!(roles["postgres"]["oid"], PostgresqlValue::Integer(10));
        assert_eq!(roles["pyinfra"]["createrole"], PostgresqlValue::Boolean(false));
        assert_eq!(roles["pyinfra"]["validuntil"], PostgresqlValue::Null);
        let databases = PostgresqlDatabases
            .process(
                &[
                    "pg_encoding_to_char|datname|datdba|encoding|datcollate|datctype|datistemplate|datallowconn|datconnlimit|datlastsysoid|datfrozenxid|datminmxid|dattablespace|datacl|owner".to_owned(),
                    "UTF8|postgres|10|6|en_US.UTF-8|en_US.UTF-8|f|t|-1|13052|548|1|1663||postgres".to_owned(),
                    "UTF8|pyinfra_stuff|16384|6|en_US.UTF-8|en_US.UTF-8|f|t|-1|13052|548|1|1663||pyinfra".to_owned(),
                    "(4 rows)".to_owned(),
                ],
                &PostgresqlConnection::default(),
            )
            .unwrap();
        assert_eq!(databases.len(), 2);
        assert_eq!(databases["postgres"]["encoding"], PostgresqlValue::Text("UTF8".to_owned()));
        assert_eq!(databases["postgres"]["dba"], PostgresqlValue::Integer(10));
        assert_eq!(databases["pyinfra_stuff"]["acl"], PostgresqlValue::Null);
        assert_eq!(databases["pyinfra_stuff"]["owner"], PostgresqlValue::Text("pyinfra".to_owned()));
    }

    #[test]
    fn connection_values_are_quoted_and_password_is_masked() {
        let input = PostgresqlConnection {
            user: Some("user name".to_owned()),
            password: Some("secret".to_owned()),
            host: Some("host; bad".to_owned()),
            ..PostgresqlConnection::default()
        };
        let built = PostgresqlRoles.command(&input);
        assert!(built.raw_value().starts_with("PGPASSWORD=secret psql -U 'user name' -h 'host; bad'"));
        assert!(built.masked_value().starts_with("PGPASSWORD='*MASKED*'"));
    }
}
