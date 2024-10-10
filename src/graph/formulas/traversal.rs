// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

use crate::{component_category::CategoryPredicates, ComponentGraph, Edge, Error, Node};

use super::expressions::FormulaExpression;

// struct FindFallback<'a, N, E, M>
// where
//     N: Node,
//     E: Edge,
//     M: Fn(FormulaExpression) -> FormulaExpression,
// {
//     prefer_meters: bool,
//     wrap_method: M,
//     graph: &'a ComponentGraph<N, E>,
// }

// impl<'a, N, E, M> FindFallback<'a, N, E, M>
// where
//     N: Node,
//     E: Edge,
//     M: Fn(FormulaExpression) -> FormulaExpression,
// {
// }

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    fn is_component_meter(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.is_pv_meter(component_id)?
            || self.is_battery_meter(component_id)?
            || self.is_ev_charger_meter(component_id)?
            || self.is_chp_meter(component_id)?)
    }
    pub(super) fn meter_with_fallback(
        &self,
        component_id: u64,
        prefer_meters: bool,
        wrap_method: impl Fn(FormulaExpression) -> FormulaExpression,
    ) -> Result<Option<FormulaExpression>, Error> {
        if self.is_component_meter(component_id)? {
            if self.successors(component_id)?.all(|x| x.is_supported()) {
                let mut exprs = vec![
                    FormulaExpression::add(
                        FormulaExpression::components(
                            self.successors(component_id)?.map(|x| x.component_id()),
                        )
                        .into_iter()
                        .map(&wrap_method)
                        .collect(),
                    ),
                    wrap_method(FormulaExpression::component(component_id)),
                ];
                if prefer_meters {
                    exprs = exprs.into_iter().rev().collect();
                }
                return Ok(Some(wrap_method(FormulaExpression::coalesce(exprs))));
            } else {
                return Ok(Some(wrap_method(FormulaExpression::component(
                    component_id,
                ))));
            }
        }
        Ok(None)
    }

    fn components_with_fallback(
        &self,
        component_ids: &mut Vec<u64>,
        component_id: u64,
        prefer_meters: bool,
        wrap_method: impl Fn(FormulaExpression) -> FormulaExpression,
    ) -> Result<Option<Vec<FormulaExpression>>, Error> {
        let mut exprs = vec![];
        let component = self.component(component_id)?;
        if component.is_battery_inverter()
            || component.is_chp()
            || component.is_pv_inverter()
            || component.is_ev_charger()
        {
            let siblings = self
                .siblings(component_id)?
                .filter(|sibling| sibling.component_id() != component_id)
                .collect::<Vec<_>>();
            if !siblings
                .iter()
                .all(|sibling| component_ids.contains(&sibling.component_id()))
            {
                exprs.push(wrap_method(FormulaExpression::component(component_id)));
                return Ok(Some(exprs));
            }
            let predecessors = self.predecessors(component_id)?.collect::<Vec<_>>();

            if predecessors.iter().all(|predecessor| {
                self.is_component_meter(predecessor.component_id())
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
                let mut expressions =
                    self.impl_with_fallback(predecessor_ids, prefer_meters, &wrap_method)?;
                exprs.append(&mut expressions);
                return Ok(Some(exprs));
            }
        }
        Ok(None)
    }

    pub(super) fn with_fallback(
        &self,
        component_ids: Vec<u64>,
        prefer_meters: bool,
        wrap_method: impl Fn(FormulaExpression) -> FormulaExpression,
    ) -> Result<FormulaExpression, Error> {
        self.impl_with_fallback(component_ids, prefer_meters, &wrap_method)
            .map(|exprs| FormulaExpression::add(exprs))
    }

    fn impl_with_fallback(
        &self,
        mut component_ids: Vec<u64>,
        prefer_meters: bool,
        wrap_method: impl Fn(FormulaExpression) -> FormulaExpression,
    ) -> Result<Vec<FormulaExpression>, Error> {
        let mut exprs = vec![];
        while let Some(component_id) = component_ids.pop() {
            if let Some(formula) =
                self.meter_with_fallback(component_id, prefer_meters, &wrap_method)?
            {
                exprs.push(formula);
            } else if let Some(formulas) = self.components_with_fallback(
                &mut component_ids,
                component_id,
                prefer_meters,
                &wrap_method,
            )? {
                exprs.extend(formulas);
            } else {
                exprs.push(wrap_method(FormulaExpression::component(component_id)));
            }
        }

        Ok(exprs)
    }
}
