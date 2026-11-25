use std::io::{self, Write};
use std::sync::Mutex;

use crate::connectors::{OutputLine, OutputStream};

use super::OperationId;

mod compose;

pub use compose::ComposeReporter;

const OPERATION_HEADER: &str = "OPERATION";
const HOSTS_HEADER: &str = "HOSTS";
const NO_CHANGE_HEADER: &str = "NO CHANGE";
const FAILED_HEADER: &str = "FAILED";
const SKIPPED_HEADER: &str = "SKIPPED";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeployPhase {
    Connect,
    Plan,
    Apply,
    Disconnect,
}

impl DeployPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Connect => "Connecting",
            Self::Plan => "Planning",
            Self::Apply => "Applying",
            Self::Disconnect => "Disconnecting",
        }
    }

    fn completed_label(self) -> &'static str {
        match self {
            Self::Connect => "Connected",
            Self::Plan => "Planned",
            Self::Apply => "Applied",
            Self::Disconnect => "Disconnected",
        }
    }
}

/// Change classification generated without applying commands to a host.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeployPreview {
    pub operations: Vec<OperationPreview>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationPreview {
    pub id: OperationId,
    pub name: String,
    pub change_hosts: Vec<String>,
    pub no_change_hosts: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostOperationStatus {
    Success,
    NoChange,
    Failed,
    IgnoredFailure,
}

/// User-relevant outcome of one operation on one host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostOperationReport {
    pub operation_id: OperationId,
    pub operation: String,
    pub host: String,
    pub status: HostOperationStatus,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeploySummary {
    pub operations: Vec<OperationSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationSummary {
    pub operation: String,
    pub hosts: usize,
    pub success: usize,
    pub no_change: usize,
    pub failed: usize,
    pub ignored_failure: usize,
    pub skipped: usize,
}

impl DeploySummary {
    #[must_use]
    pub fn from_reports(preview: &DeployPreview, reports: &[HostOperationReport]) -> Self {
        let operations = preview
            .operations
            .iter()
            .map(|operation| {
                let matching = reports.iter().filter(|report| report.operation_id == operation.id);
                let mut summary = OperationSummary {
                    operation: operation.name.clone(),
                    hosts: operation.change_hosts.len() + operation.no_change_hosts.len(),
                    success: 0,
                    no_change: 0,
                    failed: 0,
                    ignored_failure: 0,
                    skipped: 0,
                };
                for report in matching {
                    match report.status {
                        HostOperationStatus::Success => summary.success += 1,
                        HostOperationStatus::NoChange => summary.no_change += 1,
                        HostOperationStatus::Failed => summary.failed += 1,
                        HostOperationStatus::IgnoredFailure => summary.ignored_failure += 1,
                    }
                }
                let attempted = summary.success + summary.no_change + summary.failed + summary.ignored_failure;
                summary.skipped = summary.hosts.saturating_sub(attempted);
                summary
            })
            .collect();
        Self { operations }
    }
}

/// Receives deploy progress at presentation-safe boundaries.
pub trait DeployReporter: Send + Sync {
    fn phase_started(&self, _phase: DeployPhase, _total: usize) {}
    fn phase_progress(&self, _phase: DeployPhase, _completed: usize, _message: &str) {}
    fn phase_finished(&self, _phase: DeployPhase) {}
    fn operation_started(&self, _host: &str, _operation: &str) {}
    fn proposed_changes(&self, preview: &DeployPreview);
    fn operation_output(&self, host: &str, operation: &str, line: &OutputLine);
    fn operation_result(&self, result: &HostOperationReport);
    fn deploy_finished(&self, summary: &DeploySummary);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NullReporter;

impl DeployReporter for NullReporter {
    fn proposed_changes(&self, _preview: &DeployPreview) {}
    fn operation_output(&self, _host: &str, _operation: &str, _line: &OutputLine) {}
    fn operation_result(&self, _result: &HostOperationReport) {}
    fn deploy_finished(&self, _summary: &DeploySummary) {}
}

/// Plain text deploy output suitable for terminals and build logs.
pub struct TextReporter {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl TextReporter {
    #[must_use]
    pub fn stdout() -> Self {
        Self::new(io::stdout())
    }

    #[must_use]
    pub fn new(writer: impl Write + Send + 'static) -> Self {
        Self { writer: Mutex::new(Box::new(writer)) }
    }

    fn write(&self, render: impl FnOnce(&mut dyn Write) -> io::Result<()>) {
        let Ok(mut writer) = self.writer.lock() else {
            return;
        };
        let _ = render(writer.as_mut());
        let _ = writer.flush();
    }
}

impl DeployReporter for TextReporter {
    fn proposed_changes(&self, preview: &DeployPreview) {
        self.write(|writer| {
            writeln!(writer, "rustinfra: proposed changes")?;
            writeln!(writer, "{:<32} {:<7} {HOSTS_HEADER}", OPERATION_HEADER, "CHANGE")?;
            for operation in &preview.operations {
                if !operation.change_hosts.is_empty() {
                    writeln!(writer, "{:<32} {:<7} {}", operation.name, "yes", operation.change_hosts.join(", "))?;
                }
                if !operation.no_change_hosts.is_empty() {
                    writeln!(writer, "{:<32} {:<7} {}", operation.name, "no", operation.no_change_hosts.join(", "))?;
                }
            }
            Ok(())
        });
    }

    fn operation_output(&self, host: &str, operation: &str, line: &OutputLine) {
        let stream = match line.stream {
            OutputStream::Stdout => "out",
            OutputStream::Stderr => "err",
        };
        self.write(|writer| writeln!(writer, "[{host}][{operation}][{stream}] {}", line.line));
    }

    fn operation_result(&self, result: &HostOperationReport) {
        let status = match result.status {
            HostOperationStatus::Success => "Success",
            HostOperationStatus::NoChange => "No changes",
            HostOperationStatus::Failed => "Failed",
            HostOperationStatus::IgnoredFailure => "Failed (ignored)",
        };
        self.write(|writer| {
            write!(writer, "[{}][{}] {status}", result.host, result.operation)?;
            if let Some(message) = &result.message {
                write!(writer, ": {message}")?;
            }
            writeln!(writer)
        });
    }

    fn deploy_finished(&self, summary: &DeploySummary) {
        self.write(|writer| {
            writeln!(writer, "rustinfra: operation summary")?;
            writeln!(
                writer,
                "{:<32} {:>5} {:>7} {:>9} {:>7} {:>7}",
                OPERATION_HEADER, HOSTS_HEADER, "SUCCESS", NO_CHANGE_HEADER, FAILED_HEADER, SKIPPED_HEADER
            )?;
            for operation in &summary.operations {
                writeln!(
                    writer,
                    "{:<32} {:>5} {:>7} {:>9} {:>7} {:>7}",
                    operation.operation,
                    operation.hosts,
                    operation.success,
                    operation.no_change,
                    operation.failed + operation.ignored_failure,
                    operation.skipped
                )?;
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use super::{
        DeployPreview, DeployReporter, HostOperationReport, HostOperationStatus, OperationPreview, TextReporter,
    };
    use crate::connectors::{OutputLine, OutputStream};

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().map_err(|_| io::Error::other("output lock poisoned"))?.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn text_reporter_renders_plan_streamed_output_status_and_summary() {
        let writer = SharedWriter::default();
        let reporter = TextReporter::new(writer.clone());
        let preview = DeployPreview {
            operations: vec![OperationPreview {
                id: "one".into(),
                name: "files.file".to_owned(),
                change_hosts: vec!["app-1".to_owned()],
                no_change_hosts: vec!["app-2".to_owned()],
            }],
        };
        let result = HostOperationReport {
            operation_id: "one".into(),
            operation: "files.file".to_owned(),
            host: "app-1".to_owned(),
            status: HostOperationStatus::Success,
            message: None,
        };

        reporter.proposed_changes(&preview);
        reporter.operation_output(
            "app-1",
            "files.file",
            &OutputLine { stream: OutputStream::Stdout, line: "updated".to_owned() },
        );
        reporter.operation_result(&result);
        reporter.deploy_finished(&super::DeploySummary::from_reports(&preview, &[result]));

        let output = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("rustinfra: proposed changes"));
        assert!(output.contains("files.file                       yes     app-1"));
        assert!(output.contains("files.file                       no      app-2"));
        assert!(output.contains("[app-1][files.file][out] updated"));
        assert!(output.contains("[app-1][files.file] Success"));
        assert!(output.contains("rustinfra: operation summary"));
        assert!(output.contains("files.file                           2       1         0       0       1"));
    }
}
