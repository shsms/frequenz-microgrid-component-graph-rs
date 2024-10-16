// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Fallback expression generator for components and meters.

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};
use std::collections::BTreeSet;

use super::expr::Expr;

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    // Returns an expression with fallbacks where possible for the `sum` of the
    // given component ids.
    pub(super) fn fallback_expr(
        &self,
        component_ids: impl IntoIterator<Item = u64>,
        prefer_meters: bool,
    ) -> Result<Expr, Error> {
        Ok(FallbackExpr {
            prefer_meters,
            graph: self,
        }
        .generate(BTreeSet::from_iter(component_ids))?)
    }
}

struct FallbackExpr<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub(crate) prefer_meters: bool,
    pub(crate) graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> FallbackExpr<'a, N, E>
where
    N: Node,
    E: Edge,
{
    fn generate(&self, component_ids: BTreeSet<u64>) -> Result<Expr, Error> {
        self.fallback_for_each(component_ids).and_then(|exprs| {
            exprs
                .into_iter()
                .reduce(|a, b| a + b)
                .ok_or(Error::internal("Search for fallback components failed."))
        })
    }

    fn fallback_for_each(&self, mut component_ids: BTreeSet<u64>) -> Result<Vec<Expr>, Error> {
        let mut exprs = vec![];
        while let Some(component_id) = component_ids.pop_first() {
            if let Some(formula) = self.meter_fallback(component_id)? {
                exprs.push(formula);
            } else if let Some(formulas) =
                self.component_fallback(&mut component_ids, component_id)?
            {
                exprs.extend(formulas);
            } else {
                exprs.push(Expr::component(component_id));
            }
        }

        Ok(exprs)
    }

    fn meter_fallback(&self, component_id: u64) -> Result<Option<Expr>, Error> {
        let component = self.graph.component(component_id)?;
        if component.is_meter()
            && self.graph.has_successors(component_id)?
            && !self.graph.has_meter_successors(component_id)?
        {
            if self
                .graph
                .successors(component_id)?
                .all(|x| x.is_supported())
            {
                let mut exprs = vec![
                    Expr::components(
                        self.graph
                            .successors(component_id)?
                            .map(|x| x.component_id()),
                    )
                    .into_iter()
                    .reduce(|a, b| a + b)
                    .ok_or(Error::internal(
                        "Can't find successors of components with successors.",
                    ))?,
                    Expr::component(component_id),
                ];
                if self.prefer_meters {
                    exprs = exprs.into_iter().rev().collect();
                }
                return Ok(Some(Expr::coalesce(exprs)));
            } else {
                return Ok(Some(Expr::component(component_id)));
            }
        }
        Ok(None)
    }

    fn component_fallback(
        &self,
        component_ids: &mut BTreeSet<u64>,
        component_id: u64,
    ) -> Result<Option<Vec<Expr>>, Error> {
        let mut exprs = vec![];
        let component = self.graph.component(component_id)?;
        if component.is_battery_inverter()
            || component.is_chp()
            || component.is_pv_inverter()
            || component.is_ev_charger()
        {
            let siblings = self
                .graph
                .siblings_from_predecessors(component_id)?
                .filter(|sibling| sibling.component_id() != component_id)
                .collect::<Vec<_>>();
            if !siblings
                .iter()
                .all(|sibling| component_ids.contains(&sibling.component_id()))
            {
                exprs.push(Expr::component(component_id));
                return Ok(Some(exprs));
            }
            let predecessors = self.graph.predecessors(component_id)?.collect::<Vec<_>>();

            if predecessors.iter().all(|predecessor| {
                self.graph
                    .is_component_meter(predecessor.component_id())
                    .unwrap_or(false)
            }) {
                for sibling in siblings {
                    component_ids.remove(&sibling.component_id());
                }
                let predecessor_ids: BTreeSet<u64> =
                    predecessors.iter().map(|x| x.component_id()).collect();
                let mut expressions = self.fallback_for_each(predecessor_ids)?;
                exprs.append(&mut expressions);
                return Ok(Some(exprs));
            }
        }
        Ok(None)
    }
}
