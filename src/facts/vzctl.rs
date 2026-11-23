use std::error::Error;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenvzContainers;
impl Fact for OpenvzContainers {
    type Input = ();
    type Output = IndexMap<i64, Map<String, Value>>;
    fn name(&self) -> &'static str {
        "vzctl.OpenvzContainers"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["vzlist -a -j"])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("vzlist")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let values = serde_json::from_str::<Vec<Map<String, Value>>>(&output.join(""))?;
        Ok(values
            .into_iter()
            .filter_map(|mut value| {
                let id = value.shift_remove("ctid")?.as_i64()?;
                Some((id, value))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_containers_by_numeric_ctid() {
        let containers =
            OpenvzContainers.process(&[r#"[{"ctid":100},{"ctid":101,"state":"running"}]"#.to_owned()], &()).unwrap();
        assert!(containers[&100].is_empty());
        assert_eq!(containers[&101]["state"], "running");
    }
}
