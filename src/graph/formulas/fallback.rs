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
        meter_fallback_for_meters: bool,
    ) -> Result<Expr, Error> {
        FallbackExpr {
            prefer_meters,
            meter_fallback_for_meters,
        }
        .generate(self, BTreeSet::from_iter(component_ids))
    }
}

pub(crate) struct FallbackExpr {
    pub(crate) prefer_meters: bool,
    pub(crate) meter_fallback_for_meters: bool,
}

impl FallbackExpr {
    pub(crate) fn generate<N: Node, E: Edge>(
        &self,
        graph: &ComponentGraph<N, E>,
        mut component_ids: BTreeSet<u64>,
    ) -> Result<Expr, Error> {
        let mut formula = None::<Expr>;
        if graph.config.disable_fallback_components {
            while let Some(component_id) = component_ids.pop_first() {
                formula = Self::add_to_option(formula, Expr::component(component_id));
            }
            return formula.ok_or(Error::internal("No components to generate formula."));
        }
        while let Some(component_id) = component_ids.pop_first() {
            if let Some(expr) = self.meter_fallback(graph, component_id)? {
                formula = Self::add_to_option(formula, expr);
            } else if let Some(expr) =
                self.component_fallback(graph, &mut component_ids, component_id)?
            {
                formula = Self::add_to_option(formula, expr);
            } else {
                formula = Self::add_to_option(formula, Expr::component(component_id));
            }
        }

        formula.ok_or(Error::internal("Search for fallback components failed."))
    }

    /// Returns a fallback expression for a meter component.
    fn meter_fallback<N: Node, E: Edge>(
        &self,
        graph: &ComponentGraph<N, E>,
        component_id: u64,
    ) -> Result<Option<Expr>, Error> {
        let component = graph.component(component_id)?;
        if !component.is_meter() {
            return Ok(None);
        }
        let has_successor_meters = graph.has_meter_successors(component_id)?;

        if !self.meter_fallback_for_meters && has_successor_meters {
            return Ok(Some(Expr::component(component_id)));
        }

        if !graph.has_successors(component_id)? {
            return Ok(Some(Expr::component(component_id)));
        }

        let (sum_of_successors, sum_of_coalesced_successors) = graph
            .successors(component_id)?
            .map(|node| {
                (
                    Expr::from(node),
                    Expr::coalesce(Expr::from(node), Expr::number(0.0)),
                )
            })
            .reduce(|a, b| (a.0 + b.0, a.1 + b.1))
            .ok_or(Error::internal(
                "Can't find successors of components with successors.",
            ))?;

        let has_multiple_successors = matches!(sum_of_successors, Expr::Add { .. });

        // If a meter has exactly one successor and it is a meter, we consider
        // it to be a fallback meter.  If there are multiple meter successors,
        // we return the meter without fallback.
        if has_successor_meters && has_multiple_successors {
            return Ok(Some(Expr::component(component_id)));
        }

        let mut coalesced = Expr::component(component_id);

        if !self.prefer_meters {
            coalesced = sum_of_successors.clone().coalesce(coalesced);
        }

        if self.prefer_meters {
            if has_multiple_successors {
                coalesced = coalesced.coalesce(sum_of_coalesced_successors);
            } else {
                coalesced = coalesced.coalesce(sum_of_successors);
                if !has_successor_meters {
                    coalesced = coalesced.coalesce(Expr::number(0.0));
                }
            }
        } else if has_multiple_successors {
            coalesced = coalesced.coalesce(sum_of_coalesced_successors);
        } else if !has_successor_meters {
            coalesced = coalesced.coalesce(Expr::number(0.0));
        }

        Ok(Some(coalesced))
    }

