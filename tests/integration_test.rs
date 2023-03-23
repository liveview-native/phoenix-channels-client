#![cfg_attr(feature = "nightly", feature(assert_matches))]

use std::sync::Arc;
use std::time::Duration;

use phoenix_channels_client::{ChannelError, Client, Config, Payload, SendError};
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
async fn phoenix_channels_join_json_payload_test() {
    phoenix_channels_join_payload_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_join_binary_payload_test() {
    phoenix_channels_join_payload_test("binary", binary_payload()).await;
}

async fn phoenix_channels_join_payload_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let config = config();
    let client = connected_client(config).await;
    let topic = format!("channel:join:payload:{}", subtopic);

    let channel = client
        .join(&topic, Some(payload.clone()), Some(Duration::from_secs(5)))
        .await
        .unwrap();

    let received_payload = channel.send_with_timeout("send_join_payload", json!({}), Some(Duration::from_secs(5))).await.unwrap();
    assert_eq!(received_payload, payload);
}

#[tokio::test]
async fn phoenix_channels_join_json_error_test() {
    phoenix_channels_join_error_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_join_binary_error_test() {
    phoenix_channels_join_error_test("binary", binary_payload()).await;
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
    phoenix_channels_broadcast_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_binary_broadcast_test() {
    phoenix_channels_broadcast_test("binary", binary_payload()).await;
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
    let sent_payload = Arc::new(payload);
    let expected_received_payload = sent_payload.clone();
    let on_notify = Arc::new(tokio::sync::Notify::new());
    let test_notify = on_notify.clone();

    receiver_channel
        .on(EVENT, Box::new(move |_channel, payload| {
            let async_expected_received_payload = expected_received_payload.clone();
            let async_on_notify = on_notify.clone();

            Box::pin(async move {
                assert_eq!(payload, async_expected_received_payload);

                async_on_notify.notify_one();
            })
        })
        )
        .await
        .unwrap();

    let sender_client = connected_client(config).await;

    let sender_channel = sender_client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(sender_channel.is_joined());

    sender_channel
        .send_noreply(EVENT, sent_payload.as_ref().clone())
        .await
        .unwrap();

    let result = time::timeout(Duration::from_secs(5), test_notify.notified()).await;
    assert_matches!(result, Ok(_));
}

#[tokio::test]
async fn phoenix_channels_send_json_test() {
    phoenix_channels_send_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_send_binary_test() {
    phoenix_channels_send_test("binary", binary_payload()).await;
}


async fn phoenix_channels_send_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let config = config();
    let client = connected_client(config).await;

    let topic = format!("channel:send:{}", subtopic);
    let channel = client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel.is_joined());

    let reply = channel
        .send("send_reply", payload.clone())
        .await
        .unwrap();

    assert_eq!(reply, payload);
}

#[tokio::test]
async fn phoenix_channels_send_with_timeout_json_test() {
    phoenix_channels_send_with_timeout_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_send_with_timeout_binary_test() {
    phoenix_channels_send_with_timeout_test("binary", binary_payload()).await;
}

async fn phoenix_channels_send_with_timeout_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let config = config();
    let client = connected_client(config).await;

    let topic = format!("channel:send_with_timeout:{}", subtopic);
    let channel = client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel.is_joined());

    let reply = channel
        .send_with_timeout("send_reply", payload.clone(), Some(Duration::from_secs(5)))
        .await
        .unwrap();

    assert_eq!(reply, payload);
}

#[tokio::test]
async fn phoenix_channels_send_error_json_test() {
    phoenix_channels_send_error_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_send_error_binary_test() {
    phoenix_channels_send_error_test("binary", binary_payload()).await;
}

async fn phoenix_channels_send_error_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let config = config();
    let client = connected_client(config).await;

    let topic = format!("channel:raise:{}", subtopic);
    let channel = client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel.is_joined());

    let send_error = channel
        .send("raise", payload.clone())
        .await
        .unwrap_err();

    assert_matches!(send_error, SendError::ChannelError(_));
}

#[tokio::test]
async fn phoenix_channels_send_with_timeout_error_json_test() {
    phoenix_channels_send_with_timeout_error_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_send_with_timeout_error_binary_test() {
    phoenix_channels_send_with_timeout_error_test("binary", binary_payload()).await;
}

async fn phoenix_channels_send_with_timeout_error_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let config = config();
    let client = connected_client(config).await;

    let topic = format!("channel:raise:{}", subtopic);
    let channel = client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel.is_joined());

    let send_error = channel
        .send_with_timeout("raise", payload.clone(), Some(Duration::from_secs(5)))
        .await
        .unwrap_err();

    assert_matches!(send_error, SendError::ChannelError(_));
}

#[tokio::test]
async fn phoenix_channels_send_noreply_error_json_test() {
    phoenix_channels_send_noreply_error_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_send_noreply_error_binary_test() {
    phoenix_channels_send_noreply_error_test("binary", binary_payload()).await;
}

async fn phoenix_channels_send_noreply_error_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let config = config();
    let client = connected_client(config).await;

    let topic = format!("channel:raise:{}", subtopic);
    let channel = client
        .join(&topic, None, Some(Duration::from_secs(5)))
        .await
        .unwrap();
    assert!(channel.is_joined());

    let result = channel
        .send_noreply("raise", payload.clone())
        .await;

    assert_matches!(result, Ok(()));
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

fn json_payload() -> Payload {
   Payload::Value(json!({ "status": "testng", "num": 1i64 }))
}

fn binary_payload() -> Payload {
    Payload::Binary(vec![0, 1, 2, 3])
}

#[cfg(target_os = "android")]
const HOST: &str = "10.0.2.2";

#[cfg(not(target_os = "android"))]
const HOST: &str = "127.0.0.1";
