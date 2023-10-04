#![allow(unused_variables)]
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::identity::Keypair;
use libp2p::kad::{Kademlia, KademliaConfig};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{connection_limits, StreamProtocol, Transport};
use libp2p::{
    connection_limits::ConnectionLimits, dns::TokioDnsConfig, gossipsub, identity, ping, quic,
    swarm::NetworkBehaviour,
};
use libp2p::{kad::store::MemoryStore, PeerId};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use wormhole_protos::modules::gossip::{
    gossip_message::Message as MessageEnum, GossipMessage, ObservationRequest, SignedObservation,
    SignedObservationRequest, SignedVaaWithQuorum,
};
use wormhole_protos::prost::Message;
// use crossbeam_channel::{Sender, Receiver};
use ed25519_dalek::{Keypair as EdKeypair, Signer};
use sha3::{Digest, Keccak256};
use tokio::sync::mpsc::{Receiver, Sender};

use super::utils::{bootstrap_addrs, connect_peers};

pub const DEFAULT_PORT: usize = 8999;
pub const SIGNED_OBSERVATION_REQUEST_PREFIX: &[u8] = b"signed_observation_request|";
pub const MAINNET_BOOTSTRAP_ADDRS: &str = "/dns4/wormhole-mainnet-v2-bootstrap.certus.one/udp/8999/quic/p2p/12D3KooWQp644DK27fd3d4Km3jr7gHiuJJ5ZGmy8hH4py7fP4FP7";

pub const MAINNET_BOOTSTRAP_PEERS: [&str; 2] = [
    "12D3KooWQp644DK27fd3d4Km3jr7gHiuJJ5ZGmy8hH4py7fP4FP7",
    "12D3KooWNQ9tVrcb64tw6bNs2CaNrUGPM7yRrKvBBheQ5yCyPHKC",
];

pub const TESTNET_BOOTSTRAP_PEER: &str = "12D3KooWAkB9ynDur1Jtoa97LBUp8RXdhzS5uHgAfdTquJbrbN7i";

pub const MAINNET_BOOTSTRAP_MULTIADDR: &str =
    "/dns4/wormhole-mainnet-v2-bootstrap.certus.one/udp/8999/quic";
pub const TESTNET_BOOTSTRAP_MULTIADDR: &str =
    "/dns4/wormhole-testnet-v2-bootstrap.certus.one/udp/8999/quic";

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    kad: libp2p::kad::Kademlia<MemoryStore>,
    limits: libp2p::connection_limits::Behaviour,
    gossip: gossipsub::Behaviour,
    ping: ping::Behaviour,
}

#[derive(Debug, Clone)]
pub struct Components {
    pub p2p_id_in_heartbeat: bool,
    pub listening_address_patterns: Vec<String>,
    pub port: usize,
}

