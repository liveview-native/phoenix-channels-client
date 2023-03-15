#![cfg_attr(feature = "nightly", feature(assert_matches))]

use std::sync::Arc;
use std::time::Duration;

use phoenix_channels_client::{ChannelError, Client, Config, Payload};
use serde_json::{json, Value};
use tokio::time;

#[cfg(feature = "nightly")]
use std::assert_matches::assert_matches;

#[cfg(not(feature = "nightly"))]
macro_rules! assert_matches {
    ($e:expr, $p:pat) => {
        match $e {
            $p => true,
            other => panic!(
                "assert_matches failed, expected {}, got: {:#?}",
                stringify!($p),
                &other
            ),
        }
    };
}

#[tokio::test]
async fn phoenix_channels_join_payload_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let config = config();
    let client = connected_client(config).await;

    let channel = client
        .join("channel:mytopic", Some(json!({ "key": "value" }).into()), Some(Duration::from_secs(5)))
        .await
        .unwrap();

    // channel
    //     .on("send_join_payload", move |_channel, payload| {
    //         assert_eq!(payload, &Payload::Value(json!({ "key": "value"})));
    //
    //         on_notify.notify_one();
    //     })
    //     .await
    //     .unwrap();

    let payload = channel.send_with_timeout("send_join_payload", json!({}), Some(Duration::from_secs(5))).await.unwrap();
    assert_eq!(payload, Payload::Value(json!({ "key": "value" })));
}

#[tokio::test]
async fn phoenix_channels_join_error_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let config = config();
    let client = connected_client(config).await;

    let result = client
        .join("channel:error", Some(json!({ "key": "value" }).into()), Some(Duration::from_secs(5)))
        .await;

    assert!(result.is_err());

    let channel_error = result.err().unwrap();

    assert_eq!(channel_error, ChannelError::JoinFailed(Box::new(Payload::Value(json!({"reason": { "key": "value"}})))));
}

#[tokio::test]
async fn phoenix_channels_broadcast_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let config = config();
    let client1 = connected_client(config.clone()).await;

    let channel1 = client1
        .join("channel:mytopic", None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel1.is_joined());

    let notify = Arc::new(tokio::sync::Notify::new());
    let notify2 = notify.clone();

    channel1
        .on("send_all", move |channel, payload| {
            println!(
                "channel1 received {} from topic '{}'",
                payload,
                channel.topic()
            );
            notify.notify_one();
        })
        .await
        .unwrap();

    let client2 = connected_client(config).await;

    let channel2 = client2
        .join("channel:mytopic", None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel2.is_joined());
    channel2
        .on("myevent", |channel, payload| {
            let mut map = serde_json::Map::new();
            map.insert("status".to_string(), Value::String("testing".to_string()));
            map.insert("num".to_string(), Value::Number(1u64.into()));
            let expected = Payload::Value(Value::Object(map));

            assert_eq!(payload, &expected);
            println!(
                "channel2 received {} from topic '{}'",
                payload,
                channel.topic()
            );
        })
        .await
        .unwrap();

    let status = "testing".to_string();
    let num = 1i64;
    channel2
        .send_noreply("send_all", json!({ "status": status, "num": num }))
        .await
        .unwrap();

    let result = time::timeout(Duration::from_secs(5), notify2.notified()).await;
    assert_matches!(result, Ok(_));
}

#[tokio::test]
async fn phoenix_channels_reply_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let config = config();
    let client = connected_client(config).await;

    let channel = client
        .join("channel:mytopic", None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel.is_joined());

    let status = "testing".to_string();
    let num = 1i64;
    let expected = json!({ "status": status, "num": num });
    let result = channel
        .send_with_timeout("send_reply", expected.clone(), Some(Duration::from_secs(5)))
        .await
        .unwrap();

    assert_eq!(result, Payload::Value(expected));
}

async fn connected_client(config: Config) -> Client {
    let mut client = Client::new(config).unwrap();
    client.connect().await.unwrap();

    client
}

fn config() -> Config {
    let mut config = Config::new(format!("ws://{HOST}:9002/socket/websocket").as_str()).unwrap();

    config
        .reconnect(false)
        .set("shared_secret", "supersecret");

    config
}

#[cfg(target_os = "android")]
const HOST: &str = "10.0.2.2";

#[cfg(not(target_os = "android"))]
const HOST: &str = "127.0.0.1";
