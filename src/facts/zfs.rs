use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

type ZfsProperties = IndexMap<String, IndexMap<String, String>>;

fn parse_properties(output: &[String]) -> ZfsProperties {
    let mut datasets = ZfsProperties::new();
    for line in output {
        let fields = line.split('\t').collect::<Vec<_>>();
        if let [dataset, property, value, _source] = fields.as_slice() {
            datasets.entry((*dataset).to_owned()).or_default().insert((*property).to_owned(), (*value).to_owned());
        }
    }
    datasets
}

macro_rules! zfs_fact {
    ($name:ident, $fact_name:literal, $command:literal, $required:literal, $filter:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = ZfsProperties;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _: &()) -> StringCommand {
                StringCommand::new([$command])
            }
            fn default(&self) -> Self::Output {
                IndexMap::new()
            }
            fn requires_command(&self, _: &()) -> Option<&str> {
                Some($required)
            }
            fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(parse_properties(output).into_iter().filter($filter).collect())
            }
        }
    };
}

zfs_fact!(Pools, "zfs.Pools", "zpool get -H all", "zpool", |_: &(String, IndexMap<String, String>)| true);
zfs_fact!(Datasets, "zfs.Datasets", "zfs get -H all", "zfs", |_: &(String, IndexMap<String, String>)| true);
zfs_fact!(Filesystems, "zfs.Filesystems", "zfs get -H all", "zfs", |item: &(String, IndexMap<String, String>)| item
    .1
    .get("type")
    .is_some_and(|value| value == "filesystem"));
zfs_fact!(Snapshots, "zfs.Snapshots", "zfs get -H all", "zfs", |item: &(String, IndexMap<String, String>)| item
    .1
    .get("type")
    .is_some_and(|value| value == "snapshot"));
zfs_fact!(Volumes, "zfs.Volumes", "zfs get -H all", "zfs", |item: &(String, IndexMap<String, String>)| item
    .1
    .get("type")
    .is_some_and(|value| value == "volume"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_properties_and_filters_dataset_types() {
        let output = vec![
            "tank\ttype\tfilesystem\t-".to_owned(),
            "tank\tsize\t2.71T\t-".to_owned(),
            "tank/home@old\ttype\tsnapshot\t-".to_owned(),
            "tank/myvol\ttype\tvolume\t-".to_owned(),
        ];
        assert_eq!(Datasets.process(&output, &()).unwrap()["tank"]["size"], "2.71T");
        assert!(Filesystems.process(&output, &()).unwrap().contains_key("tank"));
        assert!(Snapshots.process(&output, &()).unwrap().contains_key("tank/home@old"));
        assert!(Volumes.process(&output, &()).unwrap().contains_key("tank/myvol"));
        assert_eq!(Pools.command(&()).raw_value(), "zpool get -H all");
    }
}
