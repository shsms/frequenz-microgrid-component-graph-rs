// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for creating [`ComponentGraph`] instances from given components and
//! connections.

use petgraph::graph::DiGraph;

use crate::{ComponentGraphConfig, Edge, Error, Node, component_category::CategoryPredicates};

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

        // Notify operators that any pass-through nodes in the graph
        // will be treated as transparent by validators and formula
        // generators. Logged once per node, after validation, so we
        // only warn for components that actually end up in the graph.
        for component in cg.components() {
            if component.category().is_passthrough() {
                tracing::warn!(
                    "Component {cid} ({category}) is a pass-through category and \
                     will be treated as transparent in validators and formula generators.",
                    cid = component.component_id(),
                    category = component.category(),
                );
            }
        }

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
            if component.is_unspecified_inverter(config) {
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
    use crate::ComponentCategory;
    use crate::InverterType;
    use crate::graph::test_utils::{TestComponent, graph};

    #[test]
    fn test_component_validation() {
        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
        };
        assert!(
            g.build(None)
                .is_err_and(|e| e == Error::invalid_graph("No grid component found.")),
        );

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
        };
        assert!(g.build(None).is_ok());

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
            2: meter,
        };
        assert!(
            g.build(None)
                .is_err_and(|e| e == Error::invalid_graph("Duplicate component ID found: 2"))
        );

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
            8: unspecified,
        };
        assert!(
            g.build(None).is_err_and(|e| e
                == Error::invalid_component("ComponentCategory not specified for component: 8"))
        );

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
            9: inv(Unspecified),
            0 => { 9 },
        };

        // With default config, unspecified inverter types are not accepted.
        assert!(g.build(None).is_err_and(
            |e| e == Error::invalid_component("InverterType not specified for inverter: 9")
        ));
        // With `allow_unspecified_inverters=true`, unspecified inverter types
        // are treated as battery inverters.
        let unspec_inv_config = ComponentGraphConfig::builder()
            .allow_unspecified_inverters(true)
            .build();

        assert!(g.build(Some(unspec_inv_config.clone())).is_ok());

        assert!(
            TestComponent::new(9, ComponentCategory::Inverter(InverterType::Unspecified))
                .is_battery_inverter(&unspec_inv_config)
        );

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
            10: grid,
        };
        assert!(
            g.build(None)
                .is_err_and(|e| e == Error::invalid_graph("Multiple grid components found."))
        );
    }

    #[test]
    fn test_connection_validation() {
        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0, 7 },
        };
        assert!(g.build(None).is_err_and(|e| e
            == Error::invalid_connection(
                "Connection:(7, 7) Can't connect a component to itself."
            )));

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
            0 => { 9 },
        };
        assert!(g.build(None).is_err_and(|e| e
            == Error::invalid_connection("Connection:(0, 9) Can't find a component with ID 9")));

        let g = graph! {
            0: meter => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: meter => { 5: inv_bat => { 6: bat } },
            },
            7: grid => { 0 },
        };
        assert!(g.build(None).is_ok());
    }
}
