// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeMap;

use super::super::expr::Expr;
use crate::component_category::CategoryPredicates;
use crate::graph::formulas::Formula;
use crate::graph::formulas::explain::{Explained, ExplanationKind, pluralized, sum_explained};
use crate::graph::formulas::fallback::{SourcePreference, aggregate_terms};
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct ProducerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> ProducerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self { graph })
    }

    /// Generates the production formula.
    ///
    /// The production formula is the sum of all the PV and CHP components in
    /// the graph, each measurement clamped so only production counts. Wind
    /// turbines and steam boilers have per-category formulas of their own
    /// and are not part of this sum.
    pub fn build(self) -> Result<Formula, Error> {
        Ok(Formula::new(self.build_explained()?.expr))
    }

    /// Like [`Self::build`], but also explains each formula part.
    pub fn build_explained(self) -> Result<Explained, Error> {
        // One aggregation over the component set, so a component fed
        // through two meters — or shared between two PV meters — is
        // measured once. Per-meter aggregation counted such feeds twice
        // and dropped shared components.
        let targets = self.graph.find_all(
            self.graph.root_id,
            |node| node.is_pv_inverter() || node.is_chp(),
            petgraph::Direction::Outgoing,
            false,
        )?;
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for &component_id in &targets {
            *counts
                .entry(self.graph.component(component_id)?.category().label())
                .or_default() += 1;
        }
        let mut terms = Vec::new();
        for term in aggregate_terms(self.graph, targets, SourcePreference::ComponentsFirst)? {
            // A silent term carries no expression to clamp, and MIN(_, 0.0)
            // over Expr::None would hand it the 0.0 instead.
            terms.push(match term.expr {
                Expr::None => term,
                _ => term.wrap(
                    |expr| expr.min(Expr::number(0.0)),
                    ExplanationKind::ProducerClamp,
                    "Producers feed power in, which is negative by the passive \
                     sign convention. MIN(_, 0.0) discards any consumption \
                     measured on the same lines, so only production is counted.",
                ),
            });
        }
        let count_list = counts
            .iter()
            .map(|(label, &count)| pluralized(count, label))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(sum_explained(
            terms,
            ExplanationKind::TermSum,
            format!(
                "The total production: the sum of the site's producer \
                 measurement points ({count_list})."
            ),
        )
        .unwrap_or_else(|| {
            Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "The graph has no PV or CHP components, so production is 0.0.",
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Error, graph::test_utils::ComponentGraphBuilder};

    #[test]
    fn test_producer_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter and a PV meter with two PV inverters behind it.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(formula, "0.0");

        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0)"
        );

        // Add a CHP meter to the grid with a CHP behind it.
        let meter_chp_chain = builder.meter_chp_chain(1);
        builder.connect(grid, meter_chp_chain);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0)"
            )
        );

        // Add a CHP to the grid, without a meter.
        let chp = builder.chp();
        builder.connect(grid, chp);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0)"
            )
        );

        // Add a PV inverter to the grid_meter.
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, pv_inverter);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0) + ",
                "MIN(COALESCE(#8, 0.0), 0.0)"
            )
        );

        // Add a battery chain to the grid meter.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0) + ",
                "MIN(COALESCE(#8, 0.0), 0.0)"
            )
        );

        // Add a meter to the grid meter, that has a PV inverter and a CHP
        // behind it. The pair is netted inside one clamp, like same-meter
        // siblings of one category: per-component terms would fall back to
        // `#12 - #14` and `#12 - #13`, which overstate production when one
        // sibling consumes while the other produces.
        let meter = builder.meter();
        let pv_inverter = builder.solar_inverter();
        let chp = builder.chp();
        builder.connect(meter, pv_inverter);
        builder.connect(meter, chp);
        builder.connect(grid_meter, meter);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0) + ",
                "MIN(COALESCE(#8, 0.0), 0.0) + ",
                "MIN(COALESCE(#14 + #13, #12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0)), 0.0)"
            )
        );

        Ok(())
    }

    /// A PV inverter shared by two PV meters is measured once, by its own
    /// reading with the meter diamond as fallback. Per-meter aggregation
    /// dropped the shared inverter from both meters' terms.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1 → {PV:3, PV:4}, Meter:2 →
    /// {PV:4, PV:5}}`.
    #[test]
    fn test_producer_formula_measures_a_shared_inverter_once() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let left_meter = builder.meter();
        let right_meter = builder.meter();
        let left_pv = builder.solar_inverter();
        let shared_pv = builder.solar_inverter();
        let right_pv = builder.solar_inverter();
        builder.connect(grid, left_meter);
        builder.connect(grid, right_meter);
        builder.connect(left_meter, left_pv);
        builder.connect(left_meter, shared_pv);
        builder.connect(right_meter, shared_pv);
        builder.connect(right_meter, right_pv);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.producer_formula()?.to_string(),
            "MIN(COALESCE(#3 + #4 + #5, COALESCE(#1, 0.0) + COALESCE(#2, 0.0)), 0.0)"
        );

        Ok(())
    }

    /// A PV inverter fed from two meters is not counted through both: its
    /// own reading covers both feeds. Per-meter aggregation added the
    /// pure-PV meter's reading on top of the inverter's.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1 → PV:3, Meter:2 → {PV:3,
    /// EV:4}}`.
    #[test]
    fn test_producer_formula_counts_a_double_fed_inverter_once() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pv_meter = builder.meter();
        let mixed_meter = builder.meter();
        let pv = builder.solar_inverter();
        let ev = builder.ev_charger();
        builder.connect(grid, pv_meter);
        builder.connect(grid, mixed_meter);
        builder.connect(pv_meter, pv);
        builder.connect(mixed_meter, pv);
        builder.connect(mixed_meter, ev);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.producer_formula()?.to_string(),
            "MIN(COALESCE(#3, 0.0), 0.0)"
        );

        Ok(())
    }
}
