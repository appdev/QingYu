use std::io::{self, Read as _};

use axum::{
    body::{Body, Bytes},
    http::{header, HeaderValue},
    response::Response,
};
use futures_util::stream;

use crate::resources::RetainedResource;

const STREAM_CHUNK_BYTES: usize = 64 * 1024;

pub(crate) async fn response(mut resource: RetainedResource) -> Result<Response, ()> {
    let entry = resource.entry().clone();
    let size = entry.size_bytes.get();
    if size == 0 {
        resource = tokio::task::spawn_blocking(move || {
            resource.verify_complete()?;
            Ok::<_, io::Error>(resource)
        })
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    }
    let body = if size == 0 {
        Body::empty()
    } else {
        Body::from_stream(stream::unfold(
            StreamState::Streaming {
                resource: Box::new(resource),
                pending: None,
            },
            |state| async move {
                let StreamState::Streaming { resource, pending } = state else {
                    return None;
                };
                match tokio::task::spawn_blocking(move || advance(resource, pending)).await {
                    Ok(Ok(Advance::Yield { bytes, state })) => Some((Ok(bytes), state)),
                    Ok(Ok(Advance::Complete)) => None,
                    Ok(Err(error)) => Some((Err(error), StreamState::Done)),
                    Err(_) => Some((
                        Err(io::Error::other("resource stream unavailable")),
                        StreamState::Done,
                    )),
                }
            },
        ))
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&entry.media_type).map_err(|_| ())?,
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&size.to_string()).map_err(|_| ())?,
    );
    Ok(response)
}

enum StreamState {
    Streaming {
        resource: Box<RetainedResource>,
        pending: Option<Bytes>,
    },
    Done,
}

enum Advance {
    Yield { bytes: Bytes, state: StreamState },
    Complete,
}

fn advance(mut resource: Box<RetainedResource>, mut pending: Option<Bytes>) -> io::Result<Advance> {
    loop {
        let mut buffer = vec![0_u8; STREAM_CHUNK_BYTES];
        let read = resource.read(&mut buffer)?;
        if read == 0 {
            resource.verify_complete()?;
            return Ok(pending.map_or(Advance::Complete, |bytes| Advance::Yield {
                bytes,
                state: StreamState::Done,
            }));
        }
        buffer.truncate(read);
        let current = Bytes::from(buffer);
        if let Some(bytes) = pending.replace(current) {
            return Ok(Advance::Yield {
                bytes,
                state: StreamState::Streaming { resource, pending },
            });
        }
    }
}
