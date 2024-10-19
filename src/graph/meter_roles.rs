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
                n.is_battery_inverter(&self.config)
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

    /// Returns true if the node is a component meter.
    ///
    /// A meter is a component meter if it is one of the following:
    ///  - a PV meter,
    ///  - a battery meter,
    ///  - an EV charger meter,
    ///  - a CHP meter.
    pub fn is_component_meter(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_pv_meter(component_id)?
            || self.is_battery_meter(component_id)?
            || self.is_ev_charger_meter(component_id)?
            || self.is_chp_meter(component_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_category::BatteryType;
    use crate::component_category::EvChargerType;
    use crate::error::Error;
    use crate::graph::test_utils::{TestComponent, TestConnection};
    use crate::ComponentCategory;
    use crate::ComponentGraphConfig;
    use crate::InverterType;

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

    fn with_multiple_grid_meters() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let (mut components, mut connections) = nodes_and_edges();

        // Add a meter to the grid without successors
        components.push(TestComponent::new(19, ComponentCategory::Meter));
        connections.push(TestConnection::new(1, 19));

        // Add a meter to the grid that has a battery meter and a PV meter as
        // successors.
        components.push(TestComponent::new(20, ComponentCategory::Meter));
        connections.push(TestConnection::new(1, 20));

        // battery chain
        components.push(TestComponent::new(21, ComponentCategory::Meter));
        components.push(TestComponent::new(
            22,
            ComponentCategory::Inverter(InverterType::Battery),
        ));
        components.push(TestComponent::new(
            23,
            ComponentCategory::Battery(BatteryType::Unspecified),
        ));
        connections.push(TestConnection::new(20, 21));
        connections.push(TestConnection::new(21, 22));
        connections.push(TestConnection::new(22, 23));

        // pv chain
        components.push(TestComponent::new(24, ComponentCategory::Meter));
        components.push(TestComponent::new(
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
        components.push(TestComponent::new(20, ComponentCategory::Meter));
        components.push(TestComponent::new(
            21,
            ComponentCategory::EvCharger(EvChargerType::Ac),
        ));
        connections.push(TestConnection::new(1, 20));
        connections.push(TestConnection::new(20, 21));

        (components, connections)
    }

    fn find_matching_components(
        components: Vec<TestComponent>,
        connections: Vec<TestConnection>,
        filter: impl Fn(&ComponentGraph<TestComponent, TestConnection>, u64) -> Result<bool, Error>,
    ) -> Result<Vec<u64>, Error> {
        let config = ComponentGraphConfig::default();

        let graph = ComponentGraph::try_new(components.clone(), connections.clone(), config)?;

        let mut found_meters = vec![];
        for comp in graph.components() {
            if filter(&graph, comp.component_id())? {
                found_meters.push(comp.component_id());
            }
        }

        Ok(found_meters)
    }

    #[test]
    fn test_is_pv_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_pv_meter)?,
            vec![9],
        );

        let (components, connections) = with_multiple_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_pv_meter)?,
            vec![9, 24],
        );

        let (components, connections) = without_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_pv_meter)?,
            vec![9],
        );

        Ok(())
    }

    #[test]
    fn test_is_battery_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_battery_meter)?,
            vec![3, 6],
        );

        let (components, connections) = with_multiple_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_battery_meter)?,
            vec![3, 6, 21],
        );

        let (components, connections) = without_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_battery_meter)?,
            vec![3, 6],
        );

        Ok(())
    }

    #[test]
    fn test_is_chp_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_chp_meter)?,
            vec![12],
        );

        let (components, connections) = with_multiple_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_chp_meter)?,
            vec![12],
        );

        let (components, connections) = without_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_chp_meter)?,
            vec![12],
        );

        Ok(())
    }

    #[test]
    fn test_is_ev_charger_meter() -> Result<(), Error> {
        let (components, connections) = nodes_and_edges();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_ev_charger_meter)?,
            vec![],
        );

        let (components, connections) = with_multiple_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_ev_charger_meter)?,
            vec![],
        );

        let (components, connections) = without_grid_meters();
        assert_eq!(
            find_matching_components(components, connections, ComponentGraph::is_ev_charger_meter)?,
            vec![20],
        );

        Ok(())
    }
}
