use axum::{
    body::Body,
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::get,
    Router,
};
use futures::{Stream, StreamExt, TryStreamExt};
use mcp_server::{ByteTransport, Server};
use std::collections::HashMap;
use tokio_util::codec::FramedRead;

#[cfg(test)]
// Tests in ../tests.rs

use anyhow::Result;
use mcp_server::router::RouterService;
use crate::{transport::jsonrpc_frame_codec::JsonRpcFrameCodec, tools::DocRouter};
use std::sync::Arc;
use tokio::{
    io::{self, AsyncWriteExt},
    sync::Mutex,
};

type C2SWriter = Arc<Mutex<io::WriteHalf<io::SimplexStream>>>;
type SessionId = Arc<str>;

#[derive(Clone, Default)]
pub struct App {
    pub txs: Arc<tokio::sync::RwLock<HashMap<SessionId, C2SWriter>>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            txs: Default::default(),
        }
    }
    pub fn router(&self) -> Router {
        Router::new()
            .route("/sse", get(sse_handler).post(post_event_handler))
            .with_state(self.clone())
    }
}

fn session_id() -> SessionId {
    let id = format!("{:016x}", rand::random::<u128>());
    Arc::from(id)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostEventQuery {
    pub session_id: String,
}

async fn post_event_handler(
    State(app): State<App>,
    Query(PostEventQuery { session_id }): Query<PostEventQuery>,
    body: Body,
) -> Result<StatusCode, StatusCode> {
    const BODY_BYTES_LIMIT: usize = 1 << 22;
    let write_stream = {
        let rg = app.txs.read().await;
        rg.get(session_id.as_str())
            .ok_or(StatusCode::NOT_FOUND)?
            .clone()
    };
    let mut write_stream = write_stream.lock().await;
    let mut body = body.into_data_stream();
    if let (_, Some(size)) = body.size_hint() {
        if size > BODY_BYTES_LIMIT {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    }
    // calculate the body size
    let mut size = 0;
    while let Some(chunk) = body.next().await {
        let Ok(chunk) = chunk else {
            return Err(StatusCode::BAD_REQUEST);
        };
        size += chunk.len();
        if size > BODY_BYTES_LIMIT {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        write_stream
            .write_all(&chunk)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    write_stream
        .write_u8(b'\n')
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::ACCEPTED)
}

async fn sse_handler(State(app): State<App>) -> Sse<impl Stream<Item = Result<Event, io::Error>>> {
    // it's 4KB
    const BUFFER_SIZE: usize = 1 << 12;
    let session = session_id();
    tracing::info!(%session, "sse connection");
    let (c2s_read, c2s_write) = tokio::io::simplex(BUFFER_SIZE);
    let (s2c_read, s2c_write) = tokio::io::simplex(BUFFER_SIZE);
    app.txs
        .write()
        .await
        .insert(session.clone(), Arc::new(Mutex::new(c2s_write)));
    {
        let app_clone = app.clone();
        let session = session.clone();
        tokio::spawn(async move {
            let router = RouterService(DocRouter::new());
            let server = Server::new(router);
            let bytes_transport = ByteTransport::new(c2s_read, s2c_write);
            let _result = server
                .run(bytes_transport)
                .await
                .inspect_err(|e| tracing::error!(?e, "server run error"));
            app_clone.txs.write().await.remove(&session);
        });
    }

    let stream = futures::stream::once(futures::future::ok(
        Event::default()
            .event("endpoint")
            .data(format!("?sessionId={session}")),
    ))
    .chain(
        FramedRead::new(s2c_read, JsonRpcFrameCodec)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            .and_then(move |bytes| match std::str::from_utf8(bytes.as_ref()) {
                Ok(message) => futures::future::ok(Event::default().event("message").data(message)),
                Err(e) => futures::future::err(io::Error::new(io::ErrorKind::InvalidData, e)),
            }),
    );
    Sse::new(stream)
}