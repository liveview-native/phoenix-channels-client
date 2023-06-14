#![cfg_attr(feature = "nightly", feature(assert_matches))]
#![feature(async_closure)]

use std::sync::Arc;
use std::time::Duration;

use phoenix_channels_client::{
    socket, CallError, ConnectError, Event, EventPayload, JoinError, Payload, Socket,
};
use serde_json::json;
use tokio::time;

use log::debug;
#[cfg(feature = "nightly")]
use std::assert_matches::assert_matches;
use std::io::ErrorKind;
use tokio::time::{timeout, Instant};
use tokio_tungstenite::tungstenite::Error;
use url::Url;
use uuid::Uuid;

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
async fn phoenix_channels_socket_status_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let url = url();
    let socket = Socket::spawn(url).await.unwrap();

    assert_eq!(socket.status(), socket::Status::NeverConnected);
    assert_eq!(socket.has_never_connected(), true);

    socket.connect(CONNECT_TIMEOUT).await.unwrap();
    assert_eq!(socket.status(), socket::Status::Connected);
    assert_eq!(socket.is_connected(), true);

    socket.disconnect().await.unwrap();
    assert_eq!(socket.status(), socket::Status::Disconnected);
    assert_eq!(socket.is_disconnected(), true);

    socket.shutdown().await.unwrap();
    assert_eq!(socket.status(), socket::Status::ShutDown);
    assert_eq!(socket.is_shutdown(), true);
}

#[tokio::test]
async fn phoenix_channels_socket_event_test() {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let url = url();
    let socket = Socket::spawn(url).await.unwrap();

    let mut statuses = socket.statuses();

    socket.connect(CONNECT_TIMEOUT).await.unwrap();
    assert_matches!(
        timeout(CONNECT_TIMEOUT, statuses.recv())
            .await
            .unwrap()
            .unwrap(),
        socket::Status::Connected
    );

    let channel = socket.channel("channel:disconnect", None).await.unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();
    assert_matches!(
        channel
            .call("socket_disconnect", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err(),
        CallError::SocketDisconnected
    );
    assert_matches!(
        timeout(CALL_TIMEOUT + JOIN_TIMEOUT, statuses.recv())
            .await
            .unwrap()
            .unwrap(),
        socket::Status::WaitingToReconnect
    );
    assert_matches!(
        timeout(CONNECT_TIMEOUT, statuses.recv())
            .await
            .unwrap()
            .unwrap(),
        socket::Status::Connected
    );

    socket.disconnect().await.unwrap();
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.recv())
            .await
            .unwrap()
            .unwrap(),
        socket::Status::Disconnected
    );

    socket.shutdown().await.unwrap();
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.recv())
            .await
            .unwrap()
            .unwrap(),
        socket::Status::ShuttingDown
    );
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.recv())
            .await
            .unwrap()
            .unwrap(),
        socket::Status::ShutDown
    );
}

#[tokio::test]
async fn phoenix_channels_socket_disconnect_reconnect_test() {
    phoenix_channels_reconnect_test("socket_disconnect").await;
}

#[tokio::test]
async fn phoenix_channels_transport_error_reconnect_test() {
    phoenix_channels_reconnect_test("transport_error").await;
}

async fn phoenix_channels_reconnect_test(event: &str) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let url = url();
    let socket = connected_socket(url).await;

    let channel = socket
        .channel(format!("channel:{}", event), None)
        .await
        .unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();

    let call_error = channel
        .call(event, json!({}), CALL_TIMEOUT)
        .await
        .unwrap_err();

    assert_matches!(call_error, CallError::SocketDisconnected);

    let payload = json_payload();

    debug!("Sending to check for reconnect");
    let start = Instant::now();

    match channel
        .call(
            "send_reply",
            payload.clone(),
            CONNECT_TIMEOUT + JOIN_TIMEOUT + CALL_TIMEOUT,
        )
        .await
    {
        Ok(received_payload) => assert_eq!(received_payload, payload),
        Err(call_error) => match call_error {
            CallError::Shutdown => panic!("channel shut down"),
            CallError::Timeout => {
                // debug to get time stamp
                debug!("Timeout after {:?}", start.elapsed());
                panic!("timeout");
            }
            CallError::WebSocketError(web_socket_error) => {
                panic!("web socket error {:?}", web_socket_error)
            }
            CallError::SocketDisconnected => panic!("socket disconnected"),
        },
    }
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

    let url = url();
    let socket = connected_socket(url).await;
    let topic = format!("channel:join:payload:{}", subtopic);

    let channel = socket.channel(&topic, Some(payload.clone())).await.unwrap();

    channel.join(JOIN_TIMEOUT).await.unwrap();

    let received_payload = channel
        .call("send_join_payload", json!({}), CALL_TIMEOUT)
        .await
        .unwrap();
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

    let url = url();
    let socket = connected_socket(url).await;

    let topic = format!("channel:error:{}", subtopic);
    let channel = socket.channel(&topic, Some(payload.clone())).await.unwrap();
    let result = channel.join(JOIN_TIMEOUT).await;

    assert!(result.is_err());

    let channel_error = result.err().unwrap();

    assert_eq!(channel_error, JoinError::Rejected(payload));
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

    let url = url();
    let receiver_client = connected_socket(url.clone()).await;

    let topic = format!("channel:broadcast:{}", subtopic);
    let receiver_channel = receiver_client.channel(&topic, None).await.unwrap();
    receiver_channel.join(JOIN_TIMEOUT).await.unwrap();
    assert!(receiver_channel.is_joined());

    const EVENT: &'static str = "send_all";
    let sent_payload = payload;
    let expected_received_payload = sent_payload.clone();
    let on_notify = Arc::new(tokio::sync::Notify::new());
    let test_notify = on_notify.clone();

    let mut event_receiver = receiver_channel.events();

    tokio::spawn(async move {
        loop {
            match event_receiver.recv().await.unwrap() {
                EventPayload {
                    event: Event::User(user_event_name),
                    payload,
                } if user_event_name == EVENT => {
                    assert_eq!(payload, expected_received_payload);

                    on_notify.notify_one();
                    break;
                }
                _ => continue,
            }
        }
    });

    let sender_client = connected_socket(url).await;

    let sender_channel = sender_client.channel(&topic, None).await.unwrap();
    sender_channel.join(JOIN_TIMEOUT).await.unwrap();
    assert!(sender_channel.is_joined());

    sender_channel.cast(EVENT, sent_payload).await.unwrap();

    let result = time::timeout(CALL_TIMEOUT, test_notify.notified()).await;
    assert_matches!(result, Ok(_));
}

