use super::*;
use std::time::Instant;

const INPUTS: Inputs = Inputs {
    scheduling: ManualTaskScheduling::Seeded {
        seed: SimulationSeed::new(7),
    },
    host_seed: 11,
    payload_marker: 42,
};

#[test]
fn thirty_two_nodes_replay_concurrent_traffic_and_selective_recovery() {
    let expected = run::<32>(INPUTS);
    for _ in 0..3 {
        assert_replay(&run::<32>(INPUTS), &expected);
    }
}

#[test]
#[ignore = "manual full-node BLE scaling probe; wall time is evidence, not a CI threshold"]
fn measure_paired_ble_fleet_scaling() {
    measure::<8>();
    measure::<32>();
    measure::<128>();
}

fn measure<const NODES: usize>() {
    const SAMPLES: usize = 5;
    let expected = run::<NODES>(INPUTS);
    let responses: usize = expected.responses.iter().map(Vec::len).sum();
    assert_eq!(responses, 3 * (NODES / 2) - 1);
    let wire_bytes: usize = expected
        .wire
        .values
        .iter()
        .map(|value| value.bytes.len())
        .sum();
    let mut elapsed = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let actual = run::<NODES>(INPUTS);
        elapsed.push(started.elapsed());
        assert_replay(&actual, &expected);
    }
    elapsed.sort();
    let median = elapsed[SAMPLES / 2];
    println!(
        "nodes={NODES} pairs={} responses={responses} wire_values={} wire_payload_bytes={wire_bytes} discovery_events={} samples={SAMPLES} min_ms={:.3} median_ms={:.3} max_ms={:.3} responses_per_wall_second={:.1}",
        NODES / 2,
        expected.wire.values.len(),
        expected.discovery.events.len(),
        elapsed[0].as_secs_f64() * 1000.0,
        median.as_secs_f64() * 1000.0,
        elapsed[SAMPLES - 1].as_secs_f64() * 1000.0,
        responses as f64 / median.as_secs_f64(),
    );
}
