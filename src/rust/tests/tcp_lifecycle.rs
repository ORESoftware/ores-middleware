//! Actual loopback I/O using a deliberately small test-only framing protocol.
//! This is not a production framing implementation or WebSocket certification.
use std::{
    future::{pending, ready},
    io,
    time::Duration,
};

use ores_middleware::otel::current_log_context;
use ores_middleware::{
    OperationDescriptor, OperationFailureKind, OperationOutcome, OperationScope,
    OperationTransport, RequestContext, current_context, run_operation_boundary,
    run_operation_boundary_with_timeout_and_cancellation,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn context(slot: usize, message: bool) -> RequestContext {
    RequestContext {
        request_id: format!(
            "tcp-{slot}-{}",
            if message { "message" } else { "connection" }
        ),
        trace_id: format!("{slot:032x}"),
        span_id: None,
        tenant_id: Some(format!("tenant-{slot}")),
        user_id: Some(format!("user-{slot}")),
        locale: None,
        started_at_unix_ms: 0,
        deadline_unix_ms: None,
        baggage: Default::default(),
    }
}

fn descriptor(scope: OperationScope) -> OperationDescriptor {
    OperationDescriptor {
        transport: OperationTransport::Tcp,
        scope,
        name: "test.frame".into(),
    }
}

// Test-owned bounded framing: cancellation of read_exact closes the socket;
// resuming a partially consumed frame on the same stream would be unsafe.
async fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let size = stream.read_u32().await?;
    if !(1..=64).contains(&size) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut body = vec![0; size as usize];
    stream.read_exact(&mut body).await?;
    Ok(body)
}

async fn write_frame(stream: &mut TcpStream, body: &[u8]) -> io::Result<()> {
    assert!((1..=64).contains(&body.len()));
    stream.write_u32(body.len() as u32).await?;
    stream.write_all(body).await
}

async fn pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let (client, accepted) = tokio::join!(
        TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    (client.unwrap(), accepted.unwrap().0)
}

async fn assert_closed(client: &mut TcpStream) {
    let mut byte = [0];
    let result = tokio::time::timeout(Duration::from_secs(3), client.read(&mut byte))
        .await
        .unwrap();
    match result {
        Ok(0) => (),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
            ) =>
        {
            ()
        }
        other => panic!("owned socket did not close: {other:?}"),
    }
}

