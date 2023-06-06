# Phoenix Channels

This crate implements a Phoenix Channels (v2) client in Rust.

## Status

**NOTE:** This client is still a work-in-progress, though it has enough features to support many
use cases already. The following is a list of known missing features:

- [ ] Reconnect on error/disconnect
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
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::broadcast;
use url::Url;

use phoenix_channels_client::{Event, EventPayload, Socket};

#[tokio::main]
async fn main() {
    // URL with params for authentication
    let url = Url::parse_with_params(
        "ws://127.0.0.1:9002/socket/websocket",
        &[("shared_secret", "supersecret")],
    ).unwrap();

    // Create a socket
    let socket = Socket::spawn(url).await.unwrap();

    // Connect the socket
    socket.connect(Duration::from_secs(10)).await.unwrap();

    // Create a channel with no params
    let topic = "channel:mytopic";
    let channel = socket.channel(topic, None).await.unwrap();
    let some_event_channel = channel.clone();
    
    // Events are received as a broadcast with the name of the event and payload associated with the event
    let mut event_receiver = channel.events();
    tokio::spawn(async move {
        loop {
            match event_receiver.recv().await {
                Ok(EventPayload { event, payload }) => match event {
                    Event::User(user_event_name) => println!("channel {} event {} sent with payload {:#?}", topic, user_event_name, payload),
                    Event::Phoenix(phoenix_event) => println!("channel {} {}", topic, phoenix_event),
                },
                Err(recv_error) => match recv_error {
                    broadcast::error::RecvError::Closed => break,
                    broadcast::error::RecvError::Lagged(lag) => {
                        eprintln!("{} events missed on channel {}", lag, topic);
                    }
                }
            }
        }
    });
    // Join the channel with a 15 second timeout
    channel.join(Duration::from_secs(15)).await.unwrap();
    
    // Send a message, waiting for a reply until timeout
    let reply_payload = channel.call("send_reply", json!({ "name": "foo", "message": "hi"}), Duration::from_secs(5)).await.unwrap();

    // Send a message, not waiting for a reply
    channel.cast("send_noreply", json!({ "name": "foo", "message": "jeez"})).await.unwrap();

    // Leave the channel
    channel.leave().await.unwrap();
    
    // Disconnect the socket
    socket.disconnect().await.unwrap();
}
```

## Contributing

Contributions are welcome! Before starting work on any big PRs, it is recommended you open an issue
to discuss the work with the maintainers, or you may risk wasting your time implementing something that
is already being worked on!

## License

Apache 2.0
