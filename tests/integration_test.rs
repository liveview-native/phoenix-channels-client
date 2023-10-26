#![cfg_attr(feature = "nightly", feature(assert_matches))]
#![feature(async_closure)]

#[cfg(feature = "nightly")]
use std::assert_matches::assert_matches;
use std::sync::Arc;
use std::time::Duration;

use log::debug;
use serde_json::json;
use tokio::time;
use tokio::time::{timeout, Instant};
use url::Url;
use uuid::Uuid;

// Everything should be usable from the root as `uniffi` does not support nested namespaces for
// the foreign bindings
use phoenix_channels_client::{
    CallError, ChannelJoinError, ChannelStatus, ChannelStatusJoinError, ChannelStatuses,
    ConnectError, Error, Event, EventPayload, IoError, Payload, Socket, SocketStatus, Topic,
    WebSocketError, JSON,
};

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
async fn socket_status() -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = Socket::spawn(url)?;

    let status = socket.status();
    assert_eq!(status, SocketStatus::NeverConnected);

    socket.connect(CONNECT_TIMEOUT).await?;
    let status = socket.status();
    assert_eq!(status, SocketStatus::Connected);

    socket.disconnect().await?;
    let status = socket.status();
    assert_eq!(status, SocketStatus::Disconnected);

    socket.shutdown().await?;
    let status = socket.status();
    assert_eq!(status, SocketStatus::ShutDown);

    Ok(())
}

#[tokio::test]
async fn socket_statuses() -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = Socket::spawn(url)?;

    let statuses = socket.statuses();

    socket.connect(CONNECT_TIMEOUT).await?;
    assert_matches!(
        timeout(CONNECT_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(SocketStatus::Connected)
    );

    let channel = socket
        .channel(Topic::from_string("channel:disconnect".to_string()), None)
        .await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_matches!(
        channel
            .call(
                Event::from_string("socket_disconnect".to_string()),
                Payload::json_from_serialized(json!({}).to_string()).unwrap(),
                CALL_TIMEOUT
            )
            .await
            .unwrap_err(),
        CallError::SocketDisconnected
    );
    assert_matches!(
        timeout(CALL_TIMEOUT + JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(SocketStatus::WaitingToReconnect { .. })
    );
    assert_matches!(
        timeout(CONNECT_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(SocketStatus::Connected)
    );

    socket.disconnect().await?;
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(SocketStatus::Disconnected)
    );

    socket.shutdown().await?;
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(SocketStatus::ShuttingDown)
    );
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(SocketStatus::ShutDown)
    );

    Ok(())
}

#[tokio::test]
async fn channel_status() -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = Socket::spawn(url)?;

    let channel = socket
        .channel(Topic::from_string("channel:status".to_string()), None)
        .await?;

    let status = channel.status();
    assert_eq!(status, ChannelStatus::WaitingForSocketToConnect);

    socket.connect(CONNECT_TIMEOUT).await?;
    let status = channel.status();
    assert_eq!(status, ChannelStatus::WaitingToJoin);

    channel.join(JOIN_TIMEOUT).await?;
    let status = channel.status();
    assert_eq!(status, ChannelStatus::Joined);

    channel.leave().await?;
    let status = channel.status();
    assert_eq!(status, ChannelStatus::Left);

    channel.shutdown().await?;
    let status = channel.status();
    assert_eq!(status, ChannelStatus::ShutDown);

    Ok(())
}

