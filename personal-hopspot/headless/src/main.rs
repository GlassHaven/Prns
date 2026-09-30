use std::fs::{File, OpenOptions};
use std::net::SocketAddr;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use personal_hopspot_core::{node_pages, HopspotDestinationSet, NODE_IDENTITY_STORAGE};
use personal_rns::prelude::*;
use personal_rns::runtime::{
    load_or_create_identity_secret, IdentitySecretFileError, NodePersistence,
};
use personal_rns::storage::GrowableHeap;
use personal_rns::tcp::TcpServer;

#[cfg(feature = "wifi-halow")]
use personal_hopspot_headless::halow;

const ANNOUNCE_DATA: &[u8] = b"Personal Hopspot (Headless)";

#[derive(Debug, Parser)]
#[command(version, about)]
struct Options {
    /// Dedicated private directory for this node's identity and retained state.
    #[arg(long)]
    state_dir: PathBuf,
    #[cfg(feature = "wifi-halow")]
    #[command(flatten)]
    halow: halow::HaLowOptions,
    /// Local address for the Reticulum TCP interface (not HTTP or management).
    #[arg(long)]
    listen: SocketAddr,
    #[cfg(feature = "websocket")]
    /// Plain WebSocket Reticulum listener; public transport, not administration.
    #[arg(long)]
    websocket_listen: Option<SocketAddr>,
    #[cfg(feature = "websocket")]
    /// Maximum total WebSocket sessions and handshakes.
    #[arg(long, default_value = "8", requires = "websocket_listen")]
    websocket_connections: std::num::NonZeroUsize,
    /// Stop gracefully after this many seconds; otherwise run until signalled.
    #[arg(long)]
    run_for: Option<NonZeroU64>,
}

