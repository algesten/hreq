use crate::body::BodyImpl;
use crate::h1::SendRequest;
use crate::req_ext::RequestParams;
use crate::Body;
use crate::Error;
use std::sync::Arc;

const BUF_SIZE: usize = 16_384;

pub async fn send_request_http1(
    send_req: SendRequest,
    req: http::Request<Body>,
    unfinished_recs: Arc<()>,
) -> Result<http::Response<Body>, Error> {
    //
    let params = *req.extensions().get::<RequestParams>().unwrap();

    let mut h1 = send_req; // .ready().await?;

    let (parts, mut body) = req.into_parts();
    let req = http::Request::from_parts(parts, ());

    let no_body = body.is_definitely_no_body();
    let (fut_res, mut send_body) = h1.send_request(req, no_body)?;

    if !no_body {
        let mut buf = vec![0_u8; BUF_SIZE];
        loop {
            // wait for send_body to be able to receive more data
            send_body = send_body.ready().await?;
            let amount_read = body.read(&mut buf[..]).await?;
            if amount_read == 0 {
                break;
            }
            send_body.send_data(&buf[..amount_read], false)?;
        }

        // Send end_of_stream
        send_body.send_data(&[], true)?;
    }

    let (mut parts, res_body) = fut_res.await?.into_parts();
    parts.extensions.insert(params);

    let mut res_body = Body::new(BodyImpl::Http1(res_body), None, Some(unfinished_recs));
    res_body.configure(params.deadline(), &parts.headers, true);

    let res = http::Response::from_parts(parts, res_body);

    Ok(res)
}
