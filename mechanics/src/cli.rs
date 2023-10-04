use clap::{Parser, Subcommand};
use crate::worm_p2p::p2p::MAINNET_BOOTSTRAP_ADDRS;

#[derive(Debug, Parser)]
#[clap(author, version, about)]
pub struct TheseusArgs {
    /// starts our node
    #[clap(subcommand)]
    pub run: Run,
}
#[derive(Subcommand, Debug)]
pub enum Run {
    Run {
        // mainnet or devnet
        #[arg(short, long, default_value = "/wormhole/mainnet/2")]
        network: String,
        // list of bootstrap nodes separated by a comma
        #[arg(short, long, default_value = MAINNET_BOOTSTRAP_ADDRS)]
        bootstrap: String,
        // Listen address for gRPC interface
        #[arg(short, long, default_value = "[::]:7073")]
        spy: Option<String>,
    },
}

