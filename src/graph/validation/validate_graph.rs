// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for validating the acyclicity and connectedness of a
//! [`ComponentGraph`].

use std::collections::BTreeSet;

use crate::{Edge, Error, Node};

use super::ComponentGraphValidator;

impl<N, E> ComponentGraphValidator<'_, N, E>
where
    N: Node,
    E: Edge,
{
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
            .map(|n| n.component_id())
            .filter(|id| !visited.contains(id))
            .collect::<Vec<_>>();

        if !unvisited.is_empty() {
            return Err(Error::invalid_graph(format!(
                "Nodes {:?} are not connected to the root.",
                unvisited
            )));
        }

        Ok(())
    }

    pub(super) fn validate_acyclicity(
        &self,
        node: &N,
        mut predecessors: Vec<u64>,
    ) -> Result<(), Error> {
        predecessors.push(node.component_id());
        for successor in self.cg.successors(node.component_id())? {
            if let Some(first_occurance) = predecessors
                .iter()
                .position(|id| *id == successor.component_id())
            {
                return Err(Error::invalid_graph(format!(
                    "Cycle detected: {} -> {}",
                    predecessors[first_occurance..]
                        .iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(" -> "),
                    successor.component_id()
                )));
            }
            self.validate_acyclicity(successor, predecessors.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentCategory;
    use crate::ComponentGraph;
    use crate::InverterType;

    #[derive(Clone)]
    struct TestComponent(u64, ComponentCategory);

    impl Node for TestComponent {
        fn component_id(&self) -> u64 {
            self.0
        }

        fn category(&self) -> ComponentCategory {
            self.1.clone()
        }

        fn is_supported(&self) -> bool {
            true
        }
    }

    #[derive(Clone)]
    struct TestConnection(u64, u64);

    impl TestConnection {
        fn new(source: u64, destination: u64) -> Self {
            TestConnection(source, destination)
        }
    }

    impl Edge for TestConnection {
        fn source(&self) -> u64 {
            self.0
        }

        fn destination(&self) -> u64 {
            self.1
        }
    }

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent(6, ComponentCategory::Meter),
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(10, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent(3, ComponentCategory::Meter),
            TestComponent(5, ComponentCategory::Battery),
            TestComponent(8, ComponentCategory::Battery),
            TestComponent(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(9, ComponentCategory::Meter),
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
        let (mut components, mut connections) = nodes_and_edges();

        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());
        components.push(TestComponent(11, ComponentCategory::Meter));
        let Err(err) = ComponentGraph::try_new(components.clone(), connections.clone()) else {
            panic!()
        };
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(
                |e| e == Error::invalid_graph("Nodes [11] are not connected to the root.")
            ),
            "{:?}",
            err
        );

        components.push(TestComponent(12, ComponentCategory::Meter));

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(
                |e| e == Error::invalid_graph("Nodes [11, 12] are not connected to the root.")
            )
        );

        connections.push(TestConnection::new(11, 12));

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(
                |e| e == Error::invalid_graph("Nodes [11, 12] are not connected to the root.")
            )
        );

        connections.pop();
        components.pop();
        components.pop();

        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());
    }

    #[test]
    fn test_acyclicity_validation() {
        let (components, mut connections) = nodes_and_edges();

        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());

        // add cycles at different levels
        connections.push(TestConnection::new(3, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 2 -> 3 -> 2")),
        );

        connections.pop();
        connections.push(TestConnection::new(4, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 2 -> 3 -> 4 -> 2"))
        );

        connections.pop();
        connections.push(TestConnection::new(5, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 2 -> 3 -> 4 -> 5 -> 2"))
        );

        connections.pop();
        connections.push(TestConnection::new(4, 3));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 3 -> 4 -> 3"))
        );

        connections.pop();
        connections.push(TestConnection::new(5, 3));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 3 -> 4 -> 5 -> 3"))
        );

        connections.pop();
        connections.push(TestConnection::new(5, 4));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 4 -> 5 -> 4"))
        );

        connections.pop();
        connections.push(TestConnection::new(9, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone())
                .is_err_and(|e| e == Error::invalid_graph("Cycle detected: 2 -> 9 -> 2"))
        );

        connections.pop();
        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());
    }
}