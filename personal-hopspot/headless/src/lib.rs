//! Shared host attachment options for the application and bounded probe.
#[cfg(feature = "wifi-halow")]
pub mod halow;

#[cfg(feature = "wifi-auto")]
pub mod auto_wifi;
