mod error;
mod options;

use crate::error::{ok, Error, Result};
use clap::Parser;
use futures::{try_join, TryStreamExt};
use options::Opts;
use std::ops::Bound;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    time::{sleep_until, timeout_at, Duration, Instant},
};
use tokio_stream::wrappers::LinesStream;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use zenoh::qos::Priority;

const DEFAULT_BLOCK_SIZE: usize = 8196;

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
        priority,
        express,
        min_rate,
        max_rate,
    } = Opts::parse();

    let period_range = {
        let map_rate = |rate: Option<f64>| match rate {
            Some(rate) => Bound::Included(Duration::from_secs_f64(1.0 / rate)),
            None => Bound::Unbounded,
        };
        let min_period = map_rate(max_rate);
        let max_period = map_rate(min_rate);
        (min_period, max_period)
    };

    // Start a Zenoh session.
    let config: zenoh::Config = zenoh_opts.into();
    let session = zenoh::open(config).await?;

    // Bail if both --pub are and --sub are not specified.
    if !r#pub && !sub {
        return Err(Error::NoPubSubOptions);
    }

    // Parse priority and express options
    let priority = match priority.as_str() {
        "RealTime" => Priority::RealTime,
        "InteractiveHigh" => Priority::InteractiveHigh,
        "InteractiveLow" => Priority::InteractiveLow,
        "DataHigh" => Priority::DataHigh,
        "Data" => Priority::Data,
        "DataLow" => Priority::DataLow,
        "Background" => Priority::Background,
        _ => return Err(Error::InvalidPriority),
    };

    // Run subscription task
    let sub_task = async {
        if sub {
            run_subscriber(&session, &key).await?;
        }
        ok(())
    };

    // Run publication task
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
                    Buffering::Block(DEFAULT_BLOCK_SIZE)
                }
            }
            (false, Some(block_size)) => Buffering::Block(block_size.get()),
            (true, None) => Buffering::Lines,
            (true, Some(_)) => return Err(Error::InvalidBufferingOptions),
        };

        // Run publication depending on the buffering mode.
        match buffering {
            Buffering::Lines => run_publisher_lines(&session, &key, priority, express).await?,
            Buffering::Block(block_size) => {
                run_publisher_blocks(&session, &key, block_size, priority, express, period_range)
                    .await?
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
async fn run_publisher_lines(
    session: &zenoh::Session,
    key: &str,
    priority: Priority,
    express: bool,
) -> Result<()> {
    let publisher = session
        .declare_publisher(key)
        .priority(priority)
        .express(express)
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
    priority: Priority,
    express: bool,
    (min_period, max_period): (Bound<Duration>, Bound<Duration>),
) -> Result<()> {
    let publisher = session
        .declare_publisher(key)
        .priority(priority)
        .express(express)
        .await?;

    let mut stdin = tokio::io::stdin();
    let mut buf = vec![0; block_size];
    let mut total = 0;

    // Read stdin and append the bytes to the buffer.
    macro_rules! read {
        () => {
            async {
                let size = stdin.read(&mut buf[total..]).await?;
                total += size;
                Result::<_, std::io::Error>::Ok(size > 0)
            }
        };
    }

    // Publish a message up to `total` bytes.
    macro_rules! publish {
        () => {{
            publisher.put(&buf[0..total])
        }};
    }

    // wait_until!(deadline) sleeps until the specified deadline.
    // macro_rules! wait_until {
    //     ($deadline:expr) => {{
    //         if Instant::now() < deadline {
    //             sleep_until(deadline.into()).await;
    //         }
    //     }};
    // }

    // The starting time of the current round.
    let mut round_start = Instant::now();

    'round: loop {
        let since_plus_period = |period: Bound<Duration>| -> Option<Instant> {
            match period {
                Bound::Included(period) => Some(round_start + period),
                Bound::Excluded(_) => unreachable!(),
                Bound::Unbounded => None,
            }
        };

        // The earlist time that can publish messsages
        let publish_since = since_plus_period(min_period);

        // The deadline until which the reading blocks.
        let read_deadline = since_plus_period(max_period);

        loop {
            // The result is Result<Result<bool>>.
            // - The outer Result indicates whether timeout occured or not.
            // - The inner Result gives a successful read or a I/O failure.
            // - The boolean value is true when input data is avaiable.
            let result = match read_deadline {
                Some(read_until) => timeout_at(read_until, read!()).await,
                None => Ok(read!().await),
            };

            match result {
                // Case: data available
                Ok(Ok(true)) => {
                    // Publish when the buffer is full, Otherwise go
                    // to the next round.

                    if total == block_size {
                        // Wait and publish.
                        if let Some(publish_since) = publish_since {
                            sleep_until(publish_since).await;
                        }
                        publish!().await?;

                        // Reset state variables
                        round_start = Instant::now();
                        total = 0;

                        // Go to the next round.
                        continue 'round;
                    } else {
                        // noop
                    }
                }

                // Case: stdin closed
                Ok(Ok(false)) => {
                    // Wait and publish
                    if let Some(publish_since) = publish_since {
                        sleep_until(publish_since).await;
                    }
                    publish!().await?;

                    // Stop looping because there is no more data.
                    break 'round;
                }

                // Case: I/O error
                Ok(Err(err)) => {
                    return Err(err.into());
                }

                // Case: timeout
                Err(_elapsed) => {
                    // Publish buffered data immediately.
                    if total > 0 {
                        publish!().await?;
                    }

                    round_start = Instant::now();
                    total = 0;
                    continue 'round;
                }
            };
        }
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
