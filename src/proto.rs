#[derive(Debug, PartialEq, Eq)]
pub enum Protocol {
    Http11,
    Http2,
    Unknown,
}

pub const ALPN_H1: &[u8] = b"http/1.1";
pub const ALPN_H2: &[u8] = b"h2";

impl Protocol {
    pub fn from_alpn(alpn: &Option<Vec<u8>>) -> Self {
        if let Some(v) = alpn {
            if v.len() == 8 && &v[..] == ALPN_H1 {
                Protocol::Http11
            } else if v.len() == 2 && &v[..] == ALPN_H2 {
                Protocol::Http2
            } else {
                Protocol::Unknown
            }
        } else {
            Protocol::Unknown
        }
    }
}
