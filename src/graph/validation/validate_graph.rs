// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for validating the acyclicity and connectedness of a
//! [`ComponentGraph`].

use std::collections::BTreeSet;

use crate::{Edge, Error, Node, ValidationError};

use super::ComponentGraphValidator;

impl<N, E> ComponentGraphValidator<'_, N, E>
where
    N: Node,
    E: Edge,
{
    /// Validates that all non-pass-through components are connected
    /// into a single graph.
    ///
    /// It does so by ensuring that all the components are reachable
    /// by traversing the graph from the root node. Pass-through
    /// components are exempt: they're transparent to the effective
    /// successors view, so the walk never visits them, and we don't
    /// require them to be reachable -- even when
    /// `allow_unconnected_components` is `false`.
    pub(super) fn validate_connected_graph(&self, root: &N) -> Result<(), Error> {
        let root_id = root.component_id();
        let mut visited = BTreeSet::new();
        let mut queue = vec![root_id];
        visited.insert(root_id);
        while let Some(node_id) = queue.pop() {
            for successor in self.cg.successors(node_id)? {
                visited.insert(successor.component_id());
                queue.push(successor.component_id());
            }
        }

        let unvisited = self
            .cg
            .components()
            .filter(|n| !n.category().is_passthrough())
            .map(|n| n.component_id())
            .filter(|id| !visited.contains(id))
            .collect::<Vec<_>>();

        if !unvisited.is_empty() {
            return Err(ValidationError::new(
                format!("Nodes {unvisited:?} are not connected to the root."),
                unvisited,
            )
            .into());
        }

        Ok(())
    }

    /// Validates that there are no cycles in the graph.
    ///
    /// If a cycle is detected, an error is returned, that lists the nodes in
    /// the cycle. Walks via `raw_successors` so cycles composed entirely of
    /// pass-through nodes (which are invisible to the effective view) are
    /// still detected.
    pub(super) fn validate_acyclicity(
        &self,
        node: &N,
        mut predecessors: Vec<u64>,
    ) -> Result<(), Error> {
        predecessors.push(node.component_id());
        for successor in self.cg.raw_successors(node.component_id())? {
            if let Some(first_occurrence) = predecessors
                .iter()
                .position(|id| *id == successor.component_id())
            {
                let cycle = &predecessors[first_occurrence..];
                return Err(ValidationError::new(
                    format!(
                        "Cycle detected: {} -> {}",
                        cycle
                            .iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(" -> "),
                        successor.component_id()
                    ),
                    cycle.to_vec(),
                )
                .into());
            }
            self.validate_acyclicity(successor, predecessors.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::ComponentCategory;
    use crate::ComponentGraph;
    use crate::ComponentGraphConfig;
    use crate::InverterType;
    use crate::component_category::BatteryType;
    use crate::graph::test_utils::{TestComponent, TestConnection, validation_error};

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent::new(6, ComponentCategory::Meter),
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(10, ComponentCategory::Inverter(InverterType::Pv)),
            TestComponent::new(3, ComponentCategory::Meter),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(8, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(9, ComponentCategory::Meter),
        ];
        let connections = vec![
            TestConnection::new(3, 4),
            TestConnection::new(1, 2),
            TestConnection::new(7, 8),
            TestConnection::new(4, 5),
            TestConnection::new(2, 3),
            TestConnection::new(6, 7),
            TestConnection::new(2, 6),
            TestConnection::new(2, 9),
            TestConnection::new(9, 10),
        ];

        (components, connections)
    }

    #[test]
    fn test_connected_graph_validation() {
        let config = ComponentGraphConfig::default();
        let (mut components, mut connections) = nodes_and_edges();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );
        components.push(TestComponent::new(11, ComponentCategory::Meter));
        let Err(err) =
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
        else {
            panic!()
        };
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(
                    |e| e == validation_error("Nodes [11] are not connected to the root.", &[11])
                ),
            "{err:?}"
        );

        components.push(TestComponent::new(12, ComponentCategory::Meter));

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e
                    == validation_error(
                        "Nodes [11, 12] are not connected to the root.",
                        &[11, 12]
                    ))
        );

        connections.push(TestConnection::new(11, 12));

        // With the default config, this fails validation.
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e
                    == validation_error(
                        "Nodes [11, 12] are not connected to the root.",
                        &[11, 12]
                    ))
        );
        // With `allow_unconnected_components=true`, this passes validation.
        assert!(
            ComponentGraph::try_new(
                components.clone(),
                connections.clone(),
                ComponentGraphConfig::builder()
                    .allow_unconnected_components(true)
                    .build(),
            )
            .is_ok()
        );

        connections.pop();
        components.pop();
        components.pop();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );
    }

    #[test]
    fn test_acyclicity_validation() {
        let config = ComponentGraphConfig::default();
        let (components, mut connections) = nodes_and_edges();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );

        // add cycles at different levels
        connections.push(TestConnection::new(3, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == validation_error("Cycle detected: 2 -> 3 -> 2", &[2, 3])),
        );

        connections.pop();
        connections.push(TestConnection::new(4, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(
                    |e| e == validation_error("Cycle detected: 2 -> 3 -> 4 -> 2", &[2, 3, 4])
                )
        );

        connections.pop();
        connections.push(TestConnection::new(5, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e
                    == validation_error("Cycle detected: 2 -> 3 -> 4 -> 5 -> 2", &[2, 3, 4, 5]))
        );

        connections.pop();
        connections.push(TestConnection::new(4, 3));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == validation_error("Cycle detected: 3 -> 4 -> 3", &[3, 4]))
        );

        connections.pop();
        connections.push(TestConnection::new(5, 3));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(
                    |e| e == validation_error("Cycle detected: 3 -> 4 -> 5 -> 3", &[3, 4, 5])
                )
        );

        connections.pop();
        connections.push(TestConnection::new(5, 4));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == validation_error("Cycle detected: 4 -> 5 -> 4", &[4, 5]))
        );

        connections.pop();
        connections.push(TestConnection::new(9, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == validation_error("Cycle detected: 2 -> 9 -> 2", &[2, 9]))
        );

        connections.pop();
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );
    }

    /// A tolerated component-rule violation must not cancel out an
    /// unconnected-component failure that is *not* tolerated: the checks'
    /// outcomes are combined, not overwritten by whichever runs last.
    #[test]
    fn test_tolerated_failure_does_not_mask_a_fatal_one() {
        let components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Battery(BatteryType::LiIon)),
            // Node 4 is left unconnected, which is not tolerated by default.
            TestComponent::new(4, ComponentCategory::Meter),
        ];
        // The meter -> battery edge breaks a neighbor rule, which *is* tolerated.
        let connections = vec![TestConnection::new(1, 2), TestConnection::new(2, 3)];
        let config = ComponentGraphConfig::builder()
            .allow_component_validation_failures(true)
            .build();

        assert!(ComponentGraph::try_new(components, connections, config).is_err());
    }
}
