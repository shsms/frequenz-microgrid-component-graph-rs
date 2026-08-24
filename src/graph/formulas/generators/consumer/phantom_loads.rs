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
use crate::graph::formulas::Formula;
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::{SourcePreference, aggregate};
use crate::{ComponentGraph, Edge, Error, Node};

/// Generates the consumer formula with phantom loads counted in.
pub(super) fn build<N: Node, E: Edge>(graph: &ComponentGraph<N, E>) -> Result<Formula, Error> {
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

    fn build(mut self) -> Result<Formula, Error> {
        let mut all_meters = None;
        while let Some(meter_id) = self.unvisited_meters.pop_first() {
            // A meter that provides no telemetry has no reading, so its phantom
            // load (its residual) is unknowable and it contributes no term. Its
            // nested meters still get their own terms.
            if !self.graph.component(meter_id)?.provides_telemetry() {
                continue;
            }
            let consumption = self.component_consumption(meter_id)?;
            if let Some(expr) = all_meters {
                all_meters = Some(expr + consumption);
            } else {
                all_meters = Some(consumption);
            }
        }

        let other_grid_successors = self
            .graph
            .successors(self.graph.root_id)?
            .filter(|s| {
                !s.is_meter()
                    && !s.is_battery_inverter(&self.graph.config)
                    // A component that provides no telemetry has no reading to
                    // count as consumption.
                    && s.provides_telemetry()
            })
            .map(|s| self.component_consumption(s.component_id()))
            .reduce(|a, b| Ok(a? + b?));

        let other_grid_successors = match other_grid_successors {
            Some(Ok(expr)) => Some(expr),
            Some(Err(err)) => return Err(err),
            None => None,
        };

        match (all_meters, other_grid_successors) {
            (Some(lhs), Some(rhs)) => Ok(Formula::new(lhs + rhs)),
            (None, Some(expr)) | (Some(expr), None) => Ok(Formula::new(expr)),
            (None, None) => Ok(Formula::new(Expr::number(0.0))),
        }
    }

    fn component_consumption(&mut self, component_id: u64) -> Result<Expr, Error> {
        let component = self.graph.component(component_id)?;
        if component.is_meter() {
            self.unvisited_meters.remove(&component_id);
            // Create a formula expression from the component.
            let mut expr = Expr::from(component);

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
                    if group.insert(sibling.component_id()) {
                        // A member that provides no telemetry has no reading
                        // to add to the diamond sum; its successors are
                        // still merged, so the group's residual stays
                        // best-effort instead of going null.
                        if sibling.provides_telemetry() {
                            expr = expr + sibling.into();
                        }
                        self.unvisited_meters.remove(&sibling.component_id());
                        queue.push(sibling.component_id());
                    }
                }
            }
            let mut successors = BTreeMap::new();
            for &member in &group {
                for successor in self.graph.successors(member)? {
                    successors.insert(successor.component_id(), successor);
                }
            }

            // Subtract each successor from the expression.
            for successor in successors {
                let successor_expr = if successor.1.is_meter() {
                    aggregate(
                        self.graph,
                        BTreeSet::from([successor.0]),
                        SourcePreference::MetersFirst { by_config: false },
                    )?
                } else if successor.1.provides_telemetry() {
                    Expr::from(successor.1)
                } else {
                    // No reading to subtract: the component's share stays in
                    // the meter's residual, i.e. counts as phantom load.
                    continue;
                };
                expr = expr - successor_expr;
            }

            expr = expr.max(Expr::number(0.0));

            // If the meter only has non-meter successors, its consumption
            // can be 0 when it can't be calculated.
            if self.graph.has_successors(component_id)?
                && !self.graph.has_meter_successors(component_id)?
            {
                expr = expr.coalesce(Expr::number(0.0));
            }
            Ok(expr)
        } else {
            Ok(Expr::from(component).max(Expr::number(0.0)))
        }
    }
}
