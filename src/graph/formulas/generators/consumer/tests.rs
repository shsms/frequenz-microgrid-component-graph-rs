// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Tests for the consumer formula generator.

use crate::graph::test_utils::ComponentGraphBuilder;
use crate::{ComponentCategory, ComponentGraphConfig, Error, InverterType, OperationalMode};

#[test]
fn test_zero_consumers() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a battery inverter to the grid, without a battery meter.
    let inv_bat_chain = builder.inv_bat_chain(1);
    builder.connect(grid, inv_bat_chain);

    let graph = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "0.0");

    Ok(())
}

#[test]
fn test_consumer_formula_with_grid_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a grid meter to the grid, with no successors.
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config)?;
    let graph_no_phantom = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0)");
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0)");

    // Add a battery meter with one battery inverter and one battery to the
    // grid meter.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid_meter, meter_bat_chain);

    assert_eq!(meter_bat_chain.component_id(), 2);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    // Formula subtracts the battery meter from the grid meter, and the
    // battery inverter from the battery meter.
    assert_eq!(
        formula,
        "MAX(#1 - COALESCE(#2, #3, 0.0), 0.0) + COALESCE(MAX(#2 - #3, 0.0), 0.0)"
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1 - COALESCE(#2, #3, 0.0), 0.0)");

    // Add a solar meter with two solar inverters to the grid meter.
    let meter_pv_chain = builder.meter_pv_chain(2);
    builder.connect(grid_meter, meter_pv_chain);

    assert_eq!(meter_pv_chain.component_id(), 5);

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors
            "MAX(",
            "#1 - COALESCE(#2, #3, 0.0) - COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)), ",
            "0.0",
            ") + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0)",
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            "MAX(",
            "#1 - COALESCE(#2, #3, 0.0) - COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)), ",
            "0.0",
            ")",
        )
    );

    // Add a "mixed" meter with a CHP, an ev charger and a solar inverter to
    // the grid meter.
    let solar_inverter = builder.solar_inverter();
    let chp = builder.chp();
    let ev_charger = builder.ev_charger();
    let meter = builder.meter();
    builder.connect(meter, solar_inverter);
    builder.connect(meter, chp);
    builder.connect(meter, ev_charger);
    builder.connect(grid_meter, meter);

    assert_eq!(solar_inverter.component_id(), 8);
    assert_eq!(chp.component_id(), 9);
    assert_eq!(ev_charger.component_id(), 10);
    assert_eq!(meter.component_id(), 11);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors
            "MAX(",
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)), ",
            "0.0) + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
            // difference of "mixed" meter from its successors.
            "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all non-consumer meters
            "MAX(",
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)), ",
            "0.0)"
        )
    );

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors (without fallbacks)
            "MAX(#1 - #2 - #5 - #11, 0.0) + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
            // difference of "mixed" meter from its successors.
            "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0)"
        )
    );
    let graph_no_phantom = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .include_phantom_loads_in_consumer_formula(false)
            .build(),
    ))?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1 - #2 - #5 - #8 - #9 - #10, 0.0)");

    // add a battery chain to the grid meter and a dangling meter to the grid.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    let dangling_meter = builder.meter();
    builder.connect(grid_meter, meter_bat_chain);
    builder.connect(grid, dangling_meter);

    assert_eq!(meter_bat_chain.component_id(), 12);
    assert_eq!(dangling_meter.component_id(), 15);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors
            "MAX(",
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)) - ",
            "COALESCE(#12, #13, 0.0), ",
            "0.0) + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
            // difference of "mixed" meter from its successors.
            "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0) + ",
            // difference of second battery meter from inverter.
            "COALESCE(MAX(#12 - #13, 0.0), 0.0) + ",
            // consumption component of the dangling meter.
            "MAX(#15, 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all non-consumer meters, adding
            // the dangling meter consumption.
            "MAX(",
            "#1 + #15 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)) - ",
            "COALESCE(#12, #13, 0.0), ",
            "0.0)",
        )
    );

    Ok(())
}

