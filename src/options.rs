use clap::Parser;
use std::{
    fmt::{self, Debug, Display},
    num::NonZeroUsize,
    path::PathBuf,
};
use zenoh::{config::WhatAmI, Config};

/// Command line relay for Zenoh.
#[derive(Debug, Clone, Parser)]
pub struct Opts {
    /// The key to the resource to publish or subscribe.
    pub key: String,

    /// Publish to the key.
    #[clap(short = 'p', long)]
    pub r#pub: bool,

    /// Subscribe from the key.
    #[clap(short = 's', long)]
    pub sub: bool,

    /// Publish input data in lines. It assumes plain text encoding.
    #[clap(long)]
    pub lb: bool,

    /// Publish input data in blocks with specified size. The default
    /// block size is 8196 bytes.
    #[clap(long)]
    pub block_size: Option<NonZeroUsize>,

    /// Minimum publication rate if input data is available.
    #[clap(long)]
    pub min_rate: Option<f64>,

    /// Maximum publication rate.
    #[clap(long)]
    pub max_rate: Option<f64>,

    /// QoS priority level [default: Data].
    #[clap(long, default_value = "Data")]
    pub priority: String,

    /// Disable express mode for sending data.
    #[clap(long, default_value = "false")]
    pub express: bool,

    #[clap(flatten)]
    pub zenoh_opts: ZenohOpts,
}

#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Wai {
    Peer,
    Client,
    Router,
}

impl Display for Wai {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self, f)
    }
}

#[derive(clap::Parser, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ZenohOpts {
    /// A configuration file for Zenoh.
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,

    /// Allows arbitrary configuration changes as column-separated KEY:VALUE pairs, where:
    ///   - KEY must be a valid config path.
    ///   - VALUE must be a valid JSON5 string that can be deserialized to the expected type for the KEY field.
    ///
    /// Example: `--cfg='transport/unicast/max_links:2'`
    #[arg(long)]
    pub cfg: Vec<String>,

    /// The Zenoh session mode [default: peer].
    #[arg(short = 'm', long)]
    pub mode: Option<Wai>,

    /// Endpoints to connect to.
    #[arg(short = 'e', long)]
    pub connect: Vec<String>,

    /// Endpoints to listen on.
    #[arg(short = 'l', long)]
    pub listen: Vec<String>,

    /// Disable the multicast-based scouting mechanism.
    #[arg(long)]
    pub no_multicast_scouting: bool,

    /// Enable shared-memory feature.
    #[arg(long)]
    pub enable_shm: bool,
}

impl From<ZenohOpts> for Config {
    fn from(value: ZenohOpts) -> Self {
        (&value).into()
    }
}

impl From<&ZenohOpts> for Config {
    fn from(args: &ZenohOpts) -> Self {
        let mut config = match &args.config {
            Some(path) => Config::from_file(path).unwrap(),
            None => Config::default(),
        };
        match args.mode {
            Some(Wai::Peer) => config.set_mode(Some(WhatAmI::Peer)),
            Some(Wai::Client) => config.set_mode(Some(WhatAmI::Client)),
            Some(Wai::Router) => config.set_mode(Some(WhatAmI::Router)),
            None => Ok(None),
        }
        .unwrap();
        if !args.connect.is_empty() {
            config
                .connect
                .endpoints
                .set(args.connect.iter().map(|v| v.parse().unwrap()).collect())
                .unwrap();
        }
        if !args.listen.is_empty() {
            config
                .listen
                .endpoints
                .set(args.listen.iter().map(|v| v.parse().unwrap()).collect())
                .unwrap();
        }
        if args.no_multicast_scouting {
            config.scouting.multicast.set_enabled(Some(false)).unwrap();
        }
        if args.enable_shm {
            #[cfg(feature = "shared-memory")]
            config.transport.shared_memory.set_enabled(true).unwrap();
            #[cfg(not(feature = "shared-memory"))]
            {
                eprintln!("`--enable-shm` argument: SHM cannot be enabled, because Zenoh is compiled without shared-memory feature!");
                std::process::exit(-1);
            }
        }
        for json in &args.cfg {
            if let Some((key, value)) = json.split_once(':') {
                if let Err(err) = config.insert_json5(key, value) {
                    eprintln!("`--cfg` argument: could not parse `{json}`: {err}");
                    std::process::exit(-1);
                }
            } else {
                eprintln!("`--cfg` argument: expected KEY:VALUE pair, got {json}");
                std::process::exit(-1);
            }
        }
        config
    }
}
