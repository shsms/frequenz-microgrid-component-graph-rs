use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};

use super::expr::Expr;

pub(crate) struct FallbackExpr<'a, N, E>
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
    pub(super) fn generate(&self, component_ids: Vec<u64>) -> Result<Expr, Error> {
        self.fallback_for_each(component_ids).and_then(|exprs| {
            exprs
                .into_iter()
                .reduce(|a, b| a + b)
                .ok_or(Error::internal("Search for fallback components failed."))
        })
    }

    fn fallback_for_each(&self, mut component_ids: Vec<u64>) -> Result<Vec<Expr>, Error> {
        let mut exprs = vec![];
        while let Some(component_id) = component_ids.pop() {
            if let Some(formula) = self.meter_with_fallback(component_id)? {
                exprs.push(formula);
            } else if let Some(formulas) =
                self.components_with_fallback(&mut component_ids, component_id)?
            {
                exprs.extend(formulas);
            } else {
                exprs.push(Expr::component(component_id));
            }
        }

        Ok(exprs)
    }

    fn meter_with_fallback(&self, component_id: u64) -> Result<Option<Expr>, Error> {
        if self.graph.has_successors(component_id)?
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

    fn components_with_fallback(
        &self,
        component_ids: &mut Vec<u64>,
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
                    component_ids.remove(
                        component_ids
                            .iter()
                            .position(|x| *x == sibling.component_id())
                            .unwrap(),
                    );
                }
                let predecessor_ids: Vec<u64> = predecessors
                    .iter()
                    .map(|x| x.component_id())
                    .collect::<Vec<_>>();
                let mut expressions = self.fallback_for_each(predecessor_ids)?;
                exprs.append(&mut expressions);
                return Ok(Some(exprs));
            }
        }
        Ok(None)
    }
}