#[test]
fn test_consumer_formula_producers_directly_under_grid_meter() -> Result<(), Error> {
    // A grid meter whose only children are producer components. There is
    // no separate load meter. The grid meter carries the site's residual
    // (unmodeled) consumer load. So the producers must be subtracted by
    // their own readings. The grid meter must NOT stand in for the
    // producer group: that would subtract its whole reading and zero out
    // the residual load it is meant to report. For the same reason, the
    // grid meter is never backed by its children's sum. With the meter
    // offline, that sum holds only the producers. The formula would then
    // report a false consumer value of 0 instead of no value.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    let solar_inverter = builder.solar_inverter();
    let chp = builder.chp();
    builder.connect(grid_meter, solar_inverter);
    builder.connect(grid_meter, chp);

    assert_eq!(grid_meter.component_id(), 1);
    assert_eq!(solar_inverter.component_id(), 2);
    assert_eq!(chp.component_id(), 3);

    let graph = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        // The bare grid meter minus each producer's own reading. The grid
        // meter does NOT stand in for the producer group; that would
        // cancel to zero.
        "MAX(#1 - COALESCE(#2, 0.0) - COALESCE(#3, 0.0), 0.0)",
    );

    Ok(())
}

#[test]
fn test_consumer_formula_without_grid_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a meter-inverter-battery chain to the grid component.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid, meter_bat_chain);

    assert_eq!(meter_bat_chain.component_id(), 1);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );
    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    // Formula subtracts inverter from battery meter, or shows zero
    // consumption if either of the components have no data.
    assert_eq!(formula, "COALESCE(MAX(#1 - #2, 0.0), 0.0)");
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    // The meter is treated as a battery meter, so its consumption is 0.
    assert_eq!(formula, "0.0");

    // Add a pv meter with one solar inverter and two dangling meter.
    let meter_pv_chain = builder.meter_pv_chain(1);
    let dangling_meter_1 = builder.meter();
    let dangling_meter_2 = builder.meter();
    builder.connect(grid, meter_pv_chain);
    builder.connect(grid, dangling_meter_1);
    builder.connect(grid, dangling_meter_2);

    assert_eq!(meter_pv_chain.component_id(), 4);
    assert_eq!(dangling_meter_1.component_id(), 6);
    assert_eq!(dangling_meter_2.component_id(), 7);

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // subtract meter successors from meters
            "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
            // dangling meters
            "MAX(#6, 0.0) + MAX(#7, 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#6 + #7, 0.0)");

    // Add a battery inverter to the grid, without a battery meter.
    //
    // This shouldn't show up in the formula, because battery inverter
    // consumption is charging, not site consumption.
    let inv_bat_chain = builder.inv_bat_chain(1);
    builder.connect(grid, inv_bat_chain);

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // subtract meter successors from meters
            "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
            // dangling meters
            "MAX(#6, 0.0) + MAX(#7, 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#6 + #7, 0.0)");

    // Add a PV inverter and a CHP to the grid, without a meter.
    //
    // Their consumption is counted as site consumption, because they can't
    // be taken out, by discharging the batteries, for example.
    let pv_inv = builder.solar_inverter();
    let chp = builder.chp();
    builder.connect(grid, pv_inv);
    builder.connect(grid, chp);

    assert_eq!(pv_inv.component_id(), 10);
    assert_eq!(chp.component_id(), 11);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // subtract meter successors from meters
            "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
            // dangling meters
            "MAX(#6, 0.0) + MAX(#7, 0.0) + ",
            // PV inverter and CHP
            "MAX(#11, 0.0) + MAX(#10, 0.0)",
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#6 + #7, 0.0)");

    Ok(())
}

/// In the no-grid-meter consumer formula, a consumer meter that provides no
/// telemetry has no reading and is dropped from the sum.
///
/// Topology (ids): `Grid:0 → BatMeter:1 → Inv:2 → Bat:3`, plus dangling
/// consumer meters `Grid:0 → Meter:4` and `Grid:0 → Meter:5 (no telemetry)`.
#[test]
fn test_consumer_formula_without_grid_meter_skips_no_telemetry() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    // A battery chain keeps at least one non-grid-meter successor, selecting
    // the no-grid-meter branch.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid, meter_bat_chain);
    let dangling = builder.meter();
    let dangling_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    builder.connect(grid, dangling);
    builder.connect(grid, dangling_no_telemetry);

    assert_eq!(dangling.component_id(), 4);
    assert_eq!(dangling_no_telemetry.component_id(), 5);

    let graph = builder.build(None)?;
    // Only the reporting dangling meter #4 is summed; #5 is dropped.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#4, 0.0)");
    Ok(())
}

