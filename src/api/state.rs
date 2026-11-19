use std::cmp::Ordering;
use std::thread;

use indexmap::{IndexMap, IndexSet};
use thiserror::Error;

use super::{Config, Inventory};

/// Ordered lifecycle stages of a rustinfra run.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StateStage {
    Setup,
    Connect,
    Prepare,
    Execute,
    Disconnect,
}

/// Per-host final operation accounting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostResults {
    pub operations: usize,
    pub successful_operations: usize,
    pub error_operations: usize,
    pub ignored_error_operations: usize,
    pub partial_operations: usize,
}

/// Invalid lifecycle or host-state transition.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum StateError {
    #[error("state stage cannot go backwards from {current:?} to {requested:?}")]
    StageRegression { current: StateStage, requested: StateStage },
    #[error("No hosts remaining!")]
    NoHostsRemaining,
    #[error("Over {limit}% of hosts failed ({failed_percent}%)")]
    FailureThresholdExceeded { limit: u8, failed_percent: u8 },
}

/// Mutable coordination state for one deploy run.
#[derive(Clone, Debug)]
pub struct State {
    inventory: Inventory,
    config: Config,
    stage: StateStage,
    activated_hosts: IndexSet<String>,
    active_hosts: IndexSet<String>,
    failed_hosts: IndexSet<String>,
    host_results: IndexMap<String, HostResults>,
}

impl State {
    #[must_use]
    pub fn new(inventory: Inventory, mut config: Config) -> Self {
        if config.parallel == 0 && !inventory.is_empty() {
            let processors = thread::available_parallelism().map_or(1, usize::from);
            config.parallel = processors.saturating_mul(20).min(inventory.len());
        }
        Self {
            inventory,
            config,
            stage: StateStage::Setup,
            activated_hosts: IndexSet::new(),
            active_hosts: IndexSet::new(),
            failed_hosts: IndexSet::new(),
            host_results: IndexMap::new(),
        }
    }

    #[must_use]
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    #[must_use]
    pub fn stage(&self) -> StateStage {
        self.stage
    }

    pub fn set_stage(&mut self, stage: StateStage) -> Result<(), StateError> {
        if stage < self.stage {
            return Err(StateError::StageRegression { current: self.stage, requested: stage });
        }
        self.stage = stage;
        Ok(())
    }

    pub fn activate_host(&mut self, host: impl Into<String>) {
        let host = host.into();
        self.activated_hosts.insert(host.clone());
        self.active_hosts.insert(host);
    }

    /// Marks hosts failed and applies pyinfra's strict `FAIL_PERCENT` threshold.
    pub fn fail_hosts<'a>(
        &mut self,
        hosts: impl IntoIterator<Item = &'a str>,
        activated_count: Option<usize>,
    ) -> Result<(), StateError> {
        let hosts = hosts.into_iter().collect::<Vec<_>>();
        if hosts.is_empty() {
            return Ok(());
        }
        for host in hosts {
            self.failed_hosts.insert(host.to_owned());
            self.active_hosts.shift_remove(host);
        }
        if self.active_hosts.is_empty() {
            return Err(StateError::NoHostsRemaining);
        }

        let Some(limit) = self.config.fail_percent else {
            return Ok(());
        };
        let total = activated_count.filter(|count| *count > 0).unwrap_or(self.activated_hosts.len());
        let failed_percentage_numerator = total.saturating_sub(self.active_hosts.len()).saturating_mul(100);
        if failed_percentage_numerator > usize::from(limit).saturating_mul(total) {
            let failed_percent = rounded_percentage(failed_percentage_numerator, total);
            return Err(StateError::FailureThresholdExceeded { limit, failed_percent });
        }
        Ok(())
    }

    #[must_use]
    pub fn activated_hosts(&self) -> &IndexSet<String> {
        &self.activated_hosts
    }

    #[must_use]
    pub fn active_hosts(&self) -> &IndexSet<String> {
        &self.active_hosts
    }

    #[must_use]
    pub fn failed_hosts(&self) -> &IndexSet<String> {
        &self.failed_hosts
    }

    #[must_use]
    pub fn host_results(&self, host: &str) -> Option<&HostResults> {
        self.host_results.get(host)
    }

    pub(crate) fn set_host_results(&mut self, host: String, results: HostResults) {
        self.host_results.insert(host, results);
    }
}

fn rounded_percentage(numerator: usize, denominator: usize) -> u8 {
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    let comparison = remainder.saturating_mul(2).cmp(&denominator);
    let rounded = if comparison == Ordering::Less || (comparison == Ordering::Equal && whole.is_multiple_of(2)) {
        whole
    } else {
        whole.saturating_add(1)
    };
    u8::try_from(rounded).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::{State, StateError, StateStage};
    use crate::api::{Config, Inventory, InventorySpec};

    fn state(fail_percent: Option<u8>) -> State {
        let mut config = Config::default();
        config.fail_percent = fail_percent;
        State::new(Inventory::from_spec(InventorySpec::default()).unwrap(), config)
    }

    #[test]
    fn stages_can_advance_or_repeat_but_not_go_backwards() {
        let mut state = state(None);

        state.set_stage(StateStage::Connect).unwrap();
        state.set_stage(StateStage::Connect).unwrap();

        assert_eq!(
            state.set_stage(StateStage::Setup),
            Err(StateError::StageRegression { current: StateStage::Connect, requested: StateStage::Setup })
        );
    }

    #[test]
    fn failing_exactly_the_configured_percentage_is_allowed() {
        let mut state = state(Some(50));
        state.activate_host("one");
        state.activate_host("two");

        assert_eq!(state.fail_hosts(["one"], None), Ok(()));
        assert_eq!(state.active_hosts().iter().map(String::as_str).collect::<Vec<_>>(), vec!["two"]);
    }

    #[test]
    fn failing_over_the_threshold_reports_rounded_percentage() {
        let mut state = state(Some(20));
        for host in ["one", "two", "three"] {
            state.activate_host(host);
        }

        assert_eq!(
            state.fail_hosts(["one"], None),
            Err(StateError::FailureThresholdExceeded { limit: 20, failed_percent: 33 })
        );
    }

    #[test]
    fn no_hosts_remaining_takes_precedence_over_failure_percentage() {
        let mut state = state(Some(0));
        state.activate_host("one");

        assert_eq!(state.fail_hosts(["one"], None), Err(StateError::NoHostsRemaining));
    }
}
