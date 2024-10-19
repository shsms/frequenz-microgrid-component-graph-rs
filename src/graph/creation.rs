// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for creating [`ComponentGraph`] instances from given components and
//! connections.

use petgraph::graph::DiGraph;

use crate::{component_category::CategoryPredicates, ComponentGraphConfig, Edge, Error, Node};

use super::{ComponentGraph, EdgeMap, NodeIndexMap};

/// `ComponentGraph` instantiation.
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Creates a new [`ComponentGraph`] from the given components and connections.
    ///
    /// Returns an error if the graph is invalid.
    pub fn try_new<NodeIterator: IntoIterator<Item = N>, EdgeIterator: IntoIterator<Item = E>>(
        components: NodeIterator,
        connections: EdgeIterator,
        config: ComponentGraphConfig,
    ) -> Result<Self, Error> {
        let (graph, indices) = Self::create_graph(components, &config)?;
        let root_id = Self::find_root(&graph)?.component_id();

        let mut cg = Self {
            graph,
            node_indices: indices,
            root_id,
            edges: EdgeMap::new(),
            config,
        };
        cg.add_connections(connections)?;

        cg.validate()?;

        Ok(cg)
    }

    fn find_root(graph: &DiGraph<N, ()>) -> Result<&N, Error> {
        let mut roots_iter = graph.raw_nodes().iter().filter(|n| n.weight.is_grid());

        let root = roots_iter
            .next()
            .map(|n| &n.weight)
            .ok_or_else(|| Error::invalid_graph("No grid component found."))?;

        if roots_iter.next().is_some() {
            return Err(Error::invalid_graph("Multiple grid components found."));
        }

        Ok(root)
    }

    fn create_graph(
        components: impl IntoIterator<Item = N>,
        config: &ComponentGraphConfig,
    ) -> Result<(DiGraph<N, ()>, NodeIndexMap), Error> {
        let mut graph = DiGraph::new();
        let mut indices = NodeIndexMap::new();

        for component in components {
            let cid = component.component_id();

            if component.is_unspecified() {
                return Err(Error::invalid_component(format!(
                    "ComponentCategory not specified for component: {cid}"
                )));
            }
            if component.is_unspecified_inverter() {
                return Err(Error::invalid_component(format!(
                    "InverterType not specified for inverter: {cid}"
                )));
            }
            if indices.contains_key(&cid) {
                return Err(Error::invalid_graph(format!(
                    "Duplicate component ID found: {cid}"
                )));
            }

            let idx = graph.add_node(component);
            indices.insert(cid, idx);
        }

        Ok((graph, indices))
    }

    fn add_connections(&mut self, connections: impl IntoIterator<Item = E>) -> Result<(), Error> {
        for connection in connections {
            let sid = connection.source();
            let did = connection.destination();

            if sid == did {
                return Err(Error::invalid_connection(format!(
                    "Connection:({sid}, {did}) Can't connect a component to itself."
                )));
            }
            for cid in [sid, did] {
                if !self.node_indices.contains_key(&cid) {
                    return Err(Error::invalid_connection(format!(
                        "Connection:({sid}, {did}) Can't find a component with ID {cid}"
                    )));
                }
            }

            let source_idx = self.node_indices[&connection.source()];
            let dest_idx = self.node_indices[&connection.destination()];
            self.edges.insert((source_idx, dest_idx), connection);
            self.graph.update_edge(source_idx, dest_idx, ());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_category::BatteryType;
    use crate::graph::test_utils::{TestComponent, TestConnection};
    use crate::ComponentCategory;
    use crate::InverterType;

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent::new(6, ComponentCategory::Meter),
            TestComponent::new(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(3, ComponentCategory::Meter),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::LiIon)),
            TestComponent::new(8, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(2, ComponentCategory::Meter),
        ];
        let connections = vec![
            TestConnection::new(3, 4),
            TestConnection::new(7, 8),
            TestConnection::new(4, 5),
            TestConnection::new(2, 3),
            TestConnection::new(6, 7),
            TestConnection::new(2, 6),
        ];

        (components, connections)
    }

    #[test]
    fn test_component_validation() {
        let config = ComponentGraphConfig::default();
        let (mut components, mut connections) = nodes_and_edges();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == Error::invalid_graph("No grid component found.")),
        );

        components.push(TestComponent::new(1, ComponentCategory::Grid));
        connections.push(TestConnection::new(1, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );

        components.push(TestComponent::new(2, ComponentCategory::Meter));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == Error::invalid_graph("Duplicate component ID found: 2"))
        );

        components.pop();
        components.push(TestComponent::new(9, ComponentCategory::Unspecified));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e
                    == Error::invalid_component(
                        "ComponentCategory not specified for component: 9"
                    ))
        );

        components.pop();
        components.push(TestComponent::new(
            9,
            ComponentCategory::Inverter(InverterType::Unspecified),
        ));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(
                    |e| e == Error::invalid_component("InverterType not specified for inverter: 9")
                )
        );

        components.pop();
        components.push(TestComponent::new(9, ComponentCategory::Grid));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e == Error::invalid_graph("Multiple grid components found."))
        );

        components.pop();
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );
    }

    #[test]
    fn test_connection_validation() {
        let config = ComponentGraphConfig::default();
        let (mut components, mut connections) = nodes_and_edges();

        components.push(TestComponent::new(1, ComponentCategory::Grid));
        connections.push(TestConnection::new(1, 2));

        connections.push(TestConnection::new(2, 2));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e
                    == Error::invalid_connection(
                        "Connection:(2, 2) Can't connect a component to itself."
                    ))
        );

        connections.pop();
        connections.push(TestConnection::new(2, 9));
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| e
                    == Error::invalid_connection(
                        "Connection:(2, 9) Can't find a component with ID 9"
                    ))
        );

        connections.pop();
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );
    }
}