/// In the no-grid-meter consumer formula, a no-telemetry consumer meter is
/// measured by its reporting descendant meters instead of hiding its
/// subtree.
///
/// Topology (ids): `Grid:0 → BatMeter:1 → Inv:2 → Bat:3`, plus
/// `Grid:0 → Meter:4 (no telemetry) → Meter:5`.
#[test]
fn test_consumer_formula_without_grid_meter_no_telemetry_meter_children() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    // A battery chain keeps at least one non-grid-meter successor, selecting
    // the no-grid-meter branch.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid, meter_bat_chain);
    let no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let child = builder.meter();
    builder.connect(grid, no_telemetry);
    builder.connect(no_telemetry, child);

    assert_eq!(child.component_id(), 5);

    let graph = builder.build(None)?;
    // Meter #4 has no reading; its reporting child meter #5 is summed in its
    // place.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#5, 0.0)");
    Ok(())
}

/// In the phantom-loads consumer formula, a component that provides no
/// telemetry is not subtracted from its meter (its share counts as phantom
/// load), and never contributes a reading.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, PV:3 (no telemetry)}`.
#[test]
fn test_consumer_formula_phantom_loads_skips_no_telemetry() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let sub_meter = builder.meter();
    builder.connect(grid_meter, sub_meter);
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(grid_meter, pv_no_telemetry);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // #3 is neither subtracted from meter #1 nor given a term of its own; its
    // consumption stays in meter #1's residual.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - #2, 0.0) + MAX(#2, 0.0)",
    );
    Ok(())
}

/// In the phantom-loads consumer formula, a meter that provides no telemetry
/// contributes no residual term, and a no-telemetry non-meter grid successor
/// contributes nothing.
///
/// Topology (ids): `Grid:0 → Meter:1 (no telemetry) → Meter:2`, plus
/// `Grid:0 → CHP:3 (no telemetry)`.
#[test]
fn test_consumer_formula_phantom_loads_skips_no_telemetry_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let child = builder.meter();
    builder.connect(grid, no_telemetry);
    builder.connect(no_telemetry, child);
    let chp_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Chp, OperationalMode::ControlOnly);
    builder.connect(grid, chp_no_telemetry);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // Meter #1's residual is unknowable and CHP #3 has no reading; only the
    // reporting meter #2 keeps a term.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#2, 0.0)");
    Ok(())
}

/// In the phantom-loads consumer formula, a diamond sibling meter that
/// provides no telemetry is left out of the diamond sum while the shared
/// successors are still subtracted.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2 (no telemetry)} → Meter:3`.
#[test]
fn test_consumer_formula_phantom_loads_no_telemetry_diamond_sibling() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let meter = builder.meter();
    let sibling_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let shared = builder.meter();
    builder.connect(grid, meter);
    builder.connect(grid, sibling_no_telemetry);
    builder.connect(meter, shared);
    builder.connect(sibling_no_telemetry, shared);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // Sibling #2's reading is left out of the diamond sum; the shared meter
    // #3 is still subtracted and keeps its own residual term.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - #3, 0.0) + MAX(#3, 0.0)",
    );
    Ok(())
}

/// A reporting meter whose only child is a no-telemetry meter resolves
/// through it: the grandchild reading backs the meter's own reading in
/// the subtracted term, instead of a 0.0 that would hide it.
///
/// Topology (ids): `Grid:0 → GridMeter:1 → {Meter:2 → Meter:3 (no
/// telemetry) → Meter:4, Meter:5}`.
#[test]
fn test_consumer_formula_recurses_through_no_telemetry_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let meter = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let descendant = builder.meter();
    let load = builder.meter();
    builder.connect(grid_meter, meter);
    builder.connect(meter, silent);
    builder.connect(silent, descendant);
    builder.connect(grid_meter, load);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // Line 2 is subtracted as COALESCE(#2, #4): the silent meter #3's
    // reporting child #4 backs #2's reading.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2, #4) - #5, 0.0) + MAX(#2 - #4, 0.0) + MAX(#4, 0.0) + MAX(#5, 0.0)"
    );
    Ok(())
}

/// A no-telemetry consumer meter on a parallel feed is descended past,
/// but a descendant meter that another collected meter already measures
/// is not summed next to it: that line would be counted twice.
///
/// Topology (ids): `Grid:0 → {BatMeter:1 → Inv:2 → Bat:3, Meter:4 (no
/// telemetry), Meter:5}`, with `4 → 6` and `5 → 6`.
#[test]
fn test_consumer_formula_no_telemetry_shared_descendant() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let bat_meter = builder.meter_bat_chain(1, 1);
    builder.connect(grid, bat_meter);
    let sibling_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let sibling = builder.meter();
    let shared = builder.meter();
    builder.connect(grid, sibling_no_telemetry);
    builder.connect(grid, sibling);
    builder.connect(sibling_no_telemetry, shared);
    builder.connect(sibling, shared);

    let graph = builder.build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#5, 0.0)");
    Ok(())
}

