use log::error;
use tokio::task::JoinHandle;
use uuid::Uuid;
use wormhole_protos::modules::{
    spy::{SubscribeSignedVaaRequest, SubscribeSignedVaaResponse, 
        spy_rpc_service_server::SpyRpcService, filter_entry::{Filter, self},
        spy_rpc_service_server::SpyRpcServiceServer, FilterEntry, SubscribeSignedObservationResponse, SubscribeSignedObservationRequest,
    
    },
    gossip::SignedVaaWithQuorum, publicrpc::ChainId,
};
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
type SignedVaaByTypeSender = TokioSender<Result<SubscribeSignedVaaByTypeResponse, Status>>;

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
enum SubscriptionAddedEvent {
    SignedVAASubscription{
        uuid: Uuid,
        signed_vaa_sender: SignedVaaSender,
        filter_type: Filter
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
enum SubscriptionClosedEvent {
    SignedVAASubscription(Uuid),
}

pub struct FilterSignedVaa{
   pub chain_id: ChainId,
   pub emitter_address: Address,
}

#[derive(Debug, Clone)]
pub struct SpyRpcServiceConfig{
    filters: FilterEntry,
    subscriber_buffer_size: usize,
}
pub struct SpyRpcServiceProvider{
    config: SpyRpcServiceConfig,
    subscription_added_tx: Sender<SubscriptionAddedEvent>,

    /// Used to close existing subscriptions.
    subscription_closed_sender: SubscriptionClosedSender,
    t_hdl: JoinHandle<()>
}

async fn run_spy(){}



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
                                signed_vaa_sender, 
                                filter_type: Filter::BatchFilter(*b)
                            }
                        ).map_err( |e| {
                            error!(
                                "failed to add subscribe_vaa_updates subscription: {}",
                                e
                            );
                            Status::internal("error adding subscription")
                        });
                    },

                    Some(Filter::EmitterFilter(e)) => {
                        self.subscription_added_tx.try_send(
                            SubscriptionAddedEvent::SignedVAASubscription { 
                                uuid, 
                                signed_vaa_sender, 
                                filter_type: Filter::EmitterFilter(*e)
                            }
                        ).map_err( |e| {
                            error!(
                                "failed to add subscribe_vaa_updates subscription: {}",
                                e
                            );
                            Status::internal("error adding subscription")
                        });
                    },

                    Some(Filter::BatchTransactionFilter(t)) => {
                        self.subscription_added_tx.try_send(
                            SubscriptionAddedEvent::SignedVAASubscription { 
                                uuid, 
                                signed_vaa_sender, 
                                filter_type: Filter::BatchTransactionFilter(*t)
                            }
                        ).map_err( |e| {
                            error!(
                                "failed to add subscribe_vaa_updates subscription: {}",
                                e
                            );
                            Status::internal("error adding subscription")
                        });
                    },
                    None => error!("No filters found: Invalid filter type")
                }
        
        };

        Ok(create_subscription_stream_response(uuid, &self.subscription_closed_sender)?)
    }

    type SubscribeSignedObservationsStream = SubscriptionStream<Uuid,SubscribeSignedObservationResponse>;
    async fn subscribe_signed_observations(
        &self,
        req: Request<SubscribeSignedObservationRequest>,
    ) -> Result<Response<Self::SubscribeSignedObservationsStream>, Status>{

        Ok()
    }
}
