use std::error::Error;

use thiserror::Error;

use crate::api::{Command, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{FindInFile, FindInFileInput};
use crate::facts::server::Locales;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{Line, LineInput};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

const LOCALE_DEFINITIONS_FILE: &str = "/etc/locale.gen";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocaleInput {
    pub locale: String,
    pub present: bool,
}

impl LocaleInput {
    #[must_use]
    pub fn new(locale: impl Into<String>) -> Self {
        Self { locale: locale.into(), present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Locale;

impl StatefulOperation for Locale {
    type Input = LocaleInput;

    fn name(&self) -> &'static str {
        "server.locale"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, fact_options);
            let locales = facts.get(&Locales, &()).await?;
            let matches = facts
                .get(
                    &FindInFile,
                    &FindInFileInput {
                        path: LOCALE_DEFINITIONS_FILE.to_owned(),
                        pattern: format!("^.*{}[[:space:]]\\+.*$", input.locale),
                        interpolate_variables: false,
                        extended_regex: false,
                    },
                )
                .await?;
            let matching_line = one_locale_match(input, matches.as_deref())?;
            if locales.contains(&input.locale) == input.present {
                return Ok(Vec::new());
            }
            let line = locale_line(input, matching_line);
            let mut commands = Line.commands(&line, connector, fact_options).await?;
            commands.push(Command::Shell(StringCommand::new(["locale-gen"])));
            Ok(commands)
        })
    }
}

fn locale_line(input: &LocaleInput, matching_line: &str) -> LineInput {
    let mut line = LineInput::new(LOCALE_DEFINITIONS_FILE, format!("^{matching_line}$"));
    line.replace = Some(if input.present { matching_line.replace("# ", "") } else { format!("# {matching_line}") });
    line
}

fn one_locale_match<'a>(
    input: &LocaleInput,
    matches: Option<&'a [String]>,
) -> Result<&'a str, Box<dyn Error + Send + Sync>> {
    match matches.unwrap_or_default() {
        [] => Err(Box::new(SystemConfigError::LocaleNotFound { locale: input.locale.clone() })),
        [line] => Ok(line),
        _ => Err(Box::new(SystemConfigError::MultipleLocaleMatches { locale: input.locale.clone() })),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityLimitInput {
    pub domain: String,
    pub limit_type: String,
    pub item: String,
    pub value: i64,
}

impl SecurityLimitInput {
    #[must_use]
    pub fn new(domain: impl Into<String>, limit_type: impl Into<String>, item: impl Into<String>, value: i64) -> Self {
        Self { domain: domain.into(), limit_type: limit_type.into(), item: item.into(), value }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SecurityLimit;

impl StatefulOperation for SecurityLimit {
    type Input = SecurityLimitInput;

    fn name(&self) -> &'static str {
        "server.security_limit"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let line = security_limit_line(input);
            Line.commands(&line, connector, fact_options).await
        })
    }
}

fn security_limit_line(input: &SecurityLimitInput) -> LineInput {
    let mut line = LineInput::new(
        "/etc/security/limits.conf",
        format!("^{}[[:space:]]+{}[[:space:]]+{}", input.domain, input.limit_type, input.item),
    );
    line.replace = Some(format!("{}\t{}\t{}\t{}", input.domain, input.limit_type, input.item, input.value));
    line
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SystemConfigError {
    #[error("Locale {locale} not found in {LOCALE_DEFINITIONS_FILE}")]
    LocaleNotFound { locale: String },
    #[error("Multiple locales matches for {locale} in {LOCALE_DEFINITIONS_FILE}")]
    MultipleLocaleMatches { locale: String },
}

#[cfg(test)]
mod tests {
    use super::{LocaleInput, SecurityLimitInput, locale_line, one_locale_match, security_limit_line};

    #[test]
    fn locale_requires_exactly_one_definition() {
        let input = LocaleInput::new("en_GB.UTF-8");
        assert!(one_locale_match(&input, Some(&[])).is_err());
        assert_eq!(one_locale_match(&input, Some(&["# en_GB.UTF-8 UTF-8".to_owned()])).unwrap(), "# en_GB.UTF-8 UTF-8");
        assert!(one_locale_match(&input, Some(&["one".to_owned(), "two".to_owned()])).is_err());

        let enabled = locale_line(&input, "# en_GB.UTF-8 UTF-8");
        assert_eq!(enabled.replace.as_deref(), Some("en_GB.UTF-8 UTF-8"));
        let mut absent = input;
        absent.present = false;
        let disabled = locale_line(&absent, "en_GB.UTF-8 UTF-8");
        assert_eq!(disabled.replace.as_deref(), Some("# en_GB.UTF-8 UTF-8"));
    }

    #[test]
    fn security_limit_replaces_matching_domain_type_and_item() {
        let line = security_limit_line(&SecurityLimitInput::new("*", "soft", "nofile", 1024));
        assert_eq!(line.path, "/etc/security/limits.conf");
        assert_eq!(line.line, "^*[[:space:]]+soft[[:space:]]+nofile");
        assert_eq!(line.replace.as_deref(), Some("*\tsoft\tnofile\t1024"));
    }
}
