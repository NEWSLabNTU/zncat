# zncat - Command Line Replay for Zenoh

Zncat is a command line utility that transfers bidirectional byte
streams to Zenoh.

## Installation

Install the `zncat` command using `cargo`. If you don't have cargo
command already, go to [rustup.rs](https://rustup.rs) to fix it.

```sh
cargo install zncat
```

## Usage

### Get Started

`zncat` runs in publicatoin or subscription mode when proper flags are
specified.

To publish the data from an input file,

```sh
zncat --pub my_topic < infile
```

To subscribe from a key and save the bytes to an output file,

```sh
zncat --sub my_topic > outfile
```

### Buffering

By default, `zncat` reads the standard input in line buffering mode
and publish them line-by-line. You can change the behavior to read and
publish in fixed sized blocks. For exmaple, to publish data in
8192-byte blocks,

```sh
zncat -b 8192 --pub my_topic
```


### Custom Zenoh Configuration

To establish a connection to a remote Zenoh peer.

```sh
zncat -e tcp/123.123.123.123:6777 --pub my_topic
```

To listen for incoming connections from Zenoh peers,

```sh
zncat -l tcp/127.0.0.1:6777 --pub my_topic
```

A configuration for Zenoh can be provided in the command line.

```sh
zncat --config config.json5 --pub my_topic
```

More Zenoh options can be discovered by `zncat --help`.

### Min/Max Publication Rate

The `--min-rate <HZ>` and `--max-rate <HZ>` options control the
publication message rate. They are only effective when `--pub` is
specified.

- If `--max-rate <HZ>` is provided, it throttles the publication rate.

- If `--min-rate <HZ>` is provided, it eagerly publishes messages when
  input data is available.

## License

The software is distributed with a Apache 2.0 license. You can read
the [license file](LICENSE.txt).
