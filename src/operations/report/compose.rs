use std::env;
use std::io::{self, IsTerminal, Write};
use std::sync::Mutex;
use std::time::Duration;

use console::{Term, measure_text_width};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::{
    DeployPhase, DeployPreview, DeployReporter, DeploySummary, FAILED_HEADER, HOSTS_HEADER, HostOperationReport,
    HostOperationStatus, NO_CHANGE_HEADER, OPERATION_HEADER, SKIPPED_HEADER, TextReporter,
};
use crate::connectors::OutputLine;

struct ComposeState {
    phase: Option<DeployPhase>,
    progress: Option<ProgressBar>,
    completed: usize,
}

/// Docker Compose-inspired animated output for interactive terminals.
pub struct ComposeReporter {
    progress: MultiProgress,
    state: Mutex<ComposeState>,
    fallback: Option<TextReporter>,
    writer: Mutex<Box<dyn Write + Send>>,
    indicator_column: usize,
}

impl ComposeReporter {
    #[must_use]
    pub fn auto() -> Self {
        let supports_animation =
            io::stderr().is_terminal() && env::var_os(concat!("TER", "M")).is_none_or(|terminal| terminal != "dumb");
        if supports_animation {
            Self::animated_with_writer_at_width(
                ProgressDrawTarget::stderr(),
                io::stderr(),
                usize::from(Term::stderr().size().1),
            )
        } else {
            Self {
                progress: MultiProgress::with_draw_target(ProgressDrawTarget::hidden()),
                state: Mutex::new(ComposeState { phase: None, progress: None, completed: 0 }),
                fallback: Some(TextReporter::new(io::stderr())),
                writer: Mutex::new(Box::new(io::sink())),
                indicator_column: 80,
            }
        }
    }

    #[must_use]
    pub fn animated(draw_target: ProgressDrawTarget) -> Self {
        Self::animated_with_writer(draw_target, io::stderr())
    }

    #[must_use]
    pub fn animated_with_writer(draw_target: ProgressDrawTarget, writer: impl Write + Send + 'static) -> Self {
        Self::animated_with_writer_at_width(draw_target, writer, 80)
    }

    fn animated_with_writer_at_width(
        draw_target: ProgressDrawTarget,
        writer: impl Write + Send + 'static,
        indicator_column: usize,
    ) -> Self {
        Self {
            progress: MultiProgress::with_draw_target(draw_target),
            state: Mutex::new(ComposeState { phase: None, progress: None, completed: 0 }),
            fallback: None,
            writer: Mutex::new(Box::new(writer)),
            indicator_column,
        }
    }

    fn with_progress(&self, action: impl FnOnce(&ProgressBar, &mut ComposeState)) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(progress) = state.progress.clone() else {
            return;
        };
        action(&progress, &mut state);
    }

    fn print_line(&self, line: &str) {
        self.progress.suspend(|| {
            let Ok(mut writer) = self.writer.lock() else {
                return;
            };
            let _ = writeln!(writer, "{line}");
            let _ = writer.flush();
        });
    }

    fn print_status(&self, text: &str, symbol: &str) {
        let reserved = measure_text_width(symbol).saturating_add(1);
        let text_column = self.indicator_column.saturating_sub(reserved);
        let padding = text_column.saturating_sub(measure_text_width(text));
        self.print_line(&format!("{text}{} {symbol}", " ".repeat(padding)));
    }
}

