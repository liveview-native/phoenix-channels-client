# Phoenix Channels

This crate implements a Phoenix Channels (v2) client in Rust.

## Status

**NOTE:** This client is still a work-in-progress, though it has enough features to support many
use cases already. The following is a list of known missing features:

- [ ] Reconnect on error/disconnect
- [ ] Ability to send binary messages, receive is implemented however
- [ ] More thorough integration testing
- [ ] Add support for v1 protocol

## About

This client was built to support its use in the [LiveView Native core library](https://github.com/liveviewnative/liveview-native-core), 
which is also implemented in Rust.

The client is implemented on top of `tokio`, and is designed for the Rust async ecosystem, though it is possible to use the
client in a non-async application, with the caveat that you still have to pull in `tokio` and its dependencies to do so.

This client is brand new, so it likely has bugs and missing features. Bug reports/feature requests are welcome though, so
if you do find any issues, please let us know on the issue tracker!

## Usage

Add to your dependencies like so:

```toml
[dependencies]
phoenix_channels_client = { version = "0.1" }
```

And in your `.cargo/config.toml`, turn on unstable tokio features we need for eg. cooperative scheduling:

```toml
[build]
rustflags = ["--cfg", "tokio_unstable"]
```

You can also enable nightly features using `features = ["nightly"]`, currently this only is used to make use of a few
nightly APIs for operating on slices, which we use while parsing.

## Example

```rust
use std::time::Duration;
use serde_json::json;

use phoenix_channels_client::{Config, Client};

#[tokio::main]
async fn main() {
    // Prepare configuration for the client
    let mut config = Config::new("ws://127.0.0.1:9002/socket/websocket").unwrap();
    config.set("shared_secret", "supersecret");

    // Create a client
    let mut client = Client::new(config).unwrap();

    // Connect the client
    client.connect().await.unwrap();

    // Join a channel with no params and a timeout
    let channel = client.join("channel:mytopic", None, Some(Duration::from_secs(15))).await.unwrap();

    // Register an event handler, save the ref returned and use `off` to unsubscribe
    channel.on("some_event", |channel, payload| {
        println!("channel received {} from topic '{}'", payload, channel.topic());
    }).await.unwrap();

    // Send a message, waiting for a reply indefinitely
    let result = channel.send("send_reply", json!({ "name": "foo", "message": "hi"})).await.unwrap();

    // Send a message, waiting for a reply with an optional timeout
    let result = channel.send_with_timeout("send_reply", json!({ "name": "foo", "message": "hello"}), Some(Duration::from_secs(5))).await.unwrap();

    // Send a message, not waiting for a reply
    let result = channel.send_noreply("send_noreply", json!({ "name": "foo", "message": "jeez"})).await.unwrap();

    // Leave the channel
    channel.leave().await;
}
```

## Contributing

Contributions are welcome! Before starting work on any big PRs, it is recommended you open an issue
to discuss the work with the maintainers, or you may risk wasting your time implementing something that
is already being worked on!

## License

Apache 2.0
