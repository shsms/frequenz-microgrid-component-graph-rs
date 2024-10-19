// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for retrieving components and connections from a [`ComponentGraph`].

use crate::iterators::{Components, Connections, Neighbors, Siblings};
use crate::{ComponentGraph, Edge, Error, Node};
use std::collections::BTreeSet;

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

    /// Returns an iterator over the *siblings* of the component with the
    /// given `component_id`, that have shared predecessors.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub(crate) fn siblings_from_predecessors(
        &self,
        component_id: u64,
    ) -> Result<Siblings<N>, Error> {
        Ok(Siblings::new(
            component_id,
            self.predecessors(component_id)?
                .map(|x| self.successors(x.component_id()))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten(),
        ))
    }

    /// Returns an iterator over the *siblings* of the component with the
    /// given `component_id`, that have shared successors.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub(crate) fn siblings_from_successors(&self, component_id: u64) -> Result<Siblings<N>, Error> {
        Ok(Siblings::new(
            component_id,
            self.successors(component_id)?
                .map(|x| self.predecessors(x.component_id()))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten(),
        ))
    }

    /// Returns a set of all components that match the given predicate, starting
    /// from the component with the given `component_id`.
    ///
    /// If `follow_after_match` is `true`, the search continues deeper beyond
    /// the matching components.
    pub(crate) fn find_all(
        &self,
        from: u64,
        mut pred: impl FnMut(&N) -> bool,
        follow_after_match: bool,
    ) -> Result<BTreeSet<u64>, Error> {
        let index = self.node_indices.get(&from).ok_or_else(|| {
            Error::component_not_found(format!("Component with id {} not found.", from))
        })?;
        let mut stack = vec![*index];
        let mut found = BTreeSet::new();

        while let Some(index) = stack.pop() {
            let node = &self.graph[index];
            if pred(node) {
                found.insert(node.component_id());
                if !follow_after_match {
                    continue;
                }
            }

            let neighbors = self
                .graph
                .neighbors_directed(index, petgraph::Direction::Outgoing);
            stack.extend(neighbors);
        }

        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_category::BatteryType;
    use crate::component_category::CategoryPredicates;
    use crate::error::Error;
    use crate::graph::test_utils::ComponentGraphBuilder;
    use crate::graph::test_utils::{TestComponent, TestConnection};
    use crate::ComponentCategory;
    use crate::ComponentGraphConfig;
    use crate::InverterType;

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent::new(6, ComponentCategory::Meter),
            TestComponent::new(1, ComponentCategory::Grid),
            TestComponent::new(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(3, ComponentCategory::Meter),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(8, ComponentCategory::Battery(BatteryType::LiIon)),
            TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(2, ComponentCategory::Meter),
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
        let config = ComponentGraphConfig::default();
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone(), config)?;

        assert_eq!(
            graph.component(1),
            Ok(&TestComponent::new(1, ComponentCategory::Grid))
        );
        assert_eq!(
            graph.component(5),
            Ok(&TestComponent::new(
                5,
                ComponentCategory::Battery(BatteryType::Unspecified)
            ))
        );
        assert_eq!(
            graph.component(9),
            Err(Error::component_not_found("Component with id 9 not found."))
        );

        Ok(())
    }

    #[test]
    fn test_components() -> Result<(), Error> {
        let config = ComponentGraphConfig::default();
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone(), config)?;

        assert!(graph.components().eq(&components));
        assert!(graph.components().filter(|x| x.is_battery()).eq(&[
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(8, ComponentCategory::Battery(BatteryType::LiIon))
        ]));

        Ok(())
    }

    #[test]
    fn test_connections() -> Result<(), Error> {
        let config = ComponentGraphConfig::default();
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone(), config)?;

        assert!(graph.connections().eq(&connections));

        assert!(graph
            .connections()
            .filter(|x| x.source() == 2)
            .eq(&[TestConnection::new(2, 3), TestConnection::new(2, 6)]));

        Ok(())
    }

    #[test]
    fn test_neighbors() -> Result<(), Error> {
        let config = ComponentGraphConfig::default();
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone(), config)?;

        assert!(graph.predecessors(1).is_ok_and(|x| x.eq(&[])));

        assert!(graph
            .predecessors(3)
            .is_ok_and(|x| x.eq(&[TestComponent::new(2, ComponentCategory::Meter)])));

        assert!(graph
            .successors(1)
            .is_ok_and(|x| x.eq(&[TestComponent::new(2, ComponentCategory::Meter)])));

        assert!(graph.successors(2).is_ok_and(|x| {
            x.eq(&[
                TestComponent::new(6, ComponentCategory::Meter),
                TestComponent::new(3, ComponentCategory::Meter),
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

    #[test]
    fn test_siblings() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter to the grid, with no successors.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        assert_eq!(grid_meter.component_id(), 1);

        // Add a battery chain with three inverters and two battery.
        let meter_bat_chain = builder.meter_bat_chain(3, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build()?;
        assert_eq!(
            graph
                .siblings_from_predecessors(3)
                .unwrap()
                .collect::<Vec<_>>(),
            [
                &TestComponent::new(5, ComponentCategory::Inverter(InverterType::Battery)),
                &TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery))
            ]
        );

        assert_eq!(
            graph
                .siblings_from_successors(3)
                .unwrap()
                .collect::<Vec<_>>(),
            [
                &TestComponent::new(5, ComponentCategory::Inverter(InverterType::Battery)),
                &TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery))
            ]
        );

        assert_eq!(
            graph
                .siblings_from_successors(6)
                .unwrap()
                .collect::<Vec<_>>(),
            Vec::<&TestComponent>::new()
        );

        assert_eq!(
            graph
                .siblings_from_predecessors(6)
                .unwrap()
                .collect::<Vec<_>>(),
            [&TestComponent::new(
                7,
                ComponentCategory::Battery(BatteryType::LiIon)
            )]
        );

        // Add two dangling meter to the grid meter
        let dangling_meter = builder.meter();
        builder.connect(grid_meter, dangling_meter);
        assert_eq!(dangling_meter.component_id(), 8);

        let dangling_meter = builder.meter();
        builder.connect(grid_meter, dangling_meter);
        assert_eq!(dangling_meter.component_id(), 9);

        let graph = builder.build()?;
        assert_eq!(
            graph
                .siblings_from_predecessors(8)
                .unwrap()
                .collect::<Vec<_>>(),
            [
                &TestComponent::new(9, ComponentCategory::Meter),
                &TestComponent::new(2, ComponentCategory::Meter),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_find_all() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(
            components.clone(),
            connections.clone(),
            ComponentGraphConfig::default(),
        )?;

        let found = graph.find_all(graph.root_id, |x| x.is_meter(), false)?;
        assert_eq!(found, [2].iter().cloned().collect());

        let found = graph.find_all(graph.root_id, |x| x.is_meter(), true)?;
        assert_eq!(found, [2, 3, 6].iter().cloned().collect());

        let found = graph.find_all(
            graph.root_id,
            |x| !x.is_grid() && !graph.is_component_meter(x.component_id()).unwrap_or(false),
            true,
        )?;
        assert_eq!(found, [2, 4, 5, 7, 8].iter().cloned().collect());

        let found = graph.find_all(
            6,
            |x| !x.is_grid() && !graph.is_component_meter(x.component_id()).unwrap_or(false),
            true,
        )?;
        assert_eq!(found, [7, 8].iter().cloned().collect());

        let found = graph.find_all(
            graph.root_id,
            |x| !x.is_grid() && !graph.is_component_meter(x.component_id()).unwrap_or(false),
            false,
        )?;
        assert_eq!(found, [2].iter().cloned().collect());

        let found = graph.find_all(graph.root_id, |_| true, false)?;
        assert_eq!(found, [1].iter().cloned().collect());

        let found = graph.find_all(3, |_| true, true)?;
        assert_eq!(found, [3, 4, 5].iter().cloned().collect());

        Ok(())
    }
}
