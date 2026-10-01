#![cfg(feature = "controlled-time")]
#![expect(
    clippy::expect_used,
    reason = "bounded validation assertions fail at their owning invariant"
)]

mod fixture;
mod inspection;
mod watch;

mod authorization;

mod faults;

mod durability;

mod authority;

mod revocation;

mod pressure;
