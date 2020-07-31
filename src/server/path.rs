use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct ParsedPath {
    path: String,
    segments: Vec<Segment>,
    matcher: Regex,
}

// equality for the parsed path is over the segments, not regex.
impl PartialEq for ParsedPath {
    fn eq(&self, other: &Self) -> bool {
        self.segments == other.segments
    }
}

impl Eq for ParsedPath {}

impl ParsedPath {
    pub fn parse(s: &str) -> Self {
        let segments = Segment::from(s);

        let reg_s: String = format!("^{}$", segments.as_regex());
        let matcher = Regex::new(&reg_s).unwrap();

        ParsedPath {
            path: s.into(),
            segments,
            matcher,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn path_match(&self, s: &str) -> Option<PathMatch> {
        if let Some(cap) = self.matcher.captures(s) {
            let mut ret = PathMatch::new();
            for seg in &self.segments {
                if let Segment::Wildcard(_, name) = seg {
                    if name != "" {
                        let m = cap.name(&name).expect("Path match without param");
                        ret.add(&name[..], m.as_str());
                    }
                }
            }
            return Some(ret);
        }
        None
    }
}

pub(crate) struct PathMatch {
    params: HashMap<String, String>,
}

impl PathMatch {
    fn new() -> Self {
        PathMatch {
            params: HashMap::new(),
        }
    }

    fn add(&mut self, k: &str, v: &str) {
        self.params.insert(k.to_string(), v.to_string());
    }

    pub fn get_param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(|s| s.as_ref())
    }

    pub fn all_params(&self) -> Vec<(&str, &str)> {
        self.params
            .iter()
            .map(|(k, v)| (k.as_ref(), v.as_ref()))
            .collect()
    }
}

#[derive(Debug, Clone, Eq)]
enum Segment {
    Literal(String),
    Wildcard(bool, String),
}

impl PartialEq for Segment {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Segment::Literal(l1), Segment::Literal(l2)) => l1 == l2,
            // wildcard names are not considered for equality
            (Segment::Wildcard(r1, _), Segment::Wildcard(r2, _)) => r1 == r2,
            _ => false,
        }
    }
}

trait Segments {
    fn as_regex(&self) -> String;
}

impl Segments for Vec<Segment> {
    fn as_regex(&self) -> String {
        self.iter().map(|s| s.as_regex()).collect()
    }
}

impl Segment {
    fn from(s: &str) -> Vec<Segment> {
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"(/:|/\*)([_0-9a-zA-Z]*)|(/?[^/]*)").unwrap());

        RE.captures_iter(s)
            .map(|cap| {
                let text = cap.get(3).or(cap.get(2)).unwrap().as_str();
                match cap.get(1) {
                    None => Segment::Literal(text.to_string()),
                    Some(v) => match v.as_str() {
                        "/" => Segment::Literal(format!("/{}", text)),
                        "/:" => Segment::Wildcard(false, text.to_string()),
                        "/*" => Segment::Wildcard(true, text.to_string()),
                        _ => panic!("Unexpected wildcard designator"),
                    },
                }
            })
            .fold(vec![], |mut p, c| {
                // if last is rest, no more segment.s
                let last_is_rest = p.last().map(|s| s.is_rest()).unwrap_or(false);
                let is_empty_literal = c.is_empty_literal();
                if last_is_rest || is_empty_literal {
                    return p;
                }
                if let Some(l) = p.last_mut() {
                    // merge consecutive literals
                    if !l.merge_literal(&c) {
                        p.push(c);
                    }
                } else {
                    p.push(c);
                }
                p
            })
    }

    fn is_rest(&self) -> bool {
        if let Segment::Wildcard(rest, _) = self {
            return *rest;
        }
        false
    }

    fn is_empty_literal(&self) -> bool {
        if let Segment::Literal(l) = self {
            return l == "";
        }
        false
    }

    fn merge_literal(&mut self, other: &Segment) -> bool {
        match (self, other) {
            (Segment::Literal(l), Segment::Literal(o)) => {
                l.push_str(o);
                true
            }
            _ => false,
        }
    }

    fn as_regex(&self) -> String {
        // /               => /
        // /path           => /path
        // /:              => /([^/]*)
        // /:param         => /(?P<param>[^/]*)
        // /*              => /(.*)
        // /*rest          => /(?P<rest>.*)
        match self {
            Segment::Literal(l) => format!("({})", regex::escape(l)),
            Segment::Wildcard(rest, name) => {
                let wild = if *rest { ".*" } else { "[^/]*" };
                match &name[..] {
                    "" => format!("/({})", wild),
                    _ => format!("/(?P<{}>{})", name, wild),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn segment_to() {
        use Segment::*;
        let cases = vec![
            (vec![Literal("".into())], "()"),
            (vec![Literal("foo".into())], "(foo)"),
            (vec![Wildcard(false, "".into())], "/([^/]*)"),
            (vec![Wildcard(false, "param".into())], "/(?P<param>[^/]*)"),
            (vec![Wildcard(true, "".into())], "/(.*)"),
            (vec![Wildcard(true, "rest".into())], "/(?P<rest>.*)"),
        ];

        for (segs, result) in cases {
            assert_eq!(segs.as_regex(), result);
        }
    }

    #[test]
    fn segment_from() {
        use Segment::*;
        // /               => /
        // /path           => /path
        // /:              => /([^/]*)
        // /:param         => /(?P<param>[^/]*)
        // /*              => /(.*)
        // /*rest          => /(?P<rest>.*)
        let cases = vec![
            ("", vec![]),
            ("foo", vec![Literal("foo".into())]),
            ("foo/", vec![Literal("foo/".into())]),
            ("foo/bar", vec![Literal("foo/bar".into())]),
            ("foo/bar", vec![Literal("foo/bar".into())]),
            ("/", vec![Literal("/".into())]),
            ("/foo", vec![Literal("/foo".into())]),
            ("/:", vec![Wildcard(false, "".into())]),
            ("/:", vec![Wildcard(false, "param".into())]),
            ("/*", vec![Wildcard(true, "".into())]),
            ("/*rest", vec![Wildcard(true, "rest".into())]),
            (
                "/foo/:param",
                vec![Literal("/foo".into()), Wildcard(false, "param".into())],
            ),
            (
                "/foo/*rest",
                vec![Literal("/foo".into()), Wildcard(true, "rest".into())],
            ),
            (
                "/:param/foo",
                vec![Wildcard(false, "param".into()), Literal("/foo".into())],
            ),
            ("/*rest/foo", vec![Wildcard(true, "rest".into())]),
        ];

        for (expr, result) in cases {
            assert_eq!(Segment::from(expr), result);
        }
    }
}
