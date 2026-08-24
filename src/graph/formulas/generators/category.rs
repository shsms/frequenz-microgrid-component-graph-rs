// License: MIT
// Copyright © 2026 Frequenz Energy-as-a-Service GmbH

//! A generic aggregation formula for a single component category.
//!
//! PV inverters, CHPs, EV chargers, wind turbines and steam boilers all produce
//! the same formula shape — the sum of the matching components, each resolved
//! through the meter fallback. They differ only in the category predicate, the
//! noun used in the "wrong category" error, and which `prefer_meters_in_*`
//! override applies. This function captures that shared shape.

use std::collections::BTreeSet;

use crate::graph::formulas::Formula;
use crate::graph::formulas::explain::{Explained, ExplanationKind};
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::{SourcePreference, aggregate};
use crate::{ComponentGraph, Edge, Error, Node};

/// Builds the aggregation formula for the components matching `is_category`.
///
/// When `ids` is `None`, every matching component in the graph is included;
/// otherwise the given ids are used and each is checked to match `is_category`
/// (`category` is the noun phrase for the error, e.g. `"a PV inverter"`).
pub(crate) fn category_formula<N, E>(
    graph: &ComponentGraph<N, E>,
    ids: Option<BTreeSet<u64>>,
    is_category: impl Fn(&N) -> bool,
    category: &str,
    prefer_meters: bool,
) -> Result<Formula, Error>
where
    N: Node,
    E: Edge,
{
    Ok(Formula::new(
        category_formula_explained(graph, ids, is_category, category, prefer_meters)?.expr,
    ))
}

/// Like [`category_formula`], but also explains each formula part.
pub(crate) fn category_formula_explained<N, E>(
    graph: &ComponentGraph<N, E>,
    ids: Option<BTreeSet<u64>>,
    is_category: impl Fn(&N) -> bool,
    category: &str,
    prefer_meters: bool,
) -> Result<Explained, Error>
where
    N: Node,
    E: Edge,
{
    let ids = match ids {
        Some(ids) => ids,
        None => graph.find_all(
            graph.root_id,
            |node| is_category(node),
            petgraph::Direction::Outgoing,
            false,
        )?,
    };

    if ids.is_empty() {
        return Ok(Explained::leaf(
            Expr::number(0.0),
            ExplanationKind::DefaultZero,
            format!("No component that is {category} is included, so the formula is 0.0."),
        ));
    }

    for id in &ids {
        if !is_category(graph.component(*id)?) {
            return Err(Error::invalid_component(format!(
                "Component with id {id} is not {category}."
            )));
        }
    }

    aggregate(graph, ids, SourcePreference::prefer_meters(prefer_meters))
}

