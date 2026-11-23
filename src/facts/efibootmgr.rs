use std::error::Error;
use std::io;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EfiBootManagerInfo {
    pub boot_next: Option<u16>,
    pub boot_current: Option<u16>,
    pub timeout: Option<u32>,
    pub boot_order: Vec<u16>,
    pub entries: IndexMap<u16, (bool, String)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EfiBootManager;

impl Fact for EfiBootManager {
    type Input = ();
    type Output = Option<EfiBootManagerInfo>;

    fn name(&self) -> &'static str {
        "efibootmgr.EFIBootMgr"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["efibootmgr || true"])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("efibootmgr")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        if output.is_empty() {
            return Ok(None);
        }
        let mut info = EfiBootManagerInfo {
            boot_next: None,
            boot_current: None,
            timeout: None,
            boot_order: Vec::new(),
            entries: IndexMap::new(),
        };
        for line in output {
            if let Some(value) = line.strip_prefix("BootNext: ") {
                info.boot_next = Some(u16::from_str_radix(value, 16)?);
            } else if let Some(value) = line.strip_prefix("BootCurrent: ") {
                info.boot_current = Some(u16::from_str_radix(value, 16)?);
            } else if let Some(value) = line.strip_prefix("Timeout: ").and_then(|value| value.strip_suffix(" seconds"))
            {
                info.timeout = Some(value.parse()?);
            } else if let Some(value) = line.strip_prefix("BootOrder: ") {
                info.boot_order =
                    value.split(',').map(|entry| u16::from_str_radix(entry, 16)).collect::<Result<_, _>>()?;
            } else if line.starts_with("Boot") && line.len() >= 10 {
                let number = u16::from_str_radix(&line[4..8], 16)?;
                info.entries.insert(number, (&line[8..9] == "*", line[10..].to_owned()));
            } else {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidData, format!("unexpected efibootmgr line: {line}")).into()
                );
            }
        }
        Ok(Some(info))
    }
}

#[cfg(test)]
mod tests {
    use super::{EfiBootManager, Fact};

    #[test]
    fn parses_optional_header_fields_order_and_entries() {
        let info = EfiBootManager
            .process(
                &[
                    "BootNext: 0006".to_owned(),
                    "BootCurrent: 0006".to_owned(),
                    "Timeout: 2 seconds".to_owned(),
                    "BootOrder: 0006,0001".to_owned(),
                    "Boot0001* Windows Boot Manager".to_owned(),
                    "Boot0006  debian".to_owned(),
                ],
                &(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(info.boot_next, Some(6));
        assert_eq!(info.boot_order, [6, 1]);
        assert_eq!(info.entries[&1], (true, "Windows Boot Manager".to_owned()));
        assert_eq!(info.entries[&6], (false, "debian".to_owned()));
    }
}
