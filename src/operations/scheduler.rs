use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexSet;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::{JoinError, JoinSet};

use crate::api::{State, StateError};

use super::OperationId;

/// Global operation scheduling strategy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionStrategy {
    #[default]
    Lockstep,
    Serial,
    NoWait,
}

/// Scheduling controls and applicable hosts for one operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRunSpec {
    pub id: OperationId,
    pub hosts: IndexSet<String>,
    pub serial: bool,
    pub parallel: usize,
}

/// Globally ordered operations and hosts active at execution start.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionPlan {
    pub hosts: Vec<String>,
    pub operations: Vec<OperationRunSpec>,
}

/// Expected failure while running one operation on one host.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct OperationTaskError {
    pub message: String,
}

/// Boxed operation call used for runtime dispatch.
pub type OperationFuture<'a> = Pin<Box<dyn Future<Output = Result<(), OperationTaskError>> + Send + 'a>>;

/// Executes one prepared operation on one host.
pub trait OperationRunner: Send + Sync {
    fn run<'a>(&'a self, host: &'a str, operation: &'a OperationId) -> OperationFuture<'a>;
}

/// Failure that stops scheduling a deploy.
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error("operation task failed internally: {0}")]
    Join(#[from] JoinError),
    #[error("execution concurrency must be at least one")]
    ZeroConcurrency,
}

/// Executes an operation plan while preserving pyinfra's strategy barriers.
pub struct Scheduler {
    runner: Arc<dyn OperationRunner>,
}

impl Scheduler {
    #[must_use]
    pub fn new(runner: Arc<dyn OperationRunner>) -> Self {
        Self { runner }
    }

    pub async fn execute(
        &self,
        state: &mut State,
        plan: &ExecutionPlan,
        strategy: ExecutionStrategy,
    ) -> Result<(), SchedulerError> {
        let concurrency = state.config().parallel;
        if concurrency == 0 {
            return Err(SchedulerError::ZeroConcurrency);
        }
        match strategy {
            ExecutionStrategy::Lockstep => self.execute_lockstep(state, plan, concurrency).await,
            ExecutionStrategy::Serial => self.execute_serial(state, plan).await,
            ExecutionStrategy::NoWait => self.execute_no_wait(state, plan, concurrency).await,
        }
    }

    async fn execute_lockstep(
        &self,
        state: &mut State,
        plan: &ExecutionPlan,
        global_limit: usize,
    ) -> Result<(), SchedulerError> {
        for operation in &plan.operations {
            let hosts = active_applicable_hosts(state, &plan.hosts, operation);
            let limit = if operation.serial {
                1
            } else if operation.parallel == 0 {
                global_limit
            } else {
                operation.parallel.min(global_limit)
            };
            for batch in hosts.chunks(limit) {
                let failed = self.run_batch(batch, &operation.id).await?;
                state.fail_hosts(failed.iter().map(String::as_str), None)?;
            }
        }
        Ok(())
    }

    async fn execute_serial(&self, state: &mut State, plan: &ExecutionPlan) -> Result<(), SchedulerError> {
        for host in &plan.hosts {
            if !state.active_hosts().contains(host) {
                continue;
            }
            let mut failed = false;
            for operation in &plan.operations {
                if !operation.hosts.contains(host) {
                    continue;
                }
                if self.runner.run(host, &operation.id).await.is_err() {
                    failed = true;
                    break;
                }
            }
            if failed {
                state.fail_hosts([host.as_str()], None)?;
            }
        }
        Ok(())
    }

    async fn execute_no_wait(
        &self,
        state: &mut State,
        plan: &ExecutionPlan,
        concurrency: usize,
    ) -> Result<(), SchedulerError> {
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut tasks = JoinSet::new();
        for host in &plan.hosts {
            if !state.active_hosts().contains(host) {
                continue;
            }
            let runner = Arc::clone(&self.runner);
            let semaphore = Arc::clone(&semaphore);
            let host = host.clone();
            let operations = plan.operations.clone();
            tasks.spawn(async move {
                let permit = semaphore.acquire_owned().await;
                let Ok(_permit) = permit else {
                    return (host, false);
                };
                for operation in operations {
                    if operation.hosts.contains(&host) && runner.run(&host, &operation.id).await.is_err() {
                        return (host, false);
                    }
                }
                (host, true)
            });
        }

        let mut failed = Vec::new();
        while let Some(result) = tasks.join_next().await {
            let (host, success) = result?;
            if !success {
                failed.push(host);
            }
        }
        state.fail_hosts(failed.iter().map(String::as_str), None)?;
        Ok(())
    }

