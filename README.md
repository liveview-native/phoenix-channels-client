# Phoenix Channels

This crate implements a Phoenix Channels (v2) client in Rust.

## Status

**NOTE:** This client is still a work-in-progress, though it has enough features to support many
use cases already. The following is a list of known missing features:

- [ ] Add support for v1 protocol

## About

This client was built to support its use in the [LiveView Native core library](https://github.com/liveview-native/liveview-native-core),
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

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::broadcast;
use url::Url;

use phoenix_channels_client::{Event, EventsError, EventPayload, Payload, Socket, Topic};

#[tokio::main]
async fn main() {
    // URL with params for authentication
    let url = Url::parse_with_params(
        "ws://127.0.0.1:9002/socket/websocket",
        &[("shared_secret", "supersecret"), ("id", "user-id")],
    ).unwrap();

    // Create a socket
    let socket = Socket::spawn(url, None).await.unwrap();

    // Connect the socket
    socket.connect(Duration::from_secs(10)).await.unwrap();

    // Create a channel with no params
    let topic = Topic::from_string("channel:mytopic".to_string());
    let channel = socket.channel(topic.clone(), None).await.unwrap();
    let some_event_channel = channel.clone();

    // Events are received as a broadcast with the name of the event and payload associated with the event
    let mut events = channel.events();
    tokio::spawn(async move {
        loop {
            match events.event().await {
                Ok(EventPayload { event, payload }) => match event {
                    Event::User { user: user_event_name } => println!("channel {} event {} sent with payload {:#?}", topic, user_event_name, payload),
                    Event::Phoenix { phoenix } => println!("channel {} {}", topic, phoenix),
                },
                Err(events_error) => match events_error {
                    EventsError::NoMoreEvents => break,
                    EventsError::MissedEvents { missed_event_count } => {
                        eprintln!("{} events missed on channel {}", missed_event_count, topic);
                    }
                }
            }
        }
    });
    // Join the channel with a 15 second timeout
    channel.join(Duration::from_secs(15)).await.unwrap();

    // Send a message, waiting for a reply until timeout
    let reply_payload = channel.call(
        Event::from_string("reply_ok_tuple".to_string()),
        Payload::json_from_serialized(json!({ "name": "foo", "message": "hi"}).to_string()).unwrap(),
        Duration::from_secs(5)
    ).await.unwrap();

    // Send a message, not waiting for a reply
    channel.cast(
        Event::from_string("noreply".to_string()),
        Payload::json_from_serialized(json!({ "name": "foo", "message": "jeez"}).to_string()).unwrap()
    ).await.unwrap();

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
