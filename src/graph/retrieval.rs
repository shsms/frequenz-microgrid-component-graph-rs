// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for retrieving components and connections from a [`ComponentGraph`].

use crate::iterators::{Components, Connections, Neighbors};
use crate::{ComponentGraph, Edge, Error, Node};

/// `Component` and `Connection` retrieval.
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Returns the component with the given `component_id`, if it exists.
    pub fn component(&self, component_id: u64) -> Result<&N, Error> {
        self.node_indices
            .get(&component_id)
            .map(|i| &self.graph[*i])
            .ok_or_else(|| {
                Error::component_not_found(format!("Component with id {} not found.", component_id))
            })
    }

    /// Returns an iterator over the components in the graph.
    pub fn components(&self) -> Components<N> {
        Components {
            iter: self.graph.raw_nodes().iter(),
        }
    }

    /// Returns an iterator over the connections in the graph.
    pub fn connections(&self) -> Connections<N, E> {
        Connections {
            cg: self,
            iter: self.graph.raw_edges().iter(),
        }
    }

    /// Returns an iterator over the *predecessors* of the component with the
    /// given `component_id`.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub fn predecessors(&self, component_id: u64) -> Result<Neighbors<N>, Error> {
        self.node_indices
            .get(&component_id)
            .map(|&index| Neighbors {
                graph: &self.graph,
                iter: self
                    .graph
                    .neighbors_directed(index, petgraph::Direction::Incoming),
            })
            .ok_or_else(|| {
                Error::component_not_found(format!("Component with id {} not found.", component_id))
            })
    }

    /// Returns an iterator over the *successors* of the component with the
    /// given `component_id`.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub fn successors(&self, component_id: u64) -> Result<Neighbors<N>, Error> {
        self.node_indices
            .get(&component_id)
            .map(|&index| Neighbors {
                graph: &self.graph,
                iter: self
                    .graph
                    .neighbors_directed(index, petgraph::Direction::Outgoing),
            })
            .ok_or_else(|| {
                Error::component_not_found(format!("Component with id {} not found.", component_id))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::ComponentCategory;
    use crate::InverterType;

    #[derive(Clone, Debug, PartialEq)]
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

    #[derive(Clone, Debug, PartialEq)]
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
            TestComponent(3, ComponentCategory::Meter),
            TestComponent(5, ComponentCategory::Battery),
            TestComponent(8, ComponentCategory::Battery),
            TestComponent(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(2, ComponentCategory::Meter),
        ];
        let connections = vec![
            TestConnection::new(3, 4),
            TestConnection::new(1, 2),
            TestConnection::new(7, 8),
            TestConnection::new(4, 5),
            TestConnection::new(2, 3),
            TestConnection::new(6, 7),
            TestConnection::new(2, 6),
        ];

        (components, connections)
    }

    #[test]
    fn test_component() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone())?;

        assert_eq!(
            graph.component(1),
            Ok(&TestComponent(1, ComponentCategory::Grid))
        );
        assert_eq!(
            graph.component(5),
            Ok(&TestComponent(5, ComponentCategory::Battery))
        );
        assert_eq!(
            graph.component(9),
            Err(Error::component_not_found("Component with id 9 not found."))
        );

        Ok(())
    }

    #[test]
    fn test_components() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone())?;

        assert!(graph.components().eq(&components));
        assert!(graph
            .components()
            .filter(|x| x.category() == ComponentCategory::Battery)
            .eq(&[
                TestComponent(5, ComponentCategory::Battery),
                TestComponent(8, ComponentCategory::Battery)
            ]));

        Ok(())
    }

    #[test]
    fn test_connections() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone())?;

        assert!(graph.connections().eq(&connections));

        assert!(graph
            .connections()
            .filter(|x| x.source() == 2)
            .eq(&[TestConnection::new(2, 3), TestConnection::new(2, 6)]));

        Ok(())
    }

    #[test]
    fn test_neighbors() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone())?;

        assert!(graph.predecessors(1).is_ok_and(|x| x.eq(&[])));

        assert!(graph
            .predecessors(3)
            .is_ok_and(|x| x.eq(&[TestComponent(2, ComponentCategory::Meter)])));

        assert!(graph
            .successors(1)
            .is_ok_and(|x| x.eq(&[TestComponent(2, ComponentCategory::Meter)])));

        assert!(graph.successors(2).is_ok_and(|x| {
            x.eq(&[
                TestComponent(6, ComponentCategory::Meter),
                TestComponent(3, ComponentCategory::Meter),
            ])
        }));

        assert!(graph.successors(5).is_ok_and(|x| x.eq(&[])));

        assert!(graph
            .predecessors(32)
            .is_err_and(|e| e == Error::component_not_found("Component with id 32 not found.")));
        assert!(graph
            .successors(32)
            .is_err_and(|e| e == Error::component_not_found("Component with id 32 not found.")));

        Ok(())
    }
}
