use std::collections::HashSet;

use indexmap::{IndexMap, IndexSet};
use thiserror::Error;

/// Stable identity shared by the same operation across hosts.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OperationId(String);

impl OperationId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for OperationId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Shared display and source-order metadata for an operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationMeta {
    pub order: Vec<usize>,
    pub names: IndexSet<String>,
    pub arguments: Vec<String>,
}

impl OperationMeta {
    #[must_use]
    pub fn new(order: impl Into<Vec<usize>>) -> Self {
        Self { order: order.into(), names: IndexSet::new(), arguments: Vec::new() }
    }

    #[must_use]
    pub fn label(&self) -> String {
        let mut names = self.names.iter().map(String::as_str).collect::<Vec<_>>();
        names.sort_unstable();
        let names = names.join(", ");
        if self.arguments.is_empty() { names } else { format!("{names} ({})", self.arguments.join(", ")) }
    }
}

/// One dependency in a cycle and the hosts whose paths created it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CycleEdge {
    pub before: OperationId,
    pub after: OperationId,
    pub hosts: Vec<String>,
}

/// Failure to construct a complete operation order.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum OperationOrderError {
    #[error("operation {0:?} has no metadata")]
    MissingMetadata(OperationId),
    #[error("cycle detected in operation ordering DAG: {edges:?}")]
    Cycle { edges: Vec<CycleEdge> },
}

/// Per-host operation paths combined into pyinfra's global dependency DAG.
#[derive(Clone, Debug, Default)]
pub struct OperationGraph {
    metadata: IndexMap<OperationId, OperationMeta>,
    host_paths: IndexMap<String, Vec<OperationId>>,
}

impl OperationGraph {
    /// Adds or replaces operation metadata while retaining first-insertion order.
    pub fn insert(&mut self, id: OperationId, metadata: OperationMeta) {
        self.metadata.insert(id, metadata);
    }

    /// Records the operation call sequence for one host.
    pub fn set_host_path(&mut self, host: impl Into<String>, path: Vec<OperationId>) {
        self.host_paths.insert(host.into(), path);
    }

    #[must_use]
    pub fn metadata(&self, id: &OperationId) -> Option<&OperationMeta> {
        self.metadata.get(id)
    }

    /// Returns a deterministic topological order, using source/call order to break ties.
    pub fn order(&self) -> Result<Vec<OperationId>, OperationOrderError> {
        self.validate_metadata()?;
        let (successors, mut predecessor_counts) = self.dependencies();
        let mut ordered = Vec::with_capacity(self.metadata.len());

        loop {
            let mut ready = self
                .metadata
                .keys()
                .filter(|id| predecessor_counts.get(*id) == Some(&0) && !ordered.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            ready.sort_by_key(|id| &self.metadata[id].order);
            if ready.is_empty() {
                break;
            }

            for id in &ready {
                if let Some(nodes) = successors.get(id) {
                    for node in nodes {
                        if let Some(count) = predecessor_counts.get_mut(node) {
                            *count -= 1;
                        }
                    }
                }
            }
            ordered.extend(ready);
        }

        if ordered.len() == self.metadata.len() {
            Ok(ordered)
        } else {
            let cycle = self.find_cycle(&successors, &ordered);
            Err(OperationOrderError::Cycle { edges: self.describe_cycle(&cycle) })
        }
    }

    fn validate_metadata(&self) -> Result<(), OperationOrderError> {
        for id in self.host_paths.values().flatten() {
            if !self.metadata.contains_key(id) {
                return Err(OperationOrderError::MissingMetadata(id.clone()));
            }
        }
        Ok(())
    }

    fn dependencies(&self) -> (IndexMap<OperationId, IndexSet<OperationId>>, IndexMap<OperationId, usize>) {
        let mut successors = self.metadata.keys().cloned().map(|id| (id, IndexSet::new())).collect::<IndexMap<_, _>>();
        let mut predecessor_counts = self.metadata.keys().cloned().map(|id| (id, 0)).collect::<IndexMap<_, _>>();

        for path in self.host_paths.values() {
            for pair in path.windows(2) {
                let before = &pair[0];
                let after = &pair[1];
                if successors.entry(before.clone()).or_default().insert(after.clone()) {
                    *predecessor_counts.entry(after.clone()).or_default() += 1;
                }
            }
        }
        (successors, predecessor_counts)
    }

    fn find_cycle(
        &self,
        successors: &IndexMap<OperationId, IndexSet<OperationId>>,
        ordered: &[OperationId],
    ) -> Vec<OperationId> {
        let excluded = ordered.iter().cloned().collect::<HashSet<_>>();
        let mut search = CycleSearch { successors, excluded: &excluded, visiting: HashSet::new(), path: Vec::new() };
        for start in self.metadata.keys().filter(|id| !excluded.contains(*id)) {
            if let Some(cycle) = search.find_from(start) {
                return cycle;
            }
        }
        Vec::new()
    }

    fn describe_cycle(&self, cycle: &[OperationId]) -> Vec<CycleEdge> {
        cycle
            .windows(2)
            .map(|pair| CycleEdge {
                before: pair[0].clone(),
                after: pair[1].clone(),
                hosts: self
                    .host_paths
                    .iter()
                    .filter(|(_, path)| path.windows(2).any(|edge| edge == pair))
                    .map(|(host, _)| host.clone())
                    .collect(),
            })
            .collect()
    }
}

struct CycleSearch<'a> {
    successors: &'a IndexMap<OperationId, IndexSet<OperationId>>,
    excluded: &'a HashSet<OperationId>,
    visiting: HashSet<OperationId>,
    path: Vec<OperationId>,
}

