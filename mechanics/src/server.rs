use log::error;
use tokio::task::JoinHandle;
use uuid::Uuid;
use wormhole_protos::modules::{
    spy::{SubscribeSignedVaaRequest, SubscribeSignedVaaResponse, 
        SubscribeSignedVaaByTypeRequest, SubscribeSignedVaaByTypeResponse, 
        spy_rpc_service_server::SpyRpcService, filter_entry::{Filter, self},
        spy_rpc_service_server::SpyRpcServiceServer, FilterEntry,
    
    },
    gossip::SignedVaaWithQuorum, publicrpc::ChainId,
};
use tokio::sync::mpsc::{channel, error::TrySendError as TokioTrySendError, Sender as TokioSender};
use crossbeam_channel::{tick, unbounded, Receiver, RecvError, Sender};
use wormhole_sdk::{
    Chain,
    core::Action,
    GOVERNANCE_EMITTER,
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

async fn run_spy(){

}



#[tonic::async_trait]
impl SpyRpcService for SpyRpcServiceProvider{
    type SubscribeSignedVAAStream = SubscriptionStream<Uuid, SubscribeSignedVaaResponse>;
    async fn subscribe_signed_vaa(
        &self,
        req: Request<SubscribeSignedVaaRequest>,
    ) -> Result<Response<Self::SubscribeSignedVAAStream>, Status>{
        
        let (signed_vaa_sender, signed_vaa_receiver) = channel(self.config.subscriber_buffer_size);
        let uuid = Uuid::new_v4();
        self.subscription_added_tx.try_send(
            SubscriptionAddedEvent::SignedVAASubscription { uuid, signed_vaa_sender }
        ).map_err( |e| {
            error!(
                "failed to add subscribe_vaa_updates subscription: {}",
                e
            );
            Status::internal("error adding subscription")
        });



        let s: Vec<_> = req.into_inner().filters.iter().map(|f| {
                match &f.filter {
                    Some(Filter::BatchFilter(b)) => Ok({
                        let stream= SubscriptionStream::new(
                            signed_vaa_receiver, 
                            uuid,
                            (self.subscription_closed_sender.clone(), SubscriptionClosedEvent::SignedVAASubscription(uuid)),
                             "signed_batch_vaa_stream",
                        );
                        let vec: Vec<u8> = vec![1,2,34,45,34];
                        let resp = SubscribeSignedVaaResponse { vaa_bytes: vec };
                        Response::new(stream)
                    }),
                    Some(Filter::EmitterFilter(e)) => Ok({
                        let stream= SubscriptionStream::new(
                            signed_vaa_receiver, 
                            uuid,
                            (self.subscription_closed_sender.clone(), SubscriptionClosedEvent::SignedVAASubscription(uuid)),
                             "signed_batch_vaa_stream",
                        );
                        let vec: Vec<u8> = vec![1,2,34,45,34];
                        let resp = SubscribeSignedVaaResponse { vaa_bytes: vec };
                        Response::new(stream)
                    }),
                    Some(Filter::BatchTransactionFilter(t)) => Ok({
                        let stream= SubscriptionStream::new(
                            signed_vaa_receiver, 
                            uuid,
                            (self.subscription_closed_sender.clone(), SubscriptionClosedEvent::SignedVAASubscription(uuid)),
                             "signed_batch_vaa_stream",
                            );
                        let vec: Vec<u8> = vec![1,2,34,45,34];
                        let resp = SubscribeSignedVaaResponse { vaa_bytes: vec };
                        Response::new(stream)
                    }),
                    _ => Err(Status::new(Code::InvalidArgument, "Invalid Filter type"))
                }
            
            
        }).collect();
        // let s =  req.into_inner().filters.iter().find_map(|filter_entry|{
        //     if let Some(f) =  filter_entry.filter {
                
        //     }
        // })
        // for f in req.get_ref().filters.iter(){
        //     if let Some(fentry) = f.filter{
        //         match fentry{
        //             Filter::BatchFilter(b) => {
        //                 let vec: Vec<u8> = vec![1,2,34,45,34];
        //                 let resp = SubscribeSignedVaaResponse { vaa_bytes: vec };
        //                 return  Ok(Response::new(resp))
        //             },
        //             Filter::BatchTransactionFilter(btf) =>{

        //             },
        //             Filter::EmitterFilter(ef) => {

        //             }
        //         }
        //     }
        // }
    }

    type SubscribeSignedVAAByTypeStream = SubscriptionStream<Uuid,SubscribeSignedVaaByTypeResponse>;
    async fn subscribe_signed_vaa_by_type(
        &self,
        req: Request<SubscribeSignedVaaByTypeRequest>,
    ) -> Result<Response<SubscribeSignedVaaByTypeResponse>, Status>{

        Ok()
    }
}
