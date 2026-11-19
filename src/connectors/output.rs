/// Identifies the process stream that produced a line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// One line emitted by a command, retaining its source stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub line: String,
}

/// Interleaved stdout and stderr from a command.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandOutput {
    combined_lines: Vec<OutputLine>,
}

impl CommandOutput {
    pub(crate) fn push(&mut self, stream: OutputStream, line: String) {
        self.combined_lines.push(OutputLine { stream, line });
    }

    pub(crate) fn extend(&mut self, output: Self) {
        self.combined_lines.extend(output.combined_lines);
    }

    /// Returns lines in the order they were observed.
    #[must_use]
    pub fn combined_lines(&self) -> &[OutputLine] {
        &self.combined_lines
    }

    /// Returns all output text in observed order.
    #[must_use]
    pub fn output(&self) -> String {
        join_lines(self.combined_lines.iter().map(|line| line.line.as_str()))
    }

    /// Returns stdout lines in observed order.
    #[must_use]
    pub fn stdout_lines(&self) -> Vec<&str> {
        self.lines_for(OutputStream::Stdout)
    }

    /// Returns stdout as newline-separated text.
    #[must_use]
    pub fn stdout(&self) -> String {
        join_lines(self.stdout_lines())
    }

    /// Returns stderr lines in observed order.
    #[must_use]
    pub fn stderr_lines(&self) -> Vec<&str> {
        self.lines_for(OutputStream::Stderr)
    }

    /// Returns stderr as newline-separated text.
    #[must_use]
    pub fn stderr(&self) -> String {
        join_lines(self.stderr_lines())
    }

    fn lines_for(&self, stream: OutputStream) -> Vec<&str> {
        self.combined_lines.iter().filter(|line| line.stream == stream).map(|line| line.line.as_str()).collect()
    }
}

fn join_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    lines.into_iter().collect::<Vec<_>>().join("\n")
}
