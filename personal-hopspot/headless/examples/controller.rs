//! Persistent lab controller exercising the public, identity-authenticated API.
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use personal_hopspot_headless::control::{operator_requests, public_identity};
use personal_rns::identity::PublicIdentityMaterial;
use personal_rns::interfaces::ConnectionState;
use personal_rns::prelude::*;
use personal_rns::remote_control::RemoteControlControllerAuthority;
use personal_rns::runtime::{RemoteControlIdentityDirectory, RemoteControlTargetAccessControl};

#[derive(Parser)]
struct Options {
    #[arg(long)]
    state_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create/load the controller identity and print only its public provisioning data.
    Identity,
    /// Authenticate to an explicitly provisioned target and perform one operation.
    Invoke {
        #[arg(long)]
        tcp: SocketAddr,
        #[arg(long, value_parser = public_identity)]
        target_key: PublicIdentityMaterial,
        #[arg(long, value_enum)]
        action: Action,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Action {
    Describe,
    Announce,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Identity(#[from] personal_rns::runtime::RemoteControlFileIdentityBootstrapError),
    #[error("controller state: {0}")]
    Io(#[from] std::io::Error),
    #[error("controller identity is already in use: {0}")]
    Lock(std::fs::TryLockError),
    #[error("target grant: {0:?}")]
    Grant(RemoteControlTargetAccessError),
    #[error("target provisioning: {0:?}")]
    Provision(personal_rns::runtime::SetRemoteControlTargetAccessControlError),
    #[error("target path: {0:?}")]
    Path(personal_rns::runtime::RequestPathError),
    #[error("target connection: {0:?}")]
    Connect(personal_rns::runtime::ConnectRemoteControlTargetError),
    #[error("remote operation: {0:?}")]
    Operation(personal_rns::runtime::RemoteControlTargetOperationError),
    #[error("controller exceeded 45 seconds")]
    Timeout,
    #[error("controller node stopped: {0:?}")]
    Node(Result<(), personal_rns::runtime::NodeRunError>),
}

async fn run(options: Options) -> Result<(), Error> {
    std::fs::create_dir_all(&options.state_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&options.state_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(options.state_dir.join("controller.lock"))?;
    lock.try_lock().map_err(Error::Lock)?;
    let (secrets, _) = RemoteControlIdentityDirectory::new(options.state_dir.join("identity"))
        .load_or_generate()?
        .into_parts();
    let identities = secrets.identities();
    println!(
        "controller_identity hash={} public_key={}",
        hex::encode(identities.controller().identity_hash().as_bytes()),
        hex::encode(identities.controller().public_keys().public_key_bytes())
    );
    let Command::Invoke {
        tcp,
        target_key,
        action,
    } = options.command
    else {
        return Ok(());
    };
    let target = RemoteControlTargetIdentity::new(target_key.public_keys());
    let endpoint = target.endpoint().destination_hash();
    let target_hash = target.identity_hash();
    let grant = RemoteControlTargetAccess::new(
        target,
        RemoteControlControllerAuthority::Operator,
        operator_requests(),
    )
    .map_err(Error::Grant)?;
    let interface = TcpClientInterface::new(tcp.to_string());
    let status = interface.status();
    let node = PrnsNode::new(PrnsNodeRecipe {
        transport_identity: None,
        remote_control: RemoteControlService::new(
            secrets,
            RemoteControlInitialControllerGrants::Nobody,
            RemoteControlSelfAnnouncement::Unavailable,
        ),
        pre_configured_destinations: [] as [PreConfiguredDestination<'static>; 0],
        app_state: personal_rns::runtime::NoRemoteControlHostControls,
        storage: GrowableHeap,
        request_endpoints: request_endpoints![],
        interfaces: ManuallyAttached,
        persistence: NoPersistence,
        on_event: |event, _state| {
            if let PrnsEvent::Diagnostic(Diagnostic::AnnounceHeard { destination, .. }) = event {
                println!(
                    "controller_heard_announce destination={}",
                    hex::encode(destination.as_bytes())
                );
            }
        },
    });
    let handle = node.handle();
    handle.add_interface(interface);
    let operation = async {
        while status.connection() != ConnectionState::Connected {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handle
            .set_remote_control_target_access(grant)
            .await
            .map_err(Error::Provision)?;
        handle.request_path(endpoint).await.map_err(Error::Path)?;
        let connection = handle
            .connect_remote_control_target(target_hash)
            .await
            .map_err(Error::Connect)?;
        match action {
            Action::Describe => {
                let (description, rtt) = connection.describe().await.map_err(Error::Operation)?;
                println!("controller_described {description:?} rtt={rtt:?}");
            }
            Action::Announce => {
                let rtt = connection.announce_self().await.map_err(Error::Operation)?;
                println!("controller_announced rtt={rtt:?}");
            }
        }
        connection.close();
        Ok(())
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(45), operation) => result.map_err(|_| Error::Timeout)?,
        result = node.run() => Err(Error::Node(result)),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    match run(Options::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("controller_failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
