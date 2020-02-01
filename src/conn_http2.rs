use crate::body::BodyImpl;
use crate::req_ext::RequestParams;
use crate::Body;
use crate::Error;
use bytes::Bytes;
use futures_util::future::poll_fn;
use h2::client::SendRequest;
use std::sync::Arc;

const BUF_SIZE: usize = 16_384;

pub async fn send_request_http2(
    send_req: SendRequest<Bytes>,
    req: http::Request<Body>,
    unfinished_recs: Arc<()>,
) -> Result<http::Response<Body>, Error> {
    //
    let params = *req.extensions().get::<RequestParams>().unwrap();

    let mut h2 = send_req.ready().await?;

    let (parts, mut body_read) = req.into_parts();
    let req = http::Request::from_parts(parts, ());

    let no_body = body_read.is_definitely_no_body();
    let (fut_res, mut send_body) = h2.send_request(req, no_body)?;

    if !no_body {
        let mut buf = vec![0_u8; BUF_SIZE];
        loop {
            let amount_read = body_read.read(&mut buf[..]).await?;
            if amount_read == 0 {
                break;
            }
            let mut amount_sent = 0;
            loop {
                let left_to_send = amount_read - amount_sent;
                send_body.reserve_capacity(left_to_send);
                let actual_capacity = poll_fn(|cx| send_body.poll_capacity(cx))
                    .await
                    .ok_or_else(|| Error::Static("Stream gone before capacity"))??;
                // let actual_capacity = fut_cap.await?;
                send_body.send_data(
                    // h2::SendStream lacks a sync or async function that allows us
                    // to send borrowed data. This copy is unfortunate.
                    // TODO contact h2 and ask if they would consider some kind of
                    // async variant that takes a &mut [u8].
                    Bytes::copy_from_slice(&buf[amount_sent..(amount_sent + actual_capacity)]),
                    false,
                )?;
                amount_sent += actual_capacity;
            }
        }

        // Send end_of_stream
        send_body.send_data(Bytes::new(), true)?;
    }

    let res = fut_res.await?;

    trace!("Got Http2 response: {:?}", res);

    let (mut parts, res_body) = res.into_parts();
    parts.extensions.insert(params);

    let mut res_body = Body::new(BodyImpl::Http2(res_body), None, Some(unfinished_recs));
    res_body.configure(params.deadline(), &parts.headers, true);

    let res = http::Response::from_parts(parts, res_body);

    Ok(res)
}
