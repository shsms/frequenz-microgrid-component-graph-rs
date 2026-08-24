// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! The consumer formula that counts phantom loads.
//!
//! Every reporting meter contributes its residual — its own reading minus
//! what its children report — so power drawn on a line by something the
//! graph does not model still counts as consumption. A reporting component
//! directly under the grid contributes its own reading, unless it is a
//! battery inverter: what a battery draws is not site consumption.

use std::collections::{BTreeMap, BTreeSet};

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::explain::{
    Explained, Explanation, ExplanationKind, capitalized, id_list, sum_explained,
};
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::{SourcePreference, aggregate};
use crate::{ComponentGraph, Edge, Error, Node};

/// Generates the consumer formula with phantom loads counted in.
pub(super) fn build<N: Node, E: Edge>(graph: &ComponentGraph<N, E>) -> Result<Explained, Error> {
    PhantomLoads::try_new(graph)?.build()
}

/// A walk over the graph's meters that tracks which are still to visit: a
/// meter merged into a diamond group is measured by that group's term and
/// must not also contribute one of its own.
struct PhantomLoads<'a, N, E>
where
    N: Node,
    E: Edge,
{
    unvisited_meters: BTreeSet<u64>,
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> PhantomLoads<'a, N, E>
where
    N: Node,
    E: Edge,
{
    fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self {
            unvisited_meters: graph.find_all(
                graph.root_id,
                |node| node.is_meter(),
                petgraph::Direction::Outgoing,
                true,
            )?,
            graph,
        })
    }

    fn build(mut self) -> Result<Explained, Error> {
        let mut terms = Vec::new();
        while let Some(meter_id) = self.unvisited_meters.pop_first() {
            // A meter that provides no telemetry has no reading, so its phantom
            // load (its residual) is unknowable and it contributes no term. Its
            // nested meters still get their own terms. A silent part records
            // the deliberate omission (its expression vanishes from the sum).
            let component = self.graph.component(meter_id)?;
            if !component.provides_telemetry() {
                terms.push(Explained::silent(
                    ExplanationKind::NoTelemetryZero,
                    format!(
                        "{} #{meter_id} {} and provides no telemetry. Its \
                         phantom load cannot be calculated, so it adds no \
                         term.",
                        capitalized(self.graph.meter_role_label(meter_id)?),
                        component.operational_mode().describe(),
                    ),
                    vec![meter_id],
                ));
                continue;
            }
            terms.push(self.component_consumption(meter_id)?);
        }

        // A non-meter grid successor contributes its own reading. Two kinds
        // stay out on purpose, each recorded by a silent part: a battery
        // inverter, because what a battery draws is not site consumption
        // (even when the inverter reports); and a component that provides no
        // telemetry, which has no reading to count as consumption.
        let graph = self.graph;
        let other_grid_successors = graph
            .successors(graph.root_id)?
            .filter(|s| !s.is_meter())
            .map(|s| s.component_id())
            .collect::<Vec<_>>();
        for component_id in other_grid_successors {
            let component = graph.component(component_id)?;
            let label = capitalized(component.category().label());
            if component.is_battery_inverter(&graph.config) {
                terms.push(Explained::silent(
                    ExplanationKind::StorageNotConsumption,
                    format!(
                        "{label} #{component_id} connects storage: what a \
                         battery draws is not site consumption, so it adds \
                         no term.",
                    ),
                    vec![component_id],
                ));
                continue;
            }
            if !component.provides_telemetry() {
                terms.push(Explained::silent(
                    ExplanationKind::NoTelemetryZero,
                    format!(
                        "{label} #{component_id} {} and provides no \
                         telemetry. It has no reading to count as \
                         consumption, so it adds no term.",
                        component.operational_mode().describe(),
                    ),
                    vec![component_id],
                ));
                continue;
            }
            terms.push(self.component_consumption(component_id)?);
        }

        // Silent parts emit no expression; when nothing else does either,
        // a 0.0 keeps the formula's value the same as before (a sum of
        // only-silent parts would otherwise render as `None`). It also
        // stands in for the whole sum when there are no terms at all.
        let default_zero = || {
            Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "No reporting meter or consumer adds a term, so the \
                 consumption is 0.0.",
            )
        };
        if terms.iter().all(|term| matches!(term.expr, Expr::None)) {
            terms.push(default_zero());
        }
        Ok(sum_explained(
            terms,
            ExplanationKind::TermSum,
            "Phantom loads are included (by config): each meter contributes \
             its residual (reading minus modeled successors), plus the \
             consumers connected directly to the grid.",
        )
        .unwrap_or_else(default_zero))
    }

    fn component_consumption(&mut self, component_id: u64) -> Result<Explained, Error> {
        let component = self.graph.component(component_id)?;
        if component.is_meter() {
            self.unvisited_meters.remove(&component_id);
            // Create a formula expression from the component.
            let mut expr = Expr::from(component);
            let mut parts = Vec::new();
            let mut diamond_siblings = Vec::new();

            // Siblings sharing a successor form a diamond group measured by
            // one term. The relation closes transitively: a bridge meter
            // that shares one successor with this meter and another with a
            // third meter pulls the third in too. Shared successors make
            // the lines inseparable, so only the whole group's residual is
            // well-defined — and each shared successor may be subtracted
            // only once.
            let mut group = BTreeSet::from([component_id]);
            let mut queue = vec![component_id];
            while let Some(member) = queue.pop() {
                for sibling in self.graph.siblings_from_successors(member)? {
                    let sibling_id = sibling.component_id();
                    if group.insert(sibling_id) {
                        // A member that provides no telemetry has no reading
                        // to add to the diamond sum; its successors are
                        // still merged, so the group's residual stays
                        // best-effort instead of going null. A silent part
                        // records it, unless the main loop already did (it
                        // pops meters in id order, so a lower-id silent
                        // member has its own record).
                        let unvisited = self.unvisited_meters.remove(&sibling_id);
                        if sibling.provides_telemetry() {
                            expr = expr + sibling.into();
                            diamond_siblings.push(sibling_id);
                        } else if unvisited {
                            parts.push(Explanation::silent(
                                ExplanationKind::NoTelemetryZero,
                                format!(
                                    "Diamond sibling {} #{sibling_id} {} and \
                                     provides no telemetry: its reading cannot \
                                     join the diamond sum, so the residual is \
                                     computed from the reporting siblings only.",
                                    self.graph.meter_role_label(sibling_id)?,
                                    sibling.operational_mode().describe(),
                                ),
                                vec![sibling_id],
                            ));
                        }
                        queue.push(sibling_id);
                    }
                }
            }
            diamond_siblings.sort_unstable();
            let mut successors = BTreeMap::new();
            for &member in &group {
                for successor in self.graph.successors(member)? {
                    successors.insert(successor.component_id(), successor);
                }
            }

            // Subtract each successor from the expression.
            for successor in successors {
                let successor_expr = if successor.1.is_meter() {
                    let measured = aggregate(
                        self.graph,
                        BTreeSet::from([successor.0]),
                        SourcePreference::MetersFirst { by_config: false },
                    )?;
                    parts.push(Explanation::new(
                        ExplanationKind::SubtractedSuccessor,
                        format!(
                            "Successor {} #{} is modeled in the graph, so \
                             its measurement is subtracted from the residual.",
                            self.graph.meter_role_label(successor.0)?,
                            successor.0
                        ),
                        &measured.expr,
                        vec![measured.explanation],
                    ));
                    measured.expr
                } else if successor.1.provides_telemetry() {
                    let successor_expr = Expr::from(successor.1);
                    parts.push(
                        Explanation::new(
                            ExplanationKind::SubtractedSuccessor,
                            format!(
                                "Successor {} #{} is modeled in the graph, so its \
                                 reading is subtracted from the residual.",
                                successor.1.category().label(),
                                successor.0
                            ),
                            &successor_expr,
                            Vec::new(),
                        )
                        .each(format!(
                            "Each of the {{n}} successor {}s is modeled in the \
                             graph, so its reading is subtracted from the \
                             residual.",
                            successor.1.category().label(),
                        )),
                    );
                    successor_expr
                } else {
                    // No reading to subtract: the component's share stays in
                    // the meter's residual, i.e. counts as phantom load.
                    parts.push(Explanation::silent(
                        ExplanationKind::NoTelemetryZero,
                        format!(
                            "Successor {} #{} {} and provides no telemetry. \
                             There is no reading to subtract, so its share \
                             stays in the residual and counts as phantom \
                             load.",
                            successor.1.category().label(),
                            successor.0,
                            successor.1.operational_mode().describe(),
                        ),
                        vec![successor.0],
                    ));
                    continue;
                };
                expr = expr - successor_expr;
            }

            let siblings_note = if diamond_siblings.is_empty() {
                String::new()
            } else {
                format!(
                    " (plus its diamond siblings {}, which share successors with it)",
                    id_list(&diamond_siblings)
                )
            };
            let residual = Explained::compose(
                expr,
                ExplanationKind::PhantomLoadResidual,
                format!(
                    "Meter #{component_id}'s reading{siblings_note} minus its \
                     modeled successors: the load connected to the meter that \
                     is not in the graph — its phantom load."
                ),
                parts,
            );
            let mut consumption = residual.wrap(
                |expr| expr.max(Expr::number(0.0)),
                ExplanationKind::ConsumerClamp,
                "Consumption cannot be negative. MAX(_, 0.0) discards any \
                 production measured on the same lines.",
            );

            // If the meter only has non-meter successors, its consumption
            // can be 0 when it can't be calculated.
            if self.graph.has_successors(component_id)?
                && !self.graph.has_meter_successors(component_id)?
            {
                consumption = consumption.wrap(
                    |expr| expr.coalesce(Expr::number(0.0)),
                    ExplanationKind::DefaultZero,
                    "The meter has only device successors. When its reading is \
                     missing, its residual cannot be calculated and is defined \
                     as 0.0.",
                );
            }
            Ok(consumption)
        } else {
            Ok(Explained::leaf(
                Expr::from(component).max(Expr::number(0.0)),
                ExplanationKind::ConsumerClamp,
                format!(
                    "{} #{component_id}'s reading counts as consumption. \
                     MAX(_, 0.0) discards any production it measures.",
                    capitalized(component.category().label()),
                ),
            )
            .each(format!(
                "Each of the {{n}} {}s counts its reading as consumption; \
                 MAX(_, 0.0) discards any production it measures.",
                component.category().label(),
            )))
        }
    }
}
