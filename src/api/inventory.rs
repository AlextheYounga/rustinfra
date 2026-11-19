use indexmap::IndexMap;
use serde_json::Value;
use thiserror::Error;

/// Arbitrary inventory data at the external configuration boundary.
pub type Data = IndexMap<String, Value>;

/// Inventory data key used to override the network target for an SSH host.
pub const SSH_HOSTNAME_DATA_KEY: &str = "ssh_hostname";

/// Connector selected for an expanded inventory host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectorKind {
    Ssh,
    Local,
    External(String),
}

/// A host before it is combined with group membership and data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostSpec {
    pub name: String,
    pub data: Data,
    pub connector: ConnectorKind,
}

impl HostSpec {
    /// Creates a normal pyinfra host, which uses SSH by default.
    #[must_use]
    pub fn ssh(name: impl Into<String>) -> Self {
        Self { name: name.into(), data: Data::new(), connector: ConnectorKind::Ssh }
    }

    /// Creates the singleton local host.
    #[must_use]
    pub fn local() -> Self {
        Self { name: "@local".to_owned(), data: Data::new(), connector: ConnectorKind::Local }
    }

    /// Attaches host-specific data.
    #[must_use]
    pub fn with_data(mut self, data: Data) -> Self {
        self.data = data;
        self
    }
}

/// Hosts and shared data belonging to one inventory group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupSpec {
    pub name: String,
    pub hosts: Vec<HostSpec>,
    pub data: Data,
}

/// Complete input used to construct an inventory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InventorySpec {
    pub hosts: Vec<HostSpec>,
    pub data: Data,
    pub override_data: Data,
    pub groups: Vec<GroupSpec>,
}

/// One target host after inventory expansion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Host {
    name: String,
    groups: Vec<String>,
    connector: ConnectorKind,
}

impl Host {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn groups(&self) -> &[String] {
        &self.groups
    }

    #[must_use]
    pub fn connector(&self) -> &ConnectorKind {
        &self.connector
    }
}

/// Error looking up an inventory host or group.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum InventoryError {
    #[error("No such host: {0}")]
    NoHost(String),
    #[error("No such group: {0}")]
    NoGroup(String),
    #[error("Cannot have more than one @local")]
    MultipleLocalHosts,
    #[error("Invalid connector: {0}")]
    InvalidConnector(String),
    #[error("Invalid connector: {connector} (it cannot execute commands, it only provides names)")]
    ConnectorCannotExecute { connector: String },
    #[error("connector {connector} could not expand inventory name: {message}")]
    ConnectorExpansion { connector: String, message: String },
}

/// Ordered target hosts, groups, and their layered data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inventory {
    hosts: IndexMap<String, Host>,
    groups: IndexMap<String, Vec<String>>,
    host_data: IndexMap<String, Data>,
    group_data: IndexMap<String, Data>,
    data: Data,
    override_data: Data,
    host_overrides: IndexMap<String, Data>,
}

impl Inventory {
    /// Builds an inventory while preserving source and group order.
    pub fn from_spec(spec: InventorySpec) -> Result<Self, InventoryError> {
        let mut inventory = Self { data: spec.data, override_data: spec.override_data, ..Self::default() };
        let mut host_groups: IndexMap<String, Vec<String>> = IndexMap::new();

        for group in spec.groups {
            inventory.group_data.insert(group.name.clone(), group.data);
            for host in group.hosts {
                extend_data(inventory.host_data.entry(host.name.clone()).or_default(), host.data);
                let groups = host_groups.entry(host.name).or_default();
                if !groups.contains(&group.name) {
                    groups.push(group.name.clone());
                }
            }
        }

        let mut local_seen = false;
        for host_spec in spec.hosts {
            if host_spec.connector == ConnectorKind::Local {
                if local_seen {
                    return Err(InventoryError::MultipleLocalHosts);
                }
                local_seen = true;
            }
            extend_data(inventory.host_data.entry(host_spec.name.clone()).or_default(), host_spec.data);
            let groups = host_groups.get(&host_spec.name).cloned().unwrap_or_default();
            let host = Host { name: host_spec.name.clone(), groups: groups.clone(), connector: host_spec.connector };
            inventory.hosts.insert(host_spec.name.clone(), host);
            for group in groups {
                let members = inventory.groups.entry(group).or_default();
                if !members.contains(&host_spec.name) {
                    members.push(host_spec.name.clone());
                }
            }
        }

        Ok(inventory)
    }

