// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for retrieving components and connections from a [`ComponentGraph`].

use crate::iterators::{Components, Connections, Neighbors, RawNeighbors, Siblings};
use crate::{ComponentGraph, Edge, Error, Node};
use petgraph::graph::NodeIndex;
use std::collections::{BTreeSet, HashSet, VecDeque};

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
                Error::component_not_found(format!("Component with id {component_id} not found."))
            })
    }

    /// Returns an iterator over the components in the graph.
    pub fn components(&self) -> Components<'_, N> {
        Components {
            iter: self.graph.raw_nodes().iter(),
        }
    }

    /// Returns an iterator over the connections in the graph.
    pub fn connections(&self) -> Connections<'_, N, E> {
        Connections {
            cg: self,
            iter: self.graph.raw_edges().iter(),
        }
    }

    /// Returns an iterator over the *raw* (graph-direct) predecessors of
    /// the component with the given `component_id`.
    ///
    /// "Raw" means every node connected by an incoming edge, including
    /// pass-through categories. Most callers want
    /// [`predecessors`][Self::predecessors] instead, which walks past
    /// pass-throughs transparently.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub fn raw_predecessors(&self, component_id: u64) -> Result<RawNeighbors<'_, N>, Error> {
        self.raw_neighbors(component_id, petgraph::Direction::Incoming)
    }

    /// Returns an iterator over the *raw* (graph-direct) successors of
    /// the component with the given `component_id`.
    ///
    /// "Raw" means every node connected by an outgoing edge, including
    /// pass-through categories. Most callers want
    /// [`successors`][Self::successors] instead, which walks past
    /// pass-throughs transparently.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub fn raw_successors(&self, component_id: u64) -> Result<RawNeighbors<'_, N>, Error> {
        self.raw_neighbors(component_id, petgraph::Direction::Outgoing)
    }

    /// Shared implementation for [`raw_predecessors`][Self::raw_predecessors]
    /// and [`raw_successors`][Self::raw_successors].
    fn raw_neighbors(
        &self,
        component_id: u64,
        direction: petgraph::Direction,
    ) -> Result<RawNeighbors<'_, N>, Error> {
        self.node_indices
            .get(&component_id)
            .map(|&index| RawNeighbors {
                graph: &self.graph,
                iter: self.graph.neighbors_directed(index, direction),
            })
            .ok_or_else(|| {
                Error::component_not_found(format!("Component with id {component_id} not found."))
            })
    }

    /// Returns an iterator over the *predecessors* of the component with
    /// the given `component_id`, walking transparently past pass-through
    /// categories.
    ///
    /// Pass-through nodes are skipped: their non-pass-through ancestors
    /// take their place in the iterator. For the raw (graph-direct) view
    /// that includes pass-throughs, use
    /// [`raw_predecessors`][Self::raw_predecessors].
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub fn predecessors(&self, component_id: u64) -> Result<Neighbors<'_, N>, Error> {
        self.collect_effective_neighbors(component_id, petgraph::Direction::Incoming)
    }

    /// Returns an iterator over the *successors* of the component with
    /// the given `component_id`, walking transparently past pass-through
    /// categories.
    ///
    /// Pass-through nodes are skipped: their non-pass-through descendants
    /// take their place in the iterator. For the raw (graph-direct) view
    /// that includes pass-throughs, use
    /// [`raw_successors`][Self::raw_successors].
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub fn successors(&self, component_id: u64) -> Result<Neighbors<'_, N>, Error> {
        self.collect_effective_neighbors(component_id, petgraph::Direction::Outgoing)
    }

    /// BFS through pass-through nodes in the given direction, collecting
    /// the first non-pass-through node along each branch.
    fn collect_effective_neighbors(
        &self,
        component_id: u64,
        direction: petgraph::Direction,
    ) -> Result<Neighbors<'_, N>, Error> {
        let start = *self.node_indices.get(&component_id).ok_or_else(|| {
            Error::component_not_found(format!("Component with id {component_id} not found."))
        })?;

        let mut queue: VecDeque<NodeIndex> =
            self.graph.neighbors_directed(start, direction).collect();
        let mut visited: HashSet<NodeIndex> = HashSet::new();
        let mut result: Vec<&N> = Vec::new();

        while let Some(idx) = queue.pop_front() {
            if !visited.insert(idx) {
                continue;
            }
            let node = &self.graph[idx];
            if node.category().is_passthrough() {
                queue.extend(self.graph.neighbors_directed(idx, direction));
            } else {
                result.push(node);
            }
        }

        Ok(Neighbors {
            iter: result.into_iter(),
        })
    }

    /// Returns an iterator over the *siblings* of the component with the
    /// given `component_id`, that have shared predecessors.
    ///
    /// Returns an error if the given `component_id` does not exist.
    pub(crate) fn siblings_from_predecessors(
        &self,
        component_id: u64,
    ) -> Result<Siblings<'_, N>, Error> {
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
    pub(crate) fn siblings_from_successors(
        &self,
        component_id: u64,
    ) -> Result<Siblings<'_, N>, Error> {
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
    /// from the component with the given `component_id`, in the given direction.
    ///
    /// If `follow_after_match` is `true`, the search continues deeper beyond
    /// the matching components.
    pub(crate) fn find_all(
        &self,
        from: u64,
        pred: impl Fn(&N) -> bool,
        direction: petgraph::Direction,
        follow_after_match: bool,
    ) -> Result<BTreeSet<u64>, Error> {
        let index = self.node_indices.get(&from).ok_or_else(|| {
            Error::component_not_found(format!("Component with id {from} not found."))
        })?;
        let mut stack = vec![*index];
        let mut visited = HashSet::new();
        let mut found = BTreeSet::new();

        while let Some(index) = stack.pop() {
            // Skip nodes already expanded: a DAG with diamonds reaches the
            // same node by multiple paths, and re-expanding it is redundant
            // (and exponential on chained diamonds).
            if !visited.insert(index) {
                continue;
            }
            let node = &self.graph[index];
            // Pass-through nodes are transparent: skip the predicate
            // check but follow through their neighbors.
            if !node.category().is_passthrough() && pred(node) {
                found.insert(node.component_id());
                if !follow_after_match {
                    continue;
                }
            }

            let neighbors = self.graph.neighbors_directed(index, direction);
            stack.extend(neighbors);
        }

        Ok(found)
    }

    /// Whether any component matching the given predicate is reachable from
    /// the component with the given `component_id`, in the given direction.
    /// Stops at the first match, unlike [`ComponentGraph::find_all`], which
    /// collects them all. Pass-through nodes are transparent here too: they
    /// never match, but the search follows through their neighbors.
    pub(crate) fn reaches_any(
        &self,
        from: u64,
        pred: impl Fn(&N) -> bool,
        direction: petgraph::Direction,
    ) -> Result<bool, Error> {
        let index = self.node_indices.get(&from).ok_or_else(|| {
            Error::component_not_found(format!("Component with id {from} not found."))
        })?;
        let mut stack = vec![*index];
        let mut visited = HashSet::new();

        while let Some(index) = stack.pop() {
            // Skip nodes already expanded: a DAG with diamonds reaches the
            // same node by multiple paths, and re-expanding it is redundant
            // (and exponential on chained diamonds).
            if !visited.insert(index) {
                continue;
            }
            let node = &self.graph[index];
            if !node.category().is_passthrough() && pred(node) {
                return Ok(true);
            }
            stack.extend(self.graph.neighbors_directed(index, direction));
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentCategory;
    use crate::ComponentGraphConfig;
    use crate::InverterType;
    use crate::component_category::BatteryType;
    use crate::component_category::CategoryPredicates;
    use crate::error::Error;
    use crate::graph::test_utils::ComponentGraphBuilder;
    use crate::graph::test_utils::{TestComponent, TestConnection};

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent::new(6, ComponentCategory::Meter),
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
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
            Ok(&TestComponent::new(
                1,
                ComponentCategory::GridConnectionPoint
            ))
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

        assert!(
            graph
                .connections()
                .filter(|x| x.source() == 2)
                .eq(&[TestConnection::new(2, 3), TestConnection::new(2, 6)])
        );

        Ok(())
    }

    #[test]
    fn test_neighbors() -> Result<(), Error> {
        let config = ComponentGraphConfig::default();
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone(), config)?;

        assert!(graph.predecessors(1).is_ok_and(|x| x.eq(&[])));

        assert!(
            graph
                .predecessors(3)
                .is_ok_and(|x| x.eq(&[TestComponent::new(2, ComponentCategory::Meter)]))
        );

        assert!(
            graph
                .successors(1)
                .is_ok_and(|x| x.eq(&[TestComponent::new(2, ComponentCategory::Meter)]))
        );

        assert!(graph.successors(2).is_ok_and(|x| {
            x.eq(&[
                TestComponent::new(6, ComponentCategory::Meter),
                TestComponent::new(3, ComponentCategory::Meter),
            ])
        }));

        assert!(graph.successors(5).is_ok_and(|x| x.eq(&[])));

        assert!(
            graph
                .predecessors(32)
                .is_err_and(|e| e == Error::component_not_found("Component with id 32 not found."))
        );
        assert!(
            graph
                .successors(32)
                .is_err_and(|e| e == Error::component_not_found("Component with id 32 not found."))
        );

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

        let graph = builder.build(None)?;
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

        let graph = builder.build(None)?;
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

    /// `raw_predecessors` / `raw_successors` expose the graph-direct
    /// view (including pass-through nodes), while `predecessors` /
    /// `successors` walk past them.
    ///
    /// Topology: `Grid → PT → Meter → BatteryInverter → Battery`.
    #[test]
    fn test_raw_neighbors_includes_passthroughs() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, pt);
        builder.connect(pt, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;

        // Raw view sees the PT directly.
        let raw_preds: Vec<u64> = graph
            .raw_predecessors(meter.component_id())?
            .map(|n| n.component_id())
            .collect();
        assert_eq!(raw_preds, vec![pt.component_id()]);

        let raw_succs: Vec<u64> = graph
            .raw_successors(grid.component_id())?
            .map(|n| n.component_id())
            .collect();
        assert_eq!(raw_succs, vec![pt.component_id()]);

        // Effective view walks past the PT.
        let preds: Vec<u64> = graph
            .predecessors(meter.component_id())?
            .map(|n| n.component_id())
            .collect();
        assert_eq!(preds, vec![grid.component_id()]);

        let succs: Vec<u64> = graph
            .successors(grid.component_id())?
            .map(|n| n.component_id())
            .collect();
        assert_eq!(succs, vec![meter.component_id()]);

        // Unknown component_id behaves the same as the effective methods.
        assert!(graph.raw_predecessors(999).is_err());
        assert!(graph.raw_successors(999).is_err());

        // Make sure the unused `battery` and `inverter` handles aren't
        // optimised away in unrelated test setup.
        let _ = (battery, inverter);
        Ok(())
    }

    /// `find_all` skips pass-through nodes when checking the predicate
    /// — even if the predicate would match. This keeps PTs out of
    /// callers' result sets without forcing them to filter.
    ///
    /// Topology: `Grid → PT → Meter`.
    #[test]
    fn test_find_all_skips_passthroughs() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();

        builder.connect(grid, pt);
        builder.connect(pt, meter);

        let graph = builder.build(None)?;

        // Predicate matches everything; PT is excluded from the result.
        let found = graph.find_all(
            grid.component_id(),
            |_| true,
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert_eq!(
            found,
            BTreeSet::from([grid.component_id(), meter.component_id()])
        );

        // Predicate that explicitly tries to match PTs still returns nothing.
        let found = graph.find_all(
            grid.component_id(),
            |n| n.category() == ComponentCategory::PowerTransformer,
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert!(found.is_empty());
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

        let found = graph.find_all(
            graph.root_id,
            |x| x.is_meter(),
            petgraph::Direction::Outgoing,
            false,
        )?;
        assert_eq!(found, [2].iter().cloned().collect());

        let found = graph.find_all(
            graph.root_id,
            |x| x.is_meter(),
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert_eq!(found, [2, 3, 6].iter().cloned().collect());

        let found = graph.find_all(
            graph.root_id,
            |x| !x.is_grid() && !graph.is_component_meter(x.component_id()).unwrap_or(false),
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert_eq!(found, [2, 4, 5, 7, 8].iter().cloned().collect());

        let found = graph.find_all(
            6,
            |x| !x.is_grid() && !graph.is_component_meter(x.component_id()).unwrap_or(false),
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert_eq!(found, [7, 8].iter().cloned().collect());

        let found = graph.find_all(
            graph.root_id,
            |x| !x.is_grid() && !graph.is_component_meter(x.component_id()).unwrap_or(false),
            petgraph::Direction::Outgoing,
            false,
        )?;
        assert_eq!(found, [2].iter().cloned().collect());

        let found = graph.find_all(
            graph.root_id,
            |_| true,
            petgraph::Direction::Outgoing,
            false,
        )?;
        assert_eq!(found, [1].iter().cloned().collect());

        let found = graph.find_all(3, |_| true, petgraph::Direction::Outgoing, true)?;
        assert_eq!(found, [3, 4, 5].iter().cloned().collect());

        Ok(())
    }

    /// `find_all` deduplicates on a re-converging (diamond) topology: a node
    /// reachable by two paths is expanded once, not once per path. This is the
    /// shape the `visited` set guards — the tree topologies above never exercise
    /// it. `follow_after_match = true` is the case that actually re-expands (a
    /// matched node keeps expanding), so the diamond apex and its subtree must
    /// still appear exactly once.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1, Meter:2}`, both `→ Inverter:3 → Battery:4`.
    #[test]
    fn test_find_all_dedups_on_diamond() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter_a = builder.meter();
        let meter_b = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, meter_a);
        builder.connect(grid, meter_b);
        // The inverter is the diamond apex: reachable via both meters.
        builder.connect(meter_a, inverter);
        builder.connect(meter_b, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;

        // follow_after_match = true: the inverter matches yet keeps expanding, and
        // it is reached by both meters — it and its battery must appear once each.
        let found = graph.find_all(
            grid.component_id(),
            |n| !n.is_grid(),
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert_eq!(
            found,
            BTreeSet::from([
                meter_a.component_id(),
                meter_b.component_id(),
                inverter.component_id(),
                battery.component_id(),
            ])
        );

        // A predicate matching only the apex's subtree still reaches it through
        // the diamond — the apex is expanded, not skipped before its successors.
        let found = graph.find_all(
            grid.component_id(),
            |n| n.is_battery(),
            petgraph::Direction::Outgoing,
            true,
        )?;
        assert_eq!(found, BTreeSet::from([battery.component_id()]));

        Ok(())
    }
}
