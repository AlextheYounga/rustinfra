use std::error::Error;

use crate::api::{CommandBit, StringCommand};
use crate::facts::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindInFileInput {
    pub path: String,
    pub pattern: String,
    pub interpolate_variables: bool,
    pub extended_regex: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FindInFile;

impl Fact for FindInFile {
    type Input = FindInFileInput;
    type Output = Option<Vec<String>>;

    fn name(&self) -> &'static str {
        "files.FindInFile"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        let marker = format!("__pyinfra_exists_{}", input.path);
        let grep = if input.extended_regex { "grep -E" } else { "grep" };
        let pattern = if input.interpolate_variables {
            CommandBit::from(double_quoted(&input.pattern))
        } else {
            CommandBit::quoted(input.pattern.clone())
        };
        StringCommand::new(vec![
            CommandBit::from(grep),
            CommandBit::from("-e"),
            pattern,
            CommandBit::quoted(input.path.clone()),
            CommandBit::from("2>/dev/null || ( find"),
            CommandBit::quoted(input.path.clone()),
            CommandBit::from("-type f >/dev/null && echo"),
            CommandBit::quoted(marker),
            CommandBit::from("|| true )"),
        ])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let marker = format!("__pyinfra_exists_{}", input.path);
        if output.first() == Some(&marker) { Ok(Some(Vec::new())) } else { Ok(Some(output.to_vec())) }
    }
}

fn double_quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::{Fact, FindInFile, FindInFileInput};

    #[test]
    fn distinguishes_missing_files_from_existing_files_without_matches() {
        let input = FindInFileInput {
            path: "/tmp/a file".to_owned(),
            pattern: "^line$".to_owned(),
            interpolate_variables: false,
            extended_regex: true,
        };
        let marker = "__pyinfra_exists_/tmp/a file".to_owned();

        assert_eq!(
            FindInFile.command(&input).raw_value(),
            "grep -E -e '^line$' '/tmp/a file' 2>/dev/null || ( find '/tmp/a file' -type f >/dev/null && echo '__pyinfra_exists_/tmp/a file' || true )"
        );
        assert_eq!(FindInFile.process(&[marker], &input).unwrap(), Some(Vec::new()));
        assert_eq!(FindInFile.process(&["line".to_owned()], &input).unwrap(), Some(vec!["line".to_owned()]));
        assert_eq!(FindInFile.default(), None);
    }
}
