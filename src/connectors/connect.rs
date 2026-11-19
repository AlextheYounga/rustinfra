use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::{JoinError, JoinSet};

use crate::api::{State, StateError};

use super::{Connector, ConnectorError};

/// Expected per-host failures encountered while connecting an inventory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectReport {
    pub failures: IndexMap<String, String>,
}

/// Internal failure coordinating connector lifecycle calls.
#[derive(Debug, Error)]
pub enum ConnectionManagerError {
    #[error("host {0} has no connector")]
    MissingConnector(String),
    #[error("connector task failed internally: {0}")]
    Join(#[from] JoinError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("failed to disconnect {host}: {source}")]
    Disconnect {
        host: String,
        #[source]
        source: ConnectorError,
    },
    #[error("connection concurrency must be at least one")]
    ZeroConcurrency,
}

/// Host connectors and their bounded parallel lifecycle.
#[derive(Default)]
pub struct ConnectionManager {
    connectors: IndexMap<String, Arc<dyn Connector>>,
}

impl ConnectionManager {
    pub fn insert(&mut self, host: impl Into<String>, connector: Arc<dyn Connector>) {
        self.connectors.insert(host.into(), connector);
    }

    /// Connects hosts in inventory order, optionally restricted by an initial host limit.
    pub async fn connect_all(
        &self,
        state: &mut State,
        limit: Option<&IndexSet<String>>,
    ) -> Result<ConnectReport, ConnectionManagerError> {
        let hosts = state
            .inventory()
            .hosts()
            .map(|host| host.name())
            .filter(|host| limit.is_none_or(|limit| limit.contains(*host)))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let concurrency = state.config().parallel;
        if concurrency == 0 && !hosts.is_empty() {
            return Err(ConnectionManagerError::ZeroConcurrency);
        }

        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut tasks = JoinSet::new();
        for host in &hosts {
            let connector = self
                .connectors
                .get(host)
                .cloned()
                .ok_or_else(|| ConnectionManagerError::MissingConnector(host.clone()))?;
            let semaphore = Arc::clone(&semaphore);
            let host = host.clone();
            tasks.spawn(async move {
                let permit = semaphore.acquire_owned().await;
                let result = match permit {
                    Ok(_permit) => connector.connect().await,
                    Err(error) => Err(ConnectorError::Connection { message: error.to_string() }),
                };
                (host, result)
            });
        }

        let mut report = ConnectReport::default();
        while let Some(result) = tasks.join_next().await {
            let (host, connection) = result?;
            match connection {
                Ok(()) => state.activate_host(host),
                Err(error) => {
                    report.failures.insert(host, error.to_string());
                }
            }
        }
        state.fail_hosts(report.failures.keys().map(String::as_str), Some(hosts.len()))?;
        Ok(report)
    }

    /// Disconnects only hosts that connected successfully, using the global concurrency bound.
    pub async fn disconnect_all(&self, state: &State) -> Result<(), ConnectionManagerError> {
        let concurrency = state.config().parallel;
        if concurrency == 0 && !state.activated_hosts().is_empty() {
            return Err(ConnectionManagerError::ZeroConcurrency);
        }
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut tasks = JoinSet::new();
        for host in state.activated_hosts() {
            let connector = self
                .connectors
                .get(host)
                .cloned()
                .ok_or_else(|| ConnectionManagerError::MissingConnector(host.clone()))?;
            let semaphore = Arc::clone(&semaphore);
            let host = host.clone();
            tasks.spawn(async move {
                let permit = semaphore.acquire_owned().await;
                let result = match permit {
                    Ok(_permit) => connector.disconnect().await,
                    Err(error) => Err(ConnectorError::Connection { message: error.to_string() }),
                };
                (host, result)
            });
        }

        while let Some(result) = tasks.join_next().await {
            let (host, disconnection) = result?;
            disconnection.map_err(|source| ConnectionManagerError::Disconnect { host, source })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use indexmap::IndexSet;

    use super::ConnectionManager;
    use crate::api::{Config, HostSpec, Inventory, InventorySpec, RsyncCommand, State, StringCommand};
    use crate::connectors::{
        Connector, ConnectorError, ConnectorFuture, ConnectorTransferFuture, FileTransferOptions, LocalConnector,
        RunOptions,
    };

    struct FailingConnector;

    impl Connector for FailingConnector {
        fn connect(&self) -> ConnectorTransferFuture<'_> {
            Box::pin(async { Err(ConnectorError::Connection { message: "unavailable".to_owned() }) })
        }

        fn disconnect(&self) -> ConnectorTransferFuture<'_> {
            Box::pin(async { Ok(()) })
        }

        fn run_shell_command<'a>(&'a self, command: StringCommand, options: &'a RunOptions) -> ConnectorFuture<'a> {
            Box::pin(
                async move { LocalConnector.run_shell_command(command, options).await.map_err(ConnectorError::from) },
            )
        }

        fn put_file<'a>(
            &'a self,
            source: &'a Path,
            destination: &'a str,
            options: FileTransferOptions<'a>,
        ) -> ConnectorTransferFuture<'a> {
            Box::pin(async move { Connector::put_file(&LocalConnector, source, destination, options).await })
        }

