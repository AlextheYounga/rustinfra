use std::error::Error;

use crate::api::{CommandBit, StringCommand};
use crate::facts::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindLinksInput {
    pub path: String,
    pub path_match: Option<String>,
}

impl FindLinksInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), path_match: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FindLinks;

impl Fact for FindLinks {
    type Input = FindLinksInput;
    type Output = Vec<String>;
    fn name(&self) -> &'static str {
        "files.FindLinks"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        let mut bits =
            vec![CommandBit::from("find"), CommandBit::quoted(input.path.as_str()), CommandBit::from("-type l")];
        if let Some(path_match) = &input.path_match {
            bits.extend([CommandBit::from("-path"), CommandBit::quoted(path_match.as_str())]);
        }
        bits.push(CommandBit::from("|| true"));
        StringCommand::new(bits)
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, FindLinks, FindLinksInput};

    #[test]
    fn path_glob_is_quoted_for_find_not_the_shell() {
        let mut input = FindLinksInput::new("/etc");
        input.path_match = Some("/etc/rc*.d/S*x; reboot".to_owned());
        assert_eq!(FindLinks.command(&input).raw_value(), "find /etc -type l -path '/etc/rc*.d/S*x; reboot' || true");
    }
}
