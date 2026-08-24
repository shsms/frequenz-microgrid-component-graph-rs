// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for building formulas for various microgrid metrics.

use std::collections::BTreeSet;

use crate::ComponentGraph;
use crate::Edge;
use crate::Error;
use crate::Node;
use crate::component_category::CategoryPredicates;

// With `explain` off, the machinery still runs — the plain methods build
// their formulas through it — while the outward-facing pieces (the commented
// renderer, the AST conversion) are gated out at their own definitions, so
// dead code still surfaces in both configurations.
mod explain;
mod expr;
mod fallback;
mod formula;
mod generators;
mod traversal;

use explain::Explained;
#[cfg(feature = "explain")]
pub use explain::{ExplainedFormula, Explanation, ExplanationKind, FormulaAst};
#[cfg(not(feature = "explain"))]
use explain::{ExplainedFormula, ExplanationKind};
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
        Ok(self.component_explained(component_id)?.formula)
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
        Ok(self.component_ac_coalesce_explained(component_id)?.formula)
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

    /// The explained single-component formula; also the plain
    /// [`Self::component_formula`]'s builder.
    fn component_explained(&self, component_id: u64) -> Result<ExplainedFormula, Error> {
        self.component_explained_as(
            component_id,
            "its formula",
            "component",
            "The reading of a single component.",
        )
    }

    /// The explained single-component coalesce formula; also the plain
    /// [`Self::component_ac_coalesce_formula`]'s builder.
    fn component_ac_coalesce_explained(
        &self,
        component_id: u64,
    ) -> Result<ExplainedFormula, Error> {
        self.component_explained_as(
            component_id,
            "this non-aggregating formula",
            "component_ac_coalesce",
            "A non-aggregating metric (like AC voltage or frequency) of a \
             single component.",
        )
    }

    /// One component measured by its own reading, for whichever metric asked.
    /// `formula_name` names the formula in the no-telemetry sentence; `metric`
    /// and `metric_rationale` become the root node's.
    fn component_explained_as(
        &self,
        component_id: u64,
        formula_name: &str,
        metric: &str,
        metric_rationale: &str,
    ) -> Result<ExplainedFormula, Error> {
        let component = self.component(component_id)?;
        let label = explain::capitalized(component.category().label());
        let explained = if !component.provides_telemetry() {
            Explained::silent(
                ExplanationKind::NoTelemetryZero,
                format!(
                    "{label} #{component_id} {} and provides no telemetry. It \
                     has no reading, so {formula_name} is None.",
                    component.operational_mode().describe(),
                ),
                vec![component_id],
            )
        } else {
            Explained::leaf(
                Expr::component(component_id),
                ExplanationKind::DirectReading,
                format!(
                    "{label} #{component_id}'s bare reading. The formula was \
                     asked for this component alone, so a meter reading \
                     covering more than it would answer a different question."
                ),
            )
        };
        Ok(explained.into_formula(metric, metric_rationale))
    }
}

