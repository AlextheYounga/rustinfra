use crate::api::{Command, ReconnectCommand, StringCommand};
use crate::connectors::Connector;
use crate::facts::server::{Kernel, KernelInput, Uptime};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebootInput {
    pub delay_seconds: u64,
    pub interval_seconds: u64,
    pub reboot_timeout_seconds: u64,
}

impl Default for RebootInput {
    fn default() -> Self {
        Self { delay_seconds: 10, interval_seconds: 1, reboot_timeout_seconds: 300 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Reboot;

impl StatefulOperation for Reboot {
    type Input = RebootInput;
    fn name(&self) -> &'static str {
        "server.reboot"
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
        Box::pin(async move {
            if input.interval_seconds == 0 {
                return Err("reboot reconnect interval must be greater than zero".into());
            }
            let facts = FactExecutor::new(connector, options);
            let kernel = facts.get(&Kernel, &()).await?.unwrap_or_else(|| "Linux".to_owned());
            let pre_reboot_uptime = facts.get(&Uptime, &KernelInput { kernel: kernel.clone() }).await?;
            Ok(vec![
                Command::Shell(StringCommand::new(["( sleep 1 && reboot ) </dev/null >/dev/null 2>&1 &"])),
                Command::Reconnect(ReconnectCommand {
                    delay_seconds: input.delay_seconds,
                    interval_seconds: input.interval_seconds,
                    timeout_seconds: input.reboot_timeout_seconds,
                    pre_reboot_uptime,
                    kernel,
                }),
            ])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::RebootInput;

    #[test]
    fn defaults_match_pyinfra() {
        assert_eq!(
            RebootInput::default(),
            RebootInput { delay_seconds: 10, interval_seconds: 1, reboot_timeout_seconds: 300 }
        );
    }
}
