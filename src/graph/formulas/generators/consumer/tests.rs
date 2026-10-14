// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Tests for the consumer formula generator.

use crate::graph::test_utils::graph;
use crate::{ComponentGraphConfig, Error};

#[test]
fn test_zero_consumers() -> Result<(), Error> {
    // A battery inverter on the grid, without a battery meter.
    let graph = graph! { 0: grid => { 1: inv_bat => { 2: bat } } }.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "0.0");

    Ok(())
}

#[test]
fn test_consumer_formula_with_grid_meter() -> Result<(), Error> {
    // A grid meter with no successors.
    let g = graph! { 0: grid => { 1: meter } };

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = g.build(config)?;
    let graph_no_phantom = g.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0)");
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "#1");

    let g = graph! {
        0: grid => {
            1: meter => {
                // A battery meter with one battery inverter and one battery on
                // the grid meter.
                2: meter => { 3: inv_bat => { 4: bat } },
            },
        },
    };

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = g.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    // Formula subtracts the battery meter from the grid meter, and the
    // battery inverter from the battery meter.
    assert_eq!(
        formula,
        "MAX(#1 - COALESCE(#2, #3, 0.0), 0.0) + COALESCE(MAX(#2 - #3, 0.0), 0.0)"
    );
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "#1 - COALESCE(#2, #3, 0.0)");

    let g = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                // A solar meter with two solar inverters on the grid meter.
                5: meter => { 6: inv_pv, 7: inv_pv },
            },
        },
    };

    let graph = g.build(config.clone())?;
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
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        "#1 - COALESCE(#2, #3, 0.0) - COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
    );

    let g = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                5: meter => { 6: inv_pv, 7: inv_pv },
                // A "mixed" meter with a CHP, an ev charger and a solar
                // inverter on the grid meter.
                11: meter => { 8: inv_pv, 9: chp, 10: ev },
            },
        },
    };

    let graph = g.build(config)?;
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
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all non-consumer meters
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0))",
        )
    );

    let graph = g.build(Some(
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
    let graph_no_phantom = g.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .include_phantom_loads_in_consumer_formula(false)
            .build(),
    ))?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "#1 - #2 - #5 - #8 - #9 - #10");

    let g = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                5: meter => { 6: inv_pv, 7: inv_pv },
                11: meter => { 8: inv_pv, 9: chp, 10: ev },
                // A battery chain on the grid meter.
                12: meter => { 13: inv_bat => { 14: bat } },
            },
            // A dangling meter on the grid.
            15: meter,
        },
    };

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = g.build(config)?;
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
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all non-consumer meters, adding
            // the dangling meter consumption.
            "#1 + #15 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)) - ",
            "COALESCE(#12, #13, 0.0)",
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
    let graph = graph! { 0: grid => { 1: meter => { 2: inv_pv, 3: chp } } }.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        // The bare grid meter minus each producer's own reading. The grid
        // meter does NOT stand in for the producer group; that would
        // cancel to zero.
        "#1 - COALESCE(#2, 0.0) - COALESCE(#3, 0.0)",
    );

    Ok(())
}

