// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains methods that help with graph traversal.

use crate::{component_category::CategoryPredicates, ComponentGraph, Edge, Error, Node};

/// Traversal methods.
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    fn find(
        &self,
        from: u64,
        mut pred: impl FnMut(&N) -> bool,
        direction: petgraph::Direction,
    ) -> Result<Option<&N>, Error> {
        let index = self.node_indices.get(&from).ok_or_else(|| {
            Error::component_not_found(format!("Component with id {} not found.", from))
        })?;
        let mut stack = vec![*index];

        while let Some(index) = stack.pop() {
            let node = &self.graph[index];
            if pred(node) {
                return Ok(Some(node));
            }

            let neighbors = self.graph.neighbors_directed(index, direction);
            stack.extend(neighbors);
        }

        Ok(None)
    }

    fn find_all(
        &self,
        from: u64,
        mut pred: impl FnMut(&N) -> bool,
        direction: petgraph::Direction,
    ) -> Result<Vec<&N>, Error> {
        let index = self.node_indices.get(&from).ok_or_else(|| {
            Error::component_not_found(format!("Component with id {} not found.", from))
        })?;
        let mut stack = vec![*index];
        let mut found = vec![];

        while let Some(index) = stack.pop() {
            let node = &self.graph[index];
            if pred(node) {
                found.push(node);
            }

            let neighbors = self.graph.neighbors_directed(index, direction);
            stack.extend(neighbors);
        }

        Ok(found)
    }

    /// Find the node that satisfies the given predicate starting from the given
    /// node and traversing away from the root.
    pub fn find_successor(
        &self,
        from: u64,
        pred: impl FnMut(&N) -> bool,
    ) -> Result<Option<&N>, Error> {
        self.find(from, pred, petgraph::Direction::Outgoing)
    }

    /// Find the node that satisfies the given predicate starting from the given
    /// node and traversing towards the root.
    pub fn find_predecessor(
        &self,
        from: u64,
        pred: impl FnMut(&N) -> bool,
    ) -> Result<Option<&N>, Error> {
        self.find(from, pred, petgraph::Direction::Incoming)
    }

    pub(crate) fn has_battery_successors(&self, from: u64) -> Result<bool, Error> {
        self.find(
            from,
            |n| n.is_battery() || n.is_battery_inverter(),
            petgraph::Direction::Outgoing,
        )
        .map(|n| n.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        component_category::CategoryPredicates,
        graph::test_types::{TestComponent, TestConnection},
        BatteryType, ComponentCategory, InverterType,
    };

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent::new(1, ComponentCategory::Grid),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Meter),
            TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::NaIon)),
            TestComponent::new(6, ComponentCategory::Meter),
            TestComponent::new(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(8, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(9, ComponentCategory::Meter),
            TestComponent::new(10, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent::new(11, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent::new(12, ComponentCategory::Meter),
            TestComponent::new(13, ComponentCategory::Chp),
            TestComponent::new(14, ComponentCategory::Meter),
            TestComponent::new(15, ComponentCategory::Chp),
            TestComponent::new(16, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent::new(17, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(18, ComponentCategory::Battery(BatteryType::LiIon)),
        ];
        let connections = vec![
            // Single Grid meter
            TestConnection::new(1, 2),
            // Battery chain
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
            TestConnection::new(4, 5),
            // Battery chain
            TestConnection::new(2, 6),
            TestConnection::new(6, 7),
            TestConnection::new(7, 8),
            // Solar chain
            TestConnection::new(2, 9),
            TestConnection::new(9, 10),
            TestConnection::new(9, 11),
            // CHP chain
            TestConnection::new(2, 12),
            TestConnection::new(12, 13),
            // Mixed chain
            TestConnection::new(2, 14),
            TestConnection::new(14, 15),
            TestConnection::new(14, 16),
            TestConnection::new(14, 17),
            TestConnection::new(17, 18),
        ];

        (components, connections)
    }

    #[test]
    fn test_find_successor() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        let graph = ComponentGraph::try_new(components.clone(), connections.clone())?;

        let node = graph.find_successor(1, |n| n.is_meter())?;
        assert_eq!(node, Some(&TestComponent::new(2, ComponentCategory::Meter)));

        let node = graph.find_successor(2, |n| n.is_meter())?;
        assert_eq!(node, Some(&TestComponent::new(3, ComponentCategory::Meter)));

        let node = graph.find_successor(2, |n| n.is_battery())?;
        assert_eq!(
            node,
            Some(&TestComponent::new(
                8,
                ComponentCategory::Battery(BatteryType::Unspecified)
            ))
        );

        let node = graph.find_successor(2, |n| n.is_inverter())?;
        assert_eq!(
            node,
            Some(&TestComponent::new(
                10,
                ComponentCategory::Inverter(InverterType::Solar)
            ))
        );

        let node = graph.find_successor(2, |n| n.is_chp())?;
        assert_eq!(node, Some(&TestComponent::new(13, ComponentCategory::Chp)));

        Ok(())
    }
}