        fn get_file<'a>(
            &'a self,
            source: &'a str,
            destination: &'a Path,
            options: FileTransferOptions<'a>,
        ) -> ConnectorTransferFuture<'a> {
            Box::pin(async move { Connector::get_file(&LocalConnector, source, destination, options).await })
        }

        fn rsync<'a>(&'a self, command: &'a RsyncCommand, options: &'a RunOptions) -> ConnectorTransferFuture<'a> {
            Box::pin(async move { LocalConnector.rsync(command, options).await.map_err(ConnectorError::from) })
        }
    }

    struct ObservedLocalConnector {
        disconnected: Arc<AtomicBool>,
    }

    impl Connector for ObservedLocalConnector {
        fn connect(&self) -> ConnectorTransferFuture<'_> {
            Box::pin(async { Ok(()) })
        }

        fn disconnect(&self) -> ConnectorTransferFuture<'_> {
            Box::pin(async {
                self.disconnected.store(true, Ordering::SeqCst);
                Ok(())
            })
        }

        fn run_shell_command<'a>(&'a self, command: StringCommand, options: &'a RunOptions) -> ConnectorFuture<'a> {
            Box::pin(
                async move { LocalConnector.run_shell_command(command, options).await.map_err(ConnectorError::from) },
            )
        }

        fn put_file<'a>(
            &'a self,
            source: &'a Path,
            destination: &'a str,
            options: FileTransferOptions<'a>,
        ) -> ConnectorTransferFuture<'a> {
            Box::pin(async move { Connector::put_file(&LocalConnector, source, destination, options).await })
        }

        fn get_file<'a>(
            &'a self,
            source: &'a str,
            destination: &'a Path,
            options: FileTransferOptions<'a>,
        ) -> ConnectorTransferFuture<'a> {
            Box::pin(async move { Connector::get_file(&LocalConnector, source, destination, options).await })
        }

        fn rsync<'a>(&'a self, command: &'a RsyncCommand, options: &'a RunOptions) -> ConnectorTransferFuture<'a> {
            Box::pin(async move { LocalConnector.rsync(command, options).await.map_err(ConnectorError::from) })
        }
    }

    fn state() -> State {
        let inventory = Inventory::from_spec(InventorySpec {
            hosts: vec![HostSpec::ssh("good"), HostSpec::ssh("bad"), HostSpec::ssh("limited")],
            ..InventorySpec::default()
        })
        .unwrap();
        let mut config = Config::default();
        config.parallel = 2;
        State::new(inventory, config)
    }

    #[tokio::test]
    async fn connects_limited_hosts_tracks_expected_failures_and_disconnects_activated_hosts() {
        let disconnected = Arc::new(AtomicBool::new(false));
        let mut manager = ConnectionManager::default();
        manager.insert("good", Arc::new(ObservedLocalConnector { disconnected: Arc::clone(&disconnected) }));
        manager.insert("bad", Arc::new(FailingConnector));
        manager.insert("limited", Arc::new(LocalConnector));
        let mut state = state();
        let limit = ["good".to_owned(), "bad".to_owned()].into_iter().collect::<IndexSet<_>>();

        let report = manager.connect_all(&mut state, Some(&limit)).await.unwrap();

        assert_eq!(state.activated_hosts().iter().map(String::as_str).collect::<Vec<_>>(), vec!["good"]);
        assert_eq!(state.active_hosts(), state.activated_hosts());
        assert_eq!(report.failures.keys().map(String::as_str).collect::<Vec<_>>(), vec!["bad"]);

        manager.disconnect_all(&state).await.unwrap();
        assert!(disconnected.load(Ordering::SeqCst));
    }
}