#[test]
fn test_consumer_formula_without_grid_meter() -> Result<(), Error> {
    // A meter-inverter-battery chain on the grid component.
    let g = graph! {
        0: grid => {
            1: meter => { 2: inv_bat => { 3: bat } },
        },
    };

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );
    let graph = g.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    // Formula subtracts inverter from battery meter, or shows zero
    // consumption if either of the components have no data.
    assert_eq!(formula, "COALESCE(MAX(#1 - #2, 0.0), 0.0)");
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    // The meter is treated as a battery meter, so its consumption is 0.
    assert_eq!(formula, "0.0");

    let g = graph! {
        0: grid => {
            1: meter => { 2: inv_bat => { 3: bat } },
            // A pv meter with one solar inverter and two dangling meters.
            4: meter => { 5: inv_pv },
            6: meter,
            7: meter,
        },
    };

    let graph = g.build(config.clone())?;
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
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "#6 + #7");

    let g = graph! {
        0: grid => {
            1: meter => { 2: inv_bat => { 3: bat } },
            4: meter => { 5: inv_pv },
            6: meter,
            7: meter,
            // A battery inverter on the grid, without a battery meter.
            //
            // This shouldn't show up in the formula, because battery inverter
            // consumption is charging, not site consumption.
            8: inv_bat => { 9: bat },
        },
    };

    let graph = g.build(config.clone())?;
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
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "#6 + #7");

    let g = graph! {
        0: grid => {
            1: meter => { 2: inv_bat => { 3: bat } },
            4: meter => { 5: inv_pv },
            6: meter,
            7: meter,
            8: inv_bat => { 9: bat },
            // A PV inverter and a CHP on the grid, without a meter.
            //
            // Their consumption is counted as site consumption, because they
            // can't be taken out, by discharging the batteries, for example.
            10: inv_pv,
            11: chp,
        },
    };

    let graph = g.build(config)?;
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
    let graph_no_phantom = g.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "#6 + #7");

    Ok(())
}

/// In the no-grid-meter consumer formula, a consumer meter that provides no
/// telemetry has no reading and is dropped from the sum.
#[test]
fn test_consumer_formula_without_grid_meter_skips_no_telemetry() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            // A battery chain keeps at least one non-grid-meter successor,
            // selecting the no-grid-meter branch.
            1: meter => { 2: inv_bat => { 3: bat } },
            4: meter,
            5: meter [Inactive],
        },
    }
    .build(None)?;
    // Only the reporting dangling meter #4 is summed; #5 is dropped.
    assert_eq!(graph.consumer_formula()?.to_string(), "#4");
    Ok(())
}

/// In the no-grid-meter consumer formula, a no-telemetry consumer meter is
/// measured by its reporting descendant meters instead of hiding its
/// subtree.
#[test]
fn test_consumer_formula_without_grid_meter_no_telemetry_meter_children() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            // A battery chain keeps at least one non-grid-meter successor,
            // selecting the no-grid-meter branch.
            1: meter => { 2: inv_bat => { 3: bat } },
            4: meter [Inactive] => { 5: meter },
        },
    }
    .build(None)?;
    // Meter #4 has no reading; its reporting child meter #5 is summed in its
    // place.
    assert_eq!(graph.consumer_formula()?.to_string(), "#5");
    Ok(())
}

/// In the phantom-loads consumer formula, a component that provides no
/// telemetry is not subtracted from its meter (its share counts as phantom
/// load), and never contributes a reading.
#[test]
fn test_consumer_formula_phantom_loads_skips_no_telemetry() -> Result<(), Error> {
    let graph = graph! {
        0: grid => { 1: meter => { 2: meter, 3: inv_pv [ControlOnly] } },
    }
    .build(Some(
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
#[test]
fn test_consumer_formula_phantom_loads_skips_no_telemetry_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter [Inactive] => { 2: meter },
            3: chp [ControlOnly],
        },
    }
    .build(Some(
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
#[test]
fn test_consumer_formula_phantom_loads_no_telemetry_diamond_sibling() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 3: meter },
            2: meter [Inactive] => { 3 },
        },
    }
    .build(Some(
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

/// A partial diamond closes transitively: Meter:2 bridges Meter:1 and
/// Meter:3 by sharing one child with each, so all three form one group.
/// Splitting them would add the bridge's reading to two groups and
/// subtract each shared child twice.
#[test]
fn test_consumer_formula_phantom_loads_merges_a_bridged_diamond() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 4: meter },
            2: meter => { 4, 5: meter },
            3: meter => { 5 },
        },
    }
    .build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #2 + #3 - #4 - #5, 0.0) + MAX(#4, 0.0) + MAX(#5, 0.0)",
    );
    Ok(())
}

