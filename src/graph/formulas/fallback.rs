// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Fallback expression generator for components and meters.

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};
use std::collections::BTreeSet;

use super::expr::Expr;

#[derive(Default)]
pub(crate) struct FallbackExpr {
    prefer_meters: bool,
    meter_fallback_for_meters: bool,
}

impl FallbackExpr {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prefer_meters(mut self, prefer: bool) -> Self {
        self.prefer_meters = prefer;
        self
    }

    pub fn meter_fallback_for_meters(mut self, enable: bool) -> Self {
        self.meter_fallback_for_meters = enable;
        self
    }
}

impl FallbackExpr {
    pub(crate) fn generate<N: Node, E: Edge>(
        &self,
        graph: &ComponentGraph<N, E>,
        mut component_ids: BTreeSet<u64>,
    ) -> Result<Expr, Error> {
        let mut formula = None::<Expr>;
        if graph.config.disable_fallback_components {
            while let Some(component_id) = component_ids.pop_first() {
                formula = Self::add_to_option(formula, Expr::component(component_id));
            }
            return formula.ok_or(Error::internal("No components to generate formula."));
        }
        while let Some(component_id) = component_ids.pop_first() {
            if let Some(expr) = self.meter_fallback(graph, component_id)? {
                formula = Self::add_to_option(formula, expr);
            } else if let Some(expr) =
                self.component_fallback(graph, &mut component_ids, component_id)?
            {
                formula = Self::add_to_option(formula, expr);
            } else {
                formula = Self::add_to_option(formula, Expr::component(component_id));
            }
        }

        formula.ok_or(Error::internal("Search for fallback components failed."))
    }

    /// Returns a fallback expression for a meter component.
    fn meter_fallback<N: Node, E: Edge>(
        &self,
        graph: &ComponentGraph<N, E>,
        component_id: u64,
    ) -> Result<Option<Expr>, Error> {
        let component = graph.component(component_id)?;
        if !component.is_meter() {
            return Ok(None);
        }
        let has_successor_meters = graph.has_meter_successors(component_id)?;

        if !self.meter_fallback_for_meters && has_successor_meters {
            return Ok(Some(Expr::component(component_id)));
        }

        if !graph.has_successors(component_id)? {
            return Ok(Some(Expr::component(component_id)));
        }

        let (sum_of_successors, sum_of_coalesced_successors) = graph
            .successors(component_id)?
            .map(|node| {
                (
                    Expr::from(node),
                    Expr::coalesce(Expr::from(node), Expr::number(0.0)),
                )
            })
            .reduce(|a, b| (a.0 + b.0, a.1 + b.1))
            .ok_or(Error::internal(
                "Can't find successors of components with successors.",
            ))?;

        let has_multiple_successors = matches!(sum_of_successors, Expr::Add { .. });

        // If a meter has exactly one successor and it is a meter, we consider
        // it to be a fallback meter.  If there are multiple meter successors,
        // we return the meter without fallback.
        if has_successor_meters && has_multiple_successors {
            return Ok(Some(Expr::component(component_id)));
        }

        // If a meter fallback for a meter exists, make sure it is not a component meter.
        if self.meter_fallback_for_meters && has_successor_meters {
            // we've already established that if there are successor meters,
            // then there's only one successor.
            let successor = graph
                .successors(component_id)?
                .find(|node| node.is_meter())
                .ok_or(Error::internal(
                    "Can't find successor meter of component with successor meters.",
                ))?;
            if graph.is_component_meter(successor.component_id())? {
                return Ok(Some(Expr::component(component_id)));
            }
        }

        let mut coalesced = Expr::component(component_id);

        if !self.prefer_meters {
            coalesced = sum_of_successors.clone().coalesce(coalesced);
        }

        if self.prefer_meters {
            if has_multiple_successors {
                coalesced = coalesced.coalesce(sum_of_coalesced_successors);
            } else {
                coalesced = coalesced.coalesce(sum_of_successors);
                if !has_successor_meters {
                    coalesced = coalesced.coalesce(Expr::number(0.0));
                }
            }
        } else if has_multiple_successors {
            coalesced = coalesced.coalesce(sum_of_coalesced_successors);
        } else if !has_successor_meters {
            coalesced = coalesced.coalesce(Expr::number(0.0));
        }

        Ok(Some(coalesced))
    }