fn new_progress(total: usize) -> ProgressBar {
    let progress = ProgressBar::new(total as u64);
    let style = ProgressStyle::with_template("{prefix:.bold} {pos}/{len} {wide_msg} {spinner:.cyan}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);
    progress.set_style(style);
    progress
}

impl DeployReporter for ComposeReporter {
    fn phase_started(&self, phase: DeployPhase, total: usize) {
        if self.fallback.is_some() {
            return;
        }
        let progress = self.progress.add(new_progress(total));
        progress.set_prefix(phase.label());
        progress.enable_steady_tick(Duration::from_millis(80));
        let Ok(mut state) = self.state.lock() else {
            progress.finish_and_clear();
            return;
        };
        state.phase = Some(phase);
        state.progress = Some(progress);
        state.completed = 0;
    }

    fn phase_progress(&self, phase: DeployPhase, completed: usize, message: &str) {
        if self.fallback.is_some() {
            return;
        }
        self.with_progress(|progress, state| {
            if state.phase == Some(phase) {
                progress.set_position(completed as u64);
                progress.set_message(message.to_owned());
                state.completed = completed;
            }
        });
    }

    fn phase_finished(&self, phase: DeployPhase) {
        if self.fallback.is_some() {
            return;
        }
        let progress = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.phase != Some(phase) {
                return;
            }
            state.phase = None;
            state.progress.take()
        };
        if let Some(progress) = progress {
            let total = progress.length().unwrap_or(progress.position());
            progress.set_position(total);
            progress.finish_and_clear();
            self.progress.remove(&progress);
            self.print_status(&format!("{} {total}/{total}", phase.completed_label()), "✓");
        }
    }

    fn operation_started(&self, host: &str, operation: &str) {
        if self.fallback.is_some() {
            return;
        }
        self.with_progress(|progress, state| {
            if state.phase == Some(DeployPhase::Apply) {
                progress.set_message(format!("{operation} on {host}"));
            }
        });
    }

    fn proposed_changes(&self, preview: &DeployPreview) {
        if let Some(fallback) = &self.fallback {
            fallback.proposed_changes(preview);
            return;
        }
        for operation in &preview.operations {
            let mut changes = Vec::new();
            if !operation.change_hosts.is_empty() {
                changes.push(format!("change: {}", operation.change_hosts.join(", ")));
            }
            if !operation.no_change_hosts.is_empty() {
                changes.push(format!("no change: {}", operation.no_change_hosts.join(", ")));
            }
            self.print_status(&format!(" {:<24} {}", operation.name, changes.join(" · ")), "✔");
        }
    }

    fn operation_output(&self, host: &str, operation: &str, line: &OutputLine) {
        if let Some(fallback) = &self.fallback {
            fallback.operation_output(host, operation, line);
            return;
        }
        self.print_line(&format!(" {host:<12} {operation:<24} | {}", line.line));
    }

    fn operation_result(&self, result: &HostOperationReport) {
        if let Some(fallback) = &self.fallback {
            fallback.operation_result(result);
            return;
        }
        let (symbol, status) = match result.status {
            HostOperationStatus::Success => ("✔", "Success"),
            HostOperationStatus::NoChange => ("─", "No changes"),
            HostOperationStatus::Failed => ("✘", "Failed"),
            HostOperationStatus::IgnoredFailure => ("!", "Failed (ignored)"),
        };
        let mut line = format!(" {:<12} {:<24} {status}", result.host, result.operation);
        if let Some(message) = &result.message {
            line.push_str(&format!(": {message}"));
        }
        self.with_progress(|progress, state| {
            state.completed += 1;
            progress.set_position(state.completed as u64);
        });
        self.print_status(&line, symbol);
    }

    fn deploy_finished(&self, summary: &DeploySummary) {
        if let Some(fallback) = &self.fallback {
            fallback.deploy_finished(summary);
            return;
        }
        let totals = summary.operations.iter().fold([0_usize; 5], |mut totals, operation| {
            totals[0] += operation.success;
            totals[1] += operation.no_change;
            totals[2] += operation.failed;
            totals[3] += operation.ignored_failure;
            totals[4] += operation.skipped;
            totals
        });
        let outcome = if totals[2] > 0 { "Deployment failed" } else { "Deployment complete" };
        self.print_line(outcome);
        self.print_line("");
        self.print_line(&format!(
            "{:<32} {:>5} {:>7} {:>9} {:>7} {:>7}",
            OPERATION_HEADER, HOSTS_HEADER, "CHANGED", NO_CHANGE_HEADER, FAILED_HEADER, SKIPPED_HEADER
        ));
        for operation in &summary.operations {
            self.print_line(&format!(
                "{:<32} {:>5} {:>7} {:>9} {:>7} {:>7}",
                operation.operation,
                operation.hosts,
                operation.success,
                operation.no_change,
                operation.failed + operation.ignored_failure,
                operation.skipped
            ));
        }
        self.print_line("");
        self.print_line(&format!(
            "{} changed · {} unchanged · {} failed · {} skipped",
            totals[0],
            totals[1],
            totals[2] + totals[3],
            totals[4]
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use console::measure_text_width;
    use indicatif::{InMemoryTerm, ProgressDrawTarget};

    use super::ComposeReporter;
    use crate::connectors::{OutputLine, OutputStream};
    use crate::operations::{
        DeployPhase, DeployPreview, DeployReporter, DeploySummary, HostOperationReport, HostOperationStatus,
        OperationPreview,
    };

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedWriter {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

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
    fn animated_reporter_keeps_logs_and_results_around_live_progress() {
        let terminal = InMemoryTerm::new(30, 140);
        let writer = SharedWriter::default();
        let reporter =
            ComposeReporter::animated_with_writer(ProgressDrawTarget::term_like(Box::new(terminal)), writer.clone());
        let preview = DeployPreview {
            operations: vec![OperationPreview {
                id: "files".into(),
                name: "files.file".to_owned(),
                change_hosts: vec!["web-01".to_owned()],
                no_change_hosts: vec!["web-02".to_owned()],
            }],
        };
        let result = HostOperationReport {
            operation_id: "files".into(),
            operation: "files.file".to_owned(),
            host: "web-01".to_owned(),
            status: HostOperationStatus::Success,
            message: None,
        };
        let failure = HostOperationReport {
            operation_id: "files".into(),
            operation: "files.file".to_owned(),
            host: "web-02".to_owned(),
            status: HostOperationStatus::Failed,
            message: Some("permission denied".to_owned()),
        };

        reporter.phase_started(DeployPhase::Plan, 2);
        reporter.phase_progress(DeployPhase::Plan, 2, "files.file on web-02");
        reporter.phase_finished(DeployPhase::Plan);
        reporter.proposed_changes(&preview);
        reporter.phase_started(DeployPhase::Apply, 2);
        reporter.operation_started("web-01", "files.file");
        reporter.operation_output(
            "web-01",
            "files.file",
            &OutputLine { stream: OutputStream::Stdout, line: "created".to_owned() },
        );
        reporter.operation_result(&result);
        reporter.operation_started("web-02", "files.file");
        reporter.operation_result(&failure);
        reporter.phase_finished(DeployPhase::Apply);
        reporter.deploy_finished(&DeploySummary::from_reports(&preview, &[result, failure]));

        let output = writer.text();
        let planned = output.lines().find(|line| line.starts_with("Planned 2/2")).unwrap();
        let success = output.lines().find(|line| line.contains("files.file               Success")).unwrap();
        let failure = output.lines().find(|line| line.contains("Failed: permission denied")).unwrap();
        assert!(planned.ends_with('✓'));
        assert!(success.ends_with('✔'));
        assert!(failure.ends_with('✘'));
        assert_eq!(measure_text_width(planned), 80);
        assert_eq!(measure_text_width(success), 80);
        assert_eq!(measure_text_width(failure), 80);
        assert!(output.contains("change: web-01 · no change: web-02"));
        assert!(output.contains("web-01       files.file               | created"));
        assert!(output.contains("Deployment failed"));
        assert!(output.contains("1 changed · 0 unchanged · 1 failed · 0 skipped"));
    }
}