/// The bridged diamond with the bridge silent: its reading is left out
/// of the group sum, but the group still merges, so each shared child is
/// subtracted once — not once per line it feeds.
#[test]
fn test_consumer_formula_phantom_loads_merges_a_silent_bridged_diamond() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 4: meter },
            2: meter [Inactive] => { 4, 5: meter },
            3: meter => { 5 },
        },
    }
    .build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #3 - #4 - #5, 0.0) + MAX(#4, 0.0) + MAX(#5, 0.0)",
    );
    Ok(())
}

/// A reporting meter whose only child is a no-telemetry meter resolves
/// through it: the grandchild reading backs the meter's own reading in
/// the subtracted term, instead of a 0.0 that would hide it.
#[test]
fn test_consumer_formula_recurses_through_no_telemetry_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: meter [Inactive] => { 4: meter } },
                5: meter,
            },
        },
    }
    .build(Some(
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
#[test]
fn test_consumer_formula_no_telemetry_shared_descendant() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 2: inv_bat => { 3: bat } },
            4: meter [Inactive] => { 6: meter },
            5: meter => { 6 },
        },
    }
    .build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "#5");
    Ok(())
}

#[test]
fn test_consumer_formula_diamond_meters() -> Result<(), Error> {
    // Three meters on the grid.
    let g = graph! { 0: grid => { 1: meter, 2: meter, 3: meter } };

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = g.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0) + MAX(#2, 0.0) + MAX(#3, 0.0)");

    // Two solar inverters with two grid meters as predecessors.
    let g = graph! {
        0: grid => {
            1: meter => {
                4: meter => { 5: inv_pv },
                6: meter => { 7: inv_pv },
            },
            2: meter => { 4, 6 },
            3: meter,
        },
    };

    let graph = g.build(config)?;
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

    let g = graph! {
        0: grid => {
            1: meter => {
                4: meter => { 5: inv_pv },
                6: meter => { 7: inv_pv },
            },
            2: meter => { 4, 6 },
            // A meter on grid meter 3, feeding the same two solar inverters.
            3: meter => { 8: meter => { 4, 6 } },
        },
    };

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );
    let graph = g.build(config.clone())?;
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

    let g = graph! {
        0: grid => {
            1: meter => {
                4: meter => { 5: inv_pv },
                6: meter => { 7: inv_pv },
                // A battery chain on the first grid meter.
                9: meter => { 10: inv_bat => { 11: bat } },
            },
            2: meter => { 4, 6 },
            3: meter => { 8: meter => { 4, 6 } },
        },
    };

    let graph = g.build(config)?;
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
    let graph = graph! {
        0: grid => {
            1: meter => {
                // A "mixed" meter feeds a PV inverter and a battery sub-meter
                // (itself a component chain). The battery sub-meter's id sorts
                // before the PV inverter's id. So the sub-meter is first
                // resolved onto its own measurement point. Only after that does
                // the sibling PV inverter reveal that the mixed meter covers
                // the whole group.
                2: meter => {
                    3: meter => { 4: inv_bat => { 5: bat } },
                    6: inv_pv,
                },
                // A second grid-meter child so the mixed meter is not its sole
                // child (which would make the mixed meter a fallback grid
                // meter).
                7: meter => { 8: inv_pv },
            },
        },
    }
    .build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    // The mixed meter (#2) covers its whole group. So it is subtracted
    // once, and the battery sub-meter (#3) is NOT subtracted again on its
    // own. The children of #2 back its reading. This is recursive: the
    // sub-meter's own children (#4) back the sub-meter. So an offline
    // meter does not make the whole formula null while the leaf
    // components still report.
    assert_eq!(
        formula,
        "#1 - COALESCE(#2, COALESCE(#6, 0.0) + COALESCE(#3, #4, 0.0)) - COALESCE(#7, #8, 0.0)"
    );

    Ok(())
}

/// The same overlap under a grid meter. Meter:3 and Meter:4 both feed
/// BatteryInverter:6, so neither reads the whole chain; the pair minus
/// Meter:3's other child does.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_an_outside_fed_chain_once() -> Result<(), Error>
{
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    4: meter => { 6: inv_bat => { 7: bat } },
                    5: meter,
                },
                3: meter => { 6, 8: meter },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 - COALESCE(#3 + #4 - #8, #6, 0.0)"
    );

    Ok(())
}

