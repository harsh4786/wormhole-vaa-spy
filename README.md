## Wormhole Spy Node: A Rust implementation.
Wormhole spy nodes are the types of nodes that don't participate in consensus or VAA attestations
but listen to messages passed on by the guardian nodes to each other in the p2p network. 

Technically, by design, a typical wormhole spy node basically consists of two main processes
 - A p2p networking stack.
 - A gRPC server to serve messages to subscribers. 