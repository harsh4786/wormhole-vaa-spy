use log::{error, info};
use uuid::Uuid;
use std::{thread::{Builder, JoinHandle},collections::HashMap};
use wormhole_protos::modules::{
    spy::{SubscribeSignedVaaRequest, SubscribeSignedVaaResponse, 
        spy_rpc_service_server::SpyRpcService, filter_entry::Filter,
        spy_rpc_service_server::SpyRpcServiceServer, FilterEntry, SubscribeSignedObservationResponse, SubscribeSignedObservationRequest,
    
    },
    gossip::{SignedVaaWithQuorum, SignedObservation}, publicrpc::ChainId,
};
use thiserror::Error;
use tokio::sync::mpsc::{channel, error::TrySendError as TokioTrySendError, Sender as TokioSender};
use crossbeam_channel::{tick, unbounded, Receiver, RecvError, Sender, TrySendError};
use wormhole_sdk::{
    Chain,
    core::Action,
    vaa::{
        Body,
        Header,
        Vaa,
        Signature
    }, Address,
};
use tonic::{transport::Channel, Response, Status, Request, Code, Result as TonicResult};
use crate::subscription_stream::{SubscriptionStream, StreamClosedSender};

type SignedVaaSender = TokioSender<Result<SubscribeSignedVaaResponse, Status>>;
type SignedObservationSender = TokioSender<Result<SubscribeSignedObservationResponse, Status>>;
// type SignedVaaByTypeSender = TokioSender<Result<SubscribeSignedVaaByTypeResponse, Status>>;
type GeyserServiceResult<T> = Result<T, GeyserServiceError>;


#[derive(Clone)]
struct SubscriptionClosedSender {
    inner: Sender<SubscriptionClosedEvent>,
}

