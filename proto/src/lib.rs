pub use prost;
pub use tonic;

// pub mod gossip{
//     tonic::include_proto!("gossip.v1");
// }

pub mod annotations{
    tonic::include_proto!("google.api");
}
// pub mod spy{
//     tonic::include_proto!("spy");
// }
pub mod modules{
    tonic::include_proto!("mod");
}