use crate::connectors::{CommandOutput, OutputStream};

/// Reassembles arbitrarily chunked SSH channel data into output lines.
#[derive(Default)]
pub(super) struct OutputCollector {
    output: CommandOutput,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl OutputCollector {
    pub(super) fn push(&mut self, stream: OutputStream, bytes: &[u8]) {
        let pending = match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr => &mut self.stderr,
        };
        pending.extend_from_slice(bytes);
        while let Some(position) = pending.iter().position(|byte| *byte == b'\n') {
            let mut line = pending.drain(..=position).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.output.push(stream, String::from_utf8_lossy(&line).into_owned());
        }
    }

    pub(super) fn finish(mut self) -> CommandOutput {
        if !self.stdout.is_empty() {
            self.output.push(OutputStream::Stdout, String::from_utf8_lossy(&self.stdout).into_owned());
        }
        if !self.stderr.is_empty() {
            self.output.push(OutputStream::Stderr, String::from_utf8_lossy(&self.stderr).into_owned());
        }
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::OutputCollector;
    use crate::connectors::OutputStream;

    #[test]
    fn output_chunks_are_split_into_lines_without_losing_stream_identity() {
        let mut collector = OutputCollector::default();
        collector.push(OutputStream::Stdout, b"first\nsec");
        collector.push(OutputStream::Stderr, b"warning\r\n");
        collector.push(OutputStream::Stdout, b"ond");

        let output = collector.finish();

        assert_eq!(output.stdout_lines(), vec!["first", "second"]);
        assert_eq!(output.stderr_lines(), vec!["warning"]);
    }
}