#[tokio::test]
async fn channel_statuses() -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = Socket::spawn(url)?;

    let channel = socket
        .channel(Topic::from_string("channel:status".to_string()), None)
        .await?;

    let statuses = channel.statuses();

    socket.connect(CONNECT_TIMEOUT).await?;
    assert_matches!(
        timeout(CONNECT_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::WaitingToJoin)
    );

    channel.join(JOIN_TIMEOUT).await?;
    assert_matches!(
        timeout(JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::Joining)
    );
    assert_matches!(
        timeout(JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::Joined)
    );

    assert_matches!(
        channel
            .call(
                Event::from_string("socket_disconnect".to_string()),
                Payload::json_from_serialized(json!({}).to_string()).unwrap(),
                CALL_TIMEOUT
            )
            .await
            .unwrap_err(),
        CallError::SocketDisconnected
    );
    assert_matches!(
        timeout(CALL_TIMEOUT + JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::WaitingForSocketToConnect)
    );
    assert_matches!(
        timeout(CALL_TIMEOUT + JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::WaitingToRejoin { .. })
    );
    assert_matches!(
        timeout(JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::Joining)
    );
    assert_matches!(
        timeout(JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::Joined)
    );

    channel.leave().await?;
    assert_matches!(
        timeout(JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::Leaving)
    );
    assert_matches!(
        timeout(JOIN_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::Left)
    );

    channel.shutdown().await?;
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::ShuttingDown)
    );
    assert_matches!(
        timeout(CALL_TIMEOUT, statuses.status())
            .await
            .unwrap()
            .unwrap(),
        Ok(ChannelStatus::ShutDown)
    );

    Ok(())
}

#[tokio::test]
async fn channel_key_rotation_test() -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let shared_secret_url = shared_secret_url(id.clone());
    let socket = connected_socket(shared_secret_url).await?;

    let topic = Topic::from_string("channel:protected".to_string());

    let channel = socket.channel(topic.clone(), None).await?;
    let mut statuses = channel.statuses();

    match channel.join(JOIN_TIMEOUT).await {
        Ok(_) => panic!("Joined protected channel without being authorized"),
        Err(join_error) => match join_error {
            ChannelJoinError::Rejected { rejection } => match rejection {
                Payload::JSON { .. } => {
                    assert_eq!(
                        rejection,
                        Payload::json_from_serialized(
                            json!({"reason": "unauthorized"}).to_string()
                        )
                        .unwrap()
                    )
                }
                Payload::Binary { bytes } => panic!("Unexpected binary payload: {:?}", bytes),
            },
            other => panic!("Join wasn't rejected and instead {:?}", other),
        },
    };

    assert_joining(&statuses).await;
    assert_unauthorized(&statuses).await;
    assert_waiting_to_rejoin(&statuses).await;

    // happens again because of zero delay retry on first retry
    assert_joining(&mut statuses).await;
    assert_unauthorized(&mut statuses).await;
    assert_waiting_to_rejoin(&mut statuses).await;

    authorize(&id, &topic).await;

    // next rejoin should work
    assert_joining(&mut statuses).await;
    assert_joined(&mut statuses).await;

    deauthorize(&id, &topic).await;

    // should attempt to rejoin when the server send phx_close, but see the error again.
    assert_waiting_to_rejoin(&mut statuses).await;
    assert_joining(&mut statuses).await;
    assert_unauthorized(&mut statuses).await;

    Ok(())
}