    async fn run_batch(&self, hosts: &[String], operation: &OperationId) -> Result<Vec<String>, SchedulerError> {
        let mut tasks = JoinSet::new();
        for host in hosts {
            let host = host.clone();
            let operation = operation.clone();
            let runner = Arc::clone(&self.runner);
            tasks.spawn(async move {
                let success = runner.run(&host, &operation).await.is_ok();
                (host, success)
            });
        }

        let mut failed = Vec::new();
        while let Some(result) = tasks.join_next().await {
            let (host, success) = result?;
            if !success {
                failed.push(host);
            }
        }
        Ok(failed)
    }
}

fn active_applicable_hosts(state: &State, hosts: &[String], operation: &OperationRunSpec) -> Vec<String> {
    hosts
        .iter()
        .filter(|host| state.active_hosts().contains(*host) && operation.hosts.contains(*host))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use indexmap::IndexSet;
    use tokio::time;

    use super::{ExecutionPlan, ExecutionStrategy, OperationFuture, OperationRunSpec, OperationRunner, Scheduler};
    use crate::api::{Config, Inventory, InventorySpec, State};
    use crate::operations::OperationId;

    struct RecordingRunner {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl OperationRunner for RecordingRunner {
        fn run<'a>(&'a self, host: &'a str, operation: &'a OperationId) -> OperationFuture<'a> {
            Box::pin(async move {
                self.events.lock().unwrap().push(format!("start:{host}:{}", operation.as_str()));
                time::sleep(Duration::from_millis(if host == "slow" { 15 } else { 1 })).await;
                self.events.lock().unwrap().push(format!("end:{host}:{}", operation.as_str()));
                Ok(())
            })
        }
    }

    fn setup() -> (State, ExecutionPlan, Arc<Mutex<Vec<String>>>) {
        let mut config = Config::default();
        config.parallel = 2;
        let mut state = State::new(Inventory::from_spec(InventorySpec::default()).unwrap(), config);
        state.activate_host("slow");
        state.activate_host("fast");
        let hosts = ["slow".to_owned(), "fast".to_owned()].into_iter().collect::<IndexSet<_>>();
        let operations = ["one", "two"]
            .map(|id| OperationRunSpec { id: id.into(), hosts: hosts.clone(), serial: false, parallel: 0 })
            .to_vec();
        (state, ExecutionPlan { hosts: hosts.into_iter().collect(), operations }, Arc::default())
    }

    #[tokio::test]
    async fn lockstep_waits_for_every_host_before_next_operation() {
        let (mut state, plan, events) = setup();
        let runner = Arc::new(RecordingRunner { events: Arc::clone(&events) });

        Scheduler::new(runner).execute(&mut state, &plan, ExecutionStrategy::Lockstep).await.unwrap();

        let events = events.lock().unwrap();
        let last_first = events.iter().rposition(|event| event.ends_with(":one")).unwrap();
        let first_second = events.iter().position(|event| event.ends_with(":two")).unwrap();
        assert!(last_first < first_second);
    }

    #[tokio::test]
    async fn no_wait_allows_fast_host_to_advance_without_slow_host() {
        let (mut state, plan, events) = setup();
        let runner = Arc::new(RecordingRunner { events: Arc::clone(&events) });

        Scheduler::new(runner).execute(&mut state, &plan, ExecutionStrategy::NoWait).await.unwrap();

        let events = events.lock().unwrap();
        let fast_second = events.iter().position(|event| event == "start:fast:two").unwrap();
        let slow_first_end = events.iter().position(|event| event == "end:slow:one").unwrap();
        assert!(fast_second < slow_first_end);
    }
}
