use crate::api::{Command, FunctionCommand};
use crate::operations::Operation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallInput(pub FunctionCommand);
impl CallInput {
    #[must_use]
    pub fn new(name: impl Into<String>, callback: impl Fn() -> Result<(), String> + Send + Sync + 'static) -> Self {
        Self(FunctionCommand::new(name, callback))
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Call;
impl Operation for Call {
    type Input = CallInput;
    fn name(&self) -> &'static str {
        "python.call"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![Command::Function(input.0.clone())]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaiseExceptionInput {
    pub message: String,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct RaiseException;
impl Operation for RaiseException {
    type Input = RaiseExceptionInput;
    fn name(&self) -> &'static str {
        "python.raise_exception"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let message = input.message.clone();
        vec![Command::Function(FunctionCommand::new("raise_exception", move || Err(message.clone())))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::{LocalConnector, RunOptions};
    use crate::operations::CommandExecutor;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[tokio::test]
    async fn callbacks_execute_locally_and_errors_propagate() {
        let called = Arc::new(AtomicBool::new(false));
        let state = Arc::clone(&called);
        let commands = Call.commands(&CallInput::new("callback", move || {
            state.store(true, Ordering::SeqCst);
            Ok(())
        }));
        CommandExecutor.execute(&LocalConnector, &commands, &RunOptions::default()).await.unwrap();
        assert!(called.load(Ordering::SeqCst));
        let error = CommandExecutor
            .execute(
                &LocalConnector,
                &RaiseException.commands(&RaiseExceptionInput { message: "boom".to_owned() }),
                &RunOptions::default(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("boom"));
    }
}
