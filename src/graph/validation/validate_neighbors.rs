// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for validating that all components in a [`ComponentGraph`] are
//! connected correctly.

use crate::{
    ComponentCategory, Edge, Error, InverterType, Node, ValidationError,
    component_category::CategoryPredicates,
};

use super::ComponentGraphValidator;

impl<N, E> ComponentGraphValidator<'_, N, E>
where
    N: Node,
    E: Edge,
{
    /// Validates that the root node:
    ///  - does not have any predecessors,
    ///  - is not a leaf node,
    ///  - has only exclusive successors i.e. none of the root node's successors
    ///    have any other predecessors.
    pub(super) fn validate_root(&self) -> Result<(), Error> {
        self.ensure_root(self.root)?;
        self.ensure_not_leaf(self.root)?;
        self.ensure_exclusive_successors(self.root)?;

        Ok(())
    }

    /// Validates that all meters:
    ///  - have only the Grid or another Meter as predecessors,
    ///  - don't have Batteries as successors.
    pub(super) fn validate_meters(&self) -> Result<(), Error> {
        for meter in self.cg.components().filter(|n| n.is_meter()) {
            self.ensure_on_predecessors(
                meter,
                |n| n.is_grid() || n.is_meter(),
                "the Grid or a Meter",
            )?;
            self.ensure_on_successors(meter, |n| !n.is_battery(), "not Batteries")?;
        }
        Ok(())
    }

    /// Validates that inverters have only the Grid or a Meter as predecessors.
    ///
    /// Depending on the type, the following checks are performed:
    ///  - **Battery Inverters**:
    ///    - have only Batteries as successors,
    ///    - have at least one Battery as a successor.
    ///
    ///  - **Solar Inverters**:
    ///    - don't have any successors.
    ///
    ///  - **Hybrid Inverters**:
    ///    - have only Batteries as successors.
    pub(super) fn validate_inverters(&self) -> Result<(), Error> {
        for inverter in self.cg.components().filter(|n| n.is_inverter()) {
            let ComponentCategory::Inverter(inverter_type) = inverter.category() else {
                continue;
            };

            self.ensure_on_predecessors(
                inverter,
                |n| n.is_grid() || n.is_meter(),
                "the Grid or a Meter",
            )?;

            match inverter_type {
                InverterType::Battery => {
                    self.ensure_not_leaf(inverter)?;
                    self.ensure_on_successors(inverter, |n| n.is_battery(), "Batteries")?;
                }
                InverterType::Pv => {
                    self.ensure_leaf(inverter)?;
                }
                InverterType::Hybrid => {
                    self.ensure_on_successors(inverter, |n| n.is_battery(), "Batteries")?;
                }
                InverterType::Unspecified => {
                    if !self.cg.config.allow_unspecified_inverters {
                        return Err(ValidationError::new(
                            format!(
                                "Inverter {} has an unspecified inverter type.",
                                inverter.component_id()
                            ),
                            [inverter.component_id()],
                        )
                        .into());
                    } else {
                        tracing::debug!(
                            concat!(
                                "Inverter {} has an unspecified inverter type will be ",
                                "considered a Battery Inverter."
                            ),
                            inverter.component_id()
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Validates that Batteries:
    ///  - have only BatteryInverters or HybridInverters as predecessors,
    ///  - don't have any successors.
    pub(super) fn validate_batteries(&self) -> Result<(), Error> {
        for battery in self.cg.components().filter(|n| n.is_battery()) {
            self.ensure_leaf(battery)?;
            self.ensure_on_predecessors(
                battery,
                |n| n.is_battery_inverter(&self.cg.config) || n.is_hybrid_inverter(),
                "BatteryInverters or HybridInverters",
            )?;
        }
        Ok(())
    }

    /// Validates that EV Chargers:
    ///  - have only the Grid or a Meter as predecessors,
    ///  - don't have any successors.
    pub(super) fn validate_ev_chargers(&self) -> Result<(), Error> {
        for ev_charger in self.cg.components().filter(|n| n.is_ev_charger()) {
            self.ensure_leaf(ev_charger)?;
            self.ensure_on_predecessors(
                ev_charger,
                |n| n.is_grid() || n.is_meter(),
                "the Grid or a Meter",
            )?;
        }
        Ok(())
    }

    /// Validates that CHPs:
    ///  - have only the Grid or a Meter as predecessors,
    ///  - don't have any successors.
    pub(super) fn validate_chps(&self) -> Result<(), Error> {
        for chp in self.cg.components().filter(|n| n.is_chp()) {
            self.ensure_leaf(chp)?;
            self.ensure_on_predecessors(
                chp,
                |n| n.is_grid() || n.is_meter(),
                "the Grid or a Meter",
            )?;
        }
        Ok(())
    }

    /// Validates that steam boilers:
    ///  - have only the Grid or a Meter as predecessors,
    ///  - don't have any successors.
    pub(super) fn validate_steam_boilers(&self) -> Result<(), Error> {
        for steam_boiler in self.cg.components().filter(|n| n.is_steam_boiler()) {
            self.ensure_leaf(steam_boiler)?;
            self.ensure_on_predecessors(
                steam_boiler,
                |n| n.is_grid() || n.is_meter(),
                "the Grid or a Meter",
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
    use crate::ComponentGraphConfig;
    use crate::InverterType;
    use crate::component_category::BatteryType;
    use crate::component_category::EvChargerType;
    use crate::graph::test_utils::{TestComponent, TestConnection, validation_error};

    #[test]
    fn test_validate_root() {
        let config = ComponentGraphConfig::default();
        let components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
        ];
        let connections = vec![TestConnection::new(1, 2)];
        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());

        let components = vec![TestComponent::new(
            1,
            ComponentCategory::GridConnectionPoint,
        )];
        let connections: Vec<TestConnection> = vec![];
        assert!(
            ComponentGraph::try_new(components, connections, config.clone()).is_err_and(|e| {
                e == validation_error(
                    "GridConnectionPoint:1 must have at least one successor.",
                    &[1],
                )
            }),
        );

        let components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Meter),
        ];
        let connections: Vec<TestConnection> = vec![
            TestConnection::new(1, 2),
            TestConnection::new(1, 3),
            TestConnection::new(2, 3),
        ];

        assert!(
            ComponentGraph::try_new(components, connections, config.clone()).is_err_and(|e| {
                e == validation_error(
                    concat!(
                        "GridConnectionPoint:1 can't have successors with ",
                        "multiple predecessors. Found Meter:3."
                    ),
                    &[1, 3],
                )
            }),
        );
    }

    #[test]
    fn test_validate_meter() {
        let config = ComponentGraphConfig::default();
        let components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Battery(BatteryType::LiIon)),
        ];
        let connections = vec![TestConnection::new(1, 2), TestConnection::new(2, 3)];

        let Err(error) = ComponentGraph::try_new(components, connections, config.clone()) else {
            panic!("expected validation to fail");
        };
        // A single bad meter -> battery edge trips two neighbor rules, which are
        // collected together rather than flattened into one string.
        assert_eq!(
            error,
            Error::validation_errors(vec![
                ValidationError::new(
                    "Meter:2 can only have successors that are not Batteries. Found Battery(LiIon):3.",
                    [2, 3],
                ),
                ValidationError::new(
                    concat!(
                        "Battery(LiIon):3 can only have predecessors that are ",
                        "BatteryInverters or HybridInverters. Found Meter:2."
                    ),
                    [3, 2],
                ),
            ]),
        );
    }

    #[test]
    fn test_validate_battery_inverter() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(4, ComponentCategory::WindTurbine),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        let Err(err) =
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
        else {
            panic!()
        };
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "BatteryInverter:3 can only have successors that are Batteries. Found WindTurbine:4.",
                        &[3, 4],
                    )
                }),
            "{}",
            err
        );

        components.pop();
        connections.pop();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "BatteryInverter:3 must have at least one successor.",
                        &[3],
                    )
                }),
        );

        components.push(TestComponent::new(
            4,
            ComponentCategory::Battery(BatteryType::LiIon),
        ));
        connections.push(TestConnection::new(3, 4));

        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());
    }

    #[test]
    fn test_validate_pv_inverter() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Inverter(InverterType::Pv)),
            TestComponent::new(4, ComponentCategory::WindTurbine),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        // With default config, this validation fails
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "PvInverter:3 can't have any successors. Found WindTurbine:4.",
                        &[3, 4],
                    )
                }),
        );
        // With `allow_component_validation_failures=true`, this would pass.
        assert!(
            ComponentGraph::try_new(
                components.clone(),
                connections.clone(),
                ComponentGraphConfig::builder()
                    .allow_component_validation_failures(true)
                    .build(),
            )
            .is_ok()
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());
    }

    #[test]
    fn test_validate_hybrid_inverter() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Inverter(InverterType::Hybrid)),
            TestComponent::new(4, ComponentCategory::WindTurbine),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        concat!(
                            "HybridInverter:3 can only have successors that are Batteries. ",
                            "Found WindTurbine:4."
                        ),
                        &[3, 4],
                    )
                }),
        );

        components.pop();
        connections.pop();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );

        components.push(TestComponent::new(
            4,
            ComponentCategory::Battery(BatteryType::LiIon),
        ));
        connections.push(TestConnection::new(3, 4));

        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());
    }

    #[test]
    fn test_validate_batteries() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(4, ComponentCategory::Battery(BatteryType::NaIon)),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::LiIon)),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
            TestConnection::new(4, 5),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "Battery(NaIon):4 can't have any successors. Found Battery(LiIon):5.",
                        &[4, 5],
                    )
                }),
        );

        components.pop();
        connections.pop();

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );

        components.pop();
        components.pop();

        components.push(TestComponent::new(
            3,
            ComponentCategory::Inverter(InverterType::Hybrid),
        ));
        components.push(TestComponent::new(
            4,
            ComponentCategory::Battery(BatteryType::LiIon),
        ));

        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_ok()
        );

        let components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Battery(BatteryType::LiIon)),
        ];
        let connections = vec![TestConnection::new(1, 2)];

        assert!(
            ComponentGraph::try_new(components, connections, config.clone()).is_err_and(|e| {
                e == validation_error(
                    concat!(
                        "Battery(LiIon):2 can only have predecessors that are ",
                        "BatteryInverters or HybridInverters. Found GridConnectionPoint:1."
                    ),
                    &[2, 1],
                )
            }),
        );
    }

    #[test]
    fn test_validate_ev_chargers() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::EvCharger(EvChargerType::Dc)),
            TestComponent::new(4, ComponentCategory::WindTurbine),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "EVCharger(DC):3 can't have any successors. Found WindTurbine:4.",
                        &[3, 4],
                    )
                }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());
    }

    #[test]
    fn test_validate_chps() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Chp),
            TestComponent::new(4, ComponentCategory::WindTurbine),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "CHP:3 can't have any successors. Found WindTurbine:4.",
                        &[3, 4],
                    )
                }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());
    }

    #[test]
    fn test_validate_steam_boilers() {
        let config = ComponentGraphConfig::default();
        let mut components = vec![
            TestComponent::new(1, ComponentCategory::GridConnectionPoint),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::SteamBoiler),
            TestComponent::new(4, ComponentCategory::WindTurbine),
        ];
        let mut connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
        ];
        assert!(
            ComponentGraph::try_new(components.clone(), connections.clone(), config.clone())
                .is_err_and(|e| {
                    e == validation_error(
                        "SteamBoiler:3 can't have any successors. Found WindTurbine:4.",
                        &[3, 4],
                    )
                }),
        );

        components.pop();
        connections.pop();

        assert!(ComponentGraph::try_new(components, connections, config.clone()).is_ok());
    }
}