/// The chains under a replaced meter all become targets, not just the one
/// the overlap was found through. Meter:2 also measures BatteryInverter:6,
/// which has no second feed and would otherwise go unsubtracted.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_every_chain_under_a_replaced_meter()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    4: inv_bat => { 5: bat },
                    6: inv_bat => { 7: bat },
                },
                3: meter => { 4, 8: meter },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 - COALESCE(#2 + #3 - #8, COALESCE(#4, 0.0) + COALESCE(#6, 0.0))"
    );

    Ok(())
}

/// Two independent overlaps are both resolved, not just the first.
#[test]
fn test_consumer_formula_with_grid_meter_resolves_every_overlap() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                5: meter => { 3, 6: meter },
                7: meter => { 8: inv_bat => { 9: bat } },
                10: meter => { 8, 11: meter },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 - COALESCE(#2 + #5 - #6, #3, 0.0) - COALESCE(#7 + #10 - #11, #8, 0.0)"
    );

    Ok(())
}

/// A chain that reports nothing cannot replace the meter above it: the
/// meter's reading is the only measurement left, so it stays the target.
/// BatteryInverter:3 provides no telemetry, and no feed can stand in for
/// it — Meter:1 is the grid meter — so its own term would be a plain 0.0.
#[test]
fn test_consumer_formula_with_grid_meter_keeps_the_meter_of_a_silent_chain() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat [ControlOnly] => { 4: bat } },
                3,
                5: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 - COALESCE(#2, 0.0)"
    );

    Ok(())
}

/// A chain that is silent from its meter down cannot be subtracted at
/// all: no reading can be built from any of it, so no term is emitted.
/// Battery:5 still reports, but no term reaches its reading through the
/// two silent components above it.
#[test]
fn test_consumer_formula_with_grid_meter_emits_no_term_for_a_wholly_silent_chain()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: meter [ControlOnly] => { 4: inv_bat [ControlOnly] => { 5: bat } },
                },
                4,
                6: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "#1");

    Ok(())
}

/// A chain fed through two parallel meters, one silent: the silent
/// meter's term would be a plain 0.0, and the reporting meter reads only
/// its own feed. The chain's own reading covers both feeds, so it is the
/// term — the same replacement the wholly-silent case makes, in sibling
/// form.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_a_silently_fed_chain_by_its_reading()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv },
                3: meter [ControlOnly] => { 4 },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 - COALESCE(#4, 0.0)"
    );

    Ok(())
}

/// A silent parallel meter that also feeds a chain of its own is still
/// replaced: its reading covers nothing, so at best it reaches the
/// chains only it feeds. Both chains come out through their own
/// readings; the shared one covers both feeds.
#[test]
fn test_consumer_formula_with_grid_meter_replaces_a_silent_meter_that_still_reads_a_chain()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv },
                3: meter [ControlOnly] => { 4, 5: inv_pv },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 - COALESCE(#4, 0.0) - COALESCE(#5, 0.0)"
    );

    Ok(())
}

/// Without a grid meter, a feeder meter's reading covers the whole
/// feeder, so a battery chain under it is subtracted back out. The
/// formula matches the shape the grid-meter path produces.
///
/// PV:1 is unmetered, which is what selects the no-grid-meter path.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_battery_chain() -> Result<(), Error> {
    let g = graph! {
        0: grid => {
            1: inv_pv,
            2: meter => {
                3: meter => { 4: inv_bat => { 5: bat } },
                6: meter,
            },
        },
    };

    let graph = g.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#2 - COALESCE(#3, #4, 0.0)"
    );

    // Without fallbacks the chain is subtracted through its meter alone.
    let no_fallback = g.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(no_fallback.consumer_formula()?.to_string(), "#2 - #3");

    Ok(())
}

