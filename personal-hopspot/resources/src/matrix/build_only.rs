use personal_hopspot_builder::platform::nrf52840::firmware;
use personal_hopspot_builder::LtoMode;
use personal_hopspot_memory::{
    MemoryProfile, MESH_TOWER_V2, MUZI_BASE_DUO, RAK10724, SENSECAP_SOLAR_NODE, WIO_TRACKER_L1,
};

const NRF52840_RUST_TARGET: &str = "thumbv7em-none-eabihf";
const NRF52840_PACKAGE: &str = "t-echo";

pub(super) struct BuildOnlyTarget {
    pub id: &'static str,
    pub display_name: &'static str,
    pub profile: &'static MemoryProfile,
    pub recipe: firmware::Recipe<'static>,
}

pub(super) const TARGETS: [BuildOnlyTarget; 5] = [
    BuildOnlyTarget {
        id: "rak10724",
        display_name: "RAK WisMesh 1W",
        profile: &RAK10724,
        recipe: firmware::Recipe {
            package: NRF52840_PACKAGE,
            binary: "rak10724",
            rust_target: NRF52840_RUST_TARGET,
            cargo_features: "board-rak10724",
            lto: LtoMode::Configured,
        },
    },
    BuildOnlyTarget {
        id: "sensecap-solar-node",
        display_name: "Seeed SenseCAP Solar Node P1/P1-Pro",
        profile: &SENSECAP_SOLAR_NODE,
        recipe: firmware::Recipe {
            package: NRF52840_PACKAGE,
            binary: "sensecap-solar-node",
            rust_target: NRF52840_RUST_TARGET,
            cargo_features: "board-sensecap-solar-node",
            lto: LtoMode::Configured,
        },
    },
    BuildOnlyTarget {
        id: "mesh-tower-v2",
        display_name: "Heltec MeshTower V2",
        profile: &MESH_TOWER_V2,
        recipe: firmware::Recipe {
            package: NRF52840_PACKAGE,
            binary: "heltec-mesh-tower-v2",
            rust_target: NRF52840_RUST_TARGET,
            cargo_features: "board-mesh-tower-v2,softdevice-s140-v6",
            lto: LtoMode::Thin,
        },
    },
    BuildOnlyTarget {
        id: "muzi-base-duo",
        display_name: "muzi Base Duo",
        profile: &MUZI_BASE_DUO,
        recipe: firmware::Recipe {
            package: NRF52840_PACKAGE,
            binary: "muzi-base-duo",
            rust_target: NRF52840_RUST_TARGET,
            cargo_features: "board-muzi-base-duo,softdevice-s140-v6",
            lto: LtoMode::Thin,
        },
    },
    BuildOnlyTarget {
        id: "wio-tracker-l1-pro-1w",
        display_name: "Seeed Wio Tracker L1 Pro 1W",
        profile: &WIO_TRACKER_L1,
        recipe: firmware::Recipe {
            package: NRF52840_PACKAGE,
            binary: "wio-tracker-l1",
            rust_target: NRF52840_RUST_TARGET,
            cargo_features: "board-wio-tracker-l1,wio-tracker-l1-pro-1w",
            lto: LtoMode::Configured,
        },
    },
];

pub(super) fn is_build_only(target_id: &str) -> bool {
    TARGETS.iter().any(|target| target.id == target_id)
}
