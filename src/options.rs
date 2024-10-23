use clap::Parser;
use std::{num::NonZeroUsize, path::PathBuf};

#[derive(Debug, Clone, Parser)]
pub struct Opts {
    /// The key to the resource to publish or subscribe.
    pub key: String,

    /// Subscribe the key.
    #[clap(short = 'p', long)]
    pub r#pub: bool,

    /// Subscribe the key.
    #[clap(short = 's', long)]
    pub sub: bool,

    /// Publish input data in lines. It is the default behavior.
    #[clap(short = 'l', long)]
    pub lb: bool,

    /// Publish input data in blocks with specified size.
    #[clap(short = 'b', long)]
    pub block_size: Option<NonZeroUsize>,

    /// The configuration file for Zenoh.
    #[clap(short = 'c', long)]
    pub config: Option<PathBuf>,
}

#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Wai {
    Peer,
    Client,
    Router,
}
