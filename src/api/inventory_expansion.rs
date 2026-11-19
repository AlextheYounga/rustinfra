use std::sync::Arc;

use indexmap::IndexMap;

use super::{ConnectorKind, Data, HostSpec, Inventory, InventoryError};

/// An inventory host before connector-name expansion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawHostSpec {
    pub name: String,
    pub data: Data,
}

impl RawHostSpec {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), data: Data::new() }
    }

    #[must_use]
    pub fn with_data(mut self, data: Data) -> Self {
        self.data = data;
        self
    }
}

/// Raw host names and shared data belonging to one group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawGroupSpec {
    pub name: String,
    pub hosts: Vec<RawHostSpec>,
    pub data: Data,
}

/// Unexpanded external inventory input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawInventorySpec {
    pub hosts: Vec<RawHostSpec>,
    pub data: Data,
    pub override_data: Data,
    pub groups: Vec<RawGroupSpec>,
}

/// One host produced by an inventory connector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpandedHost {
    pub name: String,
    pub data: Data,
    pub groups: Vec<String>,
}

/// Expands `@connector/argument` inventory names.
pub trait InventoryConnector: Send + Sync {
    fn handles_execution(&self) -> bool;
    fn expand(&self, argument: Option<&str>) -> Result<Vec<ExpandedHost>, String>;
}

/// Explicit registry of connector names available to inventory expansion.
#[derive(Default)]
pub struct ConnectorRegistry {
    connectors: IndexMap<String, Arc<dyn InventoryConnector>>,
}

impl ConnectorRegistry {
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        registry.register("local", Arc::new(LocalInventoryConnector));
        registry.register("ssh", Arc::new(SshInventoryConnector));
        registry
    }

    pub fn register(&mut self, name: impl Into<String>, connector: Arc<dyn InventoryConnector>) {
        self.connectors.insert(name.into(), connector);
    }

    pub fn get(&self, name: &str) -> Result<&dyn InventoryConnector, InventoryError> {
        self.connectors.get(name).map(AsRef::as_ref).ok_or_else(|| InventoryError::InvalidConnector(name.to_owned()))
    }

    pub fn get_execution(&self, name: &str) -> Result<&dyn InventoryConnector, InventoryError> {
        let connector = self.get(name)?;
        if connector.handles_execution() {
            Ok(connector)
        } else {
            Err(InventoryError::ConnectorCannotExecute { connector: name.to_owned() })
        }
    }
}

impl Inventory {
    /// Expands connector inventory names and builds the final ordered inventory.
    pub fn from_raw(spec: RawInventorySpec, registry: &ConnectorRegistry) -> Result<Self, InventoryError> {
        let mut raw_data: IndexMap<String, Data> = IndexMap::new();
        let mut raw_groups: IndexMap<String, Vec<String>> = IndexMap::new();
        let group_data =
            spec.groups.iter().map(|group| (group.name.clone(), group.data.clone())).collect::<IndexMap<_, _>>();

        for group in &spec.groups {
            for host in &group.hosts {
                merge_data(raw_data.entry(host.name.clone()).or_default(), host.data.clone());
                raw_groups.entry(host.name.clone()).or_default().push(group.name.clone());
            }
        }
        for host in &spec.hosts {
            merge_data(raw_data.entry(host.name.clone()).or_default(), host.data.clone());
        }

        let mut hosts = Vec::new();
        for raw_host in &spec.hosts {
            let host_data = raw_data.get(&raw_host.name).cloned().unwrap_or_default();
            for (expanded, connector) in expand_host(raw_host, registry)? {
                let mut data = expanded.data;
                merge_data(&mut data, host_data.clone());
                let mut groups = expanded.groups;
                if let Some(attached) = raw_groups.get(&raw_host.name) {
                    groups.extend(attached.iter().cloned());
                }
                hosts.push((HostSpec { name: expanded.name, data, connector }, groups));
            }
        }

        Inventory::from_expanded(hosts, spec.data, spec.override_data, group_data)
    }
}

fn expand_host(
    host: &RawHostSpec,
    registry: &ConnectorRegistry,
) -> Result<Vec<(ExpandedHost, ConnectorKind)>, InventoryError> {
    let Some(connector_name) = host.name.strip_prefix('@') else {
        registry.get_execution("ssh")?;
        return Ok(vec![(
            ExpandedHost { name: host.name.clone(), data: Data::new(), groups: Vec::new() },
            ConnectorKind::Ssh,
        )]);
    };
    let (name, argument) =
        connector_name.split_once('/').map_or((connector_name, None), |(name, value)| (name, Some(value)));
    if name == "local" && argument.is_some() {
        return Err(InventoryError::MultipleLocalHosts);
    }
    let connector = registry.get(name)?;
    let kind = if connector.handles_execution() {
        connector_kind(name)
    } else {
        registry.get_execution("ssh")?;
        ConnectorKind::Ssh
    };
    connector
        .expand(argument)
        .map_err(|message| InventoryError::ConnectorExpansion { connector: name.to_owned(), message })
        .map(|hosts| hosts.into_iter().map(|host| (host, kind.clone())).collect())
}

fn connector_kind(name: &str) -> ConnectorKind {
    match name {
        "local" => ConnectorKind::Local,
        "ssh" => ConnectorKind::Ssh,
        name => ConnectorKind::External(name.to_owned()),
    }
}

fn merge_data(target: &mut Data, source: Data) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

struct LocalInventoryConnector;

impl InventoryConnector for LocalInventoryConnector {
    fn handles_execution(&self) -> bool {
        true
    }

