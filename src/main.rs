mod error;
mod options;

use crate::error::{ok, Error, Result};
use clap::Parser;
use futures::{try_join, TryStreamExt};
use options::Opts;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// The main() and main_async() are separated intentionally to perform
// implicit Error -> eyre::Error conversion.
fn main() -> eyre::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));

    let fmt_layer = tracing_subscriber::fmt::Layer::new()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_level(true)
        .with_target(true);

    let tracing_sub = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    tracing_sub.init();

    // Construct a tokio runtime and block on the main_async().
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the Runtime");
    runtime.block_on(main_async())?;
    Ok(())
}

async fn main_async() -> Result<()> {
    // Parse command line options.
    let Opts {
        key,
        r#pub,
        sub,
        lb,
        block_size,
        zenoh_opts,
    } = Opts::parse();

    // Start a Zenoh session.
    let config: zenoh::Config = zenoh_opts.into();
    let session = zenoh::open(config).await?;

    // Bail if both --pub are and --sub are not specified.
    if !r#pub && !sub {
        return Err(Error::NoPubSubOptions);
    }

    // Run subscription task
    let sub_task = async {
        if sub {
            run_subscriber(&session, &key).await?;
        }
        ok(())
    };

    // Run publicastion task
    let pub_task = async {
        if !r#pub {
            return Ok(());
        }

        // Determine the buffering mode according to command line options.
        let buffering = match (lb, block_size) {
            (false, None) => {
                if atty::is(atty::Stream::Stdin) {
                    Buffering::Lines
                } else {
                    Buffering::Block(8196)
                }
            }
            (false, Some(block_size)) => Buffering::Block(block_size.get()),
            (true, None) => Buffering::Lines,
            (true, Some(_)) => return Err(Error::InvalidBufferingOptions),
        };

        // Run publication depending on the buffering mode.
        match buffering {
            Buffering::Lines => run_publisher_lines(&session, &key).await?,
            Buffering::Block(block_size) => {
                run_publisher_blocks(&session, &key, block_size).await?
            }
        }

        ok(())
    };

    try_join!(pub_task, sub_task)?;
    Ok(())
}

/// Run a Zenoh subscription loop that reads incoming samples and
/// print them to STDOUT.
async fn run_subscriber(session: &zenoh::Session, key: &str) -> Result<()> {
    let mut stdout = tokio::io::stdout();

    let subscriber = session.declare_subscriber(key).await?;

    loop {
        let sample = subscriber.recv_async().await?;
        let payload = sample.payload();
        stdout.write_all(&payload.to_bytes()).await?;
    }
}

/// Run a Zenoh publication loop that reads STDIN in lines and publish
/// them.
async fn run_publisher_lines(session: &zenoh::Session, key: &str) -> Result<()> {
    let publisher = session.declare_publisher(key).await?;

    let stdin = tokio::io::stdin();
    let lines = BufReader::new(stdin).lines();
    let line_stream = LinesStream::new(lines);

    line_stream
        .map_err(Error::from)
        .try_fold(publisher, |publisher, mut line| async move {
            line.push('\n');
            publisher.put(line).await?;
            ok(publisher)
        })
        .await?;

    Ok(())
}

/// Run a Zenoh publication loop that reads STDIN in fixed-sized
/// blocks and publish them.
async fn run_publisher_blocks(
    session: &zenoh::Session,
    key: &str,
    block_size: usize,
) -> Result<()> {
    let publisher = session.declare_publisher(key).await?;

    let mut stdin = tokio::io::stdin();
    let mut buf = vec![0; block_size];

    loop {
        let size = stdin.read(&mut buf).await?;
        if size == 0 {
            break;
        }

        publisher.put(&buf[0..size]).await?;
    }
    Ok(())
}

/// The buffering mode when reading the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Buffering {
    Lines,
    Block(usize),
}

impl Default for Buffering {
    fn default() -> Self {
        Self::Lines
    }
}
