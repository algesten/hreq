//! The structure cookie::CookieJar does not separate cookies per domain. Cookies does.

use crate::psl::PUBLIC_SUFFIX_LIST;
use crate::uri_ext::UriExt;
use cookie::{Cookie, CookieJar};
use std::collections::hash_map::HashMap;
use time::{Duration, OffsetDateTime};

/// Technically a cookie without a max age, is a session cookie. hreq currently
/// considers the lifetime of a session to be that of the Agent, we therefore
/// just offset sessions cookies indefinitely.
const DEFAULT_COOKIE_MAX_AGES_DAYS: i64 = 9999;

#[derive(Debug)]
pub(crate) struct Cookies {
    domains: HashMap<String, CookieJar>,
}

impl Cookies {
    pub fn new() -> Self {
        Cookies {
            domains: HashMap::new(),
        }
    }

    pub fn add(&mut self, uri: &http::Uri, mut cookie: Cookie<'static>) {
        let domain = match cookie.validated_domain(uri) {
            Some(v) => v,
            // the reason is logged already
            None => return,
        };
        // all cookies must have an expires so we know when to remove them.
        if cookie.expires().is_none() {
            let max = if let Some(max) = cookie.max_age() {
                max
            } else {
                Duration::days(DEFAULT_COOKIE_MAX_AGES_DAYS)
            };
            let exp = OffsetDateTime::now() + max;
            cookie.set_expires(Some(exp))
        }
        let jar = self.domains.entry(domain).or_insert_with(CookieJar::new);
        jar.add(cookie);
    }

    pub fn get(&self, uri: &http::Uri) -> Vec<&Cookie<'static>> {
        let mut ret = vec![];

        let is_secure = uri.is_secure();
        let now = OffsetDateTime::now();

        // hold current host name. will go "a.b.com", "b.com", "com"
        let mut cur = Some(uri.clone());
        loop {
            // current host name, normalized
            let maybe_host = cur
                .as_ref()
                .and_then(|c| c.host())
                .map(|h| h.to_ascii_lowercase());

            // no more host name? that breaks the loop
            let host = match maybe_host {
                Some(v) => v,
                None => break,
            };

            // if we have a jar for this hostname, add all the cookies with
            // matching path in it.
            if let Some(jar) = self.domains.get(&host) {
                for cookie in jar.iter() {
                    // if there is no path in the cookie, it's a match.
                    let path_match = cookie
                        .path()
                        .map(|p| uri.path().starts_with(p))
                        .unwrap_or(true);

                    // if we are using https, no need to check cookie.
                    let secure_match = is_secure || !cookie.secure().unwrap_or(false);

                    // unwrap is ok cause all cookies have expires() after added to jars above.
                    let expired = cookie.expires().unwrap() < now;

                    if path_match && secure_match && !expired {
                        ret.push(cookie);
                    }
                }
            }

            cur = cur.unwrap().parent_host();
        }

        ret
    }
}

pub(crate) trait CookieExt
where
    Self: Sized,
{
    fn validated_domain(&self, uri: &http::Uri) -> Option<String>;
}

impl<'c> CookieExt for Cookie<'c> {
    fn validated_domain(&self, uri: &http::Uri) -> Option<String> {
        let effective = match effective_domain(self.domain(), uri) {
            Some(v) => v,
            None => {
                return None;
            }
        };

        if !is_valid_cookie_domain(&effective, self.name()) {
            return None;
        }

        Some(effective)
    }
}

fn effective_domain(cookie_domain: Option<&str>, uri: &http::Uri) -> Option<String> {
    let host = match uri.host() {
        Some(h) => h,
        None => {
            debug!("Ignore cookie for uri without a host: {}", uri);
            return None;
        }
    }
    // normalized
    .to_ascii_lowercase();

    let cookie_domain = match cookie_domain {
        Some(v) => v.to_ascii_lowercase(),
        None => {
            trace!("No domain in cookie, using uri host: {}", host);
            return Some(host);
        }
    };

    // the cookie must be the same or a sub-domain of the uri host.
    if host.ends_with(&cookie_domain) {
        Some(cookie_domain)
    } else {
        trace!(
            "Ignore cookie where domain doesn't match host domain: {} != {}",
            cookie_domain,
            host
        );
        None
    }
}

fn is_valid_cookie_domain(domain: &str, name: &str) -> bool {
    let parsed = match PUBLIC_SUFFIX_LIST.parse_domain(domain) {
        Ok(v) => v,
        Err(e) => {
            // this will catch TLD cookie domains such as "co.uk", "com" etc.
            trace!("Ignore cookie with bad domain ({}): {}", domain, e);
            return false;
        }
    };
    // for "blah.example.com," suffix is "com" and root is "example.com"
    match (parsed.root(), parsed.suffix()) {
        (Some(root), Some(sufx)) => {
            trace!(
                "Accept cookie domain with root '{}' and suffix '{}': {}",
                root,
                sufx,
                name
            );
        }
        _ => {
            trace!(
                "Ignore cookie with root '{:?}' and suffix '{:?}': {}",
                parsed.root(),
                parsed.suffix(),
                name
            );
            return false;
        }
    }
    true
}

#[cfg(test)]
mod test {
    use super::*;

    const EXPECTED_EFFECT: &[(Option<&str>, &str, Option<&str>)] = &[
        (Some("EXAMPLE.com"), "example.com", Some("example.com")),
        (Some("other.com"), "example.com", None),
        (Some("b.com"), "sub.B.com", Some("b.com")),
        (Some("sub.b.com"), "B.com", None),
        (Some("com"), "B.com", Some("com")), // caught by is_valid_cookie_domain
    ];

    #[test]
    fn effective_cookie_domain() {
        for (test, uri, expect) in EXPECTED_EFFECT {
            let uri = http::Uri::from_static(uri);
            assert_eq!(effective_domain(*test, &uri), expect.map(|s| s.to_string()));
        }
    }

    const EXPECTED_VALID: &[(&str, bool)] = &[
        ("EXAMPLE.com", true),
        ("a.b.com", true),
        ("com", false),
        ("foo.myownspecialdomain", true),
        ("a.co.uk", true),
        ("co.uk", false),
        ("gmail", false),
        ("gmail.com", true),
        ("a.gmail.com", true),
        ("_tcp.example.com", false),
    ];

    #[test]
    fn valid_cookie_domain() {
        for (test, expect) in EXPECTED_VALID {
            assert_eq!(is_valid_cookie_domain(test, "test"), *expect);
        }
    }
}
