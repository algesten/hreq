use super::Reply;
use super::Router;
use super::{Handler, StateHandler};
use super::{Middleware, StateMiddleware};
use crate::Body;
use crate::Error;
use http::{Request, Response};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing_futures::Instrument;

/// Endpoint, handler or a router.
#[derive(Clone)]
pub(crate) enum End<State> {
    Handler(Arc<Box<dyn Handler>>),
    StateHandler(Arc<Box<dyn StateHandler<State>>>),
    Router(Router<State>),
}

impl<State> End<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub fn run<'a>(
        &'a self,
        state: Arc<State>,
        req: Request<Body>,
    ) -> impl Future<Output = Reply> + Send + 'a {
        async move {
            match self {
                End::Handler(h) => h.call(req).await,
                End::StateHandler(h) => h.call((*state).clone(), req).await,
                End::Router(r) => r.run(state, req).await,
            }
        }
        .instrument(trace_span!("endpoint_run"))
    }
}

/// Middleware, with state and not.
pub(crate) enum Mid<State> {
    Middleware(Box<dyn Middleware>),
    StateMiddleware(Box<dyn StateMiddleware<State>>),
}

impl<State> Mid<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub fn run<'a>(
        &'a self,
        state: Arc<State>,
        req: Request<Body>,
        next: Next,
    ) -> impl Future<Output = Reply> + Send + 'a {
        async move {
            match self {
                Mid::Middleware(m) => m.call(req, next).await,
                Mid::StateMiddleware(m) => m.call((*state).clone(), req, next).await,
            }
        }
        .instrument(trace_span!("middleware_run"))
    }
}

// Next struct passed to middleware to continue the request chain.
pub struct Next(NextFn);
type NextFn = Box<dyn FnOnce(Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send>> + Send>;

impl Next {
    pub async fn run(self, req: Request<Body>) -> Result<Response<Body>, Error> {
        (self.0)(req).await.into_inner()
    }
}

// Wrapper for middleware that invokes middleware and continues the chain.
#[derive(Clone)]
pub(crate) struct MidWrap<State> {
    mid: Arc<Mid<State>>,
    next: Arc<Chain<State>>,
}

impl<State> MidWrap<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub fn wrap(mid: Arc<Mid<State>>, next: Chain<State>) -> Self {
        MidWrap {
            mid,
            next: Arc::new(next),
        }
    }

    pub fn run<'a>(
        &'a self,
        state: Arc<State>,
        req: Request<Body>,
    ) -> impl Future<Output = Reply> + Send + 'a {
        let chain = self.next.clone();
        let state2 = state.clone();
        let next = Next(Box::new(|req: Request<Body>| {
            Box::pin(
                async move { chain.run(state2, req).await }.instrument(trace_span!("next_run")),
            )
        }));
        async move { self.mid.run(state, req, next).await }.instrument(trace_span!("midwrap_run"))
    }
}

// A chain of middleware ending with a request handler.
#[derive(Clone)]
pub(crate) enum Chain<State> {
    MidWrap(MidWrap<State>),
    End(End<State>),
}

impl<State> Chain<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub fn run<'a>(
        &'a self,
        state: Arc<State>,
        req: Request<Body>,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        Box::pin(
            async move {
                match self {
                    Chain::MidWrap(c) => c.run(state, req).await.into(),
                    Chain::End(e) => e.run(state, req).await,
                }
            }
            .instrument(trace_span!("chain_run")),
        )
    }
}

impl<State> Into<Mid<State>> for Box<dyn Middleware> {
    fn into(self) -> Mid<State> {
        Mid::Middleware(self)
    }
}

impl<State> Into<Mid<State>> for Box<dyn StateMiddleware<State>> {
    fn into(self) -> Mid<State> {
        Mid::StateMiddleware(self)
    }
}

impl<State> Into<End<State>> for Box<dyn Handler> {
    fn into(self) -> End<State> {
        End::Handler(Arc::new(self))
    }
}

impl<State> Into<End<State>> for Box<dyn StateHandler<State>> {
    fn into(self) -> End<State> {
        End::StateHandler(Arc::new(self))
    }
}

impl<State> Into<End<State>> for Router<State> {
    fn into(self) -> End<State> {
        End::Router(self)
    }
}

impl<State> Into<Chain<State>> for MidWrap<State> {
    fn into(self) -> Chain<State> {
        Chain::MidWrap(self)
    }
}

impl<State> Into<Chain<State>> for End<State> {
    fn into(self) -> Chain<State> {
        Chain::End(self)
    }
}

impl<State> fmt::Debug for Mid<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mid::Middleware(_) => write!(f, "Middleware"),
            Mid::StateMiddleware(_) => write!(f, "StateMiddleware"),
        }
    }
}

impl<State> fmt::Debug for MidWrap<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MidWrap {{ mid: {:?}, next: {:?} }}",
            self.mid, self.next
        )
    }
}

impl fmt::Debug for Next {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Next")
    }
}

impl<State> fmt::Debug for End<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            End::Handler(_) => write!(f, "Handler"),
            End::StateHandler(_) => write!(f, "StateHandler"),
            End::Router(_) => write!(f, "Router"),
        }
    }
}

impl<State> fmt::Debug for Chain<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Chain::MidWrap(m) => write!(f, "{:?}", m),
            Chain::End(e) => write!(f, "{:?}", e),
        }
    }
}
