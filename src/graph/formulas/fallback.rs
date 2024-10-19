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
    /// Returns a formula expression with fallbacks where possible for the `sum`
    /// of the given component ids.
    pub(super) fn fallback_expr(
        &self,
        component_ids: impl IntoIterator<Item = u64>,
        prefer_meters: bool,
    ) -> Result<Expr, Error> {
        FallbackExpr {
            prefer_meters,
            graph: self,
        }
        .generate(BTreeSet::from_iter(component_ids))
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
    fn generate(&self, mut component_ids: BTreeSet<u64>) -> Result<Expr, Error> {
        let mut formula = None::<Expr>;
        while let Some(component_id) = component_ids.pop_first() {
            if let Some(expr) = self.meter_fallback(component_id)? {
                formula = Self::add_to_option(formula, expr);
            } else if let Some(expr) = self.component_fallback(&mut component_ids, component_id)? {
                formula = Self::add_to_option(formula, expr);
            } else {
                formula = Self::add_to_option(formula, Expr::component(component_id));
            }
        }

        formula.ok_or(Error::internal("Search for fallback components failed."))
    }

    /// Returns a fallback expression for a meter component.
    fn meter_fallback(&self, component_id: u64) -> Result<Option<Expr>, Error> {
        let component = self.graph.component(component_id)?;
        if !component.is_meter() || self.graph.has_meter_successors(component_id)? {
            return Ok(None);
        }

        if !self.graph.has_successors(component_id)?
            || !self
                .graph
                .successors(component_id)?
                .all(|x| x.is_supported())
        {
            return Ok(Some(Expr::component(component_id)));
        }

        let successor_expr = self
            .graph
            .successors(component_id)?
            .map(Expr::from)
            .reduce(|a, b| a + b)
            .ok_or(Error::internal(
                "Can't find successors of components with successors.",
            ))?;

        let exprs = if self.prefer_meters {
            vec![Expr::component(component_id), successor_expr]
        } else {
            vec![successor_expr, Expr::component(component_id)]
        };

        Ok(Some(Expr::coalesce(exprs)))
    }

    /// Returns a fallback expression for components with the following categories:
    ///
    /// - CHP
    /// - Battery Inverter
    /// - PV Inverter
    /// - EV Charger
    fn component_fallback(
        &self,
        component_ids: &mut BTreeSet<u64>,
        component_id: u64,
    ) -> Result<Option<Expr>, Error> {
        let component = self.graph.component(component_id)?;
        if !component.is_battery_inverter()
            && !component.is_chp()
            && !component.is_pv_inverter()
            && !component.is_ev_charger()
        {
            return Ok(None);
        }

        let mut formula = None::<Expr>;

        // If predecessors have other successors that are not in the list of
        // component ids, the predecessors can't be used as fallback.
        let siblings = self
            .graph
            .siblings_from_predecessors(component_id)?
            .filter(|sibling| sibling.component_id() != component_id)
            .collect::<Vec<_>>();
        if !siblings
            .iter()
            .all(|sibling| component_ids.contains(&sibling.component_id()))
        {
            formula = Self::add_to_option(formula, Expr::component(component_id));
            return Ok(formula);
        }

        for sibling in siblings {
            component_ids.remove(&sibling.component_id());
        }

        let predecessor_ids: BTreeSet<u64> = self
            .graph
            .predecessors(component_id)?
            .map(|x| x.component_id())
            .collect();

        let expr = self.generate(predecessor_ids)?;
        formula = Self::add_to_option(formula, expr);

        Ok(formula)
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
    use crate::{graph::test_utils::ComponentGraphBuilder, Error};

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

        let graph = builder.build()?;
        let expr = graph.fallback_expr(vec![1, 2], false)?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#3, #2)");

        let expr = graph.fallback_expr(vec![1, 2], true)?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3)");

        let expr = graph.fallback_expr(vec![3], true)?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3)");

        // Add a battery meter with three inverter and three batteries
        let meter_bat_chain = builder.meter_bat_chain(3, 3);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build()?;
        let expr = graph.fallback_expr(vec![3, 5], false)?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#3, #2) + COALESCE(#8 + #7 + #6, #5)"
        );

        let expr = graph.fallback_expr(vec![2, 5], true)?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#2, #3) + COALESCE(#5, #8 + #7 + #6)"
        );

        let expr = graph.fallback_expr(vec![2, 6, 7, 8], true)?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#2, #3) + COALESCE(#5, #8 + #7 + #6)"
        );

        let expr = graph.fallback_expr(vec![2, 7, 8], true)?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3) + #7 + #8");

        let meter = builder.meter();
        let chp = builder.chp();
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, meter);
        builder.connect(meter, chp);
        builder.connect(meter, pv_inverter);

        assert_eq!(meter.component_id(), 12);
        assert_eq!(chp.component_id(), 13);
        assert_eq!(pv_inverter.component_id(), 14);

        let graph = builder.build()?;
        let expr = graph.fallback_expr(vec![5, 12], true)?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#5, #8 + #7 + #6) + COALESCE(#12, #14 + #13)"
        );

        let expr = graph.fallback_expr(vec![7, 14], false)?;
        assert_eq!(expr.to_string(), "#7 + #14");

        Ok(())
    }
}
