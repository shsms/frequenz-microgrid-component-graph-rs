// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid formulas.

use std::collections::BTreeSet;

use crate::{
    ComponentGraph, Edge, Error, Node,
    graph::formulas::{
        Formula,
        expr::Expr,
        fallback::{SourcePreference, aggregate},
    },
};

pub(crate) struct GridFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> GridFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self { graph })
    }

    /// Generates the grid formula for the given node.
    ///
    /// The grid formula is the sum of all components connected to the grid.
    /// This formula can be used for calculating power or current metrics at the
    /// grid connection point.
    pub fn build(self) -> Result<Formula, Error> {
        let mut expr = None;
        for comp in self.graph.successors(self.graph.root_id)? {
            let comp = aggregate(
                self.graph,
                BTreeSet::from([comp.component_id()]),
                SourcePreference::MetersFirstWithChains,
            )?;
            expr = match expr {
                None => Some(comp),
                Some(e) => Some(comp + e),
            };
        }
        Ok(expr
            .map(Formula::new)
            .unwrap_or_else(|| Formula::new(Expr::number(0.0))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_utils::ComponentGraphBuilder;
    use crate::{ComponentCategory, OperationalMode};

    #[test]
    fn test_grid_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter and a battery chain behind it.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "#1");

        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "#1");

        // Add an additional dangling meter, and a PV chain and a battery chain
        // to the grid
        let dangling_meter = builder.meter();
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        let meter_pv_chain = builder.meter_pv_chain(1);
        builder.connect(grid, dangling_meter);
        builder.connect(grid, meter_bat_chain);
        builder.connect(grid, meter_pv_chain);

        assert_eq!(dangling_meter.component_id(), 5);
        assert_eq!(meter_bat_chain.component_id(), 6);
        assert_eq!(meter_pv_chain.component_id(), 9);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(
            formula,
            "#1 + #5 + COALESCE(#6, #7, 0.0) + COALESCE(#9, #10, 0.0)"
        );

        // Add a PV inverter to the grid, without a meter.
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid, pv_inverter);

        assert_eq!(pv_inverter.component_id(), 11);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(
            formula,
            "#1 + #5 + COALESCE(#6, #7, 0.0) + COALESCE(#9, #10, 0.0) + COALESCE(#11, 0.0)"
        );

        Ok(())
    }

    #[test]
    fn test_grid_formula_with_fallback_grid_meter() -> Result<(), Error> {
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
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#1, #2)");

        // Add a battery chain directly to the grid
        let bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid, bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#1, #2) + COALESCE(#8, #9, 0.0)");

        // Add a PV chain directly to meter1, making meter2 not a fallback
        let pv_chain = builder.meter_pv_chain(1);
        builder.connect(meter1, pv_chain);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "#1 + COALESCE(#8, #9, 0.0)");

        Ok(())
    }

    /// The grid formula skips a PowerTransformer directly below the grid
    /// connection point and uses the meter beneath it as the measurement
    /// source. The formula is the same as for the equivalent
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

    /// A meter is measured from its *effective* successors: the walk passes
    /// the pass-through and reaches the inverter. The transformer itself has
    /// no measurement, so it is not part of the formula.
    ///
    /// Topology (component ids): `Grid:0 → Meter:1 → PT:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_grid_formula_skips_passthrough_successor() -> Result<(), Error> {
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

    /// A grid meter that provides no telemetry is never backed by its
    /// children (they do not carry the site's unmodeled load), so its term
    /// is null rather than a wrong children sum.
    ///
    /// Topology (ids): `Grid:0 → GridMeter:1 (no telemetry) → Meter:2`.
    #[test]
    fn test_grid_formula_no_telemetry_grid_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        let meter = builder.meter();
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, meter);

        let graph = builder.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "None");
        Ok(())
    }
}
