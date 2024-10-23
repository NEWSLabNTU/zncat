mod error;
mod options;

use crate::error::{ok, Error, Result};
use clap::Parser;
use futures::{try_join, TryStreamExt};
use options::Opts;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use zenoh::bytes::Encoding;

// The main() and main_async() are separated intentionally to perform
// implicit Error -> eyre::Error conversion.
fn main() -> eyre::Result<()> {
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
        config: zenoh_config_path,
    } = Opts::parse();

    // Determine the buffering mode according to command line
    // options. The default is line buffering.
    let buffering = match (lb, block_size) {
        (_, None) => Buffering::default(),
        (false, Some(block_size)) => Buffering::Block(block_size.get()),
        (true, Some(_)) => return Err(Error::InvalidBufferingOptions),
    };

    // Start a Zenoh session.
    let config = match zenoh_config_path {
        Some(path) => zenoh::Config::from_file(path)?,
        None => zenoh::Config::default(),
    };
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
        if r#pub {
            match buffering {
                Buffering::Lines => run_publisher_lines(&session, &key).await?,
                Buffering::Block(block_size) => {
                    run_publisher_blocks(&session, &key, block_size).await?
                }
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
    let publisher = session
        .declare_publisher(key)
        .encoding(Encoding::TEXT_PLAIN)
        .await?;

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