/// Per-category wiring tests: each public `*_formula` method passes its own
/// category predicate, prefer-meters override, and error noun. The shared
/// engine behavior is tested in `fallback`.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        ComponentGraphConfig, Error, FormulaOverrides, graph::test_utils::ComponentGraphBuilder,
    };

    #[test]
    fn test_chp_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // No CHPs in the graph.
        let graph = builder.build(None)?;
        assert_eq!(graph.chp_formula(None)?.to_string(), "0.0");

        // A CHP meter (id 2) with two CHPs (ids 3, 4).
        let meter_chp_chain = builder.meter_chp_chain(2);
        builder.connect(grid_meter, meter_chp_chain);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.chp_formula(None)?.to_string(),
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );
        assert_eq!(
            graph.chp_formula(Some(BTreeSet::from([3])))?.to_string(),
            "COALESCE(#3, #2 - #4, 0.0)"
        );

        // The per-category override flips the meter-vs-component preference.
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_chp_formula(true)
                        .build(),
                )
                .build(),
        ))?;
        assert_eq!(
            graph.chp_formula(None)?.to_string(),
            "COALESCE(#2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        // Requested targets must match the category.
        assert_eq!(
            graph
                .chp_formula(Some(BTreeSet::from([1])))
                .unwrap_err()
                .to_string(),
            "InvalidComponent: Component with id 1 is not a CHP."
        );
        Ok(())
    }

    #[test]
    fn test_ev_charger_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // No EV chargers in the graph.
        let graph = builder.build(None)?;
        assert_eq!(graph.ev_charger_formula(None)?.to_string(), "0.0");

        // An EV charger meter (id 2) with two EV chargers (ids 3, 4).
        let meter_ev_charger_chain = builder.meter_ev_charger_chain(2);
        builder.connect(grid_meter, meter_ev_charger_chain);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.ev_charger_formula(None)?.to_string(),
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );
        assert_eq!(
            graph
                .ev_charger_formula(Some(BTreeSet::from([3])))?
                .to_string(),
            "COALESCE(#3, #2 - #4, 0.0)"
        );

        // The per-category override flips the meter-vs-component preference.
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_ev_charger_formula(true)
                        .build(),
                )
                .build(),
        ))?;
        assert_eq!(
            graph.ev_charger_formula(None)?.to_string(),
            "COALESCE(#2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        // Requested targets must match the category.
        assert_eq!(
            graph
                .ev_charger_formula(Some(BTreeSet::from([1])))
                .unwrap_err()
                .to_string(),
            "InvalidComponent: Component with id 1 is not an EV charger."
        );
        Ok(())
    }

    #[test]
    fn test_pv_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // No PV inverters in the graph.
        let graph = builder.build(None)?;
        assert_eq!(graph.pv_formula(None)?.to_string(), "0.0");

        // A PV meter (id 2) with two PV inverters (ids 3, 4).
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );
        assert_eq!(
            graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
            "COALESCE(#3, #2 - #4, 0.0)"
        );

        // The per-category override flips the meter-vs-component preference.
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_pv_formula(true)
                        .build(),
                )
                .build(),
        ))?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        // Requested targets must match the category.
        assert_eq!(
            graph
                .pv_formula(Some(BTreeSet::from([1])))
                .unwrap_err()
                .to_string(),
            "InvalidComponent: Component with id 1 is not a PV inverter."
        );
        Ok(())
    }

    #[test]
    fn test_steam_boiler_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // No steam boilers in the graph.
        let graph = builder.build(None)?;
        assert_eq!(graph.steam_boiler_formula(None)?.to_string(), "0.0");

        // A steam boiler meter (id 2) with two steam boilers (ids 3, 4).
        let meter_steam_boiler_chain = builder.meter_steam_boiler_chain(2);
        builder.connect(grid_meter, meter_steam_boiler_chain);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.steam_boiler_formula(None)?.to_string(),
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );
        assert_eq!(
            graph
                .steam_boiler_formula(Some(BTreeSet::from([3])))?
                .to_string(),
            "COALESCE(#3, #2 - #4, 0.0)"
        );

        // The per-category override flips the meter-vs-component preference.
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_steam_boiler_formula(true)
                        .build(),
                )
                .build(),
        ))?;
        assert_eq!(
            graph.steam_boiler_formula(None)?.to_string(),
            "COALESCE(#2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        // Requested targets must match the category.
        assert_eq!(
            graph
                .steam_boiler_formula(Some(BTreeSet::from([1])))
                .unwrap_err()
                .to_string(),
            "InvalidComponent: Component with id 1 is not a steam boiler."
        );
        Ok(())
    }

    #[test]
    fn test_wind_turbine_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // No wind turbines in the graph.
        let graph = builder.build(None)?;
        assert_eq!(graph.wind_turbine_formula(None)?.to_string(), "0.0");

        // A wind turbine meter (id 2) with two wind turbines (ids 3, 4).
        let meter_wind_turbine_chain = builder.meter_wind_turbine_chain(2);
        builder.connect(grid_meter, meter_wind_turbine_chain);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.wind_turbine_formula(None)?.to_string(),
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );
        assert_eq!(
            graph
                .wind_turbine_formula(Some(BTreeSet::from([3])))?
                .to_string(),
            "COALESCE(#3, #2 - #4, 0.0)"
        );

        // The per-category override flips the meter-vs-component preference.
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_wind_turbine_formula(true)
                        .build(),
                )
                .build(),
        ))?;
        assert_eq!(
            graph.wind_turbine_formula(None)?.to_string(),
            "COALESCE(#2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        // Requested targets must match the category.
        assert_eq!(
            graph
                .wind_turbine_formula(Some(BTreeSet::from([1])))
                .unwrap_err()
                .to_string(),
            "InvalidComponent: Component with id 1 is not a wind turbine."
        );
        Ok(())
    }
}
