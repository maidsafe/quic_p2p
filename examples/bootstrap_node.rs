//! Bootstrap node which acts as a relay for other client nodes. It collects the info of multiple
//! client nodes and relays it to all remaining connected nodes, hence allows them all to connect
//! with each other.
//!
//! Usage:
//! ```
//! $ RUST_LOG=bootstrap_node=info cargo run --example bootstrap_node
//! ```

extern crate bytes;
extern crate config_file_handler;
#[macro_use]
extern crate log;
#[macro_use]
extern crate unwrap;
#[macro_use]
extern crate serde_derive;

mod common;
use common::Rpc;

use using_quinn::{Config, Event, Peer, PeerInfo, SerialisableCertificate};

use bincode;
use bytes::Bytes;
use config_file_handler::FileHandler;
use env_logger;
use serde_json;
use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Configuration for the bootstrap node
#[derive(Serialize, Deserialize)]
pub struct BootstrapNodeConfig {
    /// IP address that the bootstrap node should listen on
    ip: Ipv4Addr,
    /// Port that the bootstrap node should listen on
    port: u16,
    /// A number of expected connections.
    /// Once this number is reached, we'll send a list of all connections to every connected peer.
    expected_conns: usize,
}

impl Default for BootstrapNodeConfig {
    fn default() -> Self {
        BootstrapNodeConfig {
            ip: unwrap!("127.0.0.1".parse()),
            port: 5000,
            expected_conns: 3,
        }
    }
}

fn main() -> Result<(), io::Error> {
    env_logger::init();

    // Initialise configuration
    let cfg_file_handler = FileHandler::<BootstrapNodeConfig>::new("bootstrap.config", true)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let bootstrap_node_config = cfg_file_handler.read_file().unwrap_or_else(|_e| {
        info!("Using default bootstrap node config.");
        BootstrapNodeConfig::default()
    });

    // Initialise Peer
    let (ev_tx, ev_rx) = channel();

    let (mut peer, our_cert_der) = {
        let our_complete_cert = SerialisableCertificate::default();
        let cert_der = our_complete_cert.cert_der.clone();
        (
            Peer::with_config(
                ev_tx,
                Config {
                    our_complete_cert: Some(our_complete_cert),
                    port: Some(bootstrap_node_config.port),
                    ip: Some(IpAddr::V4(bootstrap_node_config.ip)),
                    ..Default::default()
                },
            ),
            cert_der,
        )
    };
    peer.start_listening();

    info!(
        "Bootstrap node started on port {}",
        bootstrap_node_config.port
    );

    let our_conn_info = PeerInfo {
        peer_addr: SocketAddr::from((bootstrap_node_config.ip, bootstrap_node_config.port)),
        peer_cert_der: our_cert_der,
    };

    println!(
        "Our connection info:\n{}\n",
        unwrap!(serde_json::to_string(&our_conn_info)),
    );

    let expected_connections = bootstrap_node_config.expected_conns;
    let mut connected_peers = HashMap::new();
    let mut test_triggered = false;

    for event in ev_rx.iter() {
        match event {
            Event::ConnectedTo { peer_info } => {
                let _ = connected_peers.insert(peer_info.peer_addr, peer_info);
                if connected_peers.len() == expected_connections && !test_triggered {
                    info!(
                        "{} connections collected, triggering the test",
                        expected_connections
                    );
                    let contacts: Vec<_> = connected_peers.values().cloned().collect();
                    let msg = Bytes::from(unwrap!(bincode::serialize(&Rpc::StartTest(contacts))));
                    for p in connected_peers.values() {
                        peer.send(p.clone(), msg.clone());
                    }
                    test_triggered = true;
                } else if connected_peers.len() >= expected_connections {
                    error!("More than expected connections received");
                }
            }
            event => warn!("Unexpected event: {:?}", event),
        }
    }

    Ok(())
}