#[derive(Debug, thiserror::Error)]
enum HostError {
    #[cfg(feature = "wifi-halow")]
    #[error(transparent)]
    HaLow(#[from] halow::Error),
    #[error("host I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("state directory is already in use or cannot be locked: {0}")]
    StateLock(std::fs::TryLockError),
    #[error("identity was refused: {0}")]
    Identity(#[from] IdentitySecretFileError),
    #[error("invalid built-in destination: {0:?}")]
    Destination(personal_rns::routing::announce::ExpandNameError),
    #[error("node stopped: {0:?}")]
    Node(personal_rns::runtime::NodeRunError),
    #[error("node did not become ready")]
    NotReady,
}

fn lock_state(directory: &Path) -> Result<File, HostError> {
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(directory.join("host.lock"))?;
    lock.try_lock().map_err(HostError::StateLock)?;
    Ok(lock)
}

async fn run(options: Options) -> Result<(), HostError> {
    #[cfg(feature = "wifi-halow")]
    let radio = options.halow.prepare()?;
    let _state_lock = lock_state(&options.state_dir)?;
    // Refuse corrupt identities rather than silently changing this node's address.
    let identity =
        load_or_create_identity_secret(&options.state_dir.join(NODE_IDENTITY_STORAGE.as_str()))?;
    let destinations = HopspotDestinationSet::new(identity.clone(), ANNOUNCE_DATA, ANNOUNCE_DATA);
    let hashes = destinations
        .destination_hashes()
        .map_err(HostError::Destination)?;
    let persistence = NodePersistence::custom_dir(options.state_dir.join("retained"))?;
    let listener = TcpServer::bind(options.listen).await?;
    let listen = listener.local_addr()?;
    #[cfg(feature = "websocket")]
    let websocket = match options.websocket_listen {
        Some(address) => Some(
            personal_rns::websocket::WebSocketServer::bind(
                address,
                personal_rns::interfaces::websocket::WEBSOCKET_BITRATE_ESTIMATE,
                personal_rns::interfaces::websocket::WebSocketFramingSelection::Fixed(
                    personal_rns::interfaces::websocket::WebSocketWireFraming::RawPacket,
                ),
            )
            .await?
            .with_connection_limit(options.websocket_connections),
        ),
        None => None,
    };
    #[cfg(feature = "websocket")]
    let websocket_address = websocket
        .as_ref()
        .map(|server| server.local_addr())
        .transpose()?;
    let node = PrnsNode::new(PrnsNodeRecipe {
        transport_identity: Some(identity),
        pre_configured_destinations: destinations.into_preconfigured_destinations(),
        app_state: personal_rns::runtime::NoRemoteControlHostControls,
        storage: GrowableHeap,
        request_endpoints: node_pages::NodePageRoutes,
        remote_control: personal_rns::remote_control::RemoteControlService::Unavailable,
        interfaces: ManuallyAttached,
        persistence,
        on_event: |event, _state: &personal_rns::runtime::NoRemoteControlHostControls| {
            if let PrnsEvent::Diagnostic(diagnostic) = event {
                match diagnostic {
                    Diagnostic::PersistenceRestored { .. }
                    | Diagnostic::PersistenceFlushed { .. }
                    | Diagnostic::PersistenceFlushFailed { .. } => eprintln!("{diagnostic:?}"),
                    _ => {}
                }
            }
        },
    });
    let handle = node.handle();
    handle.supervise(listener);
    #[cfg(feature = "websocket")]
    if let Some(websocket) = websocket {
        handle.supervise(websocket);
    }
    #[cfg(feature = "wifi-halow")]
    let radio = radio.map(|radio| radio.attach(&handle));
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = Box::pin(tokio::signal::ctrl_c());
    let shutdown = async move {
        let deadline = async {
            match options.run_for {
                Some(seconds) => tokio::time::sleep(Duration::from_secs(seconds.get())).await,
                None => std::future::pending().await,
            }
        };
        #[cfg(unix)]
        tokio::select! {
            _ = &mut interrupt => {},
            _ = terminate.recv() => {},
            () = deadline => {},
        }
        #[cfg(not(unix))]
        tokio::select! {
            _ = &mut interrupt => {},
            () = deadline => {},
        }
    };
    let task = node.run_until(shutdown);
    tokio::pin!(task);
    tokio::select! {
        snapshot = handle.engine_inspection_snapshot() => {
            if snapshot.is_none() {
                return Err(HostError::NotReady);
            }
        },
        result = &mut task => {
            result.map_err(HostError::Node)?;
            return Err(HostError::NotReady);
        },
    }
    #[cfg(feature = "wifi-halow")]
    if let Some((target, _)) = radio {
        let ready = async {
            while !handle
                .interfaces()
                .iter()
                .any(|interface| interface.id == target)
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            handle
                .engine_inspection_snapshot()
                .await
                .ok_or(HostError::NotReady)
        };
        tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(5), ready) => {
                result.map_err(|_| HostError::NotReady)??;
            }
            result = &mut task => { result.map_err(HostError::Node)?; return Err(HostError::NotReady); }
        }
    }
    println!(
        "hopspot_ready listen={listen} node_page={}",
        hex::encode(hashes.node_page.as_bytes())
    );
    #[cfg(feature = "websocket")]
    if let Some(address) = websocket_address {
        println!(
            "hopspot_websocket_ready listen={address} framing=raw connections={}",
            options.websocket_connections
        );
    }
    #[cfg(feature = "wifi-halow")]
    let task_result = {
        let announces = async {
            let Some((target, interval)) = radio else {
                return std::future::pending::<()>().await;
            };
            let mut timer = tokio::time::interval(interval);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                // Children register asynchronously. Do not announce onto TCP while
                // waiting for the radio's shared channel to become available.
                if !handle
                    .interfaces()
                    .iter()
                    .any(|interface| interface.id == target)
                {
                    continue;
                }
                for destination in [hashes.node_page, hashes.delivery] {
                    if let Err(error) = handle
                        .announce_now(AnnounceNow {
                            destination,
                            target: AnnounceTarget::Interface(target),
                            app_data: AnnounceAppData::Registered,
                        })
                        .await
                    {
                        eprintln!("halow_announce_failed: {error:?}");
                    }
                }
            }
        };
        tokio::select! {
            result = &mut task => result,
            () = announces => unreachable!("announce service runs until node shutdown"),
        }
    };
    #[cfg(not(feature = "wifi-halow"))]
    let task_result = task.await;
    task_result.map_err(HostError::Node)?;
    println!("hopspot_stopped");
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    match run(Options::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hopspot_failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_directory_has_one_writer_and_is_reusable_after_exit() {
        let directory = tempfile::tempdir().unwrap();
        let first = lock_state(directory.path()).unwrap();
        assert!(matches!(
            lock_state(directory.path()),
            Err(HostError::StateLock(_))
        ));
        drop(first);
        assert!(lock_state(directory.path()).is_ok());
    }

    #[test]
    fn explicit_state_and_listen_are_required_and_zero_duration_is_refused() {
        assert!(Options::try_parse_from(["hopspot"]).is_err());
        assert!(Options::try_parse_from([
            "hopspot",
            "--state-dir",
            "state",
            "--listen",
            "127.0.0.1:0",
            "--run-for",
            "0"
        ])
        .is_err());
    }
}