/// Every feeder meter is summed and every chain under one is
/// subtracted, each through its own meter.
///
/// PV:9 is unmetered, which is what selects the no-grid-meter path:
/// without it both feeder meters are grid meters and the graph takes
/// the grid-meter path instead.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_chain_per_feeder() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
            5: meter => {
                6: meter => { 7: inv_pv },
                8: meter,
            },
            9: inv_pv,
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 + #5 - COALESCE(#2, #3, 0.0) - COALESCE(#6, #7, 0.0)"
    );

    Ok(())
}

/// A chain that hangs off the grid rather than under a feeder meter is
/// never part of the sum, so it must not be subtracted either. It is
/// fed by the grid, not by a kept meter, so `reached_only_through`
/// rejects it.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_chain_outside_the_sum() -> Result<(), Error> {
    let graph = graph! { 0: grid => { 1: inv_pv, 2: meter => { 3: meter } } }.build(None)?;
    // #1 is a PV chain, but it sits outside every feeder meter.
    assert_eq!(graph.consumer_formula()?.to_string(), "#2");

    Ok(())
}

/// A chain fed from outside the summed meters stays counted. Only part
/// of BatteryInverter:2's throughput passes through Meter:1, so
/// subtracting its whole reading would take out power the sum never
/// added.
///
/// Meter:4 is a battery meter, so it is not a grid meter and the
/// no-grid-meter path runs.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_chain_with_an_outside_feed() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: inv_bat => { 3: bat },
                5: meter,
            },
            4: meter => { 2 },
        },
    }
    .build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "#1");

    Ok(())
}

/// A chain fed by two summed meters is subtracted once, through its own
/// meter. The chain is found once however many kept meters feed it, so
/// it gets one term and not one per feeder.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_shared_chain_once() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter => { 4: meter => { 5: inv_bat => { 6: bat } } },
            3: meter => { 4 },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#2 + #3 - COALESCE(#4, #5, 0.0)"
    );

    Ok(())
}

/// Every chain kind is subtracted: battery, PV, CHP, EV charger, wind
/// turbine and steam boiler.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_every_chain_kind() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter => {
                3: meter => { 4: inv_bat => { 5: bat } },
                6: meter => { 7: inv_pv },
                8: meter => { 9: chp },
                10: meter => { 11: ev },
                12: meter => { 13: wind },
                14: meter => { 15: boiler },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#2 - COALESCE(#3, #4, 0.0) - COALESCE(#6, #7, 0.0) \
         - COALESCE(#8, #9, 0.0) - COALESCE(#10, #11, 0.0) \
         - COALESCE(#12, #13, 0.0) - COALESCE(#14, #15, 0.0)"
    );

    Ok(())
}

/// A chain component with no meter of its own is subtracted by its own
/// reading — the "inverter wired straight to a feeder" shape.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_an_unmetered_chain() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter => {
                3: inv_bat => { 4: bat },
                5: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#2 - COALESCE(#3, 0.0)"
    );

    Ok(())
}

/// A chain under a covered meter stays counted when that meter also has
/// a feed the sum cannot see. Meter:4 is covered by Meter:2, so only
/// Meter:2 is summed, and part of the battery's power reaches Meter:4
/// through the silent Meter:3. Subtracting the chain would remove power
/// the sum never added, so the check runs against the kept meters, not
/// against every meter discovery found.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_silently_fed_chain() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter => { 4: meter => { 5: meter => { 6: inv_bat => { 7: bat } } } },
            3: meter [ControlOnly] => { 4 },
        },
    }
    .build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "#2");

    Ok(())
}

/// Two chains under one mixed meter are subtracted through that meter
/// once. Meter:3 measures a battery chain and a PV inverter, so it is
/// not a chain itself and each child becomes a target. The single
/// `aggregate_terms` call resolves the pair onto Meter:3; one call per
/// target would make each subtract the other and count Meter:3 twice.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_mixed_meter_once() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter => {
                3: meter => {
                    4: meter => { 5: inv_bat => { 6: bat } },
                    7: inv_pv,
                },
                8: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#2 - COALESCE(#3, COALESCE(#7, 0.0) + COALESCE(#4, #5, 0.0))"
    );

    Ok(())
}

