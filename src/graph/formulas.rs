// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for building formulas for various microgrid metrics.

use std::collections::BTreeSet;

use crate::ComponentGraph;
use crate::Edge;
use crate::Error;
use crate::Node;

mod expr;
mod fallback;
mod formula;
mod generators;
mod traversal;

pub use formula::AggregationFormula;

/// Formulas for various microgrid metrics.
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Returns the consumer formula for the graph.
    pub fn consumer_formula(&self) -> Result<AggregationFormula, Error> {
        generators::consumer::ConsumerFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the grid formula for the graph.
    pub fn grid_formula(&self) -> Result<AggregationFormula, Error> {
        generators::grid::GridFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the producer formula for the graph.
    pub fn producer_formula(&self) -> Result<AggregationFormula, Error> {
        generators::producer::ProducerFormulaBuilder::try_new(self)?.build()
    }

    /// Returns the battery formula with the given battery IDs.
    ///
    /// If `battery_ids` is `None`, the formula will contain all batteries in
    /// the graph.
    pub fn battery_formula(
        &self,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<AggregationFormula, Error> {
        generators::battery::BatteryFormulaBuilder::try_new(self, battery_ids)?.build()
    }

    /// Returns the CHP formula for the graph.
    pub fn chp_formula(&self, chp_ids: Option<BTreeSet<u64>>) -> Result<AggregationFormula, Error> {
        generators::chp::CHPFormulaBuilder::try_new(self, chp_ids)?.build()
    }

    /// Returns the PV formula for the graph.
    pub fn pv_formula(
        &self,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<AggregationFormula, Error> {
        generators::pv::PVFormulaBuilder::try_new(self, pv_inverter_ids)?.build()
    }

    /// Returns the coalesce formula for the given component IDs.
    ///
    /// This formula uses the `COALESCE` function to return the first non-null
    /// value from the components with the provided IDs.
    pub fn coalesce(&self, component_ids: BTreeSet<u64>) -> Result<AggregationFormula, Error> {
        generators::generic::CoalesceFormulaBuilder::try_new(self, component_ids)?.build()
    }

    /// Returns a string representing the EV charger formula for the graph.
    pub fn ev_charger_formula(
        &self,
        ev_charger_ids: Option<BTreeSet<u64>>,
    ) -> Result<AggregationFormula, Error> {
        generators::ev_charger::EVChargerFormulaBuilder::try_new(self, ev_charger_ids)?.build()
    }

    /// Returns the grid coalesce formula for the graph.
    ///
    /// This formula is used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all meters,
    /// PV inverters, and battery inverters that are directly connected to the
    /// grid.
    pub fn grid_coalesce_formula(&self) -> Result<AggregationFormula, Error> {
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
    pub fn battery_ac_coalesce_formula(
        &self,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<AggregationFormula, Error> {
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
    pub fn pv_ac_coalesce_formula(
        &self,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<AggregationFormula, Error> {
        generators::pv_ac_coalesce::PVAcCoalesceFormulaBuilder::try_new(self, pv_inverter_ids)?
            .build()
    }
}