#[tokio::test]
async fn socket_key_rotation_test() -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let shared_secret_url = shared_secret_url(id.clone());
    let socket = connected_socket(shared_secret_url).await?;
    let secret = generate_secret(&socket).await?;
    let secret_url = secret_url(id, secret);
    let secret_socket = connected_socket(secret_url).await?;

    let secret_channel = secret_socket
        .channel(Topic::from_string("channel:secret".to_string()), None)
        .await?;
    secret_channel.join(JOIN_TIMEOUT).await?;

    match secret_channel
        .call(
            Event::from_string("delete_secret".to_string()),
            Payload::json_from_serialized(json!({}).to_string()).unwrap(),
            CALL_TIMEOUT,
        )
        .await
    {
        Ok(payload) => panic!(
            "Deleting secret succeeded without disconnecting socket and returned payload: {:?}",
            payload
        ),
        Err(CallError::SocketDisconnected) => (),
        Err(other) => panic!("Error other than SocketDisconnected: {:?}", other),
    }

    let statuses = secret_socket.statuses();

    let mut reconnect_count = 0;

    loop {
        tokio::select! {
            result  = timeout(CALL_TIMEOUT, statuses.status()) => match result?? {
                Ok(SocketStatus::WaitingToReconnect { .. }) => {
                    reconnect_count += 1;

                    if reconnect_count > 5 {
                        panic!("Waiting to reconnect {} times without sending error status", reconnect_count);
                    } else {
                        continue
                    }
                },
                Err(web_socket_error) => match web_socket_error {
                    WebSocketError::Http { response } => {
                        assert_eq!(response.status_code, 403);

                        break
                    },
                    web_socket_error => panic!("Unexpected web socket error: {:?}", web_socket_error)
                }
                result => panic!("Unexpected status: {:?}", result),
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_socket_disconnect_reconnect_test() -> Result<(), Error> {
    phoenix_channels_reconnect_test(Event::from_string("socket_disconnect".to_string())).await
}

#[tokio::test]
async fn phoenix_channels_transport_error_reconnect_test() -> Result<(), Error> {
    phoenix_channels_reconnect_test(Event::from_string("transport_error".to_string())).await
}

async fn phoenix_channels_reconnect_test(event: Event) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let channel = socket
        .channel(Topic::from_string(format!("channel:{}", event)), None)
        .await?;
    channel.join(JOIN_TIMEOUT).await?;

    let call_error = channel
        .call(
            event,
            Payload::json_from_serialized(json!({}).to_string()).unwrap(),
            CALL_TIMEOUT,
        )
        .await
        .unwrap_err();

    assert_matches!(call_error, CallError::SocketDisconnected);

    let payload = json_payload();

    debug!("Sending to check for reconnect");
    let start = Instant::now();

    match channel
        .call(
            Event::from_string("reply_ok_tuple".to_string()),
            payload.clone(),
            CONNECT_TIMEOUT + JOIN_TIMEOUT + CALL_TIMEOUT,
        )
        .await
    {
        Ok(received_payload) => assert_eq!(received_payload, payload),
        Err(call_error) => match call_error {
            CallError::Shutdown => panic!("channel shut down"),
            CallError::SocketShutdown {
                socket_shutdown_error,
            } => panic!("{}", socket_shutdown_error),
            CallError::Timeout => {
                // debug to get time stamp
                debug!("Timeout after {:?}", start.elapsed());
                panic!("timeout");
            }
            CallError::WebSocketError { web_socket_error } => {
                panic!("web socket error {:?}", web_socket_error)
            }
            CallError::SocketDisconnected => panic!("socket disconnected"),
            CallError::Reply { reply } => panic!("Error from server: {:?}", reply),
        },
    };

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_join_json_payload_test() -> Result<(), Error> {
    phoenix_channels_join_payload_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_join_binary_payload_test() -> Result<(), Error> {
    phoenix_channels_join_payload_test("binary", binary_payload()).await
}

async fn phoenix_channels_join_payload_test(subtopic: &str, payload: Payload) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;
    let topic = Topic::from_string(format!("channel:join:payload:{}", subtopic));

    let channel = socket.channel(topic, Some(payload.clone())).await?;

    channel.join(JOIN_TIMEOUT).await?;

    let received_payload = channel
        .call(
            Event::from_string("reply_ok_join_payload".to_string()),
            Payload::json_from_serialized(json!({}).to_string()).unwrap(),
            CALL_TIMEOUT,
        )
        .await?;
    assert_eq!(received_payload, payload);

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_join_json_error_test() -> Result<(), Error> {
    phoenix_channels_join_error_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_join_binary_error_test() -> Result<(), Error> {
    phoenix_channels_join_error_test("binary", binary_payload()).await
}

async fn phoenix_channels_join_error_test(subtopic: &str, payload: Payload) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:error:{}", subtopic));
    let channel = socket.channel(topic, Some(payload.clone())).await.unwrap();
    let result = channel.join(JOIN_TIMEOUT).await;

    assert!(result.is_err());

    let channel_error = result.err().unwrap();

    assert_eq!(
        channel_error,
        ChannelJoinError::Rejected { rejection: payload }
    );

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_json_broadcast_test() -> Result<(), Error> {
    phoenix_channels_broadcast_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_binary_broadcast_test() -> Result<(), Error> {
    phoenix_channels_broadcast_test("binary", binary_payload()).await
}

async fn phoenix_channels_broadcast_test(subtopic: &str, payload: Payload) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let receiver_client = connected_socket(url.clone()).await?;

    let topic = Topic::from_string(format!("channel:broadcast:{}", subtopic));
    let receiver_channel = receiver_client.channel(topic.clone(), None).await?;
    receiver_channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(receiver_channel.status(), ChannelStatus::Joined);

    let event = Event::from_string("broadcast".to_string());
    let sent_payload = payload;
    let expected_received_payload = sent_payload.clone();
    let on_notify = Arc::new(tokio::sync::Notify::new());
    let test_notify = on_notify.clone();

    let events = receiver_channel.events();

    let expected_event = event.clone();
    tokio::spawn(async move {
        loop {
            match events.event().await.unwrap() {
                EventPayload {
                    event: current_event,
                    payload,
                } if current_event == expected_event => {
                    assert_eq!(payload, expected_received_payload);

                    on_notify.notify_one();
                    break;
                }
                _ => continue,
            }
        }
    });

    let sender_client = connected_socket(url).await?;

    let sender_channel = sender_client.channel(topic, None).await?;
    sender_channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(sender_channel.status(), ChannelStatus::Joined);

    sender_channel.cast(event, sent_payload).await?;

    let result = time::timeout(CALL_TIMEOUT, test_notify.notified()).await;
    assert_matches!(result, Ok(_));

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_call_with_json_payload_reply_ok_without_payload_test() -> Result<(), Error>
{
    phoenix_channels_call_reply_ok_without_payload_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_call_with_binary_payload_reply_ok_without_payload_test(
) -> Result<(), Error> {
    phoenix_channels_call_reply_ok_without_payload_test("binary", binary_payload()).await
}

async fn phoenix_channels_call_reply_ok_without_payload_test(
    subtopic: &str,
    payload: Payload,
) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:call:{}", subtopic));
    let channel = socket.channel(topic, None).await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(channel.status(), ChannelStatus::Joined);

    assert_eq!(
        channel
            .call(
                Event::from_string("reply_ok".to_string()),
                payload.clone(),
                CALL_TIMEOUT
            )
            .await?,
        Payload::json_from_serialized(json!({}).to_string()).unwrap()
    );

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_call_with_json_payload_reply_error_without_payload_test(
) -> Result<(), Error> {
    phoenix_channels_call_reply_error_without_payload_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_call_with_binary_payload_reply_error_without_payload_test(
) -> Result<(), Error> {
    phoenix_channels_call_reply_error_without_payload_test("binary", binary_payload()).await
}

async fn phoenix_channels_call_reply_error_without_payload_test(
    subtopic: &str,
    payload: Payload,
) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:call:{}", subtopic));
    let channel = socket.channel(topic, None).await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(channel.status(), ChannelStatus::Joined);

    match channel
        .call(
            Event::from_string("reply_error".to_string()),
            payload,
            CALL_TIMEOUT,
        )
        .await
    {
        Err(CallError::Reply { reply }) => assert_eq!(
            reply,
            Payload::json_from_serialized(json!({}).to_string()).unwrap()
        ),
        result => panic!("Received result {:?} when calling reply_error", result),
    };

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_call_reply_ok_with_json_payload_test() -> Result<(), Error> {
    phoenix_channels_call_reply_ok_with_payload_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_call_reply_ok_with_binary_payload_test() -> Result<(), Error> {
    phoenix_channels_call_reply_ok_with_payload_test("binary", binary_payload()).await
}

async fn phoenix_channels_call_reply_ok_with_payload_test(
    subtopic: &str,
    payload: Payload,
) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:call:{}", subtopic));
    let channel = socket.channel(topic, None).await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(channel.status(), ChannelStatus::Joined);

    match channel
        .call(
            Event::from_string("reply_ok_tuple".to_string()),
            payload.clone(),
            CALL_TIMEOUT,
        )
        .await
    {
        Ok(reply_payload) => assert_eq!(reply_payload, payload),
        Err(call_error) => panic!(
            "CallError {:?} when calling reply_ok_tuple with payload {:?}",
            call_error, payload
        ),
    };

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_call_with_json_payload_reply_error_with_json_payload_test(
) -> Result<(), Error> {
    phoenix_channels_call_with_payload_reply_error_with_payload_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_call_with_binary_payload_reply_error_with_binary_payload_test(
) -> Result<(), Error> {
    phoenix_channels_call_with_payload_reply_error_with_payload_test("binary", binary_payload())
        .await
}

async fn phoenix_channels_call_with_payload_reply_error_with_payload_test(
    subtopic: &str,
    payload: Payload,
) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:call:{}", subtopic));
    let channel = socket.channel(topic, None).await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(channel.status(), ChannelStatus::Joined);

    match channel
        .call(
            Event::from_string("reply_error_tuple".to_string()),
            payload.clone(),
            CALL_TIMEOUT,
        )
        .await
    {
        Err(CallError::Reply { reply }) => assert_eq!(reply, payload),
        result => panic!(
            "Got result {:?} when calling reply_error_tuple with payload {:?}",
            result, payload
        ),
    };

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_call_with_json_payload_raise_test() -> Result<(), Error> {
    phoenix_channels_call_raise_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_call_with_binary_payload_raise_test() -> Result<(), Error> {
    phoenix_channels_call_raise_test("binary", binary_payload()).await
}

async fn phoenix_channels_call_raise_test(subtopic: &str, payload: Payload) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:raise:{}", subtopic));
    let channel = socket.channel(topic, None).await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(channel.status(), ChannelStatus::Joined);

    let send_error = channel
        .call(
            Event::from_string("raise".to_string()),
            payload.clone(),
            CALL_TIMEOUT,
        )
        .await
        .unwrap_err();

    assert_matches!(send_error, CallError::Timeout);

    Ok(())
}

#[tokio::test]
async fn phoenix_channels_cast_error_json_test() -> Result<(), Error> {
    phoenix_channels_cast_error_test("json", json_payload()).await
}

#[tokio::test]
async fn phoenix_channels_cast_error_binary_test() -> Result<(), Error> {
    phoenix_channels_cast_error_test("binary", binary_payload()).await
}

async fn phoenix_channels_cast_error_test(subtopic: &str, payload: Payload) -> Result<(), Error> {
    let _ = env_logger::builder()
        .parse_default_env()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let id = id();
    let url = shared_secret_url(id);
    let socket = connected_socket(url).await?;

    let topic = Topic::from_string(format!("channel:raise:{}", subtopic));
    let channel = socket.channel(topic, None).await?;
    channel.join(JOIN_TIMEOUT).await?;
    assert_eq!(channel.status(), ChannelStatus::Joined);

    let result = channel
        .cast(Event::from_string("raise".to_string()), payload.clone())
        .await;

    assert_matches!(result, Ok(()));

    Ok(())
}

async fn authorize(id: &str, topic: &Topic) {
    let shared_secret_url = shared_secret_url(id.to_string());
    let socket = connected_socket(shared_secret_url).await.unwrap();
    let channel = socket
        .channel(Topic::from_string("channel:authorize".to_string()), None)
        .await
        .unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();

    channel
        .call(
            Event::from_string("authorize".to_string()),
            Payload::json_from_serialized(
                json!({"channel": topic.to_string(), "id": id}).to_string(),
            )
            .unwrap(),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();

    channel.shutdown().await.unwrap();
    socket.shutdown().await.unwrap();
}

async fn deauthorize(id: &str, topic: &Topic) {
    let shared_secret_url = shared_secret_url(id.to_string());
    let socket = connected_socket(shared_secret_url).await.unwrap();
    let channel = socket
        .channel(Topic::from_string("channel:deauthorize".to_string()), None)
        .await
        .unwrap();
    channel.join(JOIN_TIMEOUT).await.unwrap();

    channel
        .call(
            Event::from_string("deauthorize".to_string()),
            Payload::json_from_serialized(
                json!({"channel": topic.to_string(), "id": id}).to_string(),
            )
            .unwrap(),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();

    channel.shutdown().await.unwrap();
    socket.shutdown().await.unwrap();
}

async fn assert_waiting_to_rejoin(channel_statuses: &ChannelStatuses) {
    match timeout(CALL_TIMEOUT, channel_statuses.status())
        .await
        .unwrap()
        .unwrap()
    {
        Ok(status) => match status {
            ChannelStatus::WaitingToRejoin { .. } => (),
            other => panic!("Status other than waiting to rejoin: {:?}", other),
        },
        Err(payload) => panic!(
            "Join rejection seen before waiting to rejoin status: {:?}",
            payload
        ),
    }
}

async fn assert_joining(statuses: &ChannelStatuses) {
    match timeout(CALL_TIMEOUT, statuses.status())
        .await
        .unwrap()
        .unwrap()
    {
        Ok(status) => match status {
            ChannelStatus::Joining => (),
            other => panic!("Status other than joining: {:?}", other),
        },
        Err(payload) => panic!("Join rejection seen before joining status: {:?}", payload),
    }
}

async fn assert_unauthorized(channel_statuses: &ChannelStatuses) {
    match timeout(CALL_TIMEOUT, channel_statuses.status())
        .await
        .unwrap()
        .unwrap()
    {
        Ok(status) => panic!("Got status instead of join rejection: {:?}", status),
        Err(ChannelStatusJoinError::Rejected { response }) => match response {
            Payload::JSON { .. } => assert_eq!(
                response,
                Payload::json_from_serialized(json!({"reason": "unauthorized"}).to_string())
                    .unwrap()
            ),
            Payload::Binary { bytes } => panic!("Unexpected binary payload: {:?}", bytes),
        },
    }
}

async fn assert_joined(channel_statuses: &ChannelStatuses) {
    match timeout(CALL_TIMEOUT, channel_statuses.status())
        .await
        .unwrap()
        .unwrap()
    {
        Ok(status) => match status {
            ChannelStatus::Joined => (),
            other => panic!("Status other than joined: {:?}", other),
        },
        Err(payload) => panic!(
            "Join rejection seen instead of joined status: {:?}",
            payload
        ),
    }
}

async fn connected_socket(url: Url) -> Result<Arc<Socket>, Error> {
    let socket = Socket::spawn(url)?;

    if let Err(connect_error) = socket.connect(CONNECT_TIMEOUT).await {
        match connect_error {
            ConnectError::WebSocketError {
                web_socket_error:
                    WebSocketError::Io {
                        io_error: IoError::ConnectionRefused,
                    },
            } => {
                panic!(
                    "Phoenix server not started. Run: cd tests/support/test_server && iex -S mix"
                )
            }
            _ => panic!("{:?}", connect_error),
        };
    }

    Ok(socket)
}

fn shared_secret_url(id: String) -> Url {
    Url::parse_with_params(
        format!("ws://{HOST}:9002/socket/websocket").as_str(),
        &[("shared_secret", "supersecret".to_string()), ("id", id)],
    )
    .unwrap()
}

async fn generate_secret(socket: &Arc<Socket>) -> Result<String, Error> {
    let channel = socket
        .channel(
            Topic::from_string("channel:generate_secret".to_string()),
            None,
        )
        .await?;
    channel.join(JOIN_TIMEOUT).await.unwrap();

    let Payload::JSON { json } = channel
        .call(
            Event::from_string("generate_secret".to_string()),
            Payload::json_from_serialized(json!({}).to_string()).unwrap(),
            CALL_TIMEOUT,
        )
        .await?
    else {
        panic!("secret not returned")
    };

    let secret = if let JSON::String { string } = json {
        string
    } else {
        panic!("secret ({:?}) is not a string", json);
    };

    Ok(secret)
}

fn secret_url(id: String, secret: String) -> Url {
    Url::parse_with_params(
        format!("ws://{HOST}:9002/socket/websocket").as_str(),
        &[("id", id), ("secret", secret)],
    )
    .unwrap()
}

fn id() -> String {
    Uuid::new_v4()
        .hyphenated()
        .encode_upper(&mut Uuid::encode_buffer())
        .to_string()
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const JOIN_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

fn json_payload() -> Payload {
    Payload::json_from_serialized(json!({ "status": "testng", "num": 1i64 }).to_string()).unwrap()
}

fn binary_payload() -> Payload {
    Payload::binary_from_bytes(vec![0, 1, 2, 3])
}

#[cfg(target_os = "android")]
const HOST: &str = "10.0.2.2";

#[cfg(not(target_os = "android"))]
const HOST: &str = "127.0.0.1";
