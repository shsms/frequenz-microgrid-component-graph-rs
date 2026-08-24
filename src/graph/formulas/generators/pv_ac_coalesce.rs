// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating PV AC coalesce formulas.

use crate::component_category::CategoryPredicates;
use std::collections::BTreeSet;

use crate::graph::formulas::explain::Explained;
use crate::{ComponentGraph, Edge, Error, Node, graph::formulas::Formula};

pub(crate) struct PVAcCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    pv_inverter_ids: BTreeSet<u64>,
}

impl<'a, N, E> PVAcCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let pv_inverter_ids = if let Some(pv_inverter_ids) = pv_inverter_ids {
            pv_inverter_ids
        } else {
            graph.find_all(
                graph.root_id,
                |node| node.is_pv_inverter(),
                petgraph::Direction::Outgoing,
                false,
            )?
        };
        Ok(Self {
            graph,
            pv_inverter_ids,
        })
    }

    /// Generates the PV AC coalesce formula for the given PV inverter IDs.
    ///
    /// This can be used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all the specified
    /// PV meters and inverters.
    ///
    /// When the `pv_inverter_ids` parameter is `None`, it will include all PV
    /// meters and inverters in the graph.
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn build(self) -> Result<Formula, Error> {
        Ok(Formula::new(self.build_explained()?.expr))
    }

    /// Like [`Self::build`], but also explains each formula part.
    pub fn build_explained(self) -> Result<Explained, Error> {
        let mut meters: BTreeSet<u64> = BTreeSet::new();

        for inv_id in &self.pv_inverter_ids {
            if !self.graph.component(*inv_id)?.is_pv_inverter() {
                return Err(Error::invalid_component(format!(
                    "Component with id {inv_id} is not a PV inverter."
                )));
            }
            for pred in self.graph.predecessors(*inv_id)? {
                if self.graph.is_pv_meter(pred.component_id())? {
                    meters.insert(pred.component_id());
                }
            }
        }

        super::coalesce_with_telemetry(
            self.graph,
            meters.into_iter().chain(self.pv_inverter_ids),
            "A non-aggregating metric (like voltage or frequency) is the same \
             for the whole PV group, so summing makes no sense. The formula \
             takes the first reporting source: the PV meters first, then the \
             PV inverters; sources without telemetry are left out.",
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        ComponentCategory, Error, InverterType, OperationalMode,
        graph::test_utils::ComponentGraphBuilder,
    };

    #[test]
    fn test_pv_ac_coalesce_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "None");

        // Add a PV meter with one PV inverter.
        let meter_pv_chain = builder.meter_pv_chain(1);
        builder.connect(grid_meter, meter_pv_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_pv_chain.component_id(), 2);

        let graph = builder.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");

        // Add a battery meter with one inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 4);

        let graph = builder.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");

        // Add a PV meter with two PV inverters.
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        assert_eq!(meter_pv_chain.component_id(), 8);

        let graph = builder.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #3, #9, #10)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([10, 3])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #3, #10)");

        // add a meter direct to the grid with three PV inverters
        let meter_pv_chain = builder.meter_pv_chain(3);
        builder.connect(grid, meter_pv_chain);

        assert_eq!(meter_pv_chain.component_id(), 11);

        let graph = builder.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #11, #3, #9, #10, #12, #13, #14)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #11, #3, #9, #10, #12, #13)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #11, #3, #9, #10, #12, #13, #14)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([10, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#8, #11, #10, #14)");

        // Failure cases:
        let formula = graph.pv_ac_coalesce_formula(Some(BTreeSet::from([8])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 8 is not a PV inverter."
        );

        Ok(())
    }

    /// A PV inverter that provides no telemetry is dropped from the coalesce,
    /// while the PV meter measuring it stays.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → PVMeter:2 → {PV:3, PV:4 (no
    /// telemetry)}`.
    #[test]
    fn test_pv_ac_coalesce_skips_no_telemetry() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let pv_meter = builder.meter();
        builder.connect(grid_meter, pv_meter);
        let pv = builder.solar_inverter();
        let pv_no_telemetry = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Pv),
            OperationalMode::ControlOnly,
        );
        builder.connect(pv_meter, pv);
        builder.connect(pv_meter, pv_no_telemetry);

        let graph = builder.build(None)?;
        // #4 is dropped; the PV meter #2 and the reporting inverter #3 remain.
        assert_eq!(
            graph.pv_ac_coalesce_formula(None)?.to_string(),
            "COALESCE(#2, #3)",
        );
        Ok(())
    }

    /// When every source component provides no telemetry, nothing is left to
    /// coalesce and the formula is `None`.
    ///
    /// Topology (ids): `Grid:0 → PV:1 (no telemetry)`.
    #[test]
    fn test_pv_ac_coalesce_all_no_telemetry() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pv_no_telemetry = builder.add_component_with_mode(
            ComponentCategory::Inverter(InverterType::Pv),
            OperationalMode::ControlOnly,
        );
        builder.connect(grid, pv_no_telemetry);

        let graph = builder.build(None)?;
        assert_eq!(graph.pv_ac_coalesce_formula(None)?.to_string(), "None");
        Ok(())
    }
}