    /// Returns a fallback expression for components with the following categories:
    ///
    /// - CHP
    /// - Battery Inverter
    /// - PV Inverter
    /// - EV Charger
    /// - Wind Turbine
    /// - Steam Boiler
    fn component_fallback<N: Node, E: Edge>(
        &self,
        graph: &ComponentGraph<N, E>,
        component_ids: &mut BTreeSet<u64>,
        component_id: u64,
    ) -> Result<Option<Expr>, Error> {
        let component = graph.component(component_id)?;
        if !component.is_battery_inverter(&graph.config)
            && !component.is_chp()
            && !component.is_pv_inverter()
            && !component.is_ev_charger()
            && !component.is_wind_turbine()
            && !component.is_steam_boiler()
        {
            return Ok(None);
        }

        // If predecessors have other successors that are not in the list of
        // component ids, the predecessors can't be used as fallback.
        let siblings = graph
            .siblings_from_predecessors(component_id)?
            .filter(|sibling| sibling.component_id() != component_id)
            .collect::<Vec<_>>();
        if !siblings
            .iter()
            .all(|sibling| component_ids.contains(&sibling.component_id()))
        {
            return Ok(Some(Expr::coalesce(
                Expr::component(component_id),
                Expr::number(0.0),
            )));
        }

        // Collect predecessor meter ids.
        let predecessor_ids: BTreeSet<u64> = graph
            .predecessors(component_id)?
            .filter(|x| x.is_meter())
            .map(|x| x.component_id())
            .collect();

        if predecessor_ids.is_empty() {
            return Ok(Some(Expr::coalesce(
                Expr::component(component_id),
                Expr::number(0.0),
            )));
        }

        for sibling in siblings {
            component_ids.remove(&sibling.component_id());
        }

        Ok(Some(self.generate(graph, predecessor_ids)?))
    }

