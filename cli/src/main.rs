use clap::{Parser, Subcommand, Arg, Command};
use wormhole_protos::modules::gossip::*;


#[derive(Debug, Parser)]
struct Args{
    // mainnet or devnet
    #[clap(long)]
    network: String,
    // p2p UDP listener port
    #[clap(long)]
    p2p_port: u16,
    // list of bootstrap nodes separated by a comma
    #[clap(long)]
    bootstrap: String,
    // Listen address for gRPC interface
    #[clap(long)]
    spy: String,
    // Timeout for sending a message to a subscriber
    #[clap(long)]
    timeout: u64
}



#[tokio::main]
async fn main() {
    let matches = Command::new("spy")
        .about("Run gossip spy client")
        .arg(Arg::new("network")
            .long("network")
            .value_name("NETWORK")
            .default_value("/wormhole/dev")
            .help("P2P network identifier"))
        .arg(Arg::new("port")
            .long("port")
            .value_name("PORT")
            .default_value("8999")
            .help("P2P UDP listener port"))
        // Define other arguments similarly
        .get_matches();

    // let p2p_network_id = matches.subcommand_matches("network").unwrap_or("/wormhole/dev");
    // let p2p_port = matches.value_of("port").unwrap_or("8999");
}