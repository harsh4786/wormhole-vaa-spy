use ed25519_dalek::Keypair as EdKeypair;
use libp2p::identity::Keypair;
use std::{sync::Arc, error::Error};
pub mod worm_p2p;
use worm_p2p::p2p::run_p2p;
use crate::worm_p2p::p2p::{MAINNET_BOOTSTRAP_ADDRS, Components};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>>{
    use rand::prelude::*;
    let (obs_send, obs_recv) =  tokio::sync::mpsc::channel(1024);
    let (obs_req_send, obs_req_recv) = tokio::sync::mpsc::channel(1024);
    let (gossip_sendc, gossip_recvc) =  tokio::sync::mpsc::channel(1024);
    let (signedinc_s, signedinc_r) = tokio::sync::mpsc::channel(1024);
    let rg = rand::rngs::OsRng::default();
    let mut rng = StdRng::from_rng(rg).unwrap();
    let ed_keyp = EdKeypair::generate(&mut rng);
    let keyp = Keypair::generate_ed25519();
    
    let gk = Arc::new(ed_keyp);
            run_p2p(
                obs_send, 
                obs_req_send, 
                obs_req_recv,
                gossip_recvc, 
                signedinc_s, 
                keyp, 
                gk, 
                "/wormhole/mainnet/2", 
                MAINNET_BOOTSTRAP_ADDRS, 
                "", 
                Components::default(),
            ).await.expect("failed to run");
            
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1000)).await;
    }
}