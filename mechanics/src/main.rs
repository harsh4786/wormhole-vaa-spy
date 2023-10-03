use ed25519_dalek::Keypair as EdKeypair;
use libp2p::identity::Keypair;
use server::server::{SpyRpcServiceConfig, SpyRpcServiceProvider};
use std::{error::Error, sync::Arc};
use wormhole_protos::modules::{
    gossip::{SignedObservation, SignedVaaWithQuorum},
    spy::{
        spy_rpc_service_server::SpyRpcServiceServer, SubscribeSignedObservationResponse,
        SubscribeSignedVaaResponse,
    },
};
pub mod worm_p2p;
use crate::worm_p2p::p2p::{Components, MAINNET_BOOTSTRAP_ADDRS};
use crossbeam_channel::Sender as CrossbeamSender;
use serde_derive::Deserialize;
use tokio::{runtime::Runtime, sync::oneshot};
use tonic::transport::Server;
use worm_p2p::p2p::run_p2p;

pub struct Spy {
    runtime: Runtime,
    server_exit_sender: oneshot::Sender<()>,

    signed_vaa_sender: CrossbeamSender<SubscribeSignedVaaResponse>,
    signed_obs_sender: CrossbeamSender<SubscribeSignedObservationResponse>,
}

impl Spy {
    pub fn new(
        runtime: Runtime,
        server_exit_sender: oneshot::Sender<()>,
        signed_vaa_sender: CrossbeamSender<SubscribeSignedVaaResponse>,
        signed_obs_sender: CrossbeamSender<SubscribeSignedObservationResponse>,
    ) -> Self {
        Self {
            runtime,
            server_exit_sender,
            signed_vaa_sender,
            signed_obs_sender,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpyConfig {
    pub spy_service_config: SpyRpcServiceConfig,
    pub bind_address: String,
    pub signed_vaa_buffer_size: usize,
    pub signed_obs_buffer_size: usize,
}

impl SpyConfig {
    pub fn new(
        subscriber_buf_size: usize,
        signed_vaa_buf_size: usize,
        signed_obs_buf_size: usize,
        bind_address: String,
    ) -> Self {
        let spy_service_config = SpyRpcServiceConfig::new(subscriber_buf_size);
        Self {
            spy_service_config,
            bind_address,
            signed_vaa_buffer_size: signed_vaa_buf_size,
            signed_obs_buffer_size: signed_obs_buf_size,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    use rand::prelude::*;
    let (obs_send, mut obs_recv) = tokio::sync::mpsc::channel(1024);
    let (obs_req_send, obs_req_recv) = tokio::sync::mpsc::channel(1024);
    let (_gossip_sendc, gossip_recvc) = tokio::sync::mpsc::channel(1024);
    let (signedinc_s, mut signedinc_r) = tokio::sync::mpsc::channel(1024);
    let rg = rand::rngs::OsRng::default();
    let mut rng = StdRng::from_rng(rg).unwrap();
    let ed_keyp = EdKeypair::generate(&mut rng);
    let keyp = Keypair::generate_ed25519();

    let subscriber_buf_size = 1024usize;
    let signed_obs_buf_size = 1024usize;
    let signed_vaa_buf_size = 1024usize;
    let bind_address = String::from("[::]:7073");
    let spy_rpc_config = SpyRpcServiceConfig::new(subscriber_buf_size);
    let service_confg = SpyConfig::new(
        subscriber_buf_size,
        signed_vaa_buf_size,
        signed_obs_buf_size,
        bind_address.clone(),
    );

    let (signed_vaa_tx, signed_vaa_rx) =
        crossbeam_channel::bounded(service_confg.signed_vaa_buffer_size);
    let (signed_obs_tx, signed_obs_rx) =
        crossbeam_channel::bounded(service_confg.signed_vaa_buffer_size);

    //building the spy gRPC server
    tokio::spawn(async move {
        let service = SpyRpcServiceProvider::new(spy_rpc_config, signed_vaa_rx, signed_obs_rx);

        let svc = SpyRpcServiceServer::new(service);
        let runtime = Runtime::new().unwrap();
        let (server_exit_tx, server_exit_rx) = oneshot::channel();
        runtime.spawn(Server::builder().add_service(svc).serve_with_shutdown(
            bind_address.clone().parse().unwrap(),
            async move {
                let _ = server_exit_rx.await;
            },
        ));

        // This is not being used at the moment as there are no subscribing clients
        // By not being used, I mean there are no event handlers implemented for this Spy object
        // However this is not required for this demo but can implement their own event handlers
        let spy = Spy::new(runtime, server_exit_tx, signed_vaa_tx, signed_obs_tx);

        // Receive from p2p and send it through gRPC
        let signed_obs: SignedObservation = obs_recv.recv().await.expect("no observations");
        let signed_vaa: SignedVaaWithQuorum = signedinc_r
            .recv()
            .await
            .expect("failed to receive signed vaa");

        // Forming the responses
        let obs_resp = SubscribeSignedObservationResponse {
            addr: signed_obs.addr,
            hash: signed_obs.hash,
            signature: signed_obs.signature,
            tx_hash: signed_obs.tx_hash,
            message_id: signed_obs.message_id,
        };

        let vaa_resp = SubscribeSignedVaaResponse {
            vaa_bytes: signed_vaa.vaa,
        };

        //Sending them through the crossbeam channel
        spy.signed_obs_sender
            .send(obs_resp)
            .expect("failed to send signed observation: probably channel full?");
        spy.signed_vaa_sender
            .send(vaa_resp)
            .expect("failed to send vaa: probably channel full?");
    });

    // Running our p2p network
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
    )
    .await
    .expect("failed to run");

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
