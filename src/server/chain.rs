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
    pub async fn run(&self, state: Arc<State>, req: Request<Body>) -> Reply {
        match self {
            End::Handler(h) => h.call(req).await,
            End::StateHandler(h) => h.call((*state).clone(), req).await,
            End::Router(r) => r.run(state, req).await,
        }
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
    pub async fn run(&self, state: Arc<State>, req: Request<Body>, next: Next) -> Reply {
        match self {
            Mid::Middleware(m) => m.call(req, next).await,
            Mid::StateMiddleware(m) => m.call((*state).clone(), req, next).await,
        }
    }
}

/// Type passed to middleware to continue the request chain.
///
/// See [`Middleware`] trait for an example.
///
/// [`Middleware`]: trait.Middleware.html
pub struct Next(NextFn);
type NextFn = Box<dyn FnOnce(Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send>> + Send>;

impl Next {
    /// Continue the middleware chain.
    pub async fn run(self, req: Request<Body>) -> Result<Response<Body>, Error> {
        (self.0)(req).await.into_result()
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

    pub async fn run(&self, state: Arc<State>, req: Request<Body>) -> Reply {
        let chain = self.next.clone();
        let state2 = state.clone();
        let next = Next(Box::new(|req: Request<Body>| {
            Box::pin(async move { chain.run(state2, req).await })
        }));
        self.mid.run(state, req, next).await
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
    pub fn run(
        &self,
        state: Arc<State>,
        req: Request<Body>,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + '_>> {
        Box::pin(async move {
            match self {
                Chain::MidWrap(c) => c.run(state, req).await,
                Chain::End(e) => e.run(state, req).await,
            }
        })
    }
}

impl<State> From<Box<dyn Middleware>> for Mid<State> {
    fn from(v: Box<dyn Middleware>) -> Self {
        Mid::Middleware(v)
    }
}

impl<State> From<Box<dyn StateMiddleware<State>>> for Mid<State> {
    fn from(val: Box<dyn StateMiddleware<State>>) -> Self {
        Mid::StateMiddleware(val)
    }
}

impl<State> From<Box<dyn Handler>> for End<State> {
    fn from(val: Box<dyn Handler>) -> End<State> {
        End::Handler(Arc::new(val))
    }
}

impl<State> From<Box<dyn StateHandler<State>>> for End<State> {
    fn from(val: Box<dyn StateHandler<State>>) -> End<State> {
        End::StateHandler(Arc::new(val))
    }
}

impl<State> From<Router<State>> for End<State> {
    fn from(val: Router<State>) -> Self {
        End::Router(val)
    }
}

impl<State> From<MidWrap<State>> for Chain<State> {
    fn from(val: MidWrap<State>) -> Self {
        Chain::MidWrap(val)
    }
}

impl<State> From<End<State>> for Chain<State> {
    fn from(val: End<State>) -> Self {
        Chain::End(val)
    }
}

impl<State> fmt::Debug for Mid<State> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Mid::Middleware(_) => write!(f, "Middleware"),
            Mid::StateMiddleware(_) => write!(f, "StateMiddleware"),
        }
    }
}

impl<State> fmt::Debug for MidWrap<State> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "MidWrap {{ mid: {:?}, next: {:?} }}",
            self.mid, self.next
        )
    }
}

impl fmt::Debug for Next {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Next")
    }
}

impl<State> fmt::Debug for End<State> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            End::Handler(_) => write!(f, "Handler"),
            End::StateHandler(_) => write!(f, "StateHandler"),
            End::Router(_) => write!(f, "Router"),
        }
    }
}

impl<State> fmt::Debug for Chain<State> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Chain::MidWrap(m) => write!(f, "{:?}", m),
            Chain::End(e) => write!(f, "{:?}", e),
        }
    }
}