#[tokio::test]
async fn phoenix_channels_call_json_test() {
    phoenix_channels_call_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_call_binary_test() {
    phoenix_channels_call_test("binary", binary_payload()).await;
}

async fn phoenix_channels_call_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let url = url();
    let socket = connected_socket(url).await;

    let topic = format!("channel:call:{}", subtopic);
    let channel = socket.channel(&topic, None).await.unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();
    assert!(channel.is_joined());

    let reply = channel
        .call("send_reply", payload.clone(), CALL_TIMEOUT)
        .await
        .unwrap();

    assert_eq!(reply, payload);
}

#[tokio::test]
async fn phoenix_channels_call_error_json_test() {
    phoenix_channels_call_error_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_call_error_binary_test() {
    phoenix_channels_call_error_test("binary", binary_payload()).await;
}

async fn phoenix_channels_call_error_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let url = url();
    let socket = connected_socket(url).await;

    let topic = format!("channel:raise:{}", subtopic);
    let channel = socket.channel(&topic, None).await.unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();
    assert!(channel.is_joined());

    let send_error = channel
        .call("raise", payload.clone(), CALL_TIMEOUT)
        .await
        .unwrap_err();

    assert_matches!(send_error, CallError::Timeout);
}

#[tokio::test]
async fn phoenix_channels_cast_error_json_test() {
    phoenix_channels_cast_error_test("json", json_payload()).await;
}

#[tokio::test]
async fn phoenix_channels_cast_error_binary_test() {
    phoenix_channels_cast_error_test("binary", binary_payload()).await;
}

async fn phoenix_channels_cast_error_test(subtopic: &str, payload: Payload) {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let url = url();
    let socket = connected_socket(url).await;

    let topic = format!("channel:raise:{}", subtopic);
    let channel = socket.channel(&topic, None).await.unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();
    assert!(channel.is_joined());

    let result = channel.cast("raise", payload.clone()).await;

    assert_matches!(result, Ok(()));
}

async fn connected_socket(url: Url) -> Arc<Socket> {
    let socket = Socket::spawn(url).await.unwrap();

    if let Err(connect_error) = socket.connect(CONNECT_TIMEOUT).await {
        match connect_error {
            ConnectError::WebSocketError(Error::Io(io_error)) => {
                if io_error.kind() == ErrorKind::ConnectionRefused {
                    panic!("Phoenix server not started. Run: cd tests/support/test_server && iex -S mix")
                } else {
                    panic!("{:?}", io_error)
                }
            }
            _ => panic!("{:?}", connect_error),
        };
    }

    socket
}

fn url() -> Url {
    Url::parse_with_params(
        format!("ws://{HOST}:9002/socket/websocket").as_str(),
        &[
            ("shared_secret", "supersecret"),
            (
                "id",
                Uuid::new_v4()
                    .hyphenated()
                    .encode_upper(&mut Uuid::encode_buffer()),
            ),
        ],
    )
    .unwrap()
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const JOIN_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

fn json_payload() -> Payload {
    json!({ "status": "testng", "num": 1i64 }).into()
}

fn binary_payload() -> Payload {
    vec![0, 1, 2, 3].into()
}

#[cfg(target_os = "android")]
const HOST: &str = "10.0.2.2";

#[cfg(not(target_os = "android"))]
const HOST: &str = "127.0.0.1";
