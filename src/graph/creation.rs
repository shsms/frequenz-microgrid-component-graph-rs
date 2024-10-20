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
    use crate::graph::test_utils::{ComponentGraphBuilder, ComponentHandle};
    use crate::ComponentCategory;
    use crate::InverterType;

    fn nodes_and_edges() -> (ComponentGraphBuilder, ComponentHandle) {
        let mut builder = ComponentGraphBuilder::new();

        let grid_meter = builder.meter();
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        (builder, grid_meter)
    }

    #[test]
    fn test_component_validation() {
        let (mut builder, grid_meter) = nodes_and_edges();

        assert!(builder
            .build(None)
            .is_err_and(|e| e == Error::invalid_graph("No grid component found.")),);

        let grid = builder.grid();
        builder.connect(grid, grid_meter);

        assert!(builder.build(None).is_ok());

        builder.add_component_with_id(2, ComponentCategory::Meter);
        assert!(builder
            .build(None)
            .is_err_and(|e| e == Error::invalid_graph("Duplicate component ID found: 2")));

        builder.pop_component();
        builder.add_component(ComponentCategory::Unspecified);
        assert!(builder
            .build(None)
            .is_err_and(|e| e
                == Error::invalid_component("ComponentCategory not specified for component: 8")));

        builder.pop_component();
        let unspec_inv =
            builder.add_component(ComponentCategory::Inverter(InverterType::Unspecified));
        builder.connect(grid_meter, unspec_inv);

        // With default config, unspecified inverter types are not accepted.
        assert!(builder.build(None).is_err_and(
            |e| e == Error::invalid_component("InverterType not specified for inverter: 9")
        ));
        // With `allow_unspecified_inverters=true`, unspecified inverter types
        // are treated as battery inverters.
        assert!(builder
            .build(Some(ComponentGraphConfig {
                allow_unspecified_inverters: true,
                ..Default::default()
            }))
            .is_ok());

        builder.pop_component();
        builder.pop_connection();
        builder.add_component(ComponentCategory::Grid);
        assert!(builder
            .build(None)
            .is_err_and(|e| e == Error::invalid_graph("Multiple grid components found.")));

        builder.pop_component();
        assert!(builder.build(None).is_ok());
    }

    #[test]
    fn test_connection_validation() {
        let (mut builder, grid_meter) = nodes_and_edges();

        let grid = builder.grid();
        builder.connect(grid, grid_meter);

        builder.connect(grid, grid);
        assert!(builder.build(None).is_err_and(|e| e
            == Error::invalid_connection(
                "Connection:(7, 7) Can't connect a component to itself."
            )));
        builder.pop_connection();

        builder.connect(grid_meter, ComponentHandle::new(9));
        assert!(builder.build(None).is_err_and(|e| e
            == Error::invalid_connection("Connection:(0, 9) Can't find a component with ID 9")));

        builder.pop_connection();
        assert!(builder.build(None).is_ok());
    }
}
