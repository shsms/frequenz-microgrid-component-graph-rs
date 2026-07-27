// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for building formulas for various microgrid metrics.

use std::collections::BTreeSet;

use crate::ComponentGraph;
use crate::Edge;
use crate::Error;
use crate::Node;
use crate::component_category::CategoryPredicates;

mod expr;
mod fallback;
mod formula;
mod generators;
mod traversal;

use expr::Expr;
pub use formula::Formula;

/// Formulas for various microgrid metrics.
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Returns the consumer formula for the graph.
    pub fn consumer_formula(&self) -> Result<Formula, Error> {
        generators::consumer::ConsumerFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the grid formula for the graph.
    pub fn grid_formula(&self) -> Result<Formula, Error> {
        generators::grid::GridFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the producer formula for the graph.
    pub fn producer_formula(&self) -> Result<Formula, Error> {
        generators::producer::ProducerFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the battery formula with the given battery IDs.
    ///
    /// If `battery_ids` is `None`, the formula will contain all batteries in
    /// the graph.
    pub fn battery_formula(&self, battery_ids: Option<BTreeSet<u64>>) -> Result<Formula, Error> {
        generators::battery::BatteryFormulaBuilder::try_new(self, battery_ids)?.build()
    }

    /// Returns the CHP formula for the graph.
    pub fn chp_formula(&self, chp_ids: Option<BTreeSet<u64>>) -> Result<Formula, Error> {
        generators::category::category_formula(
            self,
            chp_ids,
            |node| node.is_chp(),
            "a CHP",
            self.config.prefer_meters_in_chp_formula(),
        )
    }

    /// Returns the PV formula for the graph.
    pub fn pv_formula(&self, pv_inverter_ids: Option<BTreeSet<u64>>) -> Result<Formula, Error> {
        generators::category::category_formula(
            self,
            pv_inverter_ids,
            |node| node.is_pv_inverter(),
            "a PV inverter",
            self.config.prefer_meters_in_pv_formula(),
        )
    }

    /// Returns the wind_turbine formula for the graph.
    pub fn wind_turbine_formula(
        &self,
        wind_turbine_ids: Option<BTreeSet<u64>>,
    ) -> Result<Formula, Error> {
        generators::category::category_formula(
            self,
            wind_turbine_ids,
            |node| node.is_wind_turbine(),
            "a wind turbine",
            self.config.prefer_meters_in_wind_turbine_formula(),
        )
    }

    /// Returns the EV charger formula for the graph.
    pub fn ev_charger_formula(
        &self,
        ev_charger_ids: Option<BTreeSet<u64>>,
    ) -> Result<Formula, Error> {
        generators::category::category_formula(
            self,
            ev_charger_ids,
            |node| node.is_ev_charger(),
            "an EV charger",
            self.config.prefer_meters_in_ev_charger_formula(),
        )
    }

    /// Returns the formula for a specific component by its ID.
    ///
    /// A component that provides no telemetry has no reading to emit, so its
    /// formula is `None`.
    ///
    /// Returns an error when `component_id` is not in the graph.
    pub fn component_formula(&self, component_id: u64) -> Result<Formula, Error> {
        if !self.component(component_id)?.provides_telemetry() {
            return Ok(Expr::None.into());
        }
        Ok(Expr::component(component_id).into())
    }

    /// Returns the grid coalesce formula for the graph.
    ///
    /// This formula is used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all meters,
    /// PV inverters, and battery inverters that are directly connected to the
    /// grid.
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn grid_coalesce_formula(&self) -> Result<Formula, Error> {
        generators::grid_coalesce::GridCoalesceFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the battery AC coalesce formula for the given components.
    ///
    /// This formula is used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all the specified
    /// battery meters and corresponding inverters.
    ///
    /// When the `battery_ids` parameter is `None`, it will include all the
    /// battery meters and inverters in the graph.
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn battery_ac_coalesce_formula(
        &self,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<Formula, Error> {
        generators::battery_ac_coalesce::BatteryAcCoalesceFormulaBuilder::try_new(
            self,
            battery_ids,
        )?
        .build()
    }

    /// Returns the PV AC coalesce formula for the given components.
    ///
    /// This formula is used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all the specified
    /// PV meters and corresponding inverters.
    ///
    /// When the `pv_inverter_ids` parameter is `None`, it will include all the
    /// PV meters and inverters in the graph.
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn pv_ac_coalesce_formula(
        &self,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<Formula, Error> {
        generators::pv_ac_coalesce::PVAcCoalesceFormulaBuilder::try_new(self, pv_inverter_ids)?
            .build()
    }

    /// Returns the AC coalesce formula for a specific component by its ID.
    ///
    /// A component that provides no telemetry has no reading to emit, so its
    /// formula is `None`.
    ///
    /// Returns an error when `component_id` is not in the graph.
    pub fn component_ac_coalesce_formula(&self, component_id: u64) -> Result<Formula, Error> {
        if !self.component(component_id)?.provides_telemetry() {
            return Ok(Expr::None.into());
        }
        Ok(Expr::component(component_id).into())
    }

    /// Returns the steam boiler formula for the graph.
    pub fn steam_boiler_formula(
        &self,
        steam_boiler_ids: Option<BTreeSet<u64>>,
    ) -> Result<Formula, Error> {
        generators::category::category_formula(
            self,
            steam_boiler_ids,
            |node| node.is_steam_boiler(),
            "a steam boiler",
            self.config.prefer_meters_in_steam_boiler_formula(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ComponentCategory, Error, InverterType, OperationalMode,
        graph::test_utils::ComponentGraphBuilder,
    };

    /// `component_formula` and `component_ac_coalesce_formula` return the bare
    /// reading of the requested component — no meter fallback, even when the
    /// component sits behind a meter the category formulas would drill into.
    #[test]
    fn test_component_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let inv = inverter.component_id();

        assert_eq!(graph.component_formula(inv)?.to_string(), format!("#{inv}"));
        assert_eq!(
            graph.component_ac_coalesce_formula(inv)?.to_string(),
            format!("#{inv}")
        );
        Ok(())
    }

    /// A component that provides no telemetry has no reading, so both formula
    /// variants are `None`.
    #[test]
    fn test_component_formula_no_telemetry() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let inverter = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Battery),
            OperationalMode::ControlOnly,
        );
        let battery = builder.battery();
        builder.connect(grid, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let inv = inverter.component_id();

        assert_eq!(graph.component_formula(inv)?.to_string(), "None");
        assert_eq!(
            graph.component_ac_coalesce_formula(inv)?.to_string(),
            "None"
        );
        Ok(())
    }

    /// The no-telemetry check has no category test, so a meter with no
    /// telemetry gets `None` like any other component. The meter here is not
    /// the grid meter, whose term is already null for its own reason.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 (no telemetry) → PV:3`.
    #[test]
    fn test_component_formula_no_telemetry_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        let silent_meter =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        let pv = builder.solar_inverter();
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, silent_meter);
        builder.connect(silent_meter, pv);

        let graph = builder.build(None)?;
        let id = silent_meter.component_id();

        assert_eq!(graph.component_formula(id)?.to_string(), "None");
        assert_eq!(graph.component_ac_coalesce_formula(id)?.to_string(), "None");
        Ok(())
    }

    /// Both component formula variants check the given id and return an error
    /// when it is not in the graph.
    #[test]
    fn test_component_formula_unknown_id() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        builder.connect(grid, meter);
        let graph = builder.build(None)?;

        assert!(
            graph
                .component_formula(99)
                .is_err_and(|e| e == Error::component_not_found("Component with id 99 not found."))
        );
        assert!(
            graph
                .component_ac_coalesce_formula(99)
                .is_err_and(|e| e == Error::component_not_found("Component with id 99 not found."))
        );
        Ok(())
    }
}
