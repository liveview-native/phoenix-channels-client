#![cfg_attr(feature = "nightly", feature(assert_matches))]

use std::sync::Arc;
use std::time::Duration;

use phoenix_channels_client::{ChannelError, Client, Config, Payload};
use serde_json::json;
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
async fn phoenix_channels_join_binary_error_test() {
    phoenix_channels_join_error_test("binary", Payload::Binary(vec![0, 1, 2, 3])).await;
}

async fn phoenix_channels_join_error_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let config = config();
    let client = connected_client(config).await;

    let topic = format!("channel:error:{}", subtopic);
    let result = client
        .join(&topic, Some(payload.clone()), Some(Duration::from_secs(5)))
        .await;

    assert!(result.is_err());

    let channel_error = result.err().unwrap();

    assert_eq!(channel_error, ChannelError::JoinFailed(Box::new(payload)));
}

#[tokio::test]
async fn phoenix_channels_json_broadcast_test() {
    phoenix_channels_broadcast_test("json", Payload::Value(json!({ "status": "testng", "num": 1i64 }))).await;
}

#[tokio::test]
async fn phoenix_channels_binary_broadcast_test() {
    phoenix_channels_broadcast_test("binary", Payload::Binary(vec![0, 1, 2, 3])).await;
}

async fn phoenix_channels_broadcast_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let config = config();
    let receiver_client = connected_client(config.clone()).await;

    let topic = format!("channel:broadcast:{}", subtopic);
    let receiver_channel = receiver_client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(receiver_channel.is_joined());

    const EVENT: &'static str = "send_all";
    let sent_payload = payload;
    let expected_received_payload = sent_payload.clone();
    let on_notify = Arc::new(tokio::sync::Notify::new());
    let test_notify = on_notify.clone();

    receiver_channel
        .on(EVENT, move |_channel, payload| {
            assert_eq!(payload, &expected_received_payload);

            on_notify.notify_one();
        })
        .await
        .unwrap();

    let sender_client = connected_client(config).await;

    let sender_channel = sender_client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(sender_channel.is_joined());

    sender_channel
        .send_noreply(EVENT, sent_payload)
        .await
        .unwrap();

    let result = time::timeout(Duration::from_secs(5), test_notify.notified()).await;
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