#[test]
fn test_consumer_formula_diamond_meters() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add three meters to the grid
    let grid_meter_1 = builder.meter();
    let grid_meter_2 = builder.meter();
    let grid_meter_3 = builder.meter();
    builder.connect(grid, grid_meter_1);
    builder.connect(grid, grid_meter_2);
    builder.connect(grid, grid_meter_3);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0) + MAX(#2, 0.0) + MAX(#3, 0.0)");

    // Add two solar inverters with two grid meters as predecessors.
    let meter_pv_chain_1 = builder.meter_pv_chain(1);
    let meter_pv_chain_2 = builder.meter_pv_chain(1);
    builder.connect(grid_meter_1, meter_pv_chain_1);
    builder.connect(grid_meter_1, meter_pv_chain_2);
    builder.connect(grid_meter_2, meter_pv_chain_1);
    builder.connect(grid_meter_2, meter_pv_chain_2);

    assert_eq!(meter_pv_chain_1.component_id(), 4);
    assert_eq!(meter_pv_chain_2.component_id(), 6);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of pv powers from first two grid meters
            "MAX(#1 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0), 0.0) + ",
            // third grid meter still dangling
            "MAX(#3, 0.0) + ",
            // difference of solar inverters from their meters
            "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0)"
        )
    );

    // Add a meter to grid meter 3, and then add the two solar inverters to
    // that meter.
    let meter = builder.meter();
    builder.connect(grid_meter_3, meter);
    builder.connect(meter, meter_pv_chain_1);
    builder.connect(meter, meter_pv_chain_2);

    assert_eq!(meter.component_id(), 8);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );
    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of pv powers from first two grid meters and meter#8
            "MAX(#1 + #8 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0), 0.0) + ",
            // difference of meter#8 from third grid meter
            "MAX(#3 - #8, 0.0) + ",
            // difference of solar inverters from their meters
            "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0)"
        )
    );

    // Add a battery inverter to the first grid meter.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid_meter_1, meter_bat_chain);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of pv and battery powers from first two grid
            // meters and meter#8
            "MAX(",
            "#1 + #8 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0) - COALESCE(#9, #10, 0.0), ",
            "0.0) + ",
            // difference of meter#8 from third grid meter
            "MAX(#3 - #8, 0.0) + ",
            // difference of solar inverters from their meters
            "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0) + ",
            // difference of battery inverter from battery meter
            "COALESCE(MAX(#9 - #10, 0.0), 0.0)"
        )
    );

    Ok(())
}

#[test]
fn test_consumer_formula_mixed_meter_with_component_submeter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    // A "mixed" meter feeds a PV inverter and a battery sub-meter (itself
    // a component chain). The battery sub-meter's id sorts before the PV
    // inverter's id. So the sub-meter is first resolved onto its own
    // measurement point. Only after that does the sibling PV inverter
    // reveal that the mixed meter covers the whole group.
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let bat_submeter = builder.meter_bat_chain(1, 1);
    builder.connect(mixed_meter, bat_submeter);
    let solar_inverter = builder.solar_inverter();
    builder.connect(mixed_meter, solar_inverter);

    // A second grid-meter child so the mixed meter is not its sole child
    // (which would make the mixed meter a fallback grid meter).
    let pv_meter = builder.meter_pv_chain(1);
    builder.connect(grid_meter, pv_meter);

    assert_eq!(grid.component_id(), 0);
    assert_eq!(grid_meter.component_id(), 1);
    assert_eq!(mixed_meter.component_id(), 2);
    assert_eq!(bat_submeter.component_id(), 3);
    assert_eq!(solar_inverter.component_id(), 6);
    assert_eq!(pv_meter.component_id(), 7);

    let graph = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    // The mixed meter (#2) covers its whole group. So it is subtracted
    // once, and the battery sub-meter (#3) is NOT subtracted again on its
    // own. The children of #2 back its reading. This is recursive: the
    // sub-meter's own children (#4) back the sub-meter. So an offline
    // meter does not make the whole formula null while the leaf
    // components still report.
    assert_eq!(
        formula,
        concat!(
            "MAX(#1 - COALESCE(#2, COALESCE(#6, 0.0) + COALESCE(#3, #4, 0.0)) - ",
            "COALESCE(#7, #8, 0.0), 0.0)"
        )
    );

    Ok(())
}

