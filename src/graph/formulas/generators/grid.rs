// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{
    ComponentGraph, Edge, Error, Node,
    graph::formulas::{
        Formula,
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
        let mut expr = None;
        for group in self.feed_groups()? {
            let term = match group.as_slice() {
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
            };
            expr = match expr {
                None => Some(term),
                Some(e) => Some(term + e),
            };
        }
        Ok(expr
            .map(Formula::new)
            .unwrap_or_else(|| Formula::new(Expr::number(0.0))))
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
    use crate::graph::test_utils::graph;

    #[test]
    fn test_grid_formula() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: meter } }.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "#1");

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
            },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "#1");

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
                // An additional dangling meter, and a PV chain and a battery
                // chain on the grid.
                5: meter,
                6: meter => { 7: inv_bat => { 8: bat } },
                9: meter => { 10: inv_pv },
            },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(
            formula,
            "#1 + #5 + COALESCE(#6, #7, 0.0) + COALESCE(#9, #10, 0.0)"
        );

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
                5: meter,
                6: meter => { 7: inv_bat => { 8: bat } },
                9: meter => { 10: inv_pv },
                // A PV inverter on the grid, without a meter.
                11: inv_pv,
            },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(
            formula,
            "#1 + #5 + COALESCE(#6, #7, 0.0) + COALESCE(#9, #10, 0.0) + COALESCE(#11, 0.0)"
        );

        Ok(())
    }

    #[test]
    fn test_grid_formula_with_fallback_grid_meter() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => {
                        3: meter => { 4: inv_bat => { 5: bat } },
                        6: meter => { 7: inv_pv },
                    },
                },
            },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#1, #2)");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => {
                        3: meter => { 4: inv_bat => { 5: bat } },
                        6: meter => { 7: inv_pv },
                    },
                },
                // A battery chain directly on the grid.
                8: meter => { 9: inv_bat => { 10: bat } },
            },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#1, #2) + COALESCE(#8, #9, 0.0)");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => {
                        3: meter => { 4: inv_bat => { 5: bat } },
                        6: meter => { 7: inv_pv },
                    },
                    // A PV chain directly on meter 1, making meter 2 not a
                    // fallback.
                    11: meter => { 12: inv_pv },
                },
                8: meter => { 9: inv_bat => { 10: bat } },
            },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert_eq!(formula, "#1 + COALESCE(#8, #9, 0.0)");

        Ok(())
    }

    /// The grid formula skips a PowerTransformer directly below the grid
    /// connection point and uses the meter beneath it as the measurement
    /// source. The formula is the same as for the equivalent
    /// `Grid → Meter → Inverter → Battery` graph.
    #[test]
    fn test_grid_formula_skips_passthrough_at_root() -> Result<(), Error> {
        let graph = graph! {
            0: grid => { 1: transformer => { 2: meter => { 3: inv_bat => { 4: bat } } } },
        }
        .build(None)?;
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
    #[test]
    fn test_grid_formula_skips_passthrough_successor() -> Result<(), Error> {
        let graph = graph! {
            0: grid => { 1: meter => { 2: transformer => { 3: inv_bat => { 4: bat } } } },
        }
        .build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert!(
            !formula.contains("#2"),
            "PowerTransformer #2 must not appear in grid_formula, got {formula:?}",
        );
        assert_eq!(formula, "COALESCE(#1, #3, 0.0)");
        Ok(())
    }

    /// A grid meter that provides no telemetry resolves through its
    /// reporting children — the same descent the consumer's summed-meters
    /// shape makes. The reading misses unmodeled loads on the silent
    /// segment and anything fed around the silent meter; the silent
    /// meter's own id never appears.
    #[test]
    fn test_grid_formula_no_telemetry_grid_meter() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: meter [Inactive] => { 2: meter } } }.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "#2");
        Ok(())
    }

    /// Two parallel meters over one battery chain, one silent: the pair
    /// is one diamond. A meter sum with the silent Meter:1 in it could
    /// never resolve, so it is left out: the inverter's reading, which
    /// covers both feeds exactly, is primary, and the reporting meter is
    /// the best-effort backup. A per-feed term would count the silent
    /// feed as a hard 0.0.
    #[test]
    fn test_grid_formula_backs_a_diamond_with_a_silent_leg() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter [Inactive] => { 3: inv_bat => { 4: bat } },
                2: meter => { 3 },
            },
        }
        .build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "COALESCE(#3, #2, 0.0)");

        Ok(())
    }

    /// The same diamond with both meters silent: nothing meter-side can
    /// ever resolve, so the formula is the inverter's reading alone —
    /// null when it is offline, never a fabricated zero.
    #[test]
    fn test_grid_formula_backs_a_diamond_with_both_legs_silent() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter [Inactive] => { 3: inv_bat => { 4: bat } },
                2: meter [Inactive] => { 3 },
            },
        }
        .build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "#3");

        Ok(())
    }

    /// A reporting diamond gains the same backing — one meter dropping
    /// out at runtime no longer nulls the sum — and a feed with nothing
    /// in common keeps its own term.
    #[test]
    fn test_grid_formula_groups_only_the_overlapping_feeds() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter => { 3: inv_bat => { 4: bat } },
                2: meter => { 3 },
                5: meter,
            },
        }
        .build(None)?;
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
    #[test]
    fn test_grid_formula_drills_through_a_silent_chain_link() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter [Inactive] => {
                        3: meter => {
                            4: meter => { 5: inv_bat => { 6: bat } },
                            7: meter => { 8: inv_pv },
                        },
                    },
                },
            },
        }
        .build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "COALESCE(#1, #3)");

        Ok(())
    }
    /// A reporting grid meter over a silent dead-end meter stays bare:
    /// nothing below it can back the reading, and a 0.0 fallback would
    /// assert one when the meter drops offline.
    #[test]
    fn test_grid_formula_keeps_a_grid_meter_bare_over_a_silent_dead_end() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: meter => { 2: meter [Inactive] } } }.build(None)?;
        assert_eq!(graph.grid_formula()?.to_string(), "#1");

        Ok(())
    }
}
