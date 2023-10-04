# Theseus: A Wormhole spy node implementation in Rust 
                  ![theseus](https://github.com/harsh4786/wormhole-vaa-spy/assets/50767810/0ec622e0-5e5f-433f-8384-cfe5cde7f618)

Wormhole spy nodes are the types of nodes that don't participate in consensus or VAA attestations
but listen to messages passed on by the guardian nodes to each other in the p2p network. 

Technically, by design, a typical wormhole spy node basically consists of two main processes
 - A p2p networking stack.
 - A gRPC server to serve messages to subscribers. 
