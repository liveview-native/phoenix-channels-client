#![feature(assert_matches)]

use std::assert_matches::assert_matches;
use std::sync::Arc;
use std::time::Duration;

use phoenix_channels::{Client, Config, Payload};
use serde_json::{json, Value};
use tokio::time;

#[tokio::test]
async fn phoenix_channels_broadcast_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let mut config = Config::new("ws://127.0.0.1:9002/socket/websocket").unwrap();
    config.reconnect(false).set("shared_secret", "supersecret");

    let mut client1 = Client::new(config.clone()).unwrap();
    client1.connect().await.unwrap();

    let channel1 = client1
        .join("channel:mytopic", Some(Duration::from_secs(5)))
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

    let mut client2 = Client::new(config).unwrap();
    client2.connect().await.unwrap();

    let channel2 = client2
        .join("channel:mytopic", Some(Duration::from_secs(5)))
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

    let mut config = Config::new("ws://127.0.0.1:9002/socket/websocket").unwrap();
    config.reconnect(false).set("shared_secret", "supersecret");

    let mut client = Client::new(config.clone()).unwrap();
    client.connect().await.unwrap();

    let channel = client
        .join("channel:mytopic", Some(Duration::from_secs(5)))
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