impl Default for Components {
    fn default() -> Self {
        Self {
            p2p_id_in_heartbeat: false,
            listening_address_patterns: vec![
                "/ip4/0.0.0.0/udp/{}/quic".to_string(),
                "/ip6/::/udp/{}/quic".to_string(),
            ],
            port: DEFAULT_PORT,
        }
    }
}
#[allow(non_snake_case)]
pub async fn run_p2p(
    obsvC: Sender<SignedObservation>,
    obsvReqC: Sender<ObservationRequest>,
    mut obsvReqSendC: Receiver<ObservationRequest>,
    mut gossipSendC: Receiver<Vec<u8>>,
    signedInC: Sender<SignedVaaWithQuorum>,
    privKey: Keypair,
    gk: Arc<EdKeypair>,
    networkID: &str,
    bootstrap: &str,
    nodeName: &str,
    components: Components,
) -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    let bootstrappers = bootstrap_addrs(bootstrap, &local_peer_id);

    //setup Kademlia
    let stream_protocol = StreamProtocol::new("/wormhole/mainnet/2");
    let mut cfg = KademliaConfig::default()
        .set_protocol_names(vec![stream_protocol])
        .to_owned();
    cfg.set_query_timeout(Duration::from_secs(5 * 60));
    let store = MemoryStore::new(local_peer_id);
    let mut kad_behaviour = Kademlia::with_config(local_peer_id, store, cfg);
    kad_behaviour.set_mode(Some(libp2p::kad::Mode::Server));

    //Change from mainnet to testnet and vice-versa here.
    for i in bootstrappers.0.iter() {
        kad_behaviour.add_address(i, MAINNET_BOOTSTRAP_MULTIADDR.parse()?);
    }
    kad_behaviour.bootstrap()?;

    let conn_lim = ConnectionLimits::default();

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

    let mut behaviour = Behaviour {
        kad: kad_behaviour,
        limits: connection_limits::Behaviour::new(conn_lim.with_max_established(Some(400))),
        gossip: gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .expect("Correct configuration"),
        ping: ping::Behaviour::new(ping::Config::new()),
    };
    let transport = {
        let mut quic_config = quic::Config::new(&local_key);

        // Wormhole guardian nodes aren't upgraded to quic-v1 at the time of this writing,
        // So an older version of quic is used here that is compatible with their guardian
        // nodes. More details can be found in the following links:
        // [draft-29 support](https://github.com/libp2p/rust-libp2p/pull/3151)
        // [IETF quic draft-29](https://datatracker.ietf.org/doc/html/draft-ietf-quic-transport-29)
        quic_config.support_draft_29 = true;
        
        let quic_transport = quic::tokio::Transport::new(quic_config);
        tokio::task::spawn_blocking(|| TokioDnsConfig::system(quic_transport))
            .await
            .expect("dns configuration failed")
            .unwrap()
            .map(|either_output, _| match either_output {
                (peer_id, muxer) => (peer_id, StreamMuxerBox::new(muxer)),
            })
            .boxed()
    };
    let topic = gossipsub::IdentTopic::new(format!("{}/{}", networkID, "broadcast"));
    behaviour.gossip.subscribe(&topic)?;
    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();

    swarm.listen_on(format!("/ip4/0.0.0.0/udp/{}/quic", components.port).parse()?)?;
    swarm.listen_on(format!("/ip6/::/udp/{}/quic", components.port).parse()?)?;

    let swarm = Arc::new(Mutex::new(swarm));
    let mut locked_swarm = swarm.lock().await;

    //how many successful bootstrap connections?
    let successful_connections =
        connect_peers(bootstrappers.0, &mut *locked_swarm).expect("no successful connections!");

    let swarm_clone1 = swarm.clone();
    tokio::task::spawn(async move {
        loop {
            let gk_cl = gk.clone();
            tokio::select! {
               Some(gossip_send) = gossipSendC.recv() => {
                   let mut lock = swarm_clone1.lock().await;
                   if let Err(e) =  lock.behaviour_mut().gossip.publish(topic.clone(), gossip_send){
                       println!("Publish Error: {}", e);
                   }
               },
               Some(observation_request) = obsvReqSendC.recv() => {
                   let ob_c = observation_request.clone();
                   let hash_and_signature = tokio::task::spawn_blocking(move || {
                       let mut buf = [0u8; 512];
                       let mut slice = &mut buf[..];
                       ob_c.clone().encode(&mut slice).expect("failed to encode");
                       let mut hasher = Keccak256::new();
                       hasher.update(slice);
                       let hash = hasher.finalize().to_vec();
                       let signature = gk_cl.try_sign(&hash).unwrap();
                       (hash, signature.to_bytes().to_vec(), buf)
                   }).await.expect("Failed to perform blocking operation");

                   let sreq = SignedObservationRequest {
                       observation_request: hash_and_signature.2.to_vec(),
                       signature: hash_and_signature.1,
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
                   obsvReqC.send(observation_request.clone()).await.expect("failed to send the request in the queue");
                   swarm_clone1.lock().await.behaviour_mut().gossip.publish(topic.clone(), e_slice).expect("failed to publish");

               }
            }
        }
    });

    let swarm_clone2 = swarm.clone();
    tokio::task::spawn(async move {
        loop {
            let mut swarm_lock = swarm_clone2.lock().await;
            tokio::select! {
                event = swarm_lock.select_next_some() => match event {
                    SwarmEvent::Behaviour(BehaviourEvent::Gossip(gossipsub::Event::Message {
                        propagation_source: peer_id,
                        message_id: id,
                        message,
                    })) => {

                        let message_data = message.data.as_slice();
                        let gossip_message = match GossipMessage::decode(message_data){
                            Ok(m) => {
                               Some(m)
                            },
                            Err(e) => {
                                eprintln!("Failed to decode GossipMessage: {}", e);
                                None
                            }
                        };

                        // Can add custom logic here to handle messages received from the p2p network.
                        match gossip_message.unwrap().message.unwrap(){
                            MessageEnum::SignedObservation(s) => {
                                println!("Received Signed Observation: {:?}", s);
                                obsvC.send(s).await.expect("failed to send signed observation");
                            },
                            MessageEnum::SignedVaaWithQuorum(s) => {
                                println!("signed VAA with quorum: {:?}", s);
                               signedInC.send(s).await.expect("failed to send signedVAA with quorum")
                            },
                            MessageEnum::SignedObservationRequest(r) => {
                                //  obsvReqC.send(r).await.expect("failed to send signed observation request");
                                println!("No guardian set so just logging: {:?}", r);
                            },
                            MessageEnum::SignedBatchVaaWithQuorum(v) =>{
                                print!("VAA: {:?}", v);
                            },
                            MessageEnum::SignedChainGovernorStatus(w) =>{
                                print!("GOVERNOR STATUS: {:?}", w);
                            },
                            MessageEnum::SignedBatchObservation(s) => {
                                println!("Signed Batch Obs: {:?}", s);
                            }
                           _ => {},
                        }

                    },
                    SwarmEvent::Behaviour(BehaviourEvent::Gossip(gossipsub::Event::Subscribed{
                        peer_id,
                        topic,
                    })) => println!(
                        "Subscribed to: '{}' from id: {peer_id}",
                        topic.to_string(),
                    ),
                    SwarmEvent::Behaviour(BehaviourEvent::Ping(event)) =>{
                        println!("still connected to: {:?}", event.peer);
                        println!{"ping status?: {:?}", event.result};
                    },
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Local node is listening on {address}");
                    }

                    _ => {}

                }
            }
        }
    });

    Ok(())
}
