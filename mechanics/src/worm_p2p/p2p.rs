#![allow(unused_variables)]
use std::error::Error;
use std::time::Duration;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::futures::future::Either;
use libp2p::identity::Keypair;
use libp2p::kad::{KademliaConfig, Kademlia};
use libp2p::{relay, Transport, connection_limits};
use libp2p::swarm::SwarmBuilder;
use libp2p::{PeerId, kad::store::MemoryStore};
use libp2p::{
    connection_limits::ConnectionLimits,
    identity,
    swarm::NetworkBehaviour,
    quic,
    multiaddr::Multiaddr,
    core::upgrade,
    tls,
};
use clap::Parser;
use tokio::sync::mpsc::{Sender, Receiver};
use wormhole_protos::modules::gossip::{SignedObservation, ObservationRequest, SignedVaaWithQuorum};
use log::{error, info};
use std::str::FromStr;
// use tonic::codegen::ok;



pub const DEFAULT_PORT: usize = 8999;

#[derive(Debug, Clone)]
pub struct Components{
    p2p_id_in_heartbeat: bool,
    listening_address_patterns: Vec<String>,
    port: usize,
}

impl Default for Components{
    fn default() -> Self {
        Self { 
            p2p_id_in_heartbeat: false, 
            listening_address_patterns: vec![
                "/ip4/0.0.0.0/udp/{}/quic".to_string(),
                "/ip6/::/udp/{}/quic".to_string(),
            ], 
            port: DEFAULT_PORT
        }
    }
}
#[allow(non_snake_case)]
async fn run_p2p(
    obsvC: Sender<SignedObservation>,
    obsvReqC: Sender<ObservationRequest>,

    signedInC: Sender<SignedVaaWithQuorum>,
    privKey: Keypair,
    networkID: &str,
    bootstrap: &str,
    nodeName: &str,
    
    

)-> Result<(), Box<dyn Error>>{
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    //setup Kademlia
    let mut cfg = KademliaConfig::default();
    cfg.set_query_timeout(Duration::from_secs(5 * 60));
    let store = MemoryStore::new(local_peer_id);
    let kad_behaviour = Kademlia::with_config(local_peer_id, store, cfg);
    
    let conn_lim =  ConnectionLimits::default();
    let behaviour = Behaviour{
        relay: relay::Behaviour::new(local_peer_id, Default::default()),
        kad: kad_behaviour,
        limits: connection_limits::Behaviour::new(conn_lim.with_max_established(Some(400)))
    };
    
    let quic_transport = quic::async_std::Transport::new(quic::Config::new(&local_key));
    let transport =  quic_transport.map(|either_output, _| match either_output {
        (peer_id, muxer) => (peer_id, StreamMuxerBox::new(muxer)),
    }).boxed();

    let swarm = SwarmBuilder::with_async_std_executor(transport, behaviour, local_peer_id).build();
    // swarm.listen_on(addr)
    
    Ok(())
}


#[derive(NetworkBehaviour)]
struct Behaviour{
    relay: libp2p::relay::Behaviour,
    kad: libp2p::kad::Kademlia<MemoryStore>,
    limits: libp2p::connection_limits::Behaviour,
}


pub fn bootstrap_addrs(
    bootstrap_peers: &str,
    self_id: &PeerId,
) -> (Vec<PeerId>, bool) {
    let mut bootstrappers = Vec::new();
    let mut is_bootstrap_node = false;

    for addr_str in bootstrap_peers.split(",") {
        if addr_str.is_empty() {
            continue;
        }

        let multi_address = match Multiaddr::from_str(addr_str) {
            Ok(addr) => addr,
            Err(e) => {
                error!("Invalid bootstrap address: {}, Error: {}", addr_str, e);
                continue;
            }
        };

        let peer_id = match multi_address.iter().find_map(|proto| {
            if let libp2p::multiaddr::Protocol::P2p(hash) = proto {
                Some(PeerId::from_multihash(hash.clone().into()))
            } else {
                None
            }
        }) {
            Some(id) => id,
            None => {
                error!("Invalid bootstrap address: {}", addr_str);
                continue;
            }
        };

        if peer_id.unwrap() == *self_id {
            info!("We're a bootstrap node");
            is_bootstrap_node = true;
            continue;
        }

        bootstrappers.push(peer_id.unwrap());
    }

    (bootstrappers, is_bootstrap_node)
}



#[derive(Debug, Parser)]
struct Args{
    // mainnet or devnet
    #[clap(long)]
    network: String,
    // p2p UDP listener port
    #[clap(long)]
    p2p_port: u16,
    // list of bootstrap nodes separated by a comma
    #[clap(long)]
    bootstrap: Vec<String>,
    // Listen address for gRPC interface
    #[clap(long)]
    spy: String,
    // Timeout for sending a message to a subscriber
    #[clap(long)]
    timeout: u64
}