    fn expand(&self, argument: Option<&str>) -> Result<Vec<ExpandedHost>, String> {
        if argument.is_some() {
            return Err("Cannot have more than one @local".to_owned());
        }
        Ok(vec![ExpandedHost { name: "@local".to_owned(), data: Data::new(), groups: vec!["@local".to_owned()] }])
    }
}

struct SshInventoryConnector;

impl InventoryConnector for SshInventoryConnector {
    fn handles_execution(&self) -> bool {
        true
    }

    fn expand(&self, argument: Option<&str>) -> Result<Vec<ExpandedHost>, String> {
        let hostname = argument.ok_or_else(|| "@ssh requires a hostname".to_owned())?;
        let mut data = Data::new();
        data.insert("ssh_hostname".to_owned(), hostname.into());
        Ok(vec![ExpandedHost { name: format!("@ssh/{hostname}"), data, groups: Vec::new() }])
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{ConnectorRegistry, ExpandedHost, InventoryConnector, RawGroupSpec, RawHostSpec, RawInventorySpec};
    use crate::api::{ConnectorKind, Data, Inventory, InventoryError};

    struct NamesOnlyConnector;

    impl InventoryConnector for NamesOnlyConnector {
        fn handles_execution(&self) -> bool {
            false
        }

        fn expand(&self, argument: Option<&str>) -> Result<Vec<ExpandedHost>, String> {
            Ok(vec![ExpandedHost {
                name: argument.unwrap_or("generated").to_owned(),
                data: [("provider".to_owned(), json!(true))].into_iter().collect(),
                groups: vec!["provided".to_owned()],
            }])
        }
    }

    struct PerHostGroupsConnector;

    impl InventoryConnector for PerHostGroupsConnector {
        fn handles_execution(&self) -> bool {
            false
        }

        fn expand(&self, _argument: Option<&str>) -> Result<Vec<ExpandedHost>, String> {
            Ok(vec![
                ExpandedHost {
                    name: "first".to_owned(),
                    data: Data::new(),
                    groups: vec!["one".to_owned(), "two".to_owned()],
                },
                ExpandedHost {
                    name: "second".to_owned(),
                    data: Data::new(),
                    groups: vec!["two".to_owned(), "one".to_owned()],
                },
            ])
        }
    }

    #[test]
    fn builtins_expand_normal_local_and_explicit_ssh_hosts() {
        let registry = ConnectorRegistry::with_builtins();
        let inventory = Inventory::from_raw(
            RawInventorySpec {
                hosts: vec![RawHostSpec::new("host"), RawHostSpec::new("@local"), RawHostSpec::new("@ssh/alias")],
                ..RawInventorySpec::default()
            },
            &registry,
        )
        .unwrap();

        assert_eq!(inventory.get_host("host").unwrap().connector(), &ConnectorKind::Ssh);
        assert_eq!(inventory.get_host("@local").unwrap().connector(), &ConnectorKind::Local);
        assert_eq!(inventory.resolved_host_data("@ssh/alias").unwrap()["ssh_hostname"], json!("alias"));
    }

    #[test]
    fn name_only_connector_uses_ssh_and_inventory_data_overrides_provider_data() {
        let mut registry = ConnectorRegistry::with_builtins();
        registry.register("names", Arc::new(NamesOnlyConnector));
        let host_data = [("provider".to_owned(), json!(false))].into_iter().collect::<Data>();
        let inventory = Inventory::from_raw(
            RawInventorySpec {
                hosts: vec![RawHostSpec::new("@names/generated-host").with_data(host_data)],
                groups: vec![RawGroupSpec {
                    name: "attached".to_owned(),
                    hosts: vec![RawHostSpec::new("@names/generated-host")],
                    data: Data::new(),
                }],
                ..RawInventorySpec::default()
            },
            &registry,
        )
        .unwrap();

        let host = inventory.get_host("generated-host").unwrap();
        assert_eq!(host.connector(), &ConnectorKind::Ssh);
        assert_eq!(host.groups(), &["provided", "attached"]);
        assert_eq!(inventory.resolved_host_data("generated-host").unwrap()["provider"], json!(false));
    }

    #[test]
    fn connector_group_order_is_preserved_per_host_and_declared_group_data_remains_available() {
        let mut registry = ConnectorRegistry::with_builtins();
        registry.register("groups", Arc::new(PerHostGroupsConnector));
        let inventory = Inventory::from_raw(
            RawInventorySpec {
                hosts: vec![RawHostSpec::new("@groups")],
                groups: vec![RawGroupSpec {
                    name: "declared-without-hosts".to_owned(),
                    hosts: Vec::new(),
                    data: [("value".to_owned(), json!(true))].into_iter().collect(),
                }],
                ..RawInventorySpec::default()
            },
            &registry,
        )
        .unwrap();

        assert_eq!(inventory.get_host("first").unwrap().groups(), &["one", "two"]);
        assert_eq!(inventory.get_host("second").unwrap().groups(), &["two", "one"]);
        assert_eq!(inventory.direct_group_data("declared-without-hosts").unwrap()["value"], json!(true));
    }

    #[test]
    fn local_connector_rejects_an_argument_with_pyinfra_error() {
        let error = Inventory::from_raw(
            RawInventorySpec { hosts: vec![RawHostSpec::new("@local/extra")], ..RawInventorySpec::default() },
            &ConnectorRegistry::with_builtins(),
        )
        .unwrap_err();

        assert_eq!(error, InventoryError::MultipleLocalHosts);
    }
}
