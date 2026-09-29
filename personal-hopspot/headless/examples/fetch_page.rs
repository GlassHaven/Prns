//! Verify a headless host through its Reticulum TCP interface, including crypto and Resources.
use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;
use personal_hopspot_core::node_pages;
use personal_rns::interfaces::ConnectionState;
use personal_rns::prelude::*;
use personal_rns::routing::links::request::{
    write_packed_binary_header, MAX_PACKED_BINARY_HEADER_LEN,
};
use personal_rns::routing::request_handlers::RequestPathHash;
use personal_rns::storage::GrowableHeap;
use personal_rns::tcp::TcpClientInterface;
use personal_rns::wire::DestinationHash;

#[derive(Parser)]
struct Options {
    #[arg(long)]
    target: SocketAddr,
    #[arg(long)]
    destination: String,
}

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error("destination must be a 16-byte hex address: {0}")]
    Destination(#[from] hex::FromHexError),
    #[error("path discovery failed: {0:?}")]
    Path(personal_rns::runtime::RequestPathError),
    #[error("link failed: {0:?}")]
    Link(personal_rns::runtime::SendError<personal_rns::engine::EstablishLinkFailure>),
    #[error("request failed: {0:?}")]
    Request(personal_rns::runtime::SendError<personal_rns::engine::SendRequestFailure>),
    #[error("received page differs from the shared Hopspot index")]
    PageMismatch,
    #[error("local page length cannot be encoded")]
    PageLength,
    #[error("probe exceeded 60 seconds")]
    Timeout,
    #[error("probe node stopped: {0:?}")]
    Node(Result<(), personal_rns::runtime::NodeRunError>),
}

async fn probe(options: Options) -> Result<(), ProbeError> {
    let mut destination = [0; 16];
    hex::decode_to_slice(options.destination, &mut destination)?;
    let destination = DestinationHash::new(destination);
    let interface = TcpClientInterface::new(options.target.to_string());
    let status = interface.status();
    let node = PrnsNode::new(PrnsNodeRecipe {
        transport_identity: None,
        pre_configured_destinations: [] as [PreConfiguredDestination<'static>; 0],
        app_state: personal_rns::runtime::NoRemoteControlHostControls,
        storage: GrowableHeap,
        request_endpoints: request_endpoints![],
        remote_control: personal_rns::remote_control::RemoteControlService::Unavailable,
        interfaces: ManuallyAttached,
        persistence: NoPersistence,
        on_event: |_, _| {},
    });
    let handle = node.handle();
    handle.add_interface(interface);
    let conversation = async {
        while status.connection() != ConnectionState::Connected {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handle
            .request_path(destination)
            .await
            .map_err(ProbeError::Path)?;
        let link = handle
            .establish_link(destination)
            .await
            .map_err(ProbeError::Link)?;
        let (page, rtt) = handle
            .request(link, RequestPathHash::of(node_pages::INDEX_PATH), &[])
            .await
            .map_err(ProbeError::Request)?;
        let mut header = [0; MAX_PACKED_BINARY_HEADER_LEN];
        let header_len =
            write_packed_binary_header(node_pages::HOPSPOT_INDEX_PAGE.len(), &mut header)
                .map_err(|_| ProbeError::PageLength)?;
        let expected: Vec<_> = header[..header_len]
            .iter()
            .chain(node_pages::HOPSPOT_INDEX_PAGE)
            .copied()
            .collect();
        if page != expected {
            return Err(ProbeError::PageMismatch);
        }
        println!("hopspot_page_verified bytes={} rtt={rtt:?}", page.len());
        Ok(())
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(60), conversation) => result.map_err(|_| ProbeError::Timeout)?,
        result = node.run() => Err(ProbeError::Node(result)),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    match probe(Options::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hopspot_probe_failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