impl StreamClosedSender<SubscriptionClosedEvent> for SubscriptionClosedSender{
    type Error = crossbeam_channel::TrySendError<SubscriptionClosedEvent>;
    fn send(&self, event: SubscriptionClosedEvent) -> Result<(), Self::Error> {
        self.inner.try_send(event)
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum SubscriptionAddedEvent {
    SignedVAASubscription{
        uuid: Uuid,
        sender: SignedVaaSender,
        filter_type: Filter
    },
    SignedObservationSubscription{
        uuid: Uuid,
        sender: SignedObservationSender,
    }
}

struct SignedVAASubscription{
    tx: SignedVaaSender,
    filter_type: Filter,
}

struct SignedObservationSubscription{
    tx: SignedObservationSender
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
enum SubscriptionClosedEvent {
    SignedVAASubscription(Uuid),
    SignedObservationSubscription(Uuid),
}

trait ErrorStatusStreamer {
    fn stream_error(&self, status: Status) -> GeyserServiceResult<()>;
}

impl ErrorStatusStreamer for SignedObservationSubscription{
    fn stream_error(&self, status: Status) -> GeyserServiceResult<()> {
        self.tx.try_send(Err(status)).map_err(|e|{
            match e {
                TokioTrySendError::Full(_) => GeyserServiceError::NotificationReceiverFull,
                TokioTrySendError::Closed(_) => {
                    GeyserServiceError::NotificationReceiverDisconnected
                }
            }
        })
    }
}

impl ErrorStatusStreamer for SignedVAASubscription{
    fn stream_error(&self, status: Status) -> GeyserServiceResult<()> {
        self.tx.try_send(Err(status)).map_err(|e|{
            match e {
                TokioTrySendError::Full(_) => GeyserServiceError::NotificationReceiverFull,
                TokioTrySendError::Closed(_) => {
                    GeyserServiceError::NotificationReceiverDisconnected
                }
            }
        })
    }
}

#[derive(Error, Debug)]
pub enum GeyserServiceError {
    #[error("GeyserStreamMessageError")]
    GeyserStreamMessageError(#[from] RecvError),

    #[error("The receiving side of the channel is full")]
    NotificationReceiverFull,

    #[error("The receiver is disconnected")]
    NotificationReceiverDisconnected,
}


pub struct FilterSignedVaa{
   pub chain_id: ChainId,
   pub emitter_address: Address,
}

#[derive(Debug, Clone)]
pub struct SpyRpcServiceConfig{
    _filters: FilterEntry,
    subscriber_buffer_size: usize,
}
pub struct SpyRpcServiceProvider{
    config: SpyRpcServiceConfig,
    subscription_added_tx: Sender<SubscriptionAddedEvent>,

    /// Used to close existing subscriptions.
    subscription_closed_sender: SubscriptionClosedSender,
    t_hdl: JoinHandle<()>
}

impl SpyRpcServiceProvider{
    pub fn new(
        service_config: SpyRpcServiceConfig,
        signed_vaa_rx: Receiver<SubscribeSignedVaaResponse>,
        signed_obs_rx: Receiver<SubscribeSignedObservationResponse>,
    ) -> Self{
        let (subscription_added_tx, subscription_added_rx) = unbounded();
        let (subscription_closed_tx, subscription_closed_rx) = unbounded();

        let t_hdl = Self::event_loop(
            signed_vaa_rx, 
            signed_obs_rx, 
            subscription_added_rx, 
            subscription_closed_rx
        );
        Self { 
            config: service_config,
            subscription_added_tx, 
            subscription_closed_sender: SubscriptionClosedSender { inner: subscription_closed_tx }, 
            t_hdl, 
        }
    }
    
    
    
    
    fn event_loop(
        signed_vaa_updates: Receiver<SubscribeSignedVaaResponse>,
        signed_observation_updates: Receiver<SubscribeSignedObservationResponse>,
        subscription_added_rx: Receiver<SubscriptionAddedEvent>,
        subscription_closed_rx: Receiver<SubscriptionClosedEvent>,
    )-> JoinHandle<()>{
    
        Builder::new()
        .name("spy-service-event-loop".to_string())
        .spawn(move ||{
            info!("starting spy event loop!");
            let mut signed_vaa_subscriptions: HashMap<Uuid, SignedVAASubscription>= HashMap::new();
            let mut signed_obs_subscriptions: HashMap<Uuid, SignedObservationSubscription> = HashMap::new();
            loop{
                crossbeam_channel::select! {
                    recv(subscription_added_rx) -> maybe_subscription_added => {
                        info!("received new subscription");
                        if let Err(e) = Self::handle_subscription_added(maybe_subscription_added, &mut signed_vaa_subscriptions, &mut signed_obs_subscriptions) {
                            error!("error adding new subscription: {}", e);
                            return;
                        }
                    }
                    recv(subscription_closed_rx) -> maybe_subscription_closed => {
                        info!("closed subscription");
                        if let Err(e) = Self::handle_subscription_closed(maybe_subscription_closed, &mut signed_obs_subscriptions, &mut signed_vaa_subscriptions ) {
                            error!("error closing existing subscription: {}", e);
                            return;
                        }
                    }
    
                    recv(signed_vaa_updates) -> maybe_signed_vaa_update => {
                        info!("received signed VAA");
                        match Self::handle_signed_vaa_update_event(
                            maybe_signed_vaa_update,
                            &signed_vaa_subscriptions,
                        ){
                            Err(e) => {
                                error!("error handling a slot update event: {}", e);
                                return;
                            },
                            Ok(failed) => {
                                Self::drop_subscriptions(&failed, &mut signed_vaa_subscriptions);
                            }
                        }
                    }
    
                    recv(signed_observation_updates) -> maybe_obs_update => {
                        info!("received signed observation");
                        match Self::handle_signed_obs_update_event(
                            maybe_obs_update,
                            &signed_obs_subscriptions,
                        ){
                            Err(e) => {
                                error!("error handling a slot update event: {}", e);
                                return;
                            },
                            Ok(failed) => {
                                Self::drop_subscriptions(&failed, &mut signed_obs_subscriptions);
                            }
                        }
                    }
    
                }
            }
        }).unwrap()
    
    }
    
    fn handle_subscription_added(
        maybe_subscription_added: Result<SubscriptionAddedEvent, RecvError>,
        signed_vaa_subscriptions: &mut HashMap<Uuid, SignedVAASubscription>,
        signed_obs_update_subscriptions: &mut HashMap<Uuid, SignedObservationSubscription>,
    ) -> GeyserServiceResult<()> {
        let subscription_added = maybe_subscription_added?;
        info!("new subscription: {:?}", subscription_added);
        
        match subscription_added{
            SubscriptionAddedEvent::SignedVAASubscription {
                 uuid, 
                 sender, 
                 filter_type,
            } => {
                signed_vaa_subscriptions.insert(
                    uuid, 
                    SignedVAASubscription { tx: sender , filter_type}
                );
            },
    
            SubscriptionAddedEvent::SignedObservationSubscription { 
                uuid, 
                sender 
            } => {
                signed_obs_update_subscriptions.insert(
                    uuid, 
                    SignedObservationSubscription { 
                        tx: sender 
                    },
                );
            }
        }
        Ok(())
    }
    
    fn handle_subscription_closed(
        maybe_subscription_closed: Result<SubscriptionClosedEvent, RecvError>,
        signed_obs_subscriptions: &mut HashMap<Uuid, SignedObservationSubscription>,
        signed_vaa_subscriptions: &mut HashMap<Uuid, SignedVAASubscription>,
    ) -> GeyserServiceResult<()>{
        let subscription_closed = maybe_subscription_closed?;
        info!("closing subscription: {:?}", subscription_closed);
        match subscription_closed{
            SubscriptionClosedEvent::SignedObservationSubscription(i) => {
                let _ = signed_obs_subscriptions.remove(&i);
            },
            SubscriptionClosedEvent::SignedVAASubscription(i) => {
                let _ = signed_vaa_subscriptions.remove(&i);
            }
        }
        Ok(())
    }
    
    fn handle_signed_vaa_update_event(
        maybe_signed_vaa_update: Result<SubscribeSignedVaaResponse, RecvError>,
        signed_vaa_subscriptions: &HashMap<Uuid, SignedVAASubscription>,
    ) -> GeyserServiceResult<Vec<Uuid>>{
        let signed_vaa_update = maybe_signed_vaa_update?;
        let failed_subs = signed_vaa_subscriptions
        .iter()
        .filter_map(|(uuid, sub_)|{
            if matches!(
                sub_.tx.try_send(Ok(signed_vaa_update.clone())),
                Err(TokioTrySendError::Closed(_))
            ){
                Some(*uuid)
            } else {
                None
            }
        }).collect();
        Ok(failed_subs)
    }
    
    fn handle_signed_obs_update_event(
        maybe_signed_obs_update: Result<SubscribeSignedObservationResponse, RecvError>,
        signed_obs_subscriptions: &HashMap<Uuid, SignedObservationSubscription>,
    ) -> GeyserServiceResult<Vec<Uuid>>{
        let signed_obs_update = maybe_signed_obs_update?;
        let failed_subs = signed_obs_subscriptions
        .iter()
        .filter_map(|(uuid, sub_)|{
            if matches!(
                sub_.tx.try_send(Ok(signed_obs_update.clone())),
                Err(TokioTrySendError::Closed(_))
            ){
                Some(*uuid)
            } else {
                None
            }
        }).collect();
        Ok(failed_subs)
    }

    fn drop_subscriptions<S: ErrorStatusStreamer>(
        subscription_ids: &[Uuid],
        subscriptions: &mut HashMap<Uuid, S>,
    ) {
        for sub_id in subscription_ids {
            if let Some(sub) = subscriptions.remove(sub_id) {
                let _ = sub.stream_error(Status::failed_precondition("broken connection"));
            }
        }
    }

}
// async fn run_spy(){}



#[tonic::async_trait]
impl SpyRpcService for SpyRpcServiceProvider{
    type SubscribeSignedVAAStream = SubscriptionStream<Uuid, SubscribeSignedVaaResponse>;
    async fn subscribe_signed_vaa(
        &self,
        req: Request<SubscribeSignedVaaRequest>,
    ) -> Result<Response<Self::SubscribeSignedVAAStream>, Status>{
        
        let (signed_vaa_sender, signed_vaa_receiver) = channel(self.config.subscriber_buffer_size);
        let uuid = Uuid::new_v4();
       

        let create_subscription_stream_response = 
            |uuid: Uuid,
            subscription_closed_sender: &SubscriptionClosedSender| -> Result<Response<Self::SubscribeSignedVAAStream>, Status> 
        {
            let stream = SubscriptionStream::new(
                signed_vaa_receiver,
                uuid,
                (
                    subscription_closed_sender.clone(),
                    SubscriptionClosedEvent::SignedVAASubscription(uuid)
                ),
                "signed_batch_vaa_stream",
            );
            Ok(Response::new(stream))
        };
    
        for f in req.into_inner().filters.iter(){
                match &f.filter {
                    Some(Filter::BatchFilter(b)) => {
                        self.subscription_added_tx.try_send(
                            SubscriptionAddedEvent::SignedVAASubscription { 
                                uuid, 
                                sender: signed_vaa_sender.clone(), 
                                filter_type: Filter::BatchFilter(b.clone())
                            }
                        ).map_err( |e| {
                            error!(
                                "failed to add subscribe_vaa_updates subscription: {}",
                                e
                            );
                            Status::internal("error adding subscription")
                        })?;
                    },

                    Some(Filter::EmitterFilter(e)) => {
                        self.subscription_added_tx.try_send(
                            SubscriptionAddedEvent::SignedVAASubscription { 
                                uuid, 
                                sender: signed_vaa_sender.clone(), 
                                filter_type: Filter::EmitterFilter(e.clone())
                            }
                        ).map_err( |e| {
                            error!(
                                "failed to add subscribe_vaa_updates subscription: {}",
                                e
                            );
                            Status::internal("error adding subscription")
                        })?;
                    },

                    Some(Filter::BatchTransactionFilter(t)) => {
                        self.subscription_added_tx.try_send(
                            SubscriptionAddedEvent::SignedVAASubscription { 
                                uuid, 
                                sender: signed_vaa_sender.clone(), 
                                filter_type: Filter::BatchTransactionFilter(t.clone())
                            }
                        ).map_err( |e| {
                            error!(
                                "failed to add subscribe_vaa_updates subscription: {}",
                                e
                            );
                            Status::internal("error adding subscription")
                        })?;
                    },
                    None => error!("No filters found: Invalid filter type")
                }
        
        };

        Ok(create_subscription_stream_response(uuid, &self.subscription_closed_sender)?)
    }

    type SubscribeSignedObservationsStream = SubscriptionStream<Uuid,SubscribeSignedObservationResponse>;
    async fn subscribe_signed_observations(
        &self,
        _req: Request<SubscribeSignedObservationRequest>,
    ) -> Result<Response<Self::SubscribeSignedObservationsStream>, Status>{
        let (signed_obs_sender, signed_obs_receiver) = channel(self.config.subscriber_buffer_size);
        let uuid = Uuid::new_v4();

        self.subscription_added_tx.try_send(
            SubscriptionAddedEvent::SignedObservationSubscription { 
                uuid,
                sender: signed_obs_sender,
            }
        ).map_err( |e| {
            error!(
                "failed to add subscribe_vaa_updates subscription: {}",
                e
            );
            Status::internal("error adding subscription")
        })?;

        let create_subscription_stream_response = 
            |uuid: Uuid,
            subscription_closed_sender: &SubscriptionClosedSender| -> Result<Response<Self::SubscribeSignedObservationsStream>, Status> 
            {
                let stream = SubscriptionStream::new(
                    signed_obs_receiver,
                    uuid,
                    (
                        subscription_closed_sender.clone(),
                        SubscriptionClosedEvent::SignedObservationSubscription(uuid)
                    ),
                    "signed_observation_stream",
                );
                Ok(Response::new(stream))
            };

        Ok(create_subscription_stream_response(uuid, &self.subscription_closed_sender)?)
    }
}