#[tokio::test]
async fn cancelled_connection_closes_owned_socket_without_polling_read() {
    let (mut client, mut server) = pair().await;
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(1, false),
        descriptor(OperationScope::Connection),
        Duration::from_secs(3),
        ready(()),
        async move { read_frame(&mut server).await },
    )
    .await;
    assert_eq!(
        outcome.failure().unwrap().kind,
        OperationFailureKind::Cancelled
    );
    assert_closed(&mut client).await;
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn partial_frame_deadline_closes_socket_and_restores_context() {
    let (mut client, mut server) = pair().await;
    client.write_u32(10).await.unwrap();
    client.write_all(b"x").await.unwrap();
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(2, true),
        descriptor(OperationScope::Message),
        Duration::from_millis(25),
        pending(),
        async move { read_frame(&mut server).await },
    )
    .await;
    let failure = outcome.failure().unwrap();
    assert_eq!(failure.kind, OperationFailureKind::DeadlineExceeded);
    assert_eq!(failure.request_id, context(2, true).request_id);
    assert_closed(&mut client).await;
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn truncated_frame_disconnect_is_a_correlated_error() {
    let (mut client, mut server) = pair().await;
    client.write_u32(5).await.unwrap();
    client.write_all(b"x").await.unwrap();
    client.shutdown().await.unwrap();
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(3, true),
        descriptor(OperationScope::Message),
        Duration::from_secs(3),
        pending(),
        async move { read_frame(&mut server).await },
    )
    .await;
    let failure = outcome.failure().unwrap();
    assert_eq!(failure.kind, OperationFailureKind::Error);
    assert_eq!(failure.request_id, context(3, true).request_id);
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn invalid_length_is_rejected_without_waiting_for_payload() {
    for length in [0, 65, u32::MAX] {
        let (mut client, mut server) = pair().await;
        client.write_u32(length).await.unwrap();
        let outcome = run_operation_boundary_with_timeout_and_cancellation(
            context(4, true),
            descriptor(OperationScope::Message),
            Duration::from_secs(3),
            pending(),
            async move { read_frame(&mut server).await },
        )
        .await;
        assert_eq!(outcome.failure().unwrap().kind, OperationFailureKind::Error);
        assert_closed(&mut client).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_tcp_connections_keep_message_and_connection_scopes_separate() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = async move {
            let mut connections = tokio::task::JoinSet::new();
            for slot in 1..=24 {
                let (mut stream, _) = listener.accept().await.unwrap();
                connections.spawn(async move {
                    let outer = context(slot, false);
                    let outcome = run_operation_boundary(
                        outer.clone(),
                        descriptor(OperationScope::Connection),
                        async {
                            for _ in 0..2 {
                                let inner = context(slot, true);
                                let message = run_operation_boundary_with_timeout_and_cancellation(
                                    inner.clone(),
                                    descriptor(OperationScope::Message),
                                    Duration::from_secs(5),
                                    pending(),
                                    async {
                                        let payload = read_frame(&mut stream).await?;
                                        tokio::task::yield_now().await;
                                        assert_eq!(current_context(), Some(inner));
                                        write_frame(&mut stream, &payload).await?;
                                        Ok::<_, io::Error>(())
                                    },
                                )
                                .await;
                                assert!(message.is_completed());
                                assert_eq!(current_context(), Some(outer.clone()));
                            }
                            Ok::<_, io::Error>(())
                        },
                    )
                    .await;
                    assert!(outcome.is_completed());
                    assert!(current_context().is_none());
                    assert_eq!(current_log_context(), Default::default());
                });
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        };
        let clients = async {
            futures_util::future::join_all((1..=24).map(|slot| async move {
                let mut stream = TcpStream::connect(address).await.unwrap();
                for message in 1..=2 {
                    let payload = format!("client-{slot}-{message}").into_bytes();
                    write_frame(&mut stream, &payload).await.unwrap();
                    assert_eq!(read_frame(&mut stream).await.unwrap(), payload);
                }
            }))
            .await;
        };
        tokio::join!(server, clients);
    })
    .await
    .expect("loopback test must not hang");
}

#[tokio::test]
async fn fully_read_message_panic_does_not_break_the_next_message() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut client, mut server) = pair().await;
        let server_task = async {
            for expected in [OperationFailureKind::Panic, OperationFailureKind::Error] {
                // Framing completes before the recoverable message handler.
                let body = read_frame(&mut server).await.unwrap();
                let outcome = run_operation_boundary_with_timeout_and_cancellation(
                    context(5, true),
                    descriptor(OperationScope::Message),
                    Duration::from_secs(3),
                    pending(),
                    async {
                        if body == b"panic" {
                            panic!("synthetic message panic");
                        }
                        Err::<(), _>(io::Error::other("private message error"))
                    },
                )
                .await;
                assert_eq!(outcome.failure().unwrap().kind, expected);
                write_frame(&mut server, outcome.failure().unwrap().code.as_bytes())
                    .await
                    .unwrap();
                assert!(current_context().is_none());
                assert_eq!(current_log_context(), Default::default());
            }
            let body = read_frame(&mut server).await.unwrap();
            let result = run_operation_boundary(
                context(6, true),
                descriptor(OperationScope::Message),
                async { Ok::<_, io::Error>(body) },
            )
            .await;
            let OperationOutcome::Completed(body) = result else {
                panic!("later message must work")
            };
            write_frame(&mut server, &body).await.unwrap();
        };
        let client_task = async {
            for (body, expected) in [
                ("panic", "operation_panicked"),
                ("error", "operation_failed"),
                ("ok", "ok"),
            ] {
                write_frame(&mut client, body.as_bytes()).await.unwrap();
                assert_eq!(read_frame(&mut client).await.unwrap(), expected.as_bytes());
            }
        };
        tokio::join!(server_task, client_task);
    })
    .await
    .expect("message recovery test must not hang");
}
