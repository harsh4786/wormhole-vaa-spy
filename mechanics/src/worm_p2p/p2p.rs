#![allow(unused_variables)]
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::identity::Keypair;
use libp2p::kad::{KademliaConfig, Kademlia};
use libp2p::{relay, Transport, connection_limits, Swarm};
use libp2p::swarm::{SwarmBuilder, DialError};
use libp2p::{PeerId, kad::store::MemoryStore};
use libp2p::{
    connection_limits::ConnectionLimits,
    identity,
    swarm::NetworkBehaviour,
    quic,
    multiaddr::Multiaddr,
    gossipsub,
};
use clap::Parser;
// use tokio::sync::mpsc::{Sender, Receiver};
use wormhole_protos::modules::gossip::{SignedObservation, ObservationRequest, SignedVaaWithQuorum, SignedObservationRequest, GossipMessage, gossip_message::Message as MessageEnum};
use log::{error, info};
use std::str::FromStr;
use crossbeam_channel::{Sender, Receiver};
use ed25519_dalek::{Keypair as EdKeypair, Signer};
use prost::Message;
use sha3::{Digest,Keccak256};
// use tonic::codegen::ok;



pub const DEFAULT_PORT: usize = 8999;
pub const SIGNED_OBSERVATION_REQUEST_PREFIX: &[u8] = b"signed_observation_request|";

pub const MAINNET_BOOTSTRAP_PEERS: [&str; 2] = [
    "12D3KooWQp644DK27fd3d4Km3jr7gHiuJJ5ZGmy8hH4py7fP4FP7",
    "12D3KooWNQ9tVrcb64tw6bNs2CaNrUGPM7yRrKvBBheQ5yCyPHKC"
];

pub const TESTNET_BOOTSTRAP_PEER: &str = "12D3KooWAkB9ynDur1Jtoa97LBUp8RXdhzS5uHgAfdTquJbrbN7i";

pub const MAINNET_BOOTSTRAP_MULTIADDR: &str = "/dns4/wormhole-mainnet-v2-bootstrap.certus.one/udp/8999/quic";
pub const TESTNET_BOOTSTRAP_MULTIADDR: &str = "/dns4/wormhole-testnet-v2-bootstrap.certus.one/udp/8999/quic";

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
    obsvReqSendC: Receiver<ObservationRequest>,
    gossipSendC: Receiver<Vec<u8>>,
    signedInC: Sender<SignedVaaWithQuorum>,
    privKey: Keypair,
    gk: EdKeypair,
    networkID: &str,
    bootstrap: &str,
    nodeName: &str,
    components: Components,
)-> Result<(), Box<dyn Error>>{
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    //setup Kademlia
    let mut cfg = KademliaConfig::default();
    cfg.set_query_timeout(Duration::from_secs(5 * 60));
    let store = MemoryStore::new(local_peer_id);
    let mut kad_behaviour = Kademlia::with_config(local_peer_id, store, cfg);
    kad_behaviour.set_mode(Some(libp2p::kad::Mode::Server));

    let conn_lim =  ConnectionLimits::default();

    // message id function for gossip
    let message_id_fn = |message: &gossipsub::Message| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        gossipsub::MessageId::from(s.finish().to_string())
    };
    // Set a custom gossipsub configuration
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
        .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
        .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
        .max_transmit_size(1024)
        .build()
        .expect("Valid config");

    let mut behaviour = Behaviour{
        relay: relay::Behaviour::new(local_peer_id, Default::default()),
        kad: kad_behaviour,
        limits: connection_limits::Behaviour::new(conn_lim.with_max_established(Some(400))),
        gossip: gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .expect("Correct configuration")
    };
    
    let quic_transport = quic::async_std::Transport::new(quic::Config::new(&local_key));
    let transport =  quic_transport.map(|either_output, _| match either_output {
        (peer_id, muxer) => (peer_id, StreamMuxerBox::new(muxer)),
    }).boxed();

    let bootstrappers = bootstrap_addrs(bootstrap, &local_peer_id);
    let topic =  gossipsub::IdentTopic::new(format!("{}/{}", networkID, "broadcast"));
    behaviour.gossip.subscribe(&topic);


    let mut swarm = SwarmBuilder::with_async_std_executor(transport, behaviour, local_peer_id).build();
    swarm.listen_on(format!("/ip4/0.0.0.0/udp/{}/quic", components.port).parse()?)?;
    swarm.listen_on(format!("/ip6/::/udp/{}/quic", components.port).parse()?)?;
    
    //Change from mainnet to testnet and vice-versa here.
    for i in bootstrappers.0.iter(){
        swarm.behaviour_mut().kad.add_address(i, MAINNET_BOOTSTRAP_MULTIADDR.parse()?);
    }
    //how many successful bootstrap connections?
    let successful_connections = connect_peers(bootstrappers.0, &mut swarm).expect("no successful connections!");
    
    tokio::task::spawn(async move {
        loop{
            crossbeam_channel::select! {
                recv(gossipSendC) -> gossip_send =>{
                    if let Err(e) =  swarm.behaviour_mut().gossip.publish(topic.clone(), gossip_send.expect("failed to receive")){
                        println!("Publish Error: {}", e);
                    }
                },
                recv(obsvReqSendC) -> observation_request => {
                    let mut buf = [0u8; 512];
                    let mut slice = &mut buf[..];
                    observation_request.clone().unwrap().encode(&mut slice);
                 
                    let mut hasher = Keccak256::new();
                    hasher.update(slice);
                    let hash = hasher.finalize();
                    let signature =  gk.try_sign(&hash).expect("failed to sign hash");
                    let sreq = SignedObservationRequest {
                        observation_request: buf.to_vec(),
                        signature: signature.to_bytes().to_vec(),
                        guardian_addr: gk.public.to_bytes().to_vec()
                    };
                    let envelope = GossipMessage{
                        message: Some(MessageEnum::SignedObservationRequest(sreq))
                    };
                    let mut e_buf = [0u8; 512];
                    let mut e_slice = &mut e_buf[..];
                    let serialized_envelope = match envelope.encode(&mut e_slice){
                        Ok(()) =>  {
                            println!("{:?}", e_slice);
                        },
                        Err(e) => panic!("Failed to encode message: {:?}", e),
                    };
                    obsvReqC.send(observation_request.clone().unwrap()).expect("failed to send the request in the queue");
                    swarm.behaviour_mut().gossip.publish(topic.clone(), e_slice);

                }
             }
        }
    });

    tokio::task::spawn(async move {   

    });
    Ok(())
}


#[derive(NetworkBehaviour)]
struct Behaviour{
    relay: libp2p::relay::Behaviour,
    kad: libp2p::kad::Kademlia<MemoryStore>,
    limits: libp2p::connection_limits::Behaviour,
    gossip: gossipsub::Behaviour,
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

pub fn connect_peers(
    peers: Vec<PeerId>,
    swarm: &mut Swarm<Behaviour>,
) -> Result<usize, DialError> {
    let mut success_counter = 0usize;
    for &peer in &peers {
        match swarm.dial(peer) {
            Ok(_) => success_counter += 1,
            Err(_) => return Err(DialError::Aborted),
        }
    }
    Ok(success_counter)
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