    fn add_to_option(expr: Option<Expr>, other: Expr) -> Option<Expr> {
        if let Some(expr) = expr {
            Some(expr + other)
        } else {
            Some(other)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        ComponentGraphConfig, Error,
        graph::{formulas::fallback::FallbackExpr, test_utils::ComponentGraphBuilder},
    };

    #[test]
    fn test_meter_fallback() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // Add a battery meter with one inverter and one battery.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build(None)?;
        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .meter_fallback_for_meters(true)
            .generate(&graph, BTreeSet::from([1]))?;
        assert_eq!(expr.to_string(), "#1");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .meter_fallback_for_meters(true)
            .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

        let expr = FallbackExpr::new().generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#3, #2, 0.0)");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([3]))?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .meter_fallback_for_meters(true)
            .generate(&graph, BTreeSet::from([3]))?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .meter_fallback_for_meters(true)
            .generate(&graph, BTreeSet::from([2]))?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");

        let graph = builder.build(Some(ComponentGraphConfig {
            disable_fallback_components: true,
            ..Default::default()
        }))?;
        let expr = FallbackExpr::new().generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + #2");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + #2");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([3]))?;
        assert_eq!(expr.to_string(), "#3");

        // Add a battery meter with three inverter and three batteries
        let meter_bat_chain = builder.meter_bat_chain(3, 3);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build(None)?;
        let expr = FallbackExpr::new().generate(&graph, BTreeSet::from([3, 5]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(",
                "#8 + #7 + #6, ",
                "#5, ",
                "COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)",
                ")"
            )
        );

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([2, 5]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
            )
        );

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([2, 6, 7, 8]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
            )
        );

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([2, 7, 8]))?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#2, #3, 0.0) + COALESCE(#7, 0.0) + COALESCE(#8, 0.0)"
        );

        let graph = builder.build(Some(ComponentGraphConfig {
            disable_fallback_components: true,
            ..Default::default()
        }))?;
        let expr = FallbackExpr::new().generate(&graph, BTreeSet::from([3, 5]))?;
        assert_eq!(expr.to_string(), "#3 + #5");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([2, 5]))?;
        assert_eq!(expr.to_string(), "#2 + #5");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([2, 6, 7, 8]))?;
        assert_eq!(expr.to_string(), "#2 + #6 + #7 + #8");

        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([2, 7, 8]))?;
        assert_eq!(expr.to_string(), "#2 + #7 + #8");

        let meter = builder.meter();
        let chp = builder.chp();
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, meter);
        builder.connect(meter, chp);
        builder.connect(meter, pv_inverter);

        assert_eq!(meter.component_id(), 12);
        assert_eq!(chp.component_id(), 13);
        assert_eq!(pv_inverter.component_id(), 14);

        let graph = builder.build(None)?;
        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .generate(&graph, BTreeSet::from([5, 12]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) + ",
                "COALESCE(#12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0))"
            )
        );

        let expr = FallbackExpr::new().generate(&graph, BTreeSet::from([7, 14]))?;
        assert_eq!(expr.to_string(), "COALESCE(#7, 0.0) + COALESCE(#14, 0.0)");

        Ok(())
    }

    /// Test fallback expression generation when there are no meters in the
    /// graph, with only PV inverters directly connected to the grid.
    #[test]
    fn test_no_meters() {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let inverter = builder.solar_inverter();
        builder.connect(grid, inverter);

        let graph = builder.build(None).unwrap();
        let expr = graph.pv_formula(None).unwrap().to_string();
        assert_eq!(expr, "COALESCE(#1, 0.0)");

        let inverter = builder.solar_inverter();
        builder.connect(grid, inverter);

        let graph = builder.build(None).unwrap();
        let expr = graph.pv_formula(None).unwrap().to_string();
        assert_eq!(expr, "COALESCE(#1, 0.0) + COALESCE(#2, 0.0)");
    }

    // ---------------------------------------------------------------
    // Pass-through scenarios.
    //
    // These tests document the *desired* behavior when a component
    // category that has no specific handling (here: `PowerTransformer`)
    // sits in the topology. Validators and the formula generator
    // should treat such a node as transparent — walking through it
    // instead of rejecting otherwise-valid neighbor relationships or
    // emitting it as a measurement source.
    // ---------------------------------------------------------------

    /// Validation accepts a graph where a pass-through category sits
    /// between an inverter and its meter / between a meter and the
    /// grid. Neighbor rules (`M1`, `I1-I4`, `B1`) consult the
    /// effective predecessors / successors, so the chain through the
    /// pass-through reads as if it weren't there.
    ///
    /// Topology: `Grid → PowerTransformer → Meter → BatteryInverter → Battery`
    #[test]
    fn test_validation_accepts_passthrough_predecessor() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, pt);
        builder.connect(pt, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        // Should build cleanly with the default config.
        let _graph = builder.build(None)?;
        Ok(())
    }

    /// A pass-through-only cycle attached to an otherwise-valid graph
    /// is rejected at construction time. The acyclicity validator
    /// walks the raw graph so cycles composed entirely of pass-through
    /// nodes are still detected.
    ///
    /// Topology: a normal `Grid → Meter → BatteryInverter → Battery`
    /// branch, plus a side-branch `Grid → PT1 → PT2 → PT3 → PT1` cycle.
    #[test]
    fn test_acyclicity_detects_passthrough_only_cycle() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        let pt1 = builder.power_transformer();
        let pt2 = builder.power_transformer();
        let pt3 = builder.power_transformer();

        builder.connect(grid, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        builder.connect(grid, pt1);
        builder.connect(pt1, pt2);
        builder.connect(pt2, pt3);
        builder.connect(pt3, pt1);

        assert!(
            builder.build(None).is_err(),
            "PT-only cycle reachable from the GCP must be detected at construction time"
        );
        Ok(())
    }

    /// A pass-through component preceding the GCP is tolerated: the
    /// effective predecessors view walks past it, so `ensure_root` sees
    /// no ancestor and accepts the GCP as a root.
    ///
    /// Topology: `PT → Grid → Meter → BatteryInverter → Battery`.
    #[test]
    fn test_ensure_root_tolerates_passthrough_predecessor() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(pt, grid);
        builder.connect(grid, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let _graph = builder.build(None)?;
        Ok(())
    }

    /// Grid formula skips a PowerTransformer that sits directly below
    /// the GCP and uses the meter beneath it as the measurement source
    /// — the formula is identical to the equivalent
    /// `Grid → Meter → Inverter → Battery` graph.
    ///
    /// Topology (component ids): `Grid:0 → PT:1 → Meter:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_grid_formula_skips_passthrough_at_root() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, pt);
        builder.connect(pt, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert!(
            !formula.contains("#1"),
            "PowerTransformer #1 must not appear in grid_formula, got {formula:?}",
        );
        assert_eq!(formula, "COALESCE(#2, #3, 0.0)");
        Ok(())
    }

    /// Meter fallback sums the meter's *effective* successors —
    /// walking past pass-throughs to the inverter rather than
    /// including the transformer (which has no measurement).
    ///
    /// Topology (component ids): `Grid:0 → Meter:1 → PT:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_meter_fallback_skips_passthrough_successor() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let pt = builder.power_transformer();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, meter);
        builder.connect(meter, pt);
        builder.connect(pt, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert!(
            !formula.contains("#2"),
            "PowerTransformer #2 must not appear in grid_formula, got {formula:?}",
        );
        assert_eq!(formula, "COALESCE(#1, #3, 0.0)");
        Ok(())
    }

    /// Component fallback must find a predecessor meter through a
    /// pass-through node.
    ///
    /// Topology (component ids): `Grid:0 → Meter:1 → PT:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_component_fallback_finds_meter_through_passthrough() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let pt = builder.power_transformer();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, meter);
        builder.connect(meter, pt);
        builder.connect(pt, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.battery_formula(None)?.to_string();
        // Inverter falls back to its effective predecessor meter,
        // walking past the transformer.
        assert_eq!(formula, "COALESCE(#1, #3, 0.0)");
        Ok(())
    }

    /// Test meters with meter fallback
    #[test]
    fn test_meters_with_meter_fallback() -> Result<(), Error> {
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
        let expr = FallbackExpr::new()
            .prefer_meters(true)
            .meter_fallback_for_meters(true)
            .generate(&graph, BTreeSet::from([meter1.component_id()]))?;
        assert_eq!(expr.to_string(), "COALESCE(#1, #2)");

        Ok(())
    }
}
