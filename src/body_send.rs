use crate::Error;
use bytes::Bytes;
use futures_util::future::poll_fn;
use hreq_h1 as h1;
use hreq_h2 as h2;

/// Generalisation over sending body data.
pub(crate) enum BodySender {
    H1(h1::SendStream),
    H2(h2::SendStream<Bytes>),
}

impl BodySender {
    pub async fn send_data(&mut self, mut buf: &[u8]) -> Result<(), Error> {
        if buf.is_empty() {
            return Ok(());
        }

        match self {
            BodySender::H1(s) => Ok(s.send_data(buf, false).await?),
            BodySender::H2(s) => {
                loop {
                    if buf.len() == 0 {
                        break;
                    }

                    s.reserve_capacity(buf.len());

                    let actual_capacity = {
                        let cur = s.capacity();
                        if cur > 0 {
                            cur
                        } else {
                            poll_fn(|cx| s.poll_capacity(cx)).await.ok_or_else(|| {
                                Error::Proto("Stream gone before capacity".into())
                            })??
                        }
                    };

                    // h2::SendStream lacks a sync or async function that allows us
                    // to send borrowed data. This copy is unfortunate.
                    //
                    // TODO: See if h2 could handle some kind of variant that takes a &mut [u8].
                    let data = Bytes::copy_from_slice(&buf[..actual_capacity]);

                    s.send_data(data, false)?;

                    buf = &buf[actual_capacity..];
                }

                Ok(())
            }
        }
    }

    pub async fn send_end(&mut self) -> Result<(), Error> {
        match self {
            BodySender::H1(s) => Ok(s.send_data(&[], true).await?),
            BodySender::H2(s) => Ok(s.send_data(Bytes::new(), true)?),
        }
    }
}
