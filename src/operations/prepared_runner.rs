use std::sync::Arc;

use indexmap::IndexMap;
use tokio::sync::Mutex;

use crate::api::HostResults;
use crate::connectors::{Connector, OutputHandler};

use super::scheduler::OperationFuture;
use super::{
    DeployReporter, HostOperationReport, HostOperationStatus, OperationExecution, OperationExecutor, OperationId,
    OperationRunner, OperationTaskError,
};

pub(crate) struct PreparedOperationRunner {
    connectors: IndexMap<String, Arc<dyn Connector>>,
    executions: IndexMap<(String, OperationId), OperationExecution>,
    names: IndexMap<OperationId, String>,
    reporter: Arc<dyn DeployReporter>,
    results: Arc<Mutex<IndexMap<String, HostResults>>>,
    operation_reports: Arc<Mutex<Vec<HostOperationReport>>>,
}

impl PreparedOperationRunner {
    pub fn new(
        connectors: IndexMap<String, Arc<dyn Connector>>,
        executions: IndexMap<(String, OperationId), OperationExecution>,
        names: IndexMap<OperationId, String>,
        reporter: Arc<dyn DeployReporter>,
    ) -> Self {
        Self {
            connectors,
            executions,
            names,
            reporter,
            results: Arc::new(Mutex::new(IndexMap::new())),
            operation_reports: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn results(&self) -> IndexMap<String, HostResults> {
        self.results.lock().await.clone()
    }

    pub async fn operation_reports(&self) -> Vec<HostOperationReport> {
        self.operation_reports.lock().await.clone()
    }
}

impl OperationRunner for PreparedOperationRunner {
    fn run<'a>(&'a self, host: &'a str, operation: &'a OperationId) -> OperationFuture<'a> {
        Box::pin(async move {
            let connector = self
                .connectors
                .get(host)
                .ok_or_else(|| OperationTaskError { message: format!("host {host} has no connector") })?;
            let execution = self.executions.get(&(host.to_owned(), operation.clone())).ok_or_else(|| {
                OperationTaskError { message: format!("host {host} has no prepared operation {}", operation.as_str()) }
            })?;
            let operation_name = self.names.get(operation).cloned().unwrap_or_else(|| operation.as_str().to_owned());
            self.reporter.operation_started(host, &operation_name);
            let output_reporter = Arc::clone(&self.reporter);
            let output_host = host.to_owned();
            let output_operation = operation_name.clone();
            let output_handler = OutputHandler::new(move |line| {
                output_reporter.operation_output(&output_host, &output_operation, line);
            });
            let result = OperationExecutor
                .execute_with_output_handler(connector.as_ref(), execution, Some(output_handler))
                .await;
            let return_status = result.return_status;
            let error_message = result.error.as_ref().map(ToString::to_string);
            self.record_result(host, &result).await;
            let status = if result.success && result.executed_commands == 0 {
                HostOperationStatus::NoChange
            } else if result.success {
                HostOperationStatus::Success
            } else if result.ignored_error {
                HostOperationStatus::IgnoredFailure
            } else {
                HostOperationStatus::Failed
            };
            let report = HostOperationReport {
                operation_id: operation.clone(),
                operation: operation_name,
                host: host.to_owned(),
                status,
                message: error_message.clone(),
            };
            self.operation_reports.lock().await.push(report.clone());
            self.reporter.operation_result(&report);
            if return_status {
                Ok(())
            } else {
                Err(OperationTaskError {
                    message: error_message
                        .unwrap_or_else(|| format!("operation {} failed on {host}", operation.as_str())),
                })
            }
        })
    }
}

impl PreparedOperationRunner {
    async fn record_result(&self, host: &str, result: &super::OperationExecutionResult) {
        let mut results = self.results.lock().await;
        let host_results = results.entry(host.to_owned()).or_default();
        host_results.operations += 1;
        if result.success {
            host_results.successful_operations += 1;
        } else if result.ignored_error {
            host_results.ignored_error_operations += 1;
        } else {
            host_results.error_operations += 1;
        }
        if result.partial {
            host_results.partial_operations += 1;
        }
    }
}