/// The same overlap under a grid meter. Meter:3 and Meter:4 both feed
/// BatteryInverter:6, so neither reads the whole chain; the pair minus
/// Meter:3's other child does.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2, Meter:3}`,
/// `Meter:2 → {Meter:4, Meter:5}`, `Meter:4 → BatteryInverter:6 →
/// Battery:7`, `Meter:3 → {BatteryInverter:6, Meter:8}`.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_an_outside_fed_chain_once() -> Result<(), Error>
{
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let feeder = builder.meter();
    let sibling = builder.meter();
    let battery_meter = builder.meter();
    let load = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let sibling_load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, feeder);
    builder.connect(grid_meter, sibling);
    builder.connect(feeder, battery_meter);
    builder.connect(feeder, load);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(sibling, inverter);
    builder.connect(sibling, sibling_load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#3 + #4 - #8, #6, 0.0), 0.0)"
    );

    Ok(())
}

/// The chains under a replaced meter all become targets, not just the one
/// the overlap was found through. Meter:2 also measures BatteryInverter:6,
/// which has no second feed and would otherwise go unsubtracted.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2, Meter:3}`,
/// `Meter:2 → {BatteryInverter:4 → Battery:5, BatteryInverter:6 →
/// Battery:7}`, `Meter:3 → {BatteryInverter:4, Meter:8}`.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_every_chain_under_a_replaced_meter()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let battery_meter = builder.meter();
    let sibling = builder.meter();
    let shared = builder.battery_inverter();
    let shared_battery = builder.battery();
    let own = builder.battery_inverter();
    let own_battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(grid_meter, sibling);
    builder.connect(battery_meter, shared);
    builder.connect(shared, shared_battery);
    builder.connect(battery_meter, own);
    builder.connect(own, own_battery);
    builder.connect(sibling, shared);
    builder.connect(sibling, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2 + #3 - #8, COALESCE(#4, 0.0) + COALESCE(#6, 0.0)), 0.0)"
    );

    Ok(())
}

/// Two independent overlaps are both resolved, not just the first.
///
/// Topology (ids): `Grid:0 → Meter:1`,
/// `Meter:1 → {Meter:2, Meter:5, Meter:7, Meter:10}`,
/// `Meter:2 → BatteryInverter:3 → Battery:4`, `Meter:5 → {BatteryInverter:3,
/// Meter:6}`, `Meter:7 → BatteryInverter:8 → Battery:9`,
/// `Meter:10 → {BatteryInverter:8, Meter:11}`.
#[test]
fn test_consumer_formula_with_grid_meter_resolves_every_overlap() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    for _ in 0..2 {
        let battery_meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        let sibling = builder.meter();
        let load = builder.meter();
        builder.connect(grid_meter, battery_meter);
        builder.connect(battery_meter, inverter);
        builder.connect(inverter, battery);
        builder.connect(grid_meter, sibling);
        builder.connect(sibling, inverter);
        builder.connect(sibling, load);
    }

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2 + #5 - #6, #3, 0.0) - COALESCE(#7 + #10 - #11, #8, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain that reports nothing cannot replace the meter above it: the
/// meter's reading is the only measurement left, so it stays the target.
/// BatteryInverter:3 provides no telemetry, and no feed can stand in for
/// it — Meter:1 is the grid meter — so its own term would be a plain 0.0.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2,
/// BatteryInverter:3, Meter:5}`, `Meter:2 → BatteryInverter:3 → Battery:4`.
#[test]
fn test_consumer_formula_with_grid_meter_keeps_the_meter_of_a_silent_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid_meter, inverter);
    builder.connect(grid_meter, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain that is silent from its meter down cannot be subtracted at
/// all: no reading can be built from any of it, so no term is emitted.
/// Battery:5 still reports, but no term reaches its reading through the
/// two silent components above it.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2,
/// BatteryInverter:4, Meter:6}`, `Meter:2 → Meter:3 (no telemetry) →
/// BatteryInverter:4 (no telemetry) → Battery:5`.
#[test]
fn test_consumer_formula_with_grid_meter_emits_no_term_for_a_wholly_silent_chain()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let battery_meter = builder.meter();
    let silent_meter =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let inverter = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(battery_meter, silent_meter);
    builder.connect(silent_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid_meter, inverter);
    builder.connect(grid_meter, load);

    let graph = builder.build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#1, 0.0)");

    Ok(())
}