    pub(super) fn from_expanded(
        hosts: Vec<(HostSpec, Vec<String>)>,
        data: Data,
        override_data: Data,
        group_data: IndexMap<String, Data>,
    ) -> Result<Self, InventoryError> {
        let mut inventory = Self { data, override_data, group_data, ..Self::default() };
        let mut local_seen = false;

        for (host_spec, groups) in hosts {
            if host_spec.connector == ConnectorKind::Local {
                if local_seen {
                    return Err(InventoryError::MultipleLocalHosts);
                }
                local_seen = true;
            }

            inventory.host_data.insert(host_spec.name.clone(), host_spec.data);
            let host = Host { name: host_spec.name.clone(), groups: groups.clone(), connector: host_spec.connector };
            inventory.hosts.insert(host_spec.name.clone(), host);
            for group in groups {
                let members = inventory.groups.entry(group).or_default();
                if !members.contains(&host_spec.name) {
                    members.push(host_spec.name.clone());
                }
            }
        }

        Ok(inventory)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    pub fn hosts(&self) -> impl ExactSizeIterator<Item = &Host> {
        self.hosts.values()
    }

    pub fn get_host(&self, name: &str) -> Result<&Host, InventoryError> {
        self.hosts.get(name).ok_or_else(|| InventoryError::NoHost(name.to_owned()))
    }

    pub fn get_group(&self, name: &str) -> Result<Vec<&Host>, InventoryError> {
        let members = self.groups.get(name).ok_or_else(|| InventoryError::NoGroup(name.to_owned()))?;
        Ok(members.iter().filter_map(|host| self.hosts.get(host)).collect())
    }

    #[must_use]
    pub fn data(&self) -> &Data {
        &self.data
    }

    #[must_use]
    pub fn override_data(&self) -> &Data {
        &self.override_data
    }

    #[must_use]
    pub fn direct_host_data(&self, name: &str) -> Option<&Data> {
        self.host_data.get(name)
    }

    #[must_use]
    pub fn direct_group_data(&self, name: &str) -> Option<&Data> {
        self.group_data.get(name)
    }

    /// Returns host data with pyinfra's precedence: runtime override, inventory override, host,
    /// later group, earlier group, then global data.
    pub fn resolved_host_data(&self, name: &str) -> Result<Data, InventoryError> {
        let host = self.get_host(name)?;
        let mut resolved = self.data.clone();
        for group in &host.groups {
            if let Some(data) = self.group_data.get(group) {
                extend_data(&mut resolved, data.clone());
            }
        }
        if let Some(data) = self.host_data.get(name) {
            extend_data(&mut resolved, data.clone());
        }
        extend_data(&mut resolved, self.override_data.clone());
        if let Some(data) = self.host_overrides.get(name) {
            extend_data(&mut resolved, data.clone());
        }
        Ok(resolved)
    }

    /// Sets mutable per-host data used during a deploy.
    pub fn set_host_override(
        &mut self,
        host: &str,
        key: impl Into<String>,
        value: Value,
    ) -> Result<(), InventoryError> {
        self.get_host(host)?;
        self.host_overrides.entry(host.to_owned()).or_default().insert(key.into(), value);
        Ok(())
    }
}

fn extend_data(target: &mut Data, source: Data) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Data, GroupSpec, HostSpec, Inventory, InventoryError, InventorySpec};

    fn data(values: &[(&str, serde_json::Value)]) -> Data {
        values.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect()
    }

    #[test]
    fn host_data_uses_pyinfra_precedence_and_later_groups_win() {
        let spec = InventorySpec {
            hosts: vec![HostSpec::ssh("first").with_data(data(&[("level", json!("host"))])), HostSpec::ssh("second")],
            data: data(&[("level", json!("global")), ("global", json!(true))]),
            override_data: data(&[("override", json!("inventory"))]),
            groups: vec![
                GroupSpec {
                    name: "early".to_owned(),
                    hosts: vec![HostSpec::ssh("first"), HostSpec::ssh("second")],
                    data: data(&[("group", json!("early"))]),
                },
                GroupSpec {
                    name: "late".to_owned(),
                    hosts: vec![HostSpec::ssh("first")],
                    data: data(&[("group", json!("late"))]),
                },
            ],
        };
        let mut inventory = Inventory::from_spec(spec).unwrap();
        inventory.set_host_override("first", "override", json!("runtime")).unwrap();

        let resolved = inventory.resolved_host_data("first").unwrap();

        assert_eq!(resolved["level"], json!("host"));
        assert_eq!(resolved["group"], json!("late"));
        assert_eq!(resolved["global"], json!(true));
        assert_eq!(resolved["override"], json!("runtime"));
    }

    #[test]
    fn group_membership_only_contains_top_level_hosts_and_preserves_order() {
        let inventory = Inventory::from_spec(InventorySpec {
            hosts: vec![HostSpec::ssh("second"), HostSpec::ssh("first")],
            groups: vec![GroupSpec {
                name: "web".to_owned(),
                hosts: vec![HostSpec::ssh("first"), HostSpec::ssh("group-only"), HostSpec::ssh("second")],
                data: Data::new(),
            }],
            ..InventorySpec::default()
        })
        .unwrap();

        let names = inventory.get_group("web").unwrap().into_iter().map(super::Host::name).collect::<Vec<_>>();

        assert_eq!(names, vec!["second", "first"]);
        assert_eq!(inventory.get_host("group-only"), Err(InventoryError::NoHost("group-only".to_owned())));
    }

    #[test]
    fn resolved_data_is_owned_so_nested_mutation_does_not_change_shared_group_data() {
        let inventory = Inventory::from_spec(InventorySpec {
            hosts: vec![HostSpec::ssh("first"), HostSpec::ssh("second")],
            groups: vec![GroupSpec {
                name: "all".to_owned(),
                hosts: vec![HostSpec::ssh("first"), HostSpec::ssh("second")],
                data: data(&[("nested", json!({}))]),
            }],
            ..InventorySpec::default()
        })
        .unwrap();
        let mut first = inventory.resolved_host_data("first").unwrap();
        first["nested"]["changed"] = json!(true);

        assert_eq!(inventory.resolved_host_data("second").unwrap()["nested"], json!({}));
    }
}
