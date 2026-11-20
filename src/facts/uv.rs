use std::error::Error;

use serde_json::Value;

use crate::api::StringCommand;

use super::Fact;
use super::packages::PackageMap;

const UV: &str = "uv";

fn process_json(output: &[String], key: &str, value: &str) -> PackageMap {
    let Ok(Value::Array(mut items)) = serde_json::from_str::<Value>(&output.join("\n")) else {
        return PackageMap::new();
    };
    items.sort_by(|left, right| {
        left.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(right.get(key).and_then(Value::as_str).unwrap_or_default())
    });
    let mut result = PackageMap::new();
    for item in items {
        if let (Some(key), Some(value)) =
            (item.get(key).and_then(Value::as_str), item.get(value).and_then(Value::as_str))
        {
            result.insert(key.to_owned(), vec![value.to_owned()]);
        }
    }
    result
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UvPipPackages;

impl Fact for UvPipPackages {
    type Input = ();
    type Output = PackageMap;
    fn name(&self) -> &'static str {
        "uv.UvPipPackages"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["uv pip list --format json"])
    }
    fn default(&self) -> Self::Output {
        PackageMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some(UV)
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(process_json(output, "name", "version"))
    }
}

fn python_command(managed: bool, installed: bool) -> StringCommand {
    StringCommand::new([format!(
        "uv python list {} {} --output-format=json",
        if managed { "--managed-python" } else { "" },
        if installed { "--only-installed" } else { "" }
    )])
}

macro_rules! python_fact {
    ($name:ident, $fact_name:literal, $installed:literal, $key:literal, $value:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = bool;
            type Output = PackageMap;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, managed: &Self::Input) -> StringCommand {
                python_command(*managed, $installed)
            }
            fn default(&self) -> Self::Output {
                PackageMap::new()
            }
            fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
                Some(UV)
            }
            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(process_json(output, $key, $value))
            }
        }
    };
}

python_fact!(UvAvailablePythonsByImplementation, "uv.UvAvailablePythonsByImplementation", false, "key", "version");
python_fact!(UvAvailablePythonsByVersion, "uv.UvAvailablePythonsByVersion", false, "version", "key");
python_fact!(UvInstalledPythonsByImplementation, "uv.UvInstalledPythonsByImplementation", true, "key", "version");
python_fact!(UvInstalledPythonsByVersion, "uv.UvInstalledPythonsByVersion", true, "version", "key");

macro_rules! single_line_fact {
    ($name:ident, $fact_name:literal, $command:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = String;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _input: &Self::Input) -> StringCommand {
                StringCommand::new([$command])
            }
            fn default(&self) -> Self::Output {
                String::new()
            }
            fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
                Some(UV)
            }
            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(if output.len() == 1 { output[0].clone() } else { String::new() })
            }
        }
    };
}

single_line_fact!(UvPythonDir, "uv.UvPythonDir", "uv python dir 2>/dev/null");
single_line_fact!(UvToolDir, "uv.UvToolDir", "uv tool dir 2>/dev/null");
single_line_fact!(UvVersion, "uv.UvVersion", "uv --version");

#[derive(Clone, Copy, Debug, Default)]
pub struct UvTools;

impl Fact for UvTools {
    type Input = ();
    type Output = PackageMap;
    fn name(&self) -> &'static str {
        "uv.UvTools"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["uv tool list 2>/dev/null"])
    }
    fn default(&self) -> Self::Output {
        PackageMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some(UV)
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut tools = PackageMap::new();
        for line in output.iter().filter(|line| !line.starts_with("- ")) {
            let mut fields = line.split_whitespace();
            if let (Some(name), Some(version)) = (fields.next(), fields.next()) {
                tools.insert(name.to_owned(), vec![version.trim_start_matches('v').to_owned()]);
            }
        }
        Ok(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, UvInstalledPythonsByVersion, UvPipPackages, UvTools, UvVersion};

    #[test]
    fn json_facts_parse_expected_keys_and_reject_non_lists() {
        let output = [r#"[{"name":"requests","version":"2.32.5"}]"#.to_owned()];
        assert_eq!(UvPipPackages.process(&output, &()).unwrap()["requests"], ["2.32.5"]);
        assert!(UvPipPackages.process(&["{}".to_owned()], &()).unwrap().is_empty());
        let pythons = [r#"[{"key":"cpython-3.13-linux","version":"3.13.4"}]"#.to_owned()];
        assert_eq!(UvInstalledPythonsByVersion.process(&pythons, &true).unwrap()["3.13.4"], ["cpython-3.13-linux"]);
        assert_eq!(
            UvInstalledPythonsByVersion.command(&true).raw_value(),
            "uv python list --managed-python --only-installed --output-format=json"
        );
    }

    #[test]
    fn tool_and_single_line_facts_ignore_auxiliary_output() {
        let tools = UvTools.process(&["ruff v0.14.4".to_owned(), "- ruff".to_owned()], &()).unwrap();
        assert_eq!(tools["ruff"], ["0.14.4"]);
        assert_eq!(UvVersion.process(&["uv 0.8.5".to_owned()], &()).unwrap(), "uv 0.8.5");
        assert_eq!(UvVersion.process(&["uv 0.8.5".to_owned(), "extra".to_owned()], &()).unwrap(), "");
    }
}
