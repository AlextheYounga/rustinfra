use std::sync::Arc;

use indexmap::IndexMap;
use tokio::sync::Mutex;

use crate::api::HostResults;
use crate::connectors::Connector;

use super::scheduler::OperationFuture;
use super::{OperationExecution, OperationExecutor, OperationId, OperationRunner, OperationTaskError};

pub(crate) struct PreparedOperationRunner {
    connectors: IndexMap<String, Arc<dyn Connector>>,
    executions: IndexMap<(String, OperationId), OperationExecution>,
    results: Arc<Mutex<IndexMap<String, HostResults>>>,
}

impl PreparedOperationRunner {
    pub fn new(
        connectors: IndexMap<String, Arc<dyn Connector>>,
        executions: IndexMap<(String, OperationId), OperationExecution>,
    ) -> Self {
        Self { connectors, executions, results: Arc::new(Mutex::new(IndexMap::new())) }
    }

    pub async fn results(&self) -> IndexMap<String, HostResults> {
        self.results.lock().await.clone()
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
            let result = OperationExecutor.execute(connector.as_ref(), execution).await;
            let return_status = result.return_status;
            let error_message = result.error.as_ref().map(ToString::to_string);
            self.record_result(host, &result).await;
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
