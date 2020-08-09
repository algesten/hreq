use super::chain::{Chain, End, Mid, MidWrap};
use super::path::ParsedPath;
use super::Reply;
use super::Route;
use crate::Body;
use http::Request;
use http::Response;
use std::future::Future;
use std::sync::Arc;
use tracing_futures::Instrument;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteMethod {
    All,
    Method(http::Method),
}

impl PartialEq<http::Method> for RouteMethod {
    fn eq(&self, other: &http::Method) -> bool {
        match self {
            RouteMethod::All => true,
            RouteMethod::Method(m) => m == other,
        }
    }
}

#[derive(Clone)]
pub struct Router<State> {
    prefix: String,
    endpoints: Vec<Endpoint<State>>,
}

impl<State> Router<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub fn new() -> Router<State> {
        Router {
            prefix: "".into(),
            endpoints: vec![],
        }
    }

    /// Routers added as routes receives the prefix they are "mounted" under.
    pub(crate) fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.into();
    }

    pub fn at(&mut self, path: &str) -> Route<'_, State> {
        let path = ParsedPath::parse(path);
        self.reset(&path);
        Route::new(self, path)
    }

    pub(crate) fn reset(&mut self, path: &ParsedPath) {
        self.endpoints.retain(|r| !r.is_path(path));
    }

    pub(crate) fn add_handler(
        &mut self,
        method: RouteMethod,
        path: &ParsedPath,
        mw: Vec<Arc<Mid<State>>>,
        end: End<State>,
    ) {
        let mut chain: Chain<State> = end.into();
        for mid in mw.into_iter().rev() {
            chain = MidWrap::wrap(mid, chain).into();
        }
        self.endpoints.push(Endpoint::new(method, path, chain));
    }

    pub(crate) fn run<'a>(
        &'a self,
        state: Arc<State>,
        mut req: Request<Body>,
    ) -> impl Future<Output = Reply> + Send + 'a {
        let uri = req.uri();
        let full_path = uri.path();

        assert!(full_path.starts_with(&self.prefix));
        let path = full_path.replacen(&self.prefix, "", 1);

        async move {
            for ep in &self.endpoints {
                if &ep.method != req.method() {
                    continue;
                }
                let m = ep.path.path_match(&path);
                trace!("Found endpoint: {:?}", ep);
                if let Some(m) = m {
                    req.extensions_mut().insert(m);
                    return ep.chain.run(state, req).await;
                }
            }
            trace!("No endpoint");
            Response::builder().status(404).body("Not found").into()
        }
        .instrument(trace_span!("router_run"))
    }
}

#[derive(Clone)]
struct Endpoint<State> {
    method: RouteMethod,
    path: ParsedPath,
    chain: Chain<State>,
}

impl<State> Endpoint<State> {
    fn new(method: RouteMethod, path: &ParsedPath, chain: Chain<State>) -> Self {
        Endpoint {
            method,
            path: path.clone(),
            chain,
        }
    }

    fn is_path(&self, path: &ParsedPath) -> bool {
        &self.path == path
    }
}

use std::fmt;

impl<State> fmt::Debug for Endpoint<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Endpoint {{ method: {:?}, path: {:?}, chain: {:?} }}",
            self.method, self.path, self.chain
        )
    }
}
