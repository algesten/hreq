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

                    let actual_capacity = loop {
                        s.reserve_capacity(buf.len());

                        let capacity = s.capacity();

                        if capacity > 0 {
                            break capacity;
                        }

                        // wait for capacity to increase
                        let capacity = poll_fn(|cx| s.poll_capacity(cx))
                            .await
                            .ok_or_else(|| Error::Proto("Stream gone before capacity".into()))??;

                        if capacity > 0 {
                            break capacity;
                        }
                    };

                    let data = (&buf[..actual_capacity]).to_vec().into();

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
