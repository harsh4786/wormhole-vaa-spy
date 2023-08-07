use clap::{Parser, Subcommand, Arg, Command};







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