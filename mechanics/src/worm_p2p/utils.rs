use std::{error::Error, str::FromStr};
use ed25519_dalek::{Keypair, Signer};
use libp2p::{PeerId, Multiaddr, Swarm, swarm::DialError};
use log::{info, error};
use sha3::{Keccak256, Digest};

use super::p2p::Behaviour;


pub fn hash_and_sign(
    slice: &[u8],
    gk: &Keypair,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn Error>> {
    let mut hasher = Keccak256::new();
    hasher.update(slice);
    let hash = hasher.finalize().to_vec();
    let signature = gk.try_sign(&hash)?;
    Ok((hash, signature.to_bytes().to_vec()))
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