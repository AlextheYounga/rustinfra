//! Deprecated compatibility names for the operations now exposed as `postgres`.

pub use super::postgres::{DatabaseInput, FileInput, RoleInput, SqlInput};

use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::FactOptions;

use super::{Operation, StatefulOperation, StatefulOperationFuture};

macro_rules! operation_alias {
    ($name:ident, $input:ty, $operation_name:literal, $target:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Operation for $name {
            type Input = $input;
            fn name(&self) -> &'static str {
                $operation_name
            }
            fn is_idempotent(&self) -> bool {
                false
            }
            fn commands(&self, input: &Self::Input) -> Vec<Command> {
                Operation::commands(&$target, input)
            }
        }
    };
}
operation_alias!(Sql, SqlInput, "postgresql.sql", super::postgres::Sql);
operation_alias!(Dump, FileInput, "postgresql.dump", super::postgres::Dump);
operation_alias!(Load, FileInput, "postgresql.load", super::postgres::Load);

macro_rules! stateful_alias {
    ($name:ident, $input:ty, $operation_name:literal, $target:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl StatefulOperation for $name {
            type Input = $input;
            fn name(&self) -> &'static str {
                $operation_name
            }
            fn is_idempotent(&self) -> bool {
                false
            }
            fn commands<'a>(
                &'a self,
                input: &'a Self::Input,
                connector: &'a dyn Connector,
                options: &'a FactOptions,
            ) -> StatefulOperationFuture<'a> {
                StatefulOperation::commands(&$target, input, connector, options)
            }
        }
    };
}
stateful_alias!(Role, RoleInput, "postgresql.role", super::postgres::Role);
stateful_alias!(Database, DatabaseInput, "postgresql.database", super::postgres::Database);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deprecated_names_delegate_and_remain_non_idempotent() {
        assert_eq!(Operation::name(&Sql), "postgresql.sql");
        assert!(!Operation::is_idempotent(&Sql));
        assert_eq!(StatefulOperation::name(&Role), "postgresql.role");
        assert!(!StatefulOperation::is_idempotent(&Role));
    }
}