/// A chain measured through a summed meter is subtracted by its own
/// reading. Meter:3 has a load of its own, so BatteryInverter:5 would
/// resolve to `#3 - #7`; subtracting that would cancel Meter:3 out of the
/// sum and drop its whole line, so the inverter's own reading stands in.
///
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4,
/// which are not grid meters.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_meter_measured_chain() -> Result<(), Error> {
    let g = graph! {
        0: grid => {
            1: inv_pv,
            2: meter [ControlOnly] => {
                3: meter => {
                    5: inv_bat => { 6: bat },
                    7: meter,
                },
                4: meter,
            },
        },
    };

    let graph = g.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#3 + #4 - COALESCE(#5, 0.0)"
    );

    // Without fallbacks the term is the inverter's own reading to begin
    // with, so there is nothing to stand in for.
    let no_fallback = g.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(no_fallback.consumer_formula()?.to_string(), "#3 + #4 - #5");

    Ok(())
}

/// A cancelling chain and a plain one are both subtracted, each from the
/// source that suits it. The battery chain under Meter:3 would resolve to
/// `#3 - #7`, so it falls back to its own reading; the PV chain under
/// Meter:4 has its own meter and keeps it.
///
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_cancelling_chain_and_a_plain_one()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter [ControlOnly] => {
                3: meter => {
                    5: inv_bat => { 6: bat },
                    7: meter,
                },
                4: meter => { 8: meter => { 9: inv_pv } },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#3 + #4 - COALESCE(#5, 0.0) - COALESCE(#8, #9, 0.0)"
    );

    Ok(())
}

/// A summed meter that measures nothing but chains is not substituted for
/// them. Meter:3 covers exactly BatteryInverter:5 and PV:7, so it would
/// normally stand in for both; using it would cancel Meter:3 out of the sum
/// and drop the loads it reads that the graph does not model. Each chain is
/// subtracted by its own reading instead.
///
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_covered_group_by_its_own_readings()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter [ControlOnly] => {
                3: meter => {
                    5: inv_bat => { 6: bat },
                    7: inv_pv,
                },
                4: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#3 + #4 - COALESCE(#5, 0.0) - COALESCE(#7, 0.0)"
    );

    Ok(())
}

/// A chain component that reports nothing cannot be subtracted. Its meter is
/// the only thing that reads it, and that meter is in the sum, so its power
/// stays counted; the reporting chain next to it still comes out.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_a_chain_that_reports_nothing() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter [ControlOnly] => {
                3: meter => {
                    5: inv_bat [ControlOnly] => { 6: bat },
                    7: inv_pv,
                    8: meter,
                },
                4: meter,
            },
        },
    }
    .build(None)?;
    // BatteryInverter:5 contributes a 0.0 term: there is no reading to take
    // out, so its draw stays in Meter:3's reading.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#3 + #4 - 0.0 - COALESCE(#7, 0.0)"
    );

    Ok(())
}

/// One off-limits parent meter is enough to refuse the group. PV:7 is fed by
/// Meter:5, which is not summed, and by Meter:4, which is; measuring it as
/// that pair minus their other children would cancel Meter:4 out of the sum.
/// Its own reading is used instead.
///
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_refuses_a_partly_off_limits_diamond() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter [ControlOnly] => {
                3: meter => {
                    5: meter => { 7: inv_pv, 8: meter },
                    6: meter,
                },
                4: meter => { 7, 9: meter },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#3 + #4 - COALESCE(#7, 0.0)"
    );

    Ok(())
}

/// A chain fed both through its own meter and from elsewhere is subtracted
/// once, by its own reading. Meter:5 only reads what reaches
/// BatteryInverter:7 through it, so its reading is not the whole chain's;
/// subtracting both would take that part out twice.
///
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_an_outside_fed_chain_once() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: inv_pv,
            2: meter [ControlOnly] => {
                3: meter => {
                    5: meter => { 7: inv_bat => { 8: bat } },
                    6: meter,
                },
                4: meter => { 7, 9: meter },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#3 + #4 - COALESCE(#7, 0.0)"
    );

    Ok(())
}

