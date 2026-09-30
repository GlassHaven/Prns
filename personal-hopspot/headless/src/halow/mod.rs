//! Attachment only: OpenWrt retains ownership of radio settings.
use personal_rns::interfaces::wifi_halow::InstanceTag;
use std::num::{NonZeroU32, NonZeroU8};
use std::time::Duration;

#[derive(Debug, clap::Args)]
pub struct HaLowOptions {
    /// Already configured Linux HaLoW network device (requires CAP_NET_RAW).
    #[arg(long, requires = "halow_scope")]
    pub halow_device: Option<String>,
    /// Stable local radio identity, 1–64 bytes; preserve across device renames.
    #[arg(long, requires = "halow_device", value_parser = scope)]
    pub halow_scope: Option<String>,
    /// Maximum admitted MAC peers; each owns bounded runtime and receive queues.
    #[arg(long, default_value = "16", requires = "halow_device")]
    pub halow_peers: NonZeroU8,
    /// Radio-only discovery announces; peers expire after three intervals idle.
    #[arg(long, default_value = "300", requires = "halow_device", value_parser = announce_seconds)]
    pub halow_announce_seconds: NonZeroU32,
}
fn scope(value: &str) -> Result<String, &'static str> {
    InstanceTag::new(value.as_bytes()).map_err(|_| "scope must contain 1–64 bytes")?;
    Ok(value.to_owned())
}
fn announce_seconds(value: &str) -> Result<NonZeroU32, &'static str> {
    let value: NonZeroU32 = value.parse().map_err(|_| "expected positive seconds")?;
    if value.get() > 86_400 {
        return Err("announce interval must be at most one day");
    }
    Ok(value)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[cfg(not(target_os = "linux"))]
    #[error("HaLoW attachment is supported only on Linux")]
    Unsupported,
    #[error("HaLoW requires both device and stable scope")]
    Incomplete,
    #[cfg(target_os = "linux")]
    #[error("invalid HaLoW scope: {0:?}")]
    Scope(personal_rns::interfaces::wifi_halow::InstanceTagError),
    #[cfg(target_os = "linux")]
    #[error("cannot bind HaLoW device: {0}")]
    Bind(std::io::Error),
    #[cfg(target_os = "linux")]
    #[error("invalid experimental EtherType")]
    Protocol,
}

pub struct Prepared {
    #[cfg(target_os = "linux")]
    radio: personal_rns::wifi_halow::HaLow<personal_rns::wifi_halow::HaLowSocket>,
    broadcast: personal_rns::interfaces::InterfaceId,
    interval: Duration,
}
impl HaLowOptions {
    pub fn prepare(&self) -> Result<Option<Prepared>, Error> {
        let (device, scope) = match (&self.halow_device, &self.halow_scope) {
            (None, None) => return Ok(None),
            (Some(device), Some(scope)) => (device, scope),
            _ => return Err(Error::Incomplete),
        };
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (device, scope);
            Err(Error::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            use personal_rns::interfaces::{BitrateBps, InterfaceId, InterfaceKind};
            use personal_rns::wifi_halow::{EtherType, HaLow, HaLowLimits, HaLowSocket};
            let scope = InstanceTag::new(scope.as_bytes()).map_err(Error::Scope)?;
            let protocol = EtherType::new(0x88b6).map_err(|_| Error::Protocol)?;
            let socket = HaLowSocket::bind(device, protocol).map_err(Error::Bind)?;
            let broadcast = InterfaceId::from_channel_tag(
                InterfaceKind::WifiHaLowBroadcast,
                &scope.channel_tag(),
            );
            // Conservative estimates from the 8 MHz/MCS2 lab profile, not PHY rates.
            let peer_policy = personal_rns::interfaces::wifi_halow::policy_for_bitrate(
                BitrateBps::guess(7_300_000),
            );
            let broadcast_policy = personal_rns::interfaces::wifi_halow::policy_for_bitrate(
                BitrateBps::guess(4_000_000),
            );
            let idle_seconds = self
                .halow_announce_seconds
                .saturating_mul(NonZeroU32::new(3).unwrap());
            Ok(Some(Prepared {
                radio: HaLow::new(
                    socket,
                    scope,
                    peer_policy,
                    broadcast_policy,
                    HaLowLimits {
                        peers: self.halow_peers,
                        idle_seconds,
                    },
                ),
                broadcast,
                interval: Duration::from_secs(u64::from(self.halow_announce_seconds.get())),
            }))
        }
    }
}
impl Prepared {
    pub fn attach(
        self,
        handle: &personal_rns::runtime::PrnsNodeHandle,
    ) -> (personal_rns::interfaces::InterfaceId, Duration) {
        #[cfg(target_os = "linux")]
        handle.supervise(self.radio);
        #[cfg(not(target_os = "linux"))]
        let _ = handle;
        (self.broadcast, self.interval)
    }
}

#[cfg(test)]
mod tests;
