// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for checking the roles of meters in a [`ComponentGraph`].

use crate::{component_category::CategoryPredicates, ComponentGraph, Edge, Error, Node};

/// Meter role identification.
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Returns true if a node is a grid meter.
    ///
    /// A meter is identified as a grid meter if:
    ///   - it is a successor of the grid component,
    ///   - all its siblings are meters,
    ///   - if there are siblings, the successors of it and the successors of
    ///     its siblings are meters.
    pub fn is_grid_meter(&self, component_id: u64) -> Result<bool, Error> {
        let component = self.component(component_id)?;

        // Component must be a meter.
        if !component.is_meter() {
            return Ok(false);
        }

        let mut predecessors = self.predecessors(component_id)?;

        // The meter must have a grid as a predecessor.
        let Some(grid) = predecessors.next() else {
            return Ok(false);
        };

        let has_multiple_predecessors = predecessors.next().is_some();

        if !grid.is_grid() || has_multiple_predecessors {
            return Ok(false);
        }

        // All siblings must be meters.
        let mut num_grid_successors = 0;
        let mut non_meter_successors = false;
        for grid_successor in self.successors(grid.component_id())? {
            if grid_successor.is_meter() {
                num_grid_successors += 1;
            } else {
                return Ok(false);
            }
            let mut successors = self.successors(grid_successor.component_id())?;
            if successors.any(|n| !n.is_meter()) {
                non_meter_successors = true;
            }
        }

        // If there are no siblings, the meter is a grid meter.
        if num_grid_successors == 1 {
            return Ok(true);
        }

        // If there are siblings, the meter is a grid meter if the successors of
        // it and the successors of the siblings are meters.
        if non_meter_successors {
            return Ok(false);
        }
        Ok(true)
    }

    /// Returns true if the node is a PV meter.
    ///
    /// A meter is identified as a PV meter if:
    ///   - it has atleast one successor,
    ///   - all its successors are PV inverters.
    pub fn is_pv_meter(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.component(component_id)?.is_meter()
            && self.successors(component_id)?.all(|n| {
                has_successors = true;
                n.is_pv_inverter()
            })
            && has_successors)
    }

    /// Returns true if the node is a battery meter.
    ///
    /// A meter is identified as a battery meter if
    ///   - it has atleast one successor,
    ///   - all its successors are battery inverters.
    pub fn is_battery_meter(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.component(component_id)?.is_meter()
            && self.successors(component_id)?.all(|n| {
                has_successors = true;
                n.is_battery_inverter()
            })
            && has_successors)
    }

    /// Returns true if the node is an EV charger meter.
    ///
    /// A meter is identified as an EV charger meter if
    ///   - it has atleast one successor,
    ///   - all its successors are EV chargers.
    pub fn is_ev_charger_meter(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.component(component_id)?.is_meter()
            && self.successors(component_id)?.all(|n| {
                has_successors = true;
                n.is_ev_charger()
            })
            && has_successors)
    }

    /// Returns true if the node is a CHP meter.
    ///
    /// A meter is identified as a CHP meter if
    ///   - has atleast one successor,
    ///   - all its successors are CHPs.
    pub fn is_chp_meter(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.component(component_id)?.is_meter()
            && self.successors(component_id)?.all(|n| {
                has_successors = true;
                n.is_chp()
            })
            && has_successors)
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
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Meter),
            TestComponent(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(5, ComponentCategory::Battery),
            TestComponent(6, ComponentCategory::Meter),
            TestComponent(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(8, ComponentCategory::Battery),
            TestComponent(9, ComponentCategory::Meter),
            TestComponent(10, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent(11, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent(12, ComponentCategory::Meter),
            TestComponent(13, ComponentCategory::Chp),
            TestComponent(14, ComponentCategory::Meter),
            TestComponent(15, ComponentCategory::Chp),
            TestComponent(16, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent(17, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(18, ComponentCategory::Battery),
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

    fn with_multiple_grid_meters() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let (mut components, mut connections) = nodes_and_edges();

        // Add a meter to the grid without successors
        components.push(TestComponent(19, ComponentCategory::Meter));
        connections.push(TestConnection::new(1, 19));

        // Add a meter to the grid that has a battery meter and a PV meter as
        // successors.
        components.push(TestComponent(20, ComponentCategory::Meter));
        connections.push(TestConnection::new(1, 20));

        // battery chain
        components.push(TestComponent(21, ComponentCategory::Meter));
        components.push(TestComponent(
            22,
            ComponentCategory::Inverter(InverterType::Battery),
        ));
        components.push(TestComponent(23, ComponentCategory::Battery));
        connections.push(TestConnection::new(20, 21));
        connections.push(TestConnection::new(21, 22));
        connections.push(TestConnection::new(22, 23));

        // pv chain
        components.push(TestComponent(24, ComponentCategory::Meter));
        components.push(TestComponent(
            25,
            ComponentCategory::Inverter(InverterType::Solar),
        ));
        connections.push(TestConnection::new(20, 24));
        connections.push(TestConnection::new(24, 25));

        (components, connections)
    }

    fn without_grid_meters() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let (mut components, mut connections) = nodes_and_edges();

        // Add an EV charger meter to the grid, then none of the meters
        // connected to the grid should be detected as grid meters.
        components.push(TestComponent(20, ComponentCategory::Meter));
        components.push(TestComponent(21, ComponentCategory::EvCharger));
        connections.push(TestConnection::new(1, 20));
        connections.push(TestConnection::new(20, 21));

        (components, connections)
    }

    fn assert_meter_role(
        components: Vec<TestComponent>,
        connections: Vec<TestConnection>,
        filter: impl Fn(&ComponentGraph<TestComponent, TestConnection>, u64) -> Result<bool, Error>,
        expected_grid_meters: Vec<u64>,
    ) -> Result<(), Error> {
        let graph = ComponentGraph::try_new(components.clone(), connections.clone())?;

        let mut found_meters = vec![];
        for comp in graph.components() {
            if filter(&graph, comp.component_id())? {
                found_meters.push(comp.component_id());
            }
        }
        if found_meters != expected_grid_meters {
            return Err(Error::internal(format!(
                "Found meters: {:?}, Expected meters: {:?}",
                found_meters, expected_grid_meters
            )));
        }

        Ok(())
    }

    #[test]
    fn test_is_grid_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_grid_meter,
            vec![2],
        )?;

        let (components, connections) = with_multiple_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_grid_meter,
            vec![2, 19, 20],
        )?;

        let (components, connections) = without_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_grid_meter,
            vec![],
        )?;

        Ok(())
    }

    #[test]
    fn test_is_pv_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_pv_meter,
            vec![9],
        )?;

        let (components, connections) = with_multiple_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_pv_meter,
            vec![9, 24],
        )?;

        let (components, connections) = without_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_pv_meter,
            vec![9],
        )?;

        Ok(())
    }

    #[test]
    fn test_is_battery_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_battery_meter,
            vec![3, 6],
        )?;

        let (components, connections) = with_multiple_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_battery_meter,
            vec![3, 6, 21],
        )?;

        let (components, connections) = without_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_battery_meter,
            vec![3, 6],
        )?;

        Ok(())
    }

    #[test]
    fn test_is_chp_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_chp_meter,
            vec![12],
        )?;

        let (components, connections) = with_multiple_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_chp_meter,
            vec![12],
        )?;

        let (components, connections) = without_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_chp_meter,
            vec![12],
        )?;

        Ok(())
    }

    #[test]
    fn test_is_ev_charger_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_ev_charger_meter,
            vec![],
        )?;

        let (components, connections) = with_multiple_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_ev_charger_meter,
            vec![],
        )?;

        let (components, connections) = without_grid_meters();
        assert_meter_role(
            components,
            connections,
            ComponentGraph::is_ev_charger_meter,
            vec![20],
        )?;

        Ok(())
    }
}