/// When the sum does not hold the whole chain, the meter above it is the
/// right term after all: it reads exactly the part the sum does hold.
/// BatteryInverter:3 is also fed by the silent Meter:6, whose power the sum
/// never added, so Meter:2 — not the inverter — is subtracted.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_partly_held_chain_through_its_meter()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                5: meter,
            },
            6: meter [ControlOnly] => { 7: meter, 3 },
            8: inv_pv,
        },
    }
    .build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "#1 + #7 - #2");

    Ok(())
}

/// A replacement chain the sum does not hold is dropped, not subtracted.
/// Meter:4 is replaced — BatteryInverter:5 below it is also fed by Meter:2
/// and must come out through its own reading — but its other chain,
/// BatteryInverter:7, is fed by the silent Meter:3 straight off the grid,
/// so the sum never held it in full and it stays counted.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_an_uncovered_chain_under_a_replaced_meter()
-> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                4: meter => {
                    5: inv_bat => { 6: bat },
                    7: inv_bat => { 8: bat },
                },
                9: meter,
            },
            2: meter => { 5, 10: meter },
            3: meter [ControlOnly] => { 7 },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "#1 + #2 - COALESCE(#5, 0.0)"
    );

    Ok(())
}

/// A grid meter that reports nothing leaves no reading to subtract from,
/// so the graph takes the summed-meters shape: the topmost reporting
/// meters below the silent grid meter measure the site. The battery
/// chain is not covered by the sum and stays out; the grid formula
/// descends to the same readings.
#[test]
fn test_consumer_formula_sums_reporting_meters_under_a_silent_grid_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter [ControlOnly] => {
                2: meter => { 3: inv_bat => { 4: bat } },
                5: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.grid_formula()?.to_string(),
        "#5 + COALESCE(#2, #3, 0.0)"
    );
    assert_eq!(graph.consumer_formula()?.to_string(), "#5");

    Ok(())
}

/// A silent grid meter next to a reporting one: the graph takes the
/// summed-meters shape, and the silent meter's line is measured by its
/// reporting children. The load behind it counts; the battery chain is
/// not covered by the sum and must not come out of it — subtracting
/// `COALESCE(#3, #4, 0.0)` would report a discharging battery as site
/// consumption.
#[test]
fn test_consumer_formula_descends_past_a_silent_grid_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter,
            2: meter [ControlOnly] => {
                3: meter => { 4: inv_bat => { 5: bat } },
                6: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.grid_formula()?.to_string(),
        "#1 + #6 + COALESCE(#3, #4, 0.0)"
    );
    assert_eq!(graph.consumer_formula()?.to_string(), "#1 + #6");

    Ok(())
}

/// An AC path that is silent to the leaves has no reading, even when a
/// DC-side battery reports: no emitted term could use that reading, so
/// both formulas answer `None` instead of a fabricated 0.0.
#[test]
fn test_consumer_formula_is_none_when_the_ac_path_is_silent() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter [Inactive] => {
                2: meter [Inactive] => { 3: inv_bat [ControlOnly] => { 4: bat } },
            },
        },
    }
    .build(None)?;
    assert_eq!(graph.grid_formula()?.to_string(), "None");
    assert_eq!(graph.consumer_formula()?.to_string(), "None");

    Ok(())
}

/// A silent grid meter with nothing below it gives the graph no reading
/// at all: the consumer answers `None` like the grid formula, not a
/// fabricated zero. A genuinely meterless graph still answers 0.0.
#[test]
fn test_consumer_formula_is_none_when_nothing_reports() -> Result<(), Error> {
    let graph = graph! { 0: grid => { 1: meter [Inactive] } }.build(None)?;
    assert_eq!(graph.grid_formula()?.to_string(), "None");
    assert_eq!(graph.consumer_formula()?.to_string(), "None");

    Ok(())
}
