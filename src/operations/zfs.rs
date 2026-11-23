use indexmap::IndexMap;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::zfs::{Datasets, Snapshots};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetInput {
    pub name: String,
    pub present: bool,
    pub recursive: bool,
    pub sparse: bool,
    pub volume_size: Option<String>,
    pub properties: IndexMap<String, String>,
}
impl DatasetInput {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            present: true,
            recursive: false,
            sparse: false,
            volume_size: None,
            properties: IndexMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Dataset;
impl StatefulOperation for Dataset {
    type Input = DatasetInput;
    fn name(&self) -> &'static str {
        "zfs.dataset"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let datasets = FactExecutor::new(connector, options).get(&Datasets, &()).await?;
            Ok(dataset_commands(input, datasets.get(&input.name)))
        })
    }
}

fn dataset_commands(input: &DatasetInput, current: Option<&IndexMap<String, String>>) -> Vec<Command> {
    let mut bits = Vec::new();
    if input.present && current.is_none() {
        bits.extend([raw("zfs"), raw("create")]);
        for (key, value) in &input.properties {
            bits.extend([raw("-o"), quoted(format!("{key}={value}"))]);
        }
        if input.recursive {
            bits.push(raw("-p"));
        }
        if input.sparse {
            bits.push(raw("-s"));
        }
        if let Some(size) = &input.volume_size {
            bits.extend([raw("-V"), quoted(size)]);
        }
        bits.push(quoted(&input.name));
    } else if input.present {
        let Some(current) = current else { return Vec::new() };
        let changed = input
            .properties
            .iter()
            .filter(|(key, value)| current.get(*key) != Some(*value))
            .map(|(key, value)| quoted(format!("{key}={value}")))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Vec::new();
        }
        bits.extend([raw("zfs"), raw("set")]);
        bits.extend(changed);
        bits.push(quoted(&input.name));
    } else if current.is_some() {
        bits.extend([raw("zfs"), raw("destroy")]);
        if input.recursive {
            bits.push(raw("-r"));
        }
        bits.push(quoted(&input.name));
    } else {
        return Vec::new();
    }
    vec![Command::Shell(StringCommand::new(bits))]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotInput {
    pub dataset: DatasetInput,
}
impl SnapshotInput {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { dataset: DatasetInput::new(name) }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot;
impl StatefulOperation for Snapshot {
    type Input = SnapshotInput;
    fn name(&self) -> &'static str {
        "zfs.snapshot"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, options);
            let snapshots = facts.get(&Snapshots, &()).await?;
            if snapshots.contains_key(&input.dataset.name) || !input.dataset.present {
                let datasets = facts.get(&Datasets, &()).await?;
                return Ok(dataset_commands(&input.dataset, datasets.get(&input.dataset.name)));
            }
            let mut bits = vec![raw("zfs"), raw("snap")];
            for (key, value) in &input.dataset.properties {
                bits.extend([raw("-o"), quoted(format!("{key}={value}"))]);
            }
            if input.dataset.recursive {
                bits.push(raw("-r"));
            }
            bits.push(quoted(&input.dataset.name));
            Ok(vec![Command::Shell(StringCommand::new(bits))])
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Filesystem;
impl StatefulOperation for Filesystem {
    type Input = DatasetInput;
    fn name(&self) -> &'static str {
        "zfs.filesystem"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Dataset.commands(input, connector, options)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Volume;
impl StatefulOperation for Volume {
    type Input = DatasetInput;
    fn name(&self) -> &'static str {
        "zfs.volume"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Dataset.commands(input, connector, options)
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_sets_destroys_and_noops() {
        let mut input = DatasetInput::new("tank/my data");
        input.properties.insert("compression".to_owned(), "lz4".to_owned());
        assert!(
            matches!(&dataset_commands(&input,None)[0],Command::Shell(value)if value.raw_value()=="zfs create -o compression=lz4 'tank/my data'")
        );
        let mut current = IndexMap::new();
        current.insert("compression".to_owned(), "off".to_owned());
        assert!(
            matches!(&dataset_commands(&input,Some(&current))[0],Command::Shell(value)if value.raw_value()=="zfs set compression=lz4 'tank/my data'")
        );
        current.insert("compression".to_owned(), "lz4".to_owned());
        assert!(dataset_commands(&input, Some(&current)).is_empty());
        input.present = false;
        input.recursive = true;
        assert!(
            matches!(&dataset_commands(&input,Some(&current))[0],Command::Shell(value)if value.raw_value()=="zfs destroy -r 'tank/my data'")
        );
    }
}
