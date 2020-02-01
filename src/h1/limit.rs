use super::chunked::{ChunkedDecoder, ChunkedEncoder};
use super::Error;
use super::RecvReader;
use crate::RequestExt;
use crate::ResponseExt;
use futures_util::ready;
use std::io;
use std::task::{Context, Poll};

pub(crate) enum LimitRead {
    ChunkedDecoder(ChunkedDecoder),
    ContenLength(ContentLengthRead),
    UntilEnd(UntilEnd),
}

impl LimitRead {
    pub fn from_response(res: &http::Response<()>) -> Self {
        let transfer_enc_chunk = res
            .headers()
            .get("transfer-encoding")
            .map(|h| h == "chunked")
            .unwrap_or(false);

        let content_length = res.header_as::<u64>("content-length");

        let use_chunked = transfer_enc_chunk || content_length.is_none();

        if use_chunked {
            LimitRead::ChunkedDecoder(ChunkedDecoder::new())
        } else if let Some(size) = content_length {
            LimitRead::ContenLength(ContentLengthRead::new(size))
        } else {
            LimitRead::UntilEnd(UntilEnd)
        }
    }

    pub fn is_reusable_conn(&self) -> bool {
        // limiters read to stream end can't reuse connection.
        if let LimitRead::UntilEnd(_) = self {
            return false;
        }
        true
    }

    // pub async fn read_from(
    //     &mut self,
    //     recv: &mut RecvReader,
    //     buf: &mut [u8],
    // ) -> Result<usize, Error> {
    //     match self {
    //         LimitRead::ChunkedDecoder(v) => v.read_chunk(recv, buf).await,
    //         LimitRead::ContenLength(v) => v.read_from(recv, buf).await,
    //         LimitRead::UntilEnd(v) => v.read_from(recv, buf).await,
    //     }
    // }

    pub fn poll_read(
        &mut self,
        cx: &mut Context,
        recv: &mut RecvReader,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self {
            LimitRead::ChunkedDecoder(v) => v.poll_read(cx, recv, buf),
            LimitRead::ContenLength(v) => v.poll_read(cx, recv, buf),
            LimitRead::UntilEnd(v) => v.poll_read(cx, recv, buf),
        }
    }
}

pub struct ContentLengthRead {
    limit: u64,
    total: u64,
}

impl ContentLengthRead {
    fn new(limit: u64) -> Self {
        ContentLengthRead { limit, total: 0 }
    }
    fn poll_read(
        &mut self,
        cx: &mut Context,
        recv: &mut RecvReader,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let left = (self.limit - self.total).min(usize::max_value() as u64) as usize;
        if left == 0 {
            return Ok(0).into();
        }
        let max = buf.len().min(left);
        let amount = ready!(recv.poll_read(cx, &mut buf[0..max]))?;
        self.total += amount as u64;
        Ok(amount).into()
    }
}

pub struct UntilEnd;

impl UntilEnd {
    fn poll_read(
        &mut self,
        cx: &mut Context,
        recv: &mut RecvReader,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        recv.poll_read(cx, &mut buf[..])
    }
}

pub(crate) enum LimitWrite {
    ChunkedEncoder,
    ContentLength(ContentLengthWrite),
}

impl LimitWrite {
    pub fn from_request(req: &http::Request<()>) -> Self {
        let transfer_enc_chunk = req
            .headers()
            .get("transfer-encoding")
            .map(|h| h == "chunked")
            .unwrap_or(false);

        let content_length = req.header_as::<u64>("content-length");

        if let Some(content_length) = content_length {
            if transfer_enc_chunk {
                // this is technically an error – what is the most common error combo
                // and what does the user mean with it?
                warn!("Ignoring transfer-encoding: chunked in favor of content-length");
            }
            LimitWrite::ContentLength(ContentLengthWrite::new(content_length))
        } else {
            LimitWrite::ChunkedEncoder
        }
    }

    /// Extra overhead bytes per send_data() call.
    pub fn overhead(&self) -> usize {
        match self {
            LimitWrite::ChunkedEncoder => 32,
            LimitWrite::ContentLength(_) => 0,
        }
    }

    pub fn write(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            LimitWrite::ChunkedEncoder => ChunkedEncoder::write_chunk(data, out),
            LimitWrite::ContentLength(v) => v.write(data, out),
        }
    }

    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            LimitWrite::ChunkedEncoder => ChunkedEncoder::write_finish(out),
            LimitWrite::ContentLength(_) => Ok(()),
        }
    }
}

pub struct ContentLengthWrite {
    limit: u64,
    total: u64,
}

impl ContentLengthWrite {
    fn new(limit: u64) -> Self {
        ContentLengthWrite { limit, total: 0 }
    }

    fn write(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        self.total += data.len() as u64;
        if self.total > self.limit {
            let m = format!(
                "Body data longer than content-length header: {} > {}",
                self.total, self.limit
            );
            return Err(Error::Message(m));
        }
        let cur_len = out.len();
        out.resize(cur_len + data.len(), 0);
        (&mut out[cur_len..]).copy_from_slice(data);
        Ok(())
    }
}