/// Explained variants of the formula methods: the same formulas, plus a tree
/// of reasons for their parts. Only with the `explain` feature.
#[cfg(feature = "explain")]
impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Like [`Self::consumer_formula`], but also explains each formula part.
    pub fn consumer_formula_explained(&self) -> Result<ExplainedFormula, Error> {
        Ok(generators::consumer::ConsumerFormulaBuilder::try_new(self)?
            .build_explained()?
            .into_formula(
                "consumer",
                "The site's consumption: the power drawn by loads, \
                     excluding producers and storage.",
            ))
    }

    /// Like [`Self::grid_formula`], but also explains each formula part.
    pub fn grid_formula_explained(&self) -> Result<ExplainedFormula, Error> {
        Ok(generators::grid::GridFormulaBuilder::try_new(self)?
            .build_explained()?
            .into_formula("grid", "The total power flow at the grid connection point."))
    }

    /// Like [`Self::producer_formula`], but also explains each formula part.
    pub fn producer_formula_explained(&self) -> Result<ExplainedFormula, Error> {
        Ok(generators::producer::ProducerFormulaBuilder::try_new(self)?
            .build_explained()?
            .into_formula(
                "producer",
                "The power the site's producers (PV and CHP) feed in.",
            ))
    }

    /// Like [`Self::battery_formula`], but also explains each formula part.
    pub fn battery_formula_explained(
        &self,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(
            generators::battery::BatteryFormulaBuilder::try_new(self, battery_ids)?
                .build_explained()?
                .into_formula(
                    "battery",
                    "The total battery power. Batteries are DC components \
                     with no AC reading of their own, so they are measured \
                     through their inverters (and battery meters).",
                ),
        )
    }

    /// Like [`Self::chp_formula`], but also explains each formula part.
    pub fn chp_formula_explained(
        &self,
        chp_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(generators::category::category_formula_explained(
            self,
            chp_ids,
            |node| node.is_chp(),
            "a CHP",
            self.config.prefer_meters_in_chp_formula(),
        )?
        .into_formula("chp", "The total power of the CHP plants."))
    }

    /// Like [`Self::pv_formula`], but also explains each formula part.
    pub fn pv_formula_explained(
        &self,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(generators::category::category_formula_explained(
            self,
            pv_inverter_ids,
            |node| node.is_pv_inverter(),
            "a PV inverter",
            self.config.prefer_meters_in_pv_formula(),
        )?
        .into_formula("pv", "The total power of the PV inverters."))
    }

    /// Like [`Self::wind_turbine_formula`], but also explains each formula part.
    pub fn wind_turbine_formula_explained(
        &self,
        wind_turbine_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(generators::category::category_formula_explained(
            self,
            wind_turbine_ids,
            |node| node.is_wind_turbine(),
            "a wind turbine",
            self.config.prefer_meters_in_wind_turbine_formula(),
        )?
        .into_formula("wind_turbine", "The total power of the wind turbines."))
    }

    /// Like [`Self::ev_charger_formula`], but also explains each formula part.
    pub fn ev_charger_formula_explained(
        &self,
        ev_charger_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(generators::category::category_formula_explained(
            self,
            ev_charger_ids,
            |node| node.is_ev_charger(),
            "an EV charger",
            self.config.prefer_meters_in_ev_charger_formula(),
        )?
        .into_formula("ev_charger", "The total power of the EV chargers."))
    }

    /// Like [`Self::steam_boiler_formula`], but also explains each formula part.
    pub fn steam_boiler_formula_explained(
        &self,
        steam_boiler_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(generators::category::category_formula_explained(
            self,
            steam_boiler_ids,
            |node| node.is_steam_boiler(),
            "a steam boiler",
            self.config.prefer_meters_in_steam_boiler_formula(),
        )?
        .into_formula("steam_boiler", "The total power of the steam boilers."))
    }

    /// Like [`Self::component_formula`], but also explains each formula part.
    pub fn component_formula_explained(
        &self,
        component_id: u64,
    ) -> Result<ExplainedFormula, Error> {
        self.component_explained(component_id)
    }

    /// Like [`Self::grid_coalesce_formula`], but also explains each formula
    /// part.
    pub fn grid_coalesce_formula_explained(&self) -> Result<ExplainedFormula, Error> {
        Ok(
            generators::grid_coalesce::GridCoalesceFormulaBuilder::try_new(self)?
                .build_explained()?
                .into_formula(
                    "grid_coalesce",
                    "A non-aggregating metric (like AC voltage or frequency) \
                     at the grid connection point.",
                ),
        )
    }

    /// Like [`Self::battery_ac_coalesce_formula`], but also explains each
    /// formula part.
    pub fn battery_ac_coalesce_formula_explained(
        &self,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(
            generators::battery_ac_coalesce::BatteryAcCoalesceFormulaBuilder::try_new(
                self,
                battery_ids,
            )?
            .build_explained()?
            .into_formula(
                "battery_ac_coalesce",
                "A non-aggregating metric (like AC voltage or frequency) for \
                 the battery group.",
            ),
        )
    }

    /// Like [`Self::pv_ac_coalesce_formula`], but also explains each formula
    /// part.
    pub fn pv_ac_coalesce_formula_explained(
        &self,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<ExplainedFormula, Error> {
        Ok(
            generators::pv_ac_coalesce::PVAcCoalesceFormulaBuilder::try_new(self, pv_inverter_ids)?
                .build_explained()?
                .into_formula(
                    "pv_ac_coalesce",
                    "A non-aggregating metric (like AC voltage or frequency) for \
                 the PV group.",
                ),
        )
    }

    /// Like [`Self::component_ac_coalesce_formula`], but also explains each
    /// formula part.
    pub fn component_ac_coalesce_formula_explained(
        &self,
        component_id: u64,
    ) -> Result<ExplainedFormula, Error> {
        self.component_ac_coalesce_explained(component_id)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "explain")]
    use std::collections::BTreeSet;

    use crate::{
        ComponentCategory, Error, InverterType, OperationalMode,
        graph::test_utils::ComponentGraphBuilder,
    };
    #[cfg(feature = "explain")]
    use crate::{ComponentGraphConfig, ExplainedFormula, ExplanationKind};

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

    /// Builds a site with a bit of everything: a grid meter, battery chains
    /// (incl. a diamond), PV, CHP, EV chargers, a wind turbine, a steam
    /// boiler, a meterless PV inverter, and a no-telemetry inverter.
    #[cfg(feature = "explain")]
    fn rich_builder() -> ComponentGraphBuilder {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, bat_chain);
        let bat_diamond = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, bat_diamond);
        let pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, pv_chain);
        let chp_chain = builder.meter_chp_chain(1);
        builder.connect(grid_meter, chp_chain);
        let ev_chain = builder.meter_ev_charger_chain(2);
        builder.connect(grid_meter, ev_chain);
        let wind_chain = builder.meter_wind_turbine_chain(1);
        builder.connect(grid_meter, wind_chain);
        let boiler_chain = builder.meter_steam_boiler_chain(1);
        builder.connect(grid_meter, boiler_chain);

        // A PV inverter without a meter, directly on the grid meter.
        let bare_pv = builder.solar_inverter();
        builder.connect(grid_meter, bare_pv);

        // A mixed meter: PV inverter + CHP under one meter (subtraction case).
        let mixed_meter = builder.meter();
        let mixed_pv = builder.solar_inverter();
        let mixed_chp = builder.chp();
        builder.connect(grid_meter, mixed_meter);
        builder.connect(mixed_meter, mixed_pv);
        builder.connect(mixed_meter, mixed_chp);

        // A battery inverter that provides no telemetry, behind a meter.
        let silent_meter = builder.meter();
        let silent_inverter = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Battery),
            OperationalMode::ControlOnly,
        );
        let silent_battery = builder.battery();
        builder.connect(grid_meter, silent_meter);
        builder.connect(silent_meter, silent_inverter);
        builder.connect(silent_inverter, silent_battery);

        builder
    }

    /// The configs the invariant test runs under.
    #[cfg(feature = "explain")]
    fn configs() -> Vec<Option<ComponentGraphConfig>> {
        vec![
            None,
            Some(
                ComponentGraphConfig::builder()
                    .prefer_meters_in_component_formulas(true)
                    .build(),
            ),
            Some(
                ComponentGraphConfig::builder()
                    .include_phantom_loads_in_consumer_formula(true)
                    .build(),
            ),
            Some(
                ComponentGraphConfig::builder()
                    .disable_fallback_components(true)
                    .build(),
            ),
        ]
    }

    /// Every `*_formula_explained` returns exactly the formula of its plain
    /// twin. Note: the plain methods delegate to the explained builds, so
    /// this guards the delegation wiring, not the formula strings
    /// themselves — those are asserted by the exact-string tests in
    /// `fallback/tests.rs` and the generator tests.
    #[cfg(feature = "explain")]
    #[test]
    fn test_explained_formulas_match_plain() -> Result<(), Error> {
        let builder = rich_builder();
        for config in configs() {
            let graph = builder.build(config)?;

            let cases: Vec<(String, String)> = vec![
                (
                    graph.grid_formula()?.to_string(),
                    graph.grid_formula_explained()?.formula.to_string(),
                ),
                (
                    graph.consumer_formula()?.to_string(),
                    graph.consumer_formula_explained()?.formula.to_string(),
                ),
                (
                    graph.producer_formula()?.to_string(),
                    graph.producer_formula_explained()?.formula.to_string(),
                ),
                (
                    graph.battery_formula(None)?.to_string(),
                    graph.battery_formula_explained(None)?.formula.to_string(),
                ),
                (
                    graph.pv_formula(None)?.to_string(),
                    graph.pv_formula_explained(None)?.formula.to_string(),
                ),
                (
                    graph.chp_formula(None)?.to_string(),
                    graph.chp_formula_explained(None)?.formula.to_string(),
                ),
                (
                    graph.wind_turbine_formula(None)?.to_string(),
                    graph
                        .wind_turbine_formula_explained(None)?
                        .formula
                        .to_string(),
                ),
                (
                    graph.ev_charger_formula(None)?.to_string(),
                    graph
                        .ev_charger_formula_explained(None)?
                        .formula
                        .to_string(),
                ),
                (
                    graph.steam_boiler_formula(None)?.to_string(),
                    graph
                        .steam_boiler_formula_explained(None)?
                        .formula
                        .to_string(),
                ),
                (
                    graph.component_formula(1)?.to_string(),
                    graph.component_formula_explained(1)?.formula.to_string(),
                ),
                (
                    graph.grid_coalesce_formula()?.to_string(),
                    graph.grid_coalesce_formula_explained()?.formula.to_string(),
                ),
                (
                    graph.battery_ac_coalesce_formula(None)?.to_string(),
                    graph
                        .battery_ac_coalesce_formula_explained(None)?
                        .formula
                        .to_string(),
                ),
                (
                    graph.pv_ac_coalesce_formula(None)?.to_string(),
                    graph
                        .pv_ac_coalesce_formula_explained(None)?
                        .formula
                        .to_string(),
                ),
                (
                    graph.component_ac_coalesce_formula(1)?.to_string(),
                    graph
                        .component_ac_coalesce_formula_explained(1)?
                        .formula
                        .to_string(),
                ),
            ];
            for (plain, explained) in cases {
                assert_eq!(plain, explained);
            }
        }
        Ok(())
    }

    /// Stripping the comments and layout from every commented rendering
    /// recovers the plain formula, for every metric under every config.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formulas_round_trip() -> Result<(), Error> {
        let builder = rich_builder();
        for config in configs() {
            let graph = builder.build(config)?;

            let cases: Vec<ExplainedFormula> = vec![
                graph.grid_formula_explained()?,
                graph.consumer_formula_explained()?,
                graph.producer_formula_explained()?,
                graph.battery_formula_explained(None)?,
                graph.pv_formula_explained(None)?,
                graph.chp_formula_explained(None)?,
                graph.wind_turbine_formula_explained(None)?,
                graph.ev_charger_formula_explained(None)?,
                graph.steam_boiler_formula_explained(None)?,
                graph.component_formula_explained(1)?,
                graph.grid_coalesce_formula_explained()?,
                graph.battery_ac_coalesce_formula_explained(None)?,
                graph.pv_ac_coalesce_formula_explained(None)?,
                graph.component_ac_coalesce_formula_explained(1)?,
                // The no-telemetry inverter: a 0.0 and a None formula.
                graph.component_formula_explained(27)?,
                graph.component_ac_coalesce_formula_explained(27)?,
            ];
            for explained in cases {
                assert_eq!(
                    strip_comments(&explained.to_commented_string()),
                    explained
                        .formula
                        .to_string()
                        .replace(char::is_whitespace, "")
                );
            }
        }
        Ok(())
    }

    /// The explanation root names the metric, covers all the formula's
    /// component ids, and its `rendered` text matches the formula.
    #[cfg(feature = "explain")]
    #[test]
    fn test_explanation_root_covers_formula() -> Result<(), Error> {
        let builder = rich_builder();
        let graph = builder.build(None)?;

        let explained = graph.producer_formula_explained()?;
        assert_eq!(
            explained.explanation.kind,
            ExplanationKind::Metric {
                metric: "producer".to_string()
            }
        );
        assert_eq!(
            explained.explanation.rendered().as_deref(),
            Some(explained.formula.to_string().as_str())
        );

        // Every component id in the formula is listed on the root node.
        for id in explained.formula.expr.component_ids() {
            assert!(
                explained.explanation.component_ids.contains(&id),
                "id {id} missing from root component_ids"
            );
        }
        Ok(())
    }

    /// The producer explanation contains the sign-convention clamp, and each
    /// clamp's own rendering appears in the final formula string.
    #[cfg(feature = "explain")]
    #[test]
    fn test_producer_explanation_has_clamps() -> Result<(), Error> {
        let builder = rich_builder();
        let graph = builder.build(None)?;

        let explained = graph.producer_formula_explained()?;
        let formula = explained.formula.to_string();

        let clamps: Vec<_> = explained
            .explanation
            .nodes()
            .into_iter()
            .filter(|node| node.kind == ExplanationKind::ProducerClamp)
            .collect();
        assert!(!clamps.is_empty());
        for clamp in clamps {
            let rendered = clamp.rendered().unwrap();
            assert!(rendered.starts_with("MIN("));
            assert!(
                formula.contains(&rendered),
                "{rendered:?} not found in {formula:?}"
            );
        }
        Ok(())
    }

    /// A meter drill explains its fallback ladder: the exact child sum, the
    /// meter reading, and the best-effort sum, in preference order.
    #[cfg(feature = "explain")]
    #[test]
    fn test_meter_drill_explanation() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let bat_chain = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, bat_chain);

        let graph = builder.build(None)?;
        let explained = graph.battery_formula_explained(None)?;
        assert_eq!(
            explained.formula.to_string(),
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        // Root is the metric; below it the drill ladder.
        let drill = &explained.explanation.children[0];
        assert_eq!(
            drill.kind,
            ExplanationKind::MeterDrill {
                prefers_meters: false
            }
        );
        assert_eq!(drill.children.len(), 3);
        assert_eq!(drill.children[0].kind, ExplanationKind::ExactSum);
        assert_eq!(drill.children[0].rendered().as_deref(), Some("#4 + #3"));
        assert_eq!(drill.children[1].kind, ExplanationKind::MeterReading);
        assert_eq!(drill.children[1].rendered().as_deref(), Some("#2"));
        assert_eq!(drill.children[2].kind, ExplanationKind::BestEffortSum);
        assert_eq!(
            drill.children[2].rendered().as_deref(),
            Some("COALESCE(#4, 0.0) + COALESCE(#3, 0.0)")
        );
        Ok(())
    }

    /// A partial battery selection explains the meter-minus-siblings
    /// subtraction.
    #[cfg(feature = "explain")]
    #[test]
    fn test_subtraction_explanation() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        builder.connect(grid, meter);
        let inv_bat = builder.inv_bat_chain(1);
        builder.connect(meter, inv_bat);
        let other_inv_bat = builder.inv_bat_chain(1);
        builder.connect(meter, other_inv_bat);

        let graph = builder.build(None)?;
        // Only battery 3 (behind inverter 2): measured as meter 1 minus
        // sibling inverter 4.
        let explained = graph.battery_formula_explained(Some(BTreeSet::from([3])))?;
        assert_eq!(explained.formula.to_string(), "COALESCE(#2, #1 - #4, 0.0)");

        let subtraction = &explained.explanation.children[0];
        assert_eq!(subtraction.kind, ExplanationKind::Subtraction);
        assert!(subtraction.children.iter().any(|child| child.kind
            == (ExplanationKind::MeterDifference {
                meters: vec![1],
                subtracted: vec![4],
            })
            && child.rendered().as_deref() == Some("#1 - #4")));
        Ok(())
    }

    /// A no-telemetry component is explained as having no reading.
    #[cfg(feature = "explain")]
    #[test]
    fn test_no_telemetry_explanation() -> Result<(), Error> {
        let builder = rich_builder();
        let graph = builder.build(None)?;

        // Component 27 is the no-telemetry battery inverter.
        let explained = graph.component_formula_explained(27)?;
        assert_eq!(explained.formula.to_string(), "None");
        assert_eq!(
            explained.explanation.children[0].kind,
            ExplanationKind::NoTelemetryZero
        );
        Ok(())
    }

    /// The no-telemetry explanation covers the requested component: even
    /// though the node emits no expression, a caller can still find the
    /// omission through `component_ids`.
    #[cfg(feature = "explain")]
    #[test]
    fn test_no_telemetry_explanation_covers_component() -> Result<(), Error> {
        let builder = rich_builder();
        let graph = builder.build(None)?;

        // Component 27 is the no-telemetry battery inverter.
        let explained = graph.component_formula_explained(27)?;
        assert_eq!(explained.explanation.children[0].component_ids, [27]);
        assert_eq!(explained.explanation.component_ids, [27]);
        Ok(())
    }

    /// A target dropped for lack of telemetry still gets a silent
    /// explanation node (no `rendered` text), and the formula is
    /// unchanged by it.
    #[cfg(feature = "explain")]
    #[test]
    fn test_dropped_target_gets_silent_node() -> Result<(), Error> {
        let builder = rich_builder();
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .disable_fallback_components(true)
                .build(),
        ))?;

        // Battery inverters: #3 and #6/#7 report; #27 has no telemetry.
        let explained = graph.battery_formula_explained(None)?;
        assert!(!explained.formula.to_string().contains("#27"));
        let silent = explained
            .explanation
            .nodes()
            .into_iter()
            .find(|node| node.component_ids == [27])
            .expect("silent node for #27");
        assert_eq!(silent.kind, ExplanationKind::NoTelemetryZero);
        assert_eq!(silent.rendered(), None);
        Ok(())
    }

    /// In the phantom-loads consumer formula, a no-telemetry meter adds no
    /// term but is still recorded as deliberately left out.
    #[cfg(feature = "explain")]
    #[test]
    fn test_skipped_meter_gets_silent_node() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let silent_meter =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
        builder.connect(grid_meter, silent_meter);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .include_phantom_loads_in_consumer_formula(true)
                .build(),
        ))?;
        let explained = graph.consumer_formula_explained()?;
        let plain = graph.consumer_formula()?.to_string();
        assert_eq!(explained.formula.to_string(), plain);
        assert!(!plain.contains("#2"));

        assert!(
            explained
                .explanation
                .nodes()
                .iter()
                .any(|node| node.kind == ExplanationKind::NoTelemetryZero
                    && node.component_ids == [2]
                    && node.rendered().is_none())
        );
        Ok(())
    }

    /// Removes `//` comment lines and layout whitespace, to recover the flat
    /// formula string.
    #[cfg(feature = "explain")]
    fn strip_comments(commented: &str) -> String {
        commented
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<String>()
            .replace(char::is_whitespace, "")
    }

    /// Asserts the commented rendering still reads back as `plain`: stripping
    /// its comments and layout recovers the flat formula exactly. Both sides
    /// drop whitespace, so the convention lives in one place.
    #[cfg(feature = "explain")]
    #[track_caller]
    fn assert_round_trip(commented: &str, plain: &super::Formula) {
        assert_eq!(
            strip_comments(commented),
            plain.to_string().replace(char::is_whitespace, "")
        );
    }

    /// A battery meter-drill renders as a commented `COALESCE`, with the
    /// best-effort child sum broken open one level deeper.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_expands_meter_drill() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let bat_chain = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, bat_chain);

        let graph = builder.build(None)?;
        let explained = graph.battery_formula_explained(None)?;

        assert_eq!(
            explained.to_commented_string(),
            concat!(
                "// battery: The total battery power. Batteries are DC components with no AC reading of their own, so
",
                "// they are measured through their inverters (and battery meters).
",
                "// Battery meter #2 measures exactly this group (#4, #3). Component readings are preferred, so their
",
                "// exact sum comes first; the meter reading is the fallback.
",
                "COALESCE(
",
                "    // The children's own readings, summed exactly: null unless every child reports, so a missing
",
                "    // reading moves on to the fallback instead of silently undercounting.
",
                "    #4 + #3,
",
                "    // The group's meter measures the same components together, so its reading can stand in when a
",
                "    // child reading is missing.
",
                "    #2,
",
                "    // The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "    // when only part of the group reports.
",
                "    // Each of the 2 child battery inverters adds its reading, with a 0.0 fallback so one offline
",
                "    // device does not null the whole sum.
",
                "    COALESCE(#4, 0.0) +
",
                "    COALESCE(#3, 0.0)
",
                ")",
            )
        );
        // Stripping the comments and layout recovers the plain formula.
        assert_eq!(
            strip_comments(&explained.to_commented_string()),
            graph
                .battery_formula(None)?
                .to_string()
                .replace(char::is_whitespace, "")
        );
        Ok(())
    }

    /// The consumer clamp is broken open too: `MAX(` over the grid-minus-groups
    /// subtraction, whose drilled groups nest a further call level.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_expands_consumer_clamp() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, pv_chain);

        let graph = builder.build(None)?;
        let explained = graph.consumer_formula_explained()?;
        let commented = explained.to_commented_string();

        assert_eq!(
            commented,
            concat!(
                "// consumer: The site's consumption: the power drawn by loads, excluding producers and storage.
",
                "// Consumption cannot be negative. MAX(_, 0.0) discards any surplus production the site feeds into
",
                "// the grid.
",
                "MAX(
",
                "    // The grid total minus the non-consumer groups (producers and storage): what remains is the
",
                "    // site's consumption.
",
                "    // Grid meter #1 is measured by its bare reading. Its own reading is the only source here, and
",
                "    // it also carries the site's unmodeled consumer load, which no sum of its children would
",
                "    // account for, so nothing can back it.
",
                "    #1 -
",
                "    // PV meter #2 measures exactly this group (#4, #3). Meters are preferred, so its reading comes
",
                "    // first; the sum of its children is the fallback.
",
                "    COALESCE(
",
                "        // The meter's own reading is the primary source: it measures all of its children together.
",
                "        #2,
",
                "        // The best-effort sum of the meter's usable children: each reading or 0.0, so it still
",
                "        // resolves when only part of the group reports.
",
                "        // Each of the 2 child PV inverters adds its reading, with a 0.0 fallback so one offline
",
                "        // device does not null the whole sum.
",
                "        COALESCE(#4, 0.0) +
",
                "        COALESCE(#3, 0.0)
",
                "    ),
",
                "    0.0
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.consumer_formula()?);
        // Two call levels indent: `MAX(` then `COALESCE(`. The best-effort
        // sum inside adds no indent of its own (its `+` terms sit at the
        // COALESCE argument level), so the deepest indentation is two steps.
        let max_indent = commented
            .lines()
            .map(|line| line.len() - line.trim_start().len())
            .max()
            .unwrap();
        assert_eq!(max_indent, 2 * 4);
        Ok(())
    }

    /// A component dropped for lack of telemetry keeps its reason on a
    /// comment-only line, even where the surrounding term stays inline.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_notes_dropped_component() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let live = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, live);
        let silent_meter = builder.meter();
        let silent_inverter = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Battery),
            OperationalMode::ControlOnly,
        );
        let silent_battery = builder.battery();
        builder.connect(grid_meter, silent_meter);
        builder.connect(silent_meter, silent_inverter);
        builder.connect(silent_inverter, silent_battery);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .disable_fallback_components(true)
                .build(),
        ))?;
        let explained = graph.battery_formula_explained(None)?;

        assert_eq!(
            explained.to_commented_string(),
            concat!(
                "// battery: The total battery power. Batteries are DC components with no AC reading of their own, so
",
                "// they are measured through their inverters (and battery meters).
",
                "// One term per measurement point. The points do not overlap, so summing them counts every component
",
                "// exactly once.
",
                "// Fallbacks are disabled by config, so battery inverter #3 is measured by its bare reading only.
",
                "#3
",
                "// Battery inverter #6 is in control-only mode and provides no telemetry. It has no reading to emit,
",
                "// so it is dropped.",
            )
        );
        Ok(())
    }

    /// A term that is itself a sum (a no-telemetry meter measured through its
    /// children) is flattened into the grid formula's sum. The term's reasons
    /// stay attached to the run of operands it became.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_keeps_reasons_across_flattened_sums() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let silent_meter =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
        builder.connect(grid, silent_meter);
        for _ in 0..2 {
            let inverter = builder.battery_inverter();
            let battery = builder.battery();
            builder.connect(silent_meter, inverter);
            builder.connect(inverter, battery);
        }
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid, pv_inverter);

        let graph = builder.build(None)?;
        let explained = graph.grid_formula_explained()?;
        let commented = explained.to_commented_string();

        assert_eq!(
            commented,
            concat!(
                "// grid: The total power flow at the grid connection point.
",
                "// The sum of everything connected to the grid connection point, one term per connection.
",
                "// Battery meter #1 is in control-only mode and provides no telemetry, so its own reading must never
",
                "// appear. It is measured by the sum of its children instead.
",
                "// The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "// when only part of the group reports.
",
                "// Each of the 2 child battery inverters adds its reading, with a 0.0 fallback so one offline device
",
                "// does not null the whole sum.
",
                "COALESCE(#4, 0.0) +
",
                "COALESCE(#2, 0.0) +
",
                "// PV inverter #6 is measured by its own reading. The 0.0 fallback keeps the term total when the
",
                "// reading is missing.
",
                "COALESCE(#6, 0.0)",
            )
        );
        assert_round_trip(&commented, &graph.grid_formula()?);
        // Every term here carries its own 0.0, so this sum really is
        // best-effort and keeps that kind. The kind splits on the guarantee,
        // not on how the sum was built.
        assert!(
            explained
                .explanation
                .nodes()
                .iter()
                .any(|node| node.kind == ExplanationKind::BestEffortSum),
        );
        Ok(())
    }

    /// A term's reason describes the shape it lands in. Children of
    /// different categories do not fold into a run, so each prints its own
    /// singular reason — and with siblings to be summed with, that reason
    /// may talk about the sum and the group. The counterpart is the
    /// merged-coalesce test, where a lone child says only "the term".
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_sums_unfoldable_children_each_with_its_reason() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let silent =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
        builder.connect(grid, silent);
        let pv = builder.solar_inverter();
        let chp = builder.chp();
        builder.connect(silent, pv);
        builder.connect(silent, chp);

        let graph = builder.build(None)?;
        let explained = graph.grid_formula_explained()?;
        assert_eq!(
            explained.formula.to_string(),
            "COALESCE(#3, 0.0) + COALESCE(#2, 0.0)"
        );
        let commented = explained.to_commented_string();
        for reason in [
            "// Child CHP #3's reading. The 0.0 fallback keeps the sum total when the device is offline, so one\n// silent child does not null the whole group.",
            "// Child PV inverter #2's reading. The 0.0 fallback keeps the sum total when the device is offline,\n// so one silent child does not null the whole group.",
        ] {
            assert!(
                commented.contains(reason),
                "missing:\n{reason}\nin:\n{commented}"
            );
        }
        assert_round_trip(&commented, &graph.grid_formula()?);
        Ok(())
    }

    /// A grid meter chain coalesces to the child grid meter's bare reading:
    /// `COALESCE(#1, #2)`, no 0.0 anywhere, so no comment may claim one. The
    /// sole child is also measured on its own — a sum node above it would
    /// repeat it exactly — so the drill names it as the fallback directly.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_grid_meter_chain_claims_no_zero_fallback() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter1 = builder.meter();
        let meter2 = builder.meter();
        let bat_chain = builder.meter_bat_chain(1, 1);
        let pv_chain = builder.meter_pv_chain(1);
        builder.connect(grid, meter1);
        builder.connect(meter1, meter2);
        builder.connect(meter2, bat_chain);
        builder.connect(meter2, pv_chain);

        let graph = builder.build(None)?;
        let explained = graph.grid_formula_explained()?;
        assert_eq!(explained.formula.to_string(), "COALESCE(#1, #2)");
        let commented = explained.to_commented_string();

        assert_eq!(
            commented,
            concat!(
                "// grid: The total power flow at the grid connection point.
",
                "// Grid meter #1 measures exactly this group (#2). Meters are preferred, so its reading comes first;
",
                "// its child is the fallback.
",
                "COALESCE(
",
                "    // The meter's own reading is the primary source: it measures all of its children together.
",
                "    #1,
",
                "    // Grid meter #2 is measured by its bare reading. Its own reading is the only source here, and
",
                "    // it also carries the site's unmodeled consumer load, which no sum of its children would
",
                "    // account for, so nothing can back it.
",
                "    #2
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.grid_formula()?);
        Ok(())
    }

    /// A meter with a single reporting child (meters first) merges the
    /// child's `COALESCE` into the drill's own: `COALESCE(#1, #2, 0.0)`. The
    /// merged part's reasons keep their place above the operands it became.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_keeps_reasons_across_merged_coalesce() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid, bat_chain);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .prefer_meters_in_component_formulas(true)
                .build(),
        ))?;
        let explained = graph.battery_formula_explained(None)?;
        let commented = explained.to_commented_string();

        assert_eq!(
            commented,
            concat!(
                "// battery: The total battery power. Batteries are DC components with no AC reading of their own, so
",
                "// they are measured through their inverters (and battery meters).
",
                "// Battery meter #1 measures exactly this group (#2). Meters are preferred by config, so its reading
",
                "// comes first; its child is the fallback.
",
                "COALESCE(
",
                "    // The meter's own reading is the primary source: it measures all of its children together.
",
                "    #1,
",
                "    // Child battery inverter #2's reading. The 0.0 fallback keeps the term total when the device is
",
                "    // offline.
",
                "    #2,
",
                "    0.0
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.battery_formula(None)?);
        Ok(())
    }

    /// A subtracted operand that is a sum keeps its brackets, spread over its
    /// own lines, so the reasons inside it are kept too.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_brackets_subtracted_sums() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let silent_meter =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
        builder.connect(grid_meter, silent_meter);
        for _ in 0..2 {
            let inverter = builder.battery_inverter();
            let battery = builder.battery();
            builder.connect(silent_meter, inverter);
            builder.connect(inverter, battery);
        }

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .include_phantom_loads_in_consumer_formula(true)
                .build(),
        ))?;
        let explained = graph.consumer_formula_explained()?;
        let commented = explained.to_commented_string();

        assert_eq!(
            commented,
            concat!(
                "// consumer: The site's consumption: the power drawn by loads, excluding producers and storage.
",
                "// Phantom loads are included (by config): each meter contributes its residual (reading minus
",
                "// modeled successors), plus the consumers connected directly to the grid.
",
                "// Consumption cannot be negative. MAX(_, 0.0) discards any production measured on the same lines.
",
                "MAX(
",
                "    // Meter #1's reading minus its modeled successors: the load connected to the meter that is not
",
                "    // in the graph — its phantom load.
",
                "    #1 -
",
                "    (
",
                "        // Successor battery meter #2 is modeled in the graph, so its measurement is subtracted from
",
                "        // the residual.
",
                "        // Battery meter #2 is in control-only mode and provides no telemetry, so its own reading
",
                "        // must never appear. It is measured by the sum of its children instead.
",
                "        // The best-effort sum of the meter's usable children: each reading or 0.0, so it still
",
                "        // resolves when only part of the group reports.
",
                "        // Each of the 2 child battery inverters adds its reading, with a 0.0 fallback so one
",
                "        // offline device does not null the whole sum.
",
                "        COALESCE(#5, 0.0) +
",
                "        COALESCE(#3, 0.0)
",
                "    ),
",
                "    0.0
",
                ")
",
                "// Battery meter #2 is in control-only mode and provides no telemetry. Its phantom load cannot be
",
                "// calculated, so it adds no term.",
            )
        );
        assert_round_trip(&commented, &graph.consumer_formula()?);
        Ok(())
    }

    /// A run of same-shaped producer terms folds under one comment: the
    /// shared clamp reason prints once verbatim, the per-component reason
    /// once in its plural form, and each term stays on one line. The CHP
    /// term has a different shape (its own category), so it is not part of
    /// the PV run and keeps its own reasons.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_folds_same_shaped_runs() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        for _ in 0..4 {
            let pv_inverter = builder.solar_inverter();
            builder.connect(grid_meter, pv_inverter);
        }
        let chp = builder.chp();
        builder.connect(grid_meter, chp);

        let graph = builder.build(None)?;
        let explained = graph.producer_formula_explained()?;
        let commented = explained.to_commented_string();

        assert_eq!(
            commented,
            concat!(
                "// producer: The power the site's producers (PV and CHP) feed in.
",
                "// The total production: the sum of the site's producer measurement points (1 CHP, 4 PV inverters).
",
                "// Producers feed power in, which is negative by the passive sign convention. MIN(_, 0.0) discards
",
                "// any consumption measured on the same lines, so only production is counted.
",
                "// Each of the 4 PV inverters is measured by its own reading; the 0.0 fallback keeps each term total
",
                "// when its reading is missing.
",
                "MIN(COALESCE(#2, 0.0), 0.0) +
",
                "MIN(COALESCE(#3, 0.0), 0.0) +
",
                "MIN(COALESCE(#4, 0.0), 0.0) +
",
                "MIN(COALESCE(#5, 0.0), 0.0) +
",
                "MIN(
",
                "    // CHP #6 is measured by its own reading. The 0.0 fallback keeps the term total when the reading
",
                "    // is missing.
",
                "    COALESCE(#6, 0.0),
",
                "    0.0
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.producer_formula()?);
        Ok(())
    }

    /// A None-valued formula still renders its body and its reason: the
    /// comments say why there is no source, and stripping them recovers the
    /// plain `None` string.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_renders_none() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let silent_inverter = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Battery),
            OperationalMode::ControlOnly,
        );
        let battery = builder.battery();
        builder.connect(grid, silent_inverter);
        builder.connect(silent_inverter, battery);

        let graph = builder.build(None)?;
        let explained = graph.component_ac_coalesce_formula_explained(1)?;
        assert_eq!(explained.to_string(), "None");
        assert_eq!(
            explained.to_commented_string(),
            concat!(
                "// component_ac_coalesce: A non-aggregating metric (like AC voltage or frequency) of a single
",
                "// component.
",
                "// Battery inverter #1 is in control-only mode and provides no telemetry. It has no reading, so this
",
                "// non-aggregating formula is None.
",
                "None",
            )
        );
        assert_eq!(strip_comments(&explained.to_commented_string()), "None");
        Ok(())
    }

    /// Two equal runs split by a different term each keep their own group
    /// comment: the second run's members are not left uncommented, and each
    /// count stays right.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_repeats_group_comment_for_second_run() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        for _ in 0..2 {
            let pv_inverter = builder.solar_inverter();
            builder.connect(grid_meter, pv_inverter);
        }
        let chp = builder.chp();
        builder.connect(grid_meter, chp);
        for _ in 0..2 {
            let pv_inverter = builder.solar_inverter();
            builder.connect(grid_meter, pv_inverter);
        }

        let graph = builder.build(None)?;
        let commented = graph.producer_formula_explained()?.to_commented_string();
        assert_eq!(
            commented,
            concat!(
                "// producer: The power the site's producers (PV and CHP) feed in.
",
                "// The total production: the sum of the site's producer measurement points (1 CHP, 4 PV inverters).
",
                "// Producers feed power in, which is negative by the passive sign convention. MIN(_, 0.0) discards
",
                "// any consumption measured on the same lines, so only production is counted.
",
                "// Each of the 2 PV inverters is measured by its own reading; the 0.0 fallback keeps each term total
",
                "// when its reading is missing.
",
                "MIN(COALESCE(#2, 0.0), 0.0) +
",
                "MIN(COALESCE(#3, 0.0), 0.0) +
",
                "MIN(
",
                "    // CHP #4 is measured by its own reading. The 0.0 fallback keeps the term total when the reading
",
                "    // is missing.
",
                "    COALESCE(#4, 0.0),
",
                "    0.0
",
                ") +
",
                "// Producers feed power in, which is negative by the passive sign convention. MIN(_, 0.0) discards
",
                "// any consumption measured on the same lines, so only production is counted.
",
                "// Each of the 2 PV inverters is measured by its own reading; the 0.0 fallback keeps each term total
",
                "// when its reading is missing.
",
                "MIN(COALESCE(#5, 0.0), 0.0) +
",
                "MIN(COALESCE(#6, 0.0), 0.0)",
            )
        );
        assert_round_trip(&commented, &graph.producer_formula()?);
        Ok(())
    }

    /// A run of same-shaped meter groups folds expanded: the shared ladder
    /// prose prints once above the run, each rung's id-free reason printed
    /// verbatim, and every group keeps a one-line identity comment over its
    /// fully laid-out term — even though the groups' child counts differ.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_folds_meter_groups_expanded() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        for count in [3, 2] {
            let chain = builder.meter_pv_chain(count);
            builder.connect(grid_meter, chain);
        }

        let graph = builder.build(None)?;
        let commented = graph.pv_formula_explained(None)?.to_commented_string();
        assert_eq!(
            commented,
            concat!(
                "// pv: The total power of the PV inverters.
",
                "// One term per measurement point. The points do not overlap, so summing them counts every component
",
                "// exactly once.
",
                "// Each of the 2 PV meter groups below is measured the same way, from the same sources in this
",
                "// order; each term's own comment lists its group.
",
                "// The children's own readings, summed exactly: null unless every child reports, so a missing
",
                "// reading moves on to the fallback instead of silently undercounting.
",
                "// The group's meter measures the same components together, so its reading can stand in when a child
",
                "// reading is missing.
",
                "// The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "// when only part of the group reports.
",
                "// Each of the 5 child PV inverters adds its reading, with a 0.0 fallback so one offline device does
",
                "// not null the whole sum.
",
                "// PV meter #2 measures exactly this group (#5, #4, #3).
",
                "COALESCE(
",
                "    #5 + #4 + #3,
",
                "    #2,
",
                "    COALESCE(#5, 0.0) +
",
                "    COALESCE(#4, 0.0) +
",
                "    COALESCE(#3, 0.0)
",
                ") +
",
                "// PV meter #6 measures exactly this group (#8, #7).
",
                "COALESCE(
",
                "    #8 + #7,
",
                "    #6,
",
                "    COALESCE(#8, 0.0) +
",
                "    COALESCE(#7, 0.0)
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.pv_formula(None)?);
        Ok(())
    }

    /// Meter groups fold expanded with meters preferred too: the shared
    /// reading-then-children ladder prints once, one identity line per group.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_folds_meter_groups_expanded_meters_first() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        for count in [3, 2] {
            let chain = builder.meter_pv_chain(count);
            builder.connect(grid_meter, chain);
        }

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .prefer_meters_in_component_formulas(true)
                .build(),
        ))?;
        let commented = graph.pv_formula_explained(None)?.to_commented_string();
        assert_eq!(
            commented,
            concat!(
                "// pv: The total power of the PV inverters.
",
                "// One term per measurement point. The points do not overlap, so summing them counts every component
",
                "// exactly once.
",
                "// Each of the 2 PV meter groups below is measured the same way, from the same sources in this
",
                "// order; each term's own comment lists its group.
",
                "// The meter's own reading is the primary source: it measures all of its children together.
",
                "// The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "// when only part of the group reports.
",
                "// Each of the 5 child PV inverters adds its reading, with a 0.0 fallback so one offline device does
",
                "// not null the whole sum.
",
                "// PV meter #2 measures exactly this group (#5, #4, #3).
",
                "COALESCE(
",
                "    #2,
",
                "    COALESCE(#5, 0.0) +
",
                "    COALESCE(#4, 0.0) +
",
                "    COALESCE(#3, 0.0)
",
                ") +
",
                "// PV meter #6 measures exactly this group (#8, #7).
",
                "COALESCE(
",
                "    #6,
",
                "    COALESCE(#8, 0.0) +
",
                "    COALESCE(#7, 0.0)
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.pv_formula(None)?);
        Ok(())
    }

    /// A term that cannot fold (a stands-alone meter with an inactive child,
    /// which keeps its silent note) splits the surrounding meter groups into
    /// two runs: each run repeats the shared prose for its own members, and
    /// the odd term out keeps its full reasons.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_expanded_run_splits_around_odd_term() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        for count in [3, 2] {
            let chain = builder.meter_pv_chain(count);
            builder.connect(grid_meter, chain);
        }
        let standing = builder.meter_pv_chain(2);
        builder.connect(grid_meter, standing);
        let inactive = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Pv),
            OperationalMode::Inactive,
        );
        builder.connect(standing, inactive);
        for count in [2, 2] {
            let chain = builder.meter_pv_chain(count);
            builder.connect(grid_meter, chain);
        }

        let graph = builder.build(None)?;
        let commented = graph.pv_formula_explained(None)?.to_commented_string();
        assert_eq!(
            commented,
            concat!(
                "// pv: The total power of the PV inverters.
",
                "// One term per measurement point. The points do not overlap, so summing them counts every component
",
                "// exactly once.
",
                "// Each of the 2 PV meter groups below is measured the same way, from the same sources in this
",
                "// order; each term's own comment lists its group.
",
                "// The children's own readings, summed exactly: null unless every child reports, so a missing
",
                "// reading moves on to the fallback instead of silently undercounting.
",
                "// The group's meter measures the same components together, so its reading can stand in when a child
",
                "// reading is missing.
",
                "// The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "// when only part of the group reports.
",
                "// Each of the 5 child PV inverters adds its reading, with a 0.0 fallback so one offline device does
",
                "// not null the whole sum.
",
                "// PV meter #2 measures exactly this group (#5, #4, #3).
",
                "COALESCE(
",
                "    #5 + #4 + #3,
",
                "    #2,
",
                "    COALESCE(#5, 0.0) +
",
                "    COALESCE(#4, 0.0) +
",
                "    COALESCE(#3, 0.0)
",
                ") +
",
                "// PV meter #6 measures exactly this group (#8, #7).
",
                "COALESCE(
",
                "    #8 + #7,
",
                "    #6,
",
                "    COALESCE(#8, 0.0) +
",
                "    COALESCE(#7, 0.0)
",
                ") +
",
                "// PV meter #9 is measured by its own reading. If the reading goes missing, the best-effort sum of
",
                "// its usable children keeps the term total.
",
                "COALESCE(
",
                "    // The meter's own reading is the primary source: it is the only source that covers its whole
",
                "    // group.
",
                "    #9,
",
                "    // The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "    // when only part of the group reports.
",
                "    // Each of the 2 child PV inverters adds its reading, with a 0.0 fallback so one offline device
",
                "    // does not null the whole sum.
",
                "    COALESCE(#11, 0.0) +
",
                "    COALESCE(#10, 0.0)
",
                "    // Child PV inverter #12 is inactive and provides no telemetry: it has no reading to add, so the
",
                "    // child sums leave it out. The meter's own reading still covers its flow.
",
                ") +
",
                "// Each of the 2 PV meter groups below is measured the same way, from the same sources in this
",
                "// order; each term's own comment lists its group.
",
                "// The children's own readings, summed exactly: null unless every child reports, so a missing
",
                "// reading moves on to the fallback instead of silently undercounting.
",
                "// The group's meter measures the same components together, so its reading can stand in when a child
",
                "// reading is missing.
",
                "// The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "// when only part of the group reports.
",
                "// Each of the 4 child PV inverters adds its reading, with a 0.0 fallback so one offline device does
",
                "// not null the whole sum.
",
                "// PV meter #13 measures exactly this group (#15, #14).
",
                "COALESCE(
",
                "    #15 + #14,
",
                "    #13,
",
                "    COALESCE(#15, 0.0) +
",
                "    COALESCE(#14, 0.0)
",
                ") +
",
                "// PV meter #16 measures exactly this group (#18, #17).
",
                "COALESCE(
",
                "    #18 + #17,
",
                "    #16,
",
                "    COALESCE(#18, 0.0) +
",
                "    COALESCE(#17, 0.0)
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.pv_formula(None)?);
        Ok(())
    }

    /// Meter groups fold expanded inside a subtraction layout too: the
    /// shared prose prints once, each subtracted group keeps its identity
    /// line, and the joining `-` lands on each group's closing bracket.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_folds_meter_groups_expanded_in_subtraction() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        for count in [3, 2] {
            let chain = builder.meter_pv_chain(count);
            builder.connect(grid_meter, chain);
        }

        let graph = builder.build(None)?;
        let commented = graph.consumer_formula_explained()?.to_commented_string();
        assert_eq!(
            commented,
            concat!(
                "// consumer: The site's consumption: the power drawn by loads, excluding producers and storage.
",
                "// Consumption cannot be negative. MAX(_, 0.0) discards any surplus production the site feeds into
",
                "// the grid.
",
                "MAX(
",
                "    // The grid total minus the non-consumer groups (producers and storage): what remains is the
",
                "    // site's consumption.
",
                "    // Grid meter #1 is measured by its bare reading. Its own reading is the only source here, and
",
                "    // it also carries the site's unmodeled consumer load, which no sum of its children would
",
                "    // account for, so nothing can back it.
",
                "    #1 -
",
                "    // Each of the 2 PV meter groups below is measured the same way, from the same sources in this
",
                "    // order; each term's own comment lists its group.
",
                "    // The meter's own reading is the primary source: it measures all of its children together.
",
                "    // The best-effort sum of the meter's usable children: each reading or 0.0, so it still resolves
",
                "    // when only part of the group reports.
",
                "    // Each of the 5 child PV inverters adds its reading, with a 0.0 fallback so one offline device
",
                "    // does not null the whole sum.
",
                "    // PV meter #2 measures exactly this group (#5, #4, #3).
",
                "    COALESCE(
",
                "        #2,
",
                "        COALESCE(#5, 0.0) +
",
                "        COALESCE(#4, 0.0) +
",
                "        COALESCE(#3, 0.0)
",
                "    ) -
",
                "    // PV meter #6 measures exactly this group (#8, #7).
",
                "    COALESCE(
",
                "        #6,
",
                "        COALESCE(#8, 0.0) +
",
                "        COALESCE(#7, 0.0)
",
                "    ),
",
                "    0.0
",
                ")",
            )
        );
        assert_round_trip(&commented, &graph.consumer_formula()?);
        Ok(())
    }

    /// A one-line term's silent notes join the comment block above the line:
    /// below it, they would read as the next operand's reason.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_notes_precede_one_line_term() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, bat_chain);
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, pv_inverter);
        let inactive = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Pv),
            OperationalMode::Inactive,
        );
        builder.connect(grid_meter, inactive);

        let graph = builder.build(None)?;
        let commented = graph.grid_formula_explained()?.to_commented_string();
        assert_eq!(
            commented,
            concat!(
                "// grid: The total power flow at the grid connection point.
",
                "// Grid meter #1 is measured by its bare reading. Its own reading is the only source here, and it
",
                "// also carries the site's unmodeled consumer load, which no sum of its children would account for,
",
                "// so nothing can back it.
",
                "// Child PV inverter #6 is inactive and provides no telemetry: it has no reading to add, so the
",
                "// child sums leave it out. The meter's own reading still covers its flow.
",
                "#1",
            )
        );
        assert_round_trip(&commented, &graph.grid_formula()?);
        Ok(())
    }

    /// `Display` (and `to_string`) give the plain formula, without comments.
    #[cfg(feature = "explain")]
    #[test]
    fn test_commented_formula_display_is_plain() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let bat_chain = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, bat_chain);

        let graph = builder.build(None)?;
        let explained = graph.battery_formula_explained(None)?;

        assert_eq!(
            explained.to_string(),
            graph.battery_formula(None)?.to_string()
        );
        assert!(!explained.to_string().contains("//"));
        Ok(())
    }
}
