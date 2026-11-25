use std::sync::Arc;

use indexmap::IndexMap;
use thiserror::Error;
use tokio::task::JoinSet;

use crate::connectors::Connector;
use crate::facts::Phase;

use super::PreparedDeploy;
use crate::operations::{DeployPhase, DeployPreview, DeployReporter, OperationId, OperationPreview};

impl PreparedDeploy {
    /// Reads current host state and classifies each operation without applying its commands.
    pub async fn preview(
        &self,
        connectors: &IndexMap<String, Arc<dyn Connector>>,
        parallel: usize,
    ) -> Result<DeployPreview, DeployPreviewError> {
        self.preview_inner(connectors, parallel, None).await
    }

    pub(crate) async fn preview_reported(
        &self,
        connectors: &IndexMap<String, Arc<dyn Connector>>,
        parallel: usize,
        reporter: &dyn DeployReporter,
    ) -> Result<DeployPreview, DeployPreviewError> {
        self.preview_inner(connectors, parallel, Some(reporter)).await
    }

    async fn preview_inner(
        &self,
        connectors: &IndexMap<String, Arc<dyn Connector>>,
        parallel: usize,
        reporter: Option<&dyn DeployReporter>,
    ) -> Result<DeployPreview, DeployPreviewError> {
        if parallel == 0 {
            return Err(DeployPreviewError::ZeroConcurrency);
        }
        let mut preview = DeployPreview::default();
        let mut completed = 0;
        for run_spec in &self.plan.operations {
            let operation = self
                .operations
                .get(&run_spec.id)
                .ok_or_else(|| DeployPreviewError::MissingOperation(run_spec.id.clone()))?;
            let mut changes = IndexMap::new();
            for hosts in run_spec.hosts.iter().cloned().collect::<Vec<_>>().chunks(parallel) {
                let mut tasks = JoinSet::new();
                for host in hosts {
                    let host = host.clone();
                    let connector = connectors
                        .get(&host)
                        .cloned()
                        .ok_or_else(|| DeployPreviewError::MissingConnector(host.clone()))?;
                    let execution =
                        self.executions.get(&(host.clone(), run_spec.id.clone())).cloned().ok_or_else(|| {
                            DeployPreviewError::MissingExecution { host: host.clone(), operation: run_spec.id.clone() }
                        })?;
                    tasks.spawn(async move {
                        let result = execution
                            .commands(connector.as_ref(), Phase::Prepare)
                            .await
                            .map(|commands| !commands.is_empty())
                            .map_err(|error| error.to_string());
                        (host, result)
                    });
                }
                while let Some(result) = tasks.join_next().await {
                    let (host, changed) = result.map_err(|error| DeployPreviewError::Task(error.to_string()))?;
                    let changed = changed.map_err(|message| DeployPreviewError::Generation {
                        host: host.clone(),
                        operation: operation.display_name.clone(),
                        message,
                    })?;
                    let progress_message = format!("{} on {host}", operation.display_name);
                    changes.insert(host, changed);
                    completed += 1;
                    if let Some(reporter) = reporter {
                        reporter.phase_progress(DeployPhase::Plan, completed, &progress_message);
                    }
                }
            }
            let mut change_hosts = Vec::new();
            let mut no_change_hosts = Vec::new();
            for host in &run_spec.hosts {
                match changes.get(host) {
                    Some(true) => change_hosts.push(host.clone()),
                    Some(false) => no_change_hosts.push(host.clone()),
                    None => {}
                }
            }
            preview.operations.push(OperationPreview {
                id: run_spec.id.clone(),
                name: operation.display_name.clone(),
                change_hosts,
                no_change_hosts,
            });
        }
        Ok(preview)
    }
}

#[derive(Debug, Error)]
pub enum DeployPreviewError {
    #[error("preview concurrency must be at least one")]
    ZeroConcurrency,
    #[error("host {0} has no connector during preparation")]
    MissingConnector(String),
    #[error("prepared operation {0:?} is missing its metadata")]
    MissingOperation(OperationId),
    #[error("host {host} has no prepared execution for {operation:?}")]
    MissingExecution { host: String, operation: OperationId },
    #[error("could not determine whether {operation} changes {host}: {message}")]
    Generation { host: String, operation: String, message: String },
    #[error("preparation task failed internally: {0}")]
    Task(String),
}
