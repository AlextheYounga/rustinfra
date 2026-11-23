use std::error::Error;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub service: String,
    pub jail: Option<String>,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), jail: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ServiceScript;

impl Fact for ServiceScript {
    type Input = ServiceInput;
    type Output = String;

    fn name(&self) -> &'static str {
        "freebsd.ServiceScript"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        let jail = quote(input.jail.as_deref().unwrap_or_default());
        let service = quote(&input.service);
        StringCommand::new([format!(
            "for service in `service -j {jail} -l`; do if [ {service} = \"$service\" ]; then echo \"$service\"; fi; done"
        )])
    }
    fn default(&self) -> Self::Output {
        String::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned().unwrap_or_default())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ServiceStatus;

impl Fact for ServiceStatus {
    type Input = ServiceInput;
    type Output = String;

    fn name(&self) -> &'static str {
        "freebsd.ServiceStatus"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([format!(
            "service -j {} {} status > /dev/null 2>&1; if [ $? -eq 0 ]; then echo running; fi",
            quote(input.jail.as_deref().unwrap_or_default()),
            quote(&input.service)
        )])
    }
    fn default(&self) -> Self::Output {
        String::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned().unwrap_or_default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueInput {
    pub value: String,
    pub jail: Option<String>,
}

impl ValueInput {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into(), jail: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sysrc;

impl Fact for Sysrc {
    type Input = ValueInput;
    type Output = String;

    fn name(&self) -> &'static str {
        "freebsd.Sysrc"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        let value = quote(&input.value);
        let command = input.jail.as_deref().map_or_else(
            || format!("sysrc -in -- {value} || true"),
            |jail| format!("sysrc -j {} -in -- {value} || true", quote(jail)),
        );
        StringCommand::new([command])
    }
    fn default(&self) -> Self::Output {
        String::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned().unwrap_or_default())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PkgPackage;

impl Fact for PkgPackage {
    type Input = ValueInput;
    type Output = String;

    fn name(&self) -> &'static str {
        "freebsd.PkgPackage"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        let package = quote(&input.value);
        let command = input.jail.as_deref().map_or_else(
            || format!("pkg info -E -- {package} 2> /dev/null || true"),
            |jail| format!("pkg -j {} info -E -- {package} 2> /dev/null || true", quote(jail)),
        );
        StringCommand::new([command])
    }
    fn default(&self) -> Self::Output {
        String::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned().unwrap_or_default())
    }
}

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}

#[cfg(test)]
mod tests {
    use super::{Fact, PkgPackage, ServiceInput, ServiceScript, ServiceStatus, Sysrc, ValueInput};

    #[test]
    fn commands_match_host_and_jail_reference_forms() {
        let service = ServiceInput::new("supervisord");
        assert_eq!(
            ServiceScript.command(&service).raw_value(),
            "for service in `service -j '' -l`; do if [ supervisord = \"$service\" ]; then echo \"$service\"; fi; done"
        );
        assert_eq!(
            ServiceStatus.command(&ServiceInput::new("nginx")).raw_value(),
            "service -j '' nginx status > /dev/null 2>&1; if [ $? -eq 0 ]; then echo running; fi"
        );
        assert_eq!(
            Sysrc.command(&ValueInput::new("memcached_enable")).raw_value(),
            "sysrc -in -- memcached_enable || true"
        );
        let mut package = ValueInput::new("screen");
        package.jail = Some("nginx".to_owned());
        assert_eq!(PkgPackage.command(&package).raw_value(), "pkg -j nginx info -E -- screen 2> /dev/null || true");
    }

    #[test]
    fn quotes_all_user_values() {
        let mut input = ValueInput::new("screen; reboot");
        input.jail = Some("jail; id".to_owned());
        assert_eq!(
            PkgPackage.command(&input).raw_value(),
            "pkg -j 'jail; id' info -E -- 'screen; reboot' 2> /dev/null || true"
        );
    }
}