    /// Returns a fallback expression for components with the following categories:
    ///
    /// - CHP
    /// - Battery Inverter
    /// - PV Inverter
    /// - EV Charger
    fn component_fallback<N: Node, E: Edge>(
        &self,
        graph: &ComponentGraph<N, E>,
        component_ids: &mut BTreeSet<u64>,
        component_id: u64,
    ) -> Result<Option<Expr>, Error> {
        let component = graph.component(component_id)?;
        if !component.is_battery_inverter(&graph.config)
            && !component.is_chp()
            && !component.is_pv_inverter()
            && !component.is_ev_charger()
        {
            return Ok(None);
        }

        // If predecessors have other successors that are not in the list of
        // component ids, the predecessors can't be used as fallback.
        let siblings = graph
            .siblings_from_predecessors(component_id)?
            .filter(|sibling| sibling.component_id() != component_id)
            .collect::<Vec<_>>();
        if !siblings
            .iter()
            .all(|sibling| component_ids.contains(&sibling.component_id()))
        {
            return Ok(Some(Expr::coalesce(
                Expr::component(component_id),
                Expr::number(0.0),
            )));
        }

        // Collect predecessor meter ids.
        let predecessor_ids: BTreeSet<u64> = graph
            .predecessors(component_id)?
            .filter(|x| x.is_meter())
            .map(|x| x.component_id())
            .collect();

        if predecessor_ids.is_empty() {
            return Ok(Some(Expr::coalesce(
                Expr::component(component_id),
                Expr::number(0.0),
            )));
        }

        for sibling in siblings {
            component_ids.remove(&sibling.component_id());
        }

        Ok(Some(self.generate(graph, predecessor_ids)?))
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
    use std::collections::BTreeSet;

    use crate::{
        graph::{formulas::fallback::FallbackExpr, test_utils::ComponentGraphBuilder},
        ComponentGraphConfig, Error,
    };

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

        let graph = builder.build(None)?;
        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: true,
        }
        .generate(&graph, BTreeSet::from([1]))?;
        assert_eq!(expr.to_string(), "COALESCE(#1, #2)");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: true,
        }
        .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "COALESCE(#1, #2) + COALESCE(#2, #3, 0.0)");

        let expr = FallbackExpr {
            prefer_meters: false,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#3, #2, 0.0)");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([3]))?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: true,
        }
        .generate(&graph, BTreeSet::from([3]))?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: true,
        }
        .generate(&graph, BTreeSet::from([2]))?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");

        let graph = builder.build(Some(ComponentGraphConfig {
            disable_fallback_components: true,
            ..Default::default()
        }))?;
        let expr = FallbackExpr {
            prefer_meters: false,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + #2");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([1, 2]))?;
        assert_eq!(expr.to_string(), "#1 + #2");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([3]))?;
        assert_eq!(expr.to_string(), "#3");

        // Add a battery meter with three inverter and three batteries
        let meter_bat_chain = builder.meter_bat_chain(3, 3);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build(None)?;
        let expr = FallbackExpr {
            prefer_meters: false,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([3, 5]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(",
                "#8 + #7 + #6, ",
                "#5, ",
                "COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)",
                ")"
            )
        );

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([2, 5]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
            )
        );

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([2, 6, 7, 8]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
            )
        );

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([2, 7, 8]))?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#2, #3, 0.0) + COALESCE(#7, 0.0) + COALESCE(#8, 0.0)"
        );

        let graph = builder.build(Some(ComponentGraphConfig {
            disable_fallback_components: true,
            ..Default::default()
        }))?;
        let expr = FallbackExpr {
            prefer_meters: false,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([3, 5]))?;
        assert_eq!(expr.to_string(), "#3 + #5");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([2, 5]))?;
        assert_eq!(expr.to_string(), "#2 + #5");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([2, 6, 7, 8]))?;
        assert_eq!(expr.to_string(), "#2 + #6 + #7 + #8");

        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([2, 7, 8]))?;
        assert_eq!(expr.to_string(), "#2 + #7 + #8");

        let meter = builder.meter();
        let chp = builder.chp();
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, meter);
        builder.connect(meter, chp);
        builder.connect(meter, pv_inverter);

        assert_eq!(meter.component_id(), 12);
        assert_eq!(chp.component_id(), 13);
        assert_eq!(pv_inverter.component_id(), 14);

        let graph = builder.build(None)?;
        let expr = FallbackExpr {
            prefer_meters: true,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([5, 12]))?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) + ",
                "COALESCE(#12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0))"
            )
        );

        let expr = FallbackExpr {
            prefer_meters: false,
            meter_fallback_for_meters: false,
        }
        .generate(&graph, BTreeSet::from([7, 14]))?;
        assert_eq!(expr.to_string(), "COALESCE(#7, 0.0) + COALESCE(#14, 0.0)");

        Ok(())
    }

    /// Test fallback expression generation when there are no meters in the
    /// graph, with only PV inverters directly connected to the grid.
    #[test]
    fn test_no_meters() {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let inverter = builder.solar_inverter();
        builder.connect(grid, inverter);

        let graph = builder.build(None).unwrap();
        let expr = graph.pv_formula(None).unwrap().to_string();
        assert_eq!(expr, "COALESCE(#1, 0.0)");

        let inverter = builder.solar_inverter();
        builder.connect(grid, inverter);

        let graph = builder.build(None).unwrap();
        let expr = graph.pv_formula(None).unwrap().to_string();
        assert_eq!(expr, "COALESCE(#1, 0.0) + COALESCE(#2, 0.0)");
    }
}
