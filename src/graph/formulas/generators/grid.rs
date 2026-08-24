// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{
    ComponentGraph, Edge, Error, Node,
    graph::formulas::{
        Formula,
        explain::{Explained, ExplanationKind, sum_explained},
        expr::Expr,
        fallback::{SourcePreference, aggregate, diamond_term, is_grid_meter},
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
    ///
    /// Feeds that share components below them — parallel meters over one
    /// chain — are measured as one diamond: the meter readings sum to the
    /// group's throughput, backed by the shared components' own readings.
    /// A per-feed term would have no backing, and a feed whose meter
    /// reports nothing would count as zero flow.
    pub fn build(self) -> Result<Formula, Error> {
        Ok(Formula::new(self.build_explained()?.expr))
    }

    /// Like [`Self::build`], but also explains each formula part.
    pub fn build_explained(self) -> Result<Explained, Error> {
        let mut terms = Vec::new();
        for group in self.feed_groups()? {
            terms.push(match group.as_slice() {
                [feed] => aggregate(
                    self.graph,
                    BTreeSet::from([*feed]),
                    SourcePreference::MetersFirstWithChains,
                )?,
                meters => {
                    let mut components = BTreeSet::new();
                    for &meter in meters {
                        components.extend(self.graph.successors(meter)?.map(|s| s.component_id()));
                    }
                    let components: Vec<u64> = components.into_iter().collect();
                    diamond_term(
                        self.graph,
                        &components,
                        meters,
                        SourcePreference::MetersFirstWithChains,
                    )?
                }
            });
        }
        // Successors iterate in reverse insertion order; the sum is built in
        // insertion order.
        terms.reverse();
        Ok(sum_explained(
            terms,
            ExplanationKind::TermSum,
            "The sum of everything connected to the grid connection point, \
             one term per connection.",
        )
        .unwrap_or_else(|| {
            Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "Nothing is connected to the grid connection point, so the \
                 formula is 0.0.",
            )
        }))
    }

    /// The grid's feeds, with parallel feeds over shared components
    /// grouped. Feeds whose subtrees overlap belong together, closed
    /// transitively. A group is measured as a diamond only when every
    /// member is a meter, none is a grid meter — a grid meter also
    /// carries the site's unmodeled load, which no component sum accounts
    /// for — and no member feeds another (a serial pair is not a
    /// diamond). Any other overlapping group falls back to per-feed
    /// terms: the members come back as singletons.
    fn feed_groups(&self) -> Result<Vec<Vec<u64>>, Error> {
        let feeds: Vec<&N> = self.graph.successors(self.graph.root_id)?.collect();
        let mut reaches = Vec::new();
        for feed in &feeds {
            reaches.push(self.graph.find_all(
                feed.component_id(),
                |_| true,
                petgraph::Direction::Outgoing,
                true,
            )?);
        }

        // Merge overlapping subtrees into groups of feed indices, keeping
        // first-seen order. A feed overlapping several earlier groups
        // bridges them into one.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for index in 0..feeds.len() {
            let mut target: Option<usize> = None;
            let mut position = 0;
            while position < groups.len() {
                let overlaps = groups[position]
                    .iter()
                    .any(|&member| !reaches[member].is_disjoint(&reaches[index]));
                match (overlaps, target) {
                    (true, None) => {
                        target = Some(position);
                        position += 1;
                    }
                    (true, Some(first)) => {
                        let bridged = groups.remove(position);
                        groups[first].extend(bridged);
                    }
                    (false, _) => position += 1,
                }
            }
            match target {
                Some(first) => groups[first].push(index),
                None => groups.push(vec![index]),
            }
        }

        let mut result = Vec::new();
        for group in groups {
            let diamond = group.len() > 1
                && group.iter().try_fold(true, |ok, &member| {
                    Ok::<bool, Error>(
                        ok && feeds[member].is_meter()
                            && !is_grid_meter(self.graph, feeds[member])?
                            && group.iter().all(|&other| {
                                other == member
                                    || !reaches[member].contains(&feeds[other].component_id())
                            }),
                    )
                })?;
            if diamond {
                result.push(
                    group
                        .iter()
                        .map(|&member| feeds[member].component_id())
                        .collect(),
                );
            } else {
                for &member in &group {
                    result.push(vec![feeds[member].component_id()]);
                }
            }
        }
        Ok(result)
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

    /// A no-telemetry grid meter with nothing below it to read: every child
    /// is control-only, so no child sum is left and the term is null. The
    /// excluded children are still recorded — the meter's silent node
    /// carries one silent part per child, and the commented rendering
    /// prints their reasons and the `None` body.
    ///
    /// Topology (ids): `Grid:0 → GridMeter:1 (control-only) → {CHP:2, PV:3}
    /// (both control-only)`.
    #[test]
    fn test_no_telemetry_grid_meter_records_children() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
        builder.connect(grid, grid_meter);
        let chp =
            builder.add_component_with_mode(ComponentCategory::Chp, OperationalMode::ControlOnly);
        builder.connect(grid_meter, chp);
        let pv = builder.add_component_with_mode(
            ComponentCategory::Inverter(crate::InverterType::Pv),
            OperationalMode::ControlOnly,
        );
        builder.connect(grid_meter, pv);

        let graph = builder.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "None");

        // A single silent term: the sum collapses to the meter's own node.
        let explained = GridFormulaBuilder::try_new(&graph)?.build_explained()?;
        let meter_node = &explained.explanation;
        assert_eq!(meter_node.kind, ExplanationKind::NoTelemetryZero);
        assert_eq!(meter_node.rendered(), None);
        assert_eq!(meter_node.component_ids, vec![1, 2, 3]);
        assert_eq!(meter_node.children.len(), 2);
        // Successors iterate in reverse insertion order: PV:3, then CHP:2.
        for (child, id) in meter_node.children.iter().zip([3u64, 2]) {
            assert_eq!(child.kind, ExplanationKind::NoTelemetryZero);
            assert_eq!(child.component_ids, vec![id]);
            assert_eq!(child.rendered(), None);
        }
        // The commented rendering keeps every reason and the `None` body.
        #[cfg(feature = "explain")]
        {
            let commented = explained
                .into_formula("grid", "The total power flow at the grid connection point.")
                .to_commented_string();
            assert!(commented.contains("Child CHP #2"));
            assert!(commented.contains("Child PV inverter #3"));
            assert!(commented.ends_with("\nNone"));
        }
        Ok(())
    }

    /// A grid meter that provides no telemetry resolves through its
    /// reporting children — the same descent the consumer's summed-meters
    /// shape makes. The reading misses unmodeled loads on the silent
    /// segment and anything fed around the silent meter; the silent
    /// meter's own id never appears.
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
        assert_eq!(graph.grid_formula()?.to_string(), "#2");
        Ok(())
    }

    /// Two parallel meters over one battery chain, one silent: the pair
    /// is one diamond. A meter sum with the silent Meter:1 in it could
    /// never resolve, so it is left out: the inverter's reading, which
    /// covers both feeds exactly, is primary, and the reporting meter is
    /// the best-effort backup. A per-feed term would count the silent
    /// feed as a hard 0.0.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1 (no telemetry), Meter:2}`,
    /// both → `BatteryInverter:3 → Battery:4`.
    #[test]
    fn test_grid_formula_backs_a_diamond_with_a_silent_leg() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let silent =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        let reporting = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid, silent);
        builder.connect(grid, reporting);
        builder.connect(silent, inverter);
        builder.connect(reporting, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "COALESCE(#3, #2, 0.0)");

        Ok(())
    }

    /// The same diamond with both meters silent: nothing meter-side can
    /// ever resolve, so the formula is the inverter's reading alone —
    /// null when it is offline, never a fabricated zero.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1, Meter:2} (both no
    /// telemetry)`, both → `BatteryInverter:3 → Battery:4`.
    #[test]
    fn test_grid_formula_backs_a_diamond_with_both_legs_silent() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let a =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        let b =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid, a);
        builder.connect(grid, b);
        builder.connect(a, inverter);
        builder.connect(b, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "#3");

        Ok(())
    }

    /// A reporting diamond gains the same backing — one meter dropping
    /// out at runtime no longer nulls the sum — and a feed with nothing
    /// in common keeps its own term.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1, Meter:2, Meter:5}`,
    /// `{Meter:1, Meter:2} → BatteryInverter:3 → Battery:4`.
    #[test]
    fn test_grid_formula_groups_only_the_overlapping_feeds() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let a = builder.meter();
        let b = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        let lone = builder.meter();
        builder.connect(grid, a);
        builder.connect(grid, b);
        builder.connect(a, inverter);
        builder.connect(b, inverter);
        builder.connect(inverter, battery);
        builder.connect(grid, lone);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.grid_formula()?.to_string(),
            "COALESCE(#2 + #1, #3, COALESCE(#2, 0.0) + COALESCE(#1, 0.0)) + #5"
        );

        Ok(())
    }
    /// A silent link in a grid-meter chain does not cut the fallback
    /// ladder: Meter:2 reports nothing, but Meter:3 below it covers the
    /// same line, so it backs the top reading. The silent meter's own id
    /// never appears. A silent grid meter with no child read through it
    /// alone still yields `None`; one with reporting children resolves
    /// through them, as the reporting ladder already does at runtime.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 (no telemetry) →
    /// Meter:3 → {battery chain 4-6, PV chain 7-8}`.
    #[test]
    fn test_grid_formula_drills_through_a_silent_chain_link() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let m1 = builder.meter();
        let m2 =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        let m3 = builder.meter();
        builder.connect(grid, m1);
        builder.connect(m1, m2);
        builder.connect(m2, m3);
        let bat = builder.meter_bat_chain(1, 1);
        let pv = builder.meter_pv_chain(1);
        builder.connect(m3, bat);
        builder.connect(m3, pv);

        let graph = builder.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "COALESCE(#1, #3)");

        Ok(())
    }
    /// A reporting grid meter over a silent dead-end meter stays bare:
    /// nothing below it can back the reading, and a 0.0 fallback would
    /// assert one when the meter drops offline.
    ///
    /// Topology (ids): `Grid:0 → GridMeter:1 → Meter:2 (no telemetry)`.
    #[test]
    fn test_grid_formula_keeps_a_grid_meter_bare_over_a_silent_dead_end() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        let silent =
            builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, silent);

        let graph = builder.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "#1");

        Ok(())
    }
}
