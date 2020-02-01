use crate::h1;
use std::fmt;
use std::io;
use tls_api::Error as TlsError;

#[derive(Debug)]
pub enum Error {
    Message(String),
    Static(&'static str),
    Io(io::Error),
    TlsError(tls_api::Error),
    Http11Parser(httparse::Error),
    H2(h2::Error),
    Http(http::Error),
    #[cfg(test)]
    StopTest,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Message(v) => write!(f, "{}", v),
            Error::Static(v) => write!(f, "{}", v),
            Error::Io(v) => fmt::Display::fmt(v, f),
            Error::TlsError(v) => write!(f, "tls: {}", v),
            Error::Http11Parser(v) => write!(f, "http11 parser: {}", v),
            Error::H2(v) => write!(f, "http2: {}", v),
            Error::Http(v) => write!(f, "http api: {}", v),
            #[cfg(test)]
            Error::StopTest => write!(f, "stop test"),
        }
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Message(s)
    }
}

impl<'a> From<&'a str> for Error {
    fn from(s: &'a str) -> Self {
        Error::Message(s.to_owned())
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<TlsError> for Error {
    fn from(e: TlsError) -> Self {
        Error::TlsError(e)
    }
}

impl From<h1::Error> for Error {
    fn from(e: h1::Error) -> Self {
        match e {
            h1::Error::Message(v) => Error::Message(v),
            h1::Error::Io(v) => Error::Io(v),
            h1::Error::Http11Parser(v) => Error::Http11Parser(v),
            h1::Error::Http(v) => Error::Http(v),
        }
    }
}

impl From<h2::Error> for Error {
    fn from(e: h2::Error) -> Self {
        if e.is_io() {
            Error::Io(e.into_io().unwrap())
        } else {
            Error::H2(e)
        }
    }
}

impl From<http::Error> for Error {
    fn from(e: http::Error) -> Self {
        Error::Http(e)
    }
}
