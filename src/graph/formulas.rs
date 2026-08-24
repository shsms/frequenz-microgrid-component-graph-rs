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
// their formulas through it — while the outward-facing pieces (the AST
// conversion) are gated out at their own definitions, so dead code still
// surfaces in both configurations.
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
    use crate::{ComponentGraphConfig, ExplanationKind};

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

}
