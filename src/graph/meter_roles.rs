// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for checking the roles of meters in a [`ComponentGraph`].

use crate::{ComponentGraph, Edge, Error, Node, component_category::CategoryPredicates};

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

    /// Returns true if the node is a Wind Turbine meter.
    ///
    /// A meter is identified as a Wind Turbine meter if
    ///   - has atleast one successor
    ///   - all its successors are Wind Turbines.
    pub fn is_wind_turbine_meter(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.component(component_id)?.is_meter()
            && self.successors(component_id)?.all(|n| {
                has_successors = true;
                n.is_wind_turbine()
            })
            && has_successors)
    }

    /// Returns true if the node is a steam boiler meter.
    ///
    /// A meter is identified as a steam boiler meter if
    ///   - it has at least one successor
    ///   - all its successors are steam boilers.
    pub fn is_steam_boiler_meter(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.component(component_id)?.is_meter()
            && self.successors(component_id)?.all(|n| {
                has_successors = true;
                n.is_steam_boiler()
            })
            && has_successors)
    }

    /// Returns true if the node is a component meter.
    ///
    /// A meter is a component meter if it is one of the following:
    ///  - a PV meter,
    ///  - a battery meter,
    ///  - an EV charger meter,
    ///  - a CHP meter,
    ///  - a Wind Turbine meter,
    ///  - a Steam Boiler meter.
    pub fn is_component_meter(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_pv_meter(component_id)?
            || self.is_battery_meter(component_id)?
            || self.is_ev_charger_meter(component_id)?
            || self.is_chp_meter(component_id)?
            || self.is_wind_turbine_meter(component_id)?
            || self.is_steam_boiler_meter(component_id)?)
    }

    /// Returns true if the node is part of a battery chain.
    ///
    /// A component is part of a battery chain if it is one of the following:
    ///  - a battery meter,
    ///  - a battery inverter,
    ///  - a battery.
    pub fn is_battery_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_battery_meter(component_id)? || {
            let component = self.component(component_id)?;
            component.is_battery() || component.is_battery_inverter(&self.config)
        })
    }

    /// Returns true if the node is part of a PV chain.
    ///
    /// A component is part of a PV chain if it is one of the following:
    ///  - a PV meter,
    ///  - a PV inverter.
    pub fn is_pv_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_pv_meter(component_id)? || self.component(component_id)?.is_pv_inverter())
    }

    /// Returns true if the node is part of a CHP chain.
    ///
    /// A component is part of a CHP chain if it is one of the following:
    ///  - a CHP meter,
    ///  - a CHP.
    pub fn is_chp_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_chp_meter(component_id)? || self.component(component_id)?.is_chp())
    }

    /// Returns true if the node is part of an EV charger chain.
    ///
    /// A component is part of an EV charger chain if it is one of the following:
    ///  - an EV charger meter,
    ///  - an EV charger.
    pub fn is_ev_charger_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(
            self.is_ev_charger_meter(component_id)?
                || self.component(component_id)?.is_ev_charger(),
        )
    }

    /// Returns true if the node is part of a Wind Turbine chain.
    ///
    /// A component is part of a Wind Turbine chain if it is one of the following:
    /// - a Wind Turbine meter,
    /// - a Wind Turbine.
    pub fn is_wind_turbine_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_wind_turbine_meter(component_id)?
            || self.component(component_id)?.is_wind_turbine())
    }

    /// Returns true if the node is part of a steam boiler chain.
    ///
    /// A component is part of a steam boiler chain if it is one of the following:
    /// - a steam boiler meter,
    /// - a steam boiler.
    pub fn is_steam_boiler_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_steam_boiler_meter(component_id)?
            || self.component(component_id)?.is_steam_boiler())
    }

    /// Returns true if the node is part of a component chain.
    ///
    /// A component is part of a component chain if it is part of one of the
    /// following:
    ///  - a battery chain,
    ///  - a PV chain,
    ///  - an EV charger chain,
    ///  - a CHP chain,
    ///  - a Wind Turbine chain,
    ///  - a steam boiler chain.
    pub fn is_component_chain(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_battery_chain(component_id)?
            || self.is_pv_chain(component_id)?
            || self.is_ev_charger_chain(component_id)?
            || self.is_chp_chain(component_id)?
            || self.is_wind_turbine_chain(component_id)?
            || self.is_steam_boiler_chain(component_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::graph::test_utils::{TestComponent, TestConnection, TestGraph, graph};

    fn nodes_and_edges() -> TestGraph {
        graph! {
            1: grid => {
                // Single grid meter
                2: meter => {
                    3: meter => { 4: inv_bat => { 5: bat(NaIon) } },
                    6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
                    9: meter => { 10: inv_pv, 11: inv_pv },
                    12: meter => { 13: chp },
                    // Mixed chain
                    14: meter => { 15: chp, 16: inv_pv, 17: inv_bat => { 18: bat } },
                    19: meter => { 20: boiler },
                },
            },
        }
    }

    fn with_multiple_grid_meters() -> TestGraph {
        graph! {
            1: grid => {
                2: meter => {
                    3: meter => { 4: inv_bat => { 5: bat(NaIon) } },
                    6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
                    9: meter => { 10: inv_pv, 11: inv_pv },
                    12: meter => { 13: chp },
                    // Mixed chain
                    14: meter => { 15: chp, 16: inv_pv, 17: inv_bat => { 18: bat } },
                    19: meter => { 20: boiler },
                },
                // A meter on the grid without successors.
                21: meter,
                // A meter on the grid that has a battery meter, a PV meter and
                // a steam boiler meter as successors.
                22: meter => {
                    23: meter => { 24: inv_bat => { 25: bat(Unspecified) } },
                    26: meter => { 27: inv_pv },
                    28: meter => { 29: boiler },
                },
            },
        }
    }

    fn without_grid_meters() -> TestGraph {
        graph! {
            1: grid => {
                2: meter => {
                    3: meter => { 4: inv_bat => { 5: bat(NaIon) } },
                    6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
                    9: meter => { 10: inv_pv, 11: inv_pv },
                    12: meter => { 13: chp },
                    // Mixed chain
                    14: meter => { 15: chp, 16: inv_pv, 17: inv_bat => { 18: bat } },
                    19: meter => { 20: boiler },
                },
                // An EV charger meter on the grid: none of the meters connected
                // to the grid should be detected as grid meters.
                21: meter => { 22: ev },
            },
        }
    }

    fn find_matching_components(
        g: TestGraph,
        filter: impl Fn(&ComponentGraph<TestComponent, TestConnection>, u64) -> Result<bool, Error>,
    ) -> Result<Vec<u64>, Error> {
        let graph = g.build(None)?;

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
        assert_eq!(
            find_matching_components(nodes_and_edges(), ComponentGraph::is_pv_meter)?,
            vec![9],
        );

        assert_eq!(
            find_matching_components(with_multiple_grid_meters(), ComponentGraph::is_pv_meter)?,
            vec![9, 26],
        );

        assert_eq!(
            find_matching_components(without_grid_meters(), ComponentGraph::is_pv_meter)?,
            vec![9],
        );

        Ok(())
    }

    #[test]
    fn test_is_battery_meter() -> Result<(), Error> {
        assert_eq!(
            find_matching_components(nodes_and_edges(), ComponentGraph::is_battery_meter)?,
            vec![3, 6],
        );

        assert_eq!(
            find_matching_components(
                with_multiple_grid_meters(),
                ComponentGraph::is_battery_meter
            )?,
            vec![3, 6, 23],
        );

        assert_eq!(
            find_matching_components(without_grid_meters(), ComponentGraph::is_battery_meter)?,
            vec![3, 6],
        );

        Ok(())
    }

    #[test]
    fn test_is_chp_meter() -> Result<(), Error> {
        assert_eq!(
            find_matching_components(nodes_and_edges(), ComponentGraph::is_chp_meter)?,
            vec![12],
        );

        assert_eq!(
            find_matching_components(with_multiple_grid_meters(), ComponentGraph::is_chp_meter)?,
            vec![12],
        );

        assert_eq!(
            find_matching_components(without_grid_meters(), ComponentGraph::is_chp_meter)?,
            vec![12],
        );

        Ok(())
    }

    #[test]
    fn test_is_ev_charger_meter() -> Result<(), Error> {
        assert_eq!(
            find_matching_components(nodes_and_edges(), ComponentGraph::is_ev_charger_meter)?,
            vec![],
        );

        assert_eq!(
            find_matching_components(
                with_multiple_grid_meters(),
                ComponentGraph::is_ev_charger_meter
            )?,
            vec![],
        );

        assert_eq!(
            find_matching_components(without_grid_meters(), ComponentGraph::is_ev_charger_meter)?,
            vec![21],
        );

        Ok(())
    }

    #[test]
    fn test_is_steam_boiler_meter() -> Result<(), Error> {
        assert_eq!(
            find_matching_components(nodes_and_edges(), ComponentGraph::is_steam_boiler_meter)?,
            vec![19],
        );

        assert_eq!(
            find_matching_components(
                with_multiple_grid_meters(),
                ComponentGraph::is_steam_boiler_meter
            )?,
            vec![19, 28],
        );

        assert_eq!(
            find_matching_components(without_grid_meters(), ComponentGraph::is_steam_boiler_meter)?,
            vec![19],
        );

        Ok(())
    }
}
