// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for validating that all components in a [`ComponentGraph`] are
//! connected correctly.

use crate::{
    component_category::CategoryPredicates, ComponentCategory, Edge, Error, InverterType, Node,
};

use super::ComponentGraphValidator;

impl<N, E> ComponentGraphValidator<'_, N, E>
where
    N: Node,
    E: Edge,
{
    pub(super) fn validate_root(&self) -> Result<(), Error> {
        self.ensure_root(self.root)?;
        self.ensure_not_leaf(self.root)?;
        self.ensure_exclusive_successors(self.root)?;

        Ok(())
    }

    pub(super) fn validate_meters(&self) -> Result<(), Error> {
        for meter in self.cg.components().filter(|n| n.is_meter()) {
            self.ensure_predecessor_categories(
                meter,
                &[ComponentCategory::Grid, ComponentCategory::Meter],
            )?;
            self.ensure_successor_not_categories(meter, &[ComponentCategory::Battery])?;
        }
        Ok(())
    }

    pub(super) fn validate_inverters(&self) -> Result<(), Error> {
        for inverter in self.cg.components().filter(|n| n.is_inverter()) {
            let ComponentCategory::Inverter(inverter_type) = inverter.category() else {
                continue;
            };

            self.ensure_predecessor_categories(
                inverter,
                &[ComponentCategory::Meter, ComponentCategory::Grid],
            )?;

            match inverter_type {
                InverterType::Battery => {
                    self.ensure_not_leaf(inverter)?;
                    self.ensure_successor_categories(inverter, &[ComponentCategory::Battery])?;
                }
                InverterType::Solar => {
                    self.ensure_leaf(inverter)?;
                }
                InverterType::Hybrid => {
                    self.ensure_successor_categories(inverter, &[ComponentCategory::Battery])?;
                }
                InverterType::Unspecified => {
                    return Err(Error::invalid_graph(format!(
                        "Inverter {} has an unspecified inverter type.",
                        inverter.component_id()
                    )));
                }
            }
        }

        Ok(())
    }

    pub(super) fn validate_batteries(&self) -> Result<(), Error> {
        for battery in self.cg.components().filter(|n| n.is_battery()) {
            self.ensure_leaf(battery)?;
            self.ensure_predecessor_categories(
                battery,
                &[
                    ComponentCategory::Inverter(InverterType::Battery),
                    ComponentCategory::Inverter(InverterType::Hybrid),
                ],
            )?;
        }
        Ok(())
    }

    pub(super) fn validate_ev_chargers(&self) -> Result<(), Error> {
        for ev_charger in self.cg.components().filter(|n| n.is_ev_charger()) {
            self.ensure_leaf(ev_charger)?;
            self.ensure_predecessor_categories(
                ev_charger,
                &[ComponentCategory::Meter, ComponentCategory::Grid],
            )?;
        }
        Ok(())
    }

    pub(super) fn validate_chps(&self) -> Result<(), Error> {
        for chp in self.cg.components().filter(|n| n.is_chp()) {
            self.ensure_leaf(chp)?;
            self.ensure_predecessor_categories(
                chp,
                &[ComponentCategory::Meter, ComponentCategory::Grid],
            )?;
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

    #[test]
    fn test_validate_root() {
        let components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
        ];
        let connections = vec![TestConnection::new(1, 2)];
        assert!(ComponentGraph::try_new(components, connections).is_ok());

        let components = vec![TestComponent(1, ComponentCategory::Grid)];
        let connections: Vec<TestConnection> = vec![];
        assert!(
            ComponentGraph::try_new(components, connections).is_err_and(|e| {
                e == Error::invalid_graph("Grid:1 must have at least one successor.")
            }),
        );

        let components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Meter),
        ];
        let connections: Vec<TestConnection> = vec![
            TestConnection::new(1, 2),
            TestConnection::new(1, 3),
            TestConnection::new(2, 3),
        ];

        assert!(
            ComponentGraph::try_new(components, connections).is_err_and(|e| {
                e == Error::invalid_graph(
                    "Grid:1 can't have successors with multiple predecessors. Found Meter:3.",
                )
            }),
        );
    }

    #[test]
    fn test_validate_meter() {
        let components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Battery),
        ];
        let connections = vec![TestConnection::new(1, 2), TestConnection::new(2, 3)];
        assert!(
            ComponentGraph::try_new(components, connections).is_err_and(|e| {
                e == Error::invalid_graph(
                    "Meter:2 can't have successors with categories [Battery]. Found Battery:3.",
                )
            }),
        );
    }

    #[test]
    fn test_validate_battery_inverter() {
        let mut components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(4, ComponentCategory::Electrolyzer),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph(
                    "BatteryInverter:3 can only have successors with categories [Battery]. Found Electrolyzer:4.",
                )
            }),
        );

        components.pop();
        connections.pop();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph("BatteryInverter:3 must have at least one successor.")
            }),
        );

        components.push(TestComponent(4, ComponentCategory::Battery));
        connections.push(TestConnection::new(3, 4));

        assert!(ComponentGraph::try_new(components, connections).is_ok());
    }

    #[test]
    fn test_validate_pv_inverter() {
        let mut components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent(4, ComponentCategory::Electrolyzer),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph(
                    "SolarInverter:3 can't have any successors. Found Electrolyzer:4.",
                )
            }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections).is_ok());
    }

    #[test]
    fn test_validate_hybrid_inverter() {
        let mut components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Inverter(InverterType::Hybrid)),
            TestComponent(4, ComponentCategory::Electrolyzer),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph(
                    "HybridInverter:3 can only have successors with categories [Battery]. Found Electrolyzer:4.",
                )
            }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());

        components.push(TestComponent(4, ComponentCategory::Battery));
        connections.push(TestConnection::new(3, 4));

        assert!(ComponentGraph::try_new(components, connections).is_ok());
    }

    #[test]
    fn test_validate_batteries() {
        let mut components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent(4, ComponentCategory::Battery),
            TestComponent(5, ComponentCategory::Battery),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
            TestConnection::new(4, 5),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph("Battery:4 can't have any successors. Found Battery:5.")
            }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());

        components.pop();
        components.pop();

        components.push(TestComponent(
            3,
            ComponentCategory::Inverter(InverterType::Hybrid),
        ));
        components.push(TestComponent(4, ComponentCategory::Battery));

        assert!(ComponentGraph::try_new(components.clone(), connections.clone()).is_ok());

        let components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Battery),
        ];
        let connections = vec![TestConnection::new(1, 2)];

        assert!(
            ComponentGraph::try_new(components, connections).is_err_and(|e| {
                e == Error::invalid_graph(concat!(
                    "Battery:2 can only have predecessors with categories: ",
                    "[BatteryInverter, HybridInverter]. Found Grid:1."
                ))
            }),
        );
    }

    #[test]
    fn test_validate_ev_chargers() {
        let mut components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::EvCharger),
            TestComponent(4, ComponentCategory::Electrolyzer),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph(
                    "EVCharger:3 can't have any successors. Found Electrolyzer:4.",
                )
            }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections).is_ok());
    }

    #[test]
    fn test_validate_chps() {
        let mut components = vec![
            TestComponent(1, ComponentCategory::Grid),
            TestComponent(2, ComponentCategory::Meter),
            TestComponent(3, ComponentCategory::Chp),
            TestComponent(4, ComponentCategory::Electrolyzer),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone()).is_err_and(|e| {
                e == Error::invalid_graph("CHP:3 can't have any successors. Found Electrolyzer:4.")
            }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections).is_ok());
    }
}
