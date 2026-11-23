use std::error::Error;

use serde_json::Value;

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Copy, Debug, Default)]
pub struct LxdContainers;
impl Fact for LxdContainers {
    type Input = ();
    type Output = Vec<Value>;
    fn name(&self) -> &'static str {
        "lxd.LxdContainers"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["lxc list --format json --fast"])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("lxc")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(serde_json::from_str(&output.join(""))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_container_json() {
        let containers = LxdContainers.process(&[r#"[{"name":"one","status":"Running"}]"#.to_owned()], &()).unwrap();
        assert_eq!(containers[0]["name"], "one");
    }
}
