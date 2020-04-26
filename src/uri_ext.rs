use crate::Error;
use std::fmt;

const DEFAULT_PORT_HTTP: &str = "80";
const DEFAULT_PORT_HTTPS: &str = "443";

pub trait MethodExt {
    fn indicates_body(&self) -> bool;
}

impl MethodExt for http::Method {
    fn indicates_body(&self) -> bool {
        match *self {
            http::Method::POST | http::Method::PUT | http::Method::PATCH => true,
            _ => false,
        }
    }
}

pub(crate) trait UriExt {
    fn host_port(&self) -> Result<HostPort<'_>, Error>;
    fn parse_relative(&self, from: &str) -> Result<http::Uri, Error>;
    fn parent_host(&self) -> Option<http::Uri>;
    fn is_secure(&self) -> bool;
}

impl UriExt for http::Uri {
    fn host_port(&self) -> Result<HostPort<'_>, Error> {
        HostPort::from_uri(self)
    }
    fn parse_relative(&self, from: &str) -> Result<http::Uri, Error> {
        let uri_res: Result<http::Uri, http::Error> =
            from.parse::<http::Uri>().map_err(|e| e.into());
        let uri = uri_res?;
        match (uri.scheme(), uri.authority()) {
            (Some(_), Some(_)) => Ok(uri),
            (None, None) => {
                // it's relative to the original url
                let mut parts = uri.into_parts();
                parts.scheme = self.scheme().cloned();
                parts.authority = self.authority().cloned();
                Ok(http::Uri::from_parts(parts).unwrap())
            }
            _ => Err(Error::Proto(format!(
                "Failed to parse '{}' relative to: {}",
                uri, from
            ))),
        }
    }
    fn parent_host(&self) -> Option<http::Uri> {
        let mut parts = self.clone().into_parts();
        let auth = parts.authority?;

        // from the current host, try to figure out a parent host.
        let host = auth.host();
        if !host.contains('.') {
            // no parent to this uri
            return None;
        }
        let parent = host.split('.').skip(1).collect::<Vec<_>>().join(".");

        // http::uri::Authority doesn't give us easy access to this part sadly.
        let upwd = if auth.as_str().contains('@') {
            let upwd: String = auth.as_str().chars().take_while(|c| c != &'@').collect();
            Some(upwd)
        } else {
            None
        };

        // assemble the new authority
        let mut new_auth = parent;
        if let Some(upwd) = upwd {
            new_auth = format!("{}@{}", upwd, new_auth);
        };
        if let Some(port) = auth.port() {
            new_auth = format!("{}:{}", new_auth, port);
        }
        let fake_uri = format!("http://{}", new_auth);
        let new_auth = fake_uri
            .parse::<http::Uri>()
            .expect("Parse fake uri")
            .into_parts()
            .authority;

        // change only the authority of the parts
        parts.authority = new_auth;

        Some(http::Uri::from_parts(parts).expect("Parent uri"))
    }
    fn is_secure(&self) -> bool {
        self.host_port().ok().map(|x| x.is_tls()).unwrap_or(false)
    }
}

pub enum HostPort<'a> {
    HasPort {
        host: &'a str,
        with_port: &'a str,
        is_tls: bool,
    },
    DefaultPort {
        host: &'a str,
        port: &'a str,
        is_tls: bool,
    },
}

impl<'a> HostPort<'a> {
    pub fn from_uri(uri: &'a http::Uri) -> Result<Self, Error> {
        let scheme = uri
            .scheme()
            .ok_or_else(|| Error::User(format!("URI without scheme: {}", uri)))?
            .as_str();

        let authority = uri
            .authority()
            .ok_or_else(|| Error::User(format!("URI without authority: {}", uri)))?;

        let has_port = authority.port().is_some();

        let hostport = if has_port {
            HostPort::HasPort {
                host: authority.host(),
                with_port: authority.as_str(),
                is_tls: scheme == "https",
            }
        } else {
            let scheme_default = match scheme {
                "http" => DEFAULT_PORT_HTTP,
                "https" => DEFAULT_PORT_HTTPS,
                _ => return Err(Error::User(format!("Unknown URI scheme: {}", uri))),
            };
            HostPort::DefaultPort {
                host: authority.as_str(),
                port: scheme_default,
                is_tls: scheme == "https",
            }
        };

        Ok(hostport)
    }

    pub fn host(&self) -> &str {
        match self {
            HostPort::HasPort { host, .. } => host,
            HostPort::DefaultPort { host, .. } => host,
        }
    }

    pub fn is_tls(&self) -> bool {
        match self {
            HostPort::HasPort { is_tls, .. } => *is_tls,
            HostPort::DefaultPort { is_tls, .. } => *is_tls,
        }
    }
}

impl<'a> fmt::Display for HostPort<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostPort::HasPort { with_port, .. } => write!(f, "{}", with_port),
            HostPort::DefaultPort { host, port, .. } => write!(f, "{}:{}", host, port),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const PARENT_HOST: &[(&str, Option<&str>)] = &[
        ("http://a.example.com/", Some("http://example.com/")),
        ("http://example.com/", Some("http://com/")),
        ("http://com/", None),
        (
            "http://user:pass@a.example.com:1234/path",
            Some("http://user:pass@example.com:1234/path"),
        ),
        ("/path", None),
    ];

    #[test]
    fn parent_host() {
        for (test, expect) in PARENT_HOST {
            let uri = test.parse::<http::Uri>().unwrap();
            let parent = uri.parent_host();
            assert_eq!(parent.map(|u| u.to_string()), expect.map(|s| s.to_string()));
        }
    }
}