impl CycleSearch<'_> {
    fn find_from(&mut self, node: &OperationId) -> Option<Vec<OperationId>> {
        if let Some(start) = self.path.iter().position(|item| item == node) {
            let mut cycle = self.path[start..].to_vec();
            cycle.push(node.clone());
            return Some(cycle);
        }
        if self.excluded.contains(node) || !self.visiting.insert(node.clone()) {
            return None;
        }

        self.path.push(node.clone());
        if let Some(nodes) = self.successors.get(node) {
            for successor in nodes {
                if let Some(cycle) = self.find_from(successor) {
                    return Some(cycle);
                }
            }
        }
        self.path.pop();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationGraph, OperationId, OperationMeta, OperationOrderError};

    fn add(graph: &mut OperationGraph, id: &'static str, order: &[usize]) {
        graph.insert(id.into(), OperationMeta::new(order));
    }

    #[test]
    fn combines_host_paths_and_uses_source_order_for_ready_operations() {
        let mut graph = OperationGraph::default();
        add(&mut graph, "first", &[1]);
        add(&mut graph, "second", &[2]);
        add(&mut graph, "third", &[3]);
        add(&mut graph, "last", &[4]);
        graph.set_host_path("one", vec!["first".into(), "third".into(), "last".into()]);
        graph.set_host_path("two", vec!["first".into(), "second".into(), "last".into()]);

        let order = graph.order().unwrap();

        assert_eq!(order, ["first", "second", "third", "last"].map(OperationId::from));
    }

    #[test]
    fn conflicting_host_paths_report_cycle_edges_and_hosts() {
        let mut graph = OperationGraph::default();
        add(&mut graph, "a", &[1]);
        add(&mut graph, "b", &[2]);
        graph.set_host_path("one", vec!["a".into(), "b".into()]);
        graph.set_host_path("two", vec!["b".into(), "a".into()]);

        let error = graph.order().unwrap_err();

        let OperationOrderError::Cycle { edges } = error else {
            panic!("expected a cycle");
        };
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].hosts, vec!["one"]);
        assert_eq!(edges[1].hosts, vec!["two"]);
    }

    #[test]
    fn operations_absent_from_host_paths_are_still_ordered() {
        let mut graph = OperationGraph::default();
        add(&mut graph, "later", &[2]);
        add(&mut graph, "earlier", &[1]);

        assert_eq!(graph.order().unwrap(), ["earlier", "later"].map(OperationId::from));
    }
}
