//! Management auth as a seam: routes depend on [`AuthProvider`], not on Basic.
//! An OIDC provider for Entra can be added later without touching route
//! definitions. Do not implement it now.
//!
//! Two providers exist and either one grants access: [`BasicAuth`] and
//! [`CidrAuth`] (source address in an allow-list), combined by [`AnyOf`].

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use ipnet::IpNet;
use subtle::ConstantTimeEq;

use crate::problem::Problem;

pub trait AuthProvider: Send + Sync {
    /// `peer` is the TCP peer address, `None` when the connection info is
    /// unavailable (which no provider may treat as a match).
    fn authenticate(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> bool;
    /// Value for `WWW-Authenticate` on a 401, if the scheme uses one.
    fn challenge(&self) -> Option<&'static str>;
}

pub struct BasicAuth {
    username: String,
    password: String,
}

impl BasicAuth {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}

impl AuthProvider for BasicAuth {
    fn authenticate(&self, headers: &HeaderMap, _peer: Option<IpAddr>) -> bool {
        let decoded = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Basic "))
            .and_then(|b| {
                base64::engine::general_purpose::STANDARD
                    .decode(b.trim())
                    .ok()
            })
            .and_then(|b| String::from_utf8(b).ok());
        let Some((user, pass)) = decoded.as_deref().and_then(|s| s.split_once(':')) else {
            return false;
        };
        // Evaluate both comparisons so timing doesn't reveal which one failed.
        let user_ok = user.as_bytes().ct_eq(self.username.as_bytes());
        let pass_ok = pass.as_bytes().ct_eq(self.password.as_bytes());
        (user_ok & pass_ok).into()
    }

    // Deliberately no `WWW-Authenticate`: the UI turns a 401 into a login form,
    // and that header would trigger the browser's own dialog.
    fn challenge(&self) -> Option<&'static str> {
        None
    }
}

/// Grants access by source address. Behind an Ingress the TCP peer is the
/// proxy, so `X-Forwarded-For` is consulted only when the peer itself is in
/// `trusted_proxies`; otherwise the header is attacker-controlled and ignored.
pub struct CidrAuth {
    allowed: Vec<IpNet>,
    trusted_proxies: Vec<IpNet>,
}

fn contains(nets: &[IpNet], ip: IpAddr) -> bool {
    nets.iter().any(|n| n.contains(&ip))
}

impl CidrAuth {
    pub fn new(allowed: Vec<IpNet>, trusted_proxies: Vec<IpNet>) -> Self {
        Self {
            allowed,
            trusted_proxies,
        }
    }

    /// The address the request originated from, or `None` if it cannot be
    /// established (fail closed). Walks `X-Forwarded-For` from the right,
    /// skipping trusted proxies; the first other address is the client.
    fn client_ip(&self, headers: &HeaderMap, peer: IpAddr) -> Option<IpAddr> {
        let peer = peer.to_canonical();
        if !contains(&self.trusted_proxies, peer) {
            return Some(peer);
        }
        let mut hops = Vec::new();
        for v in headers.get_all("x-forwarded-for") {
            for part in v.to_str().ok()?.split(',') {
                hops.push(part.trim().parse::<IpAddr>().ok()?.to_canonical());
            }
        }
        // All hops trusted (or no header): the leftmost is the best we have.
        Some(
            hops.iter()
                .rev()
                .find(|ip| !contains(&self.trusted_proxies, **ip))
                .or(hops.first())
                .copied()
                .unwrap_or(peer),
        )
    }
}

impl AuthProvider for CidrAuth {
    fn authenticate(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> bool {
        peer.and_then(|p| self.client_ip(headers, p))
            .is_some_and(|ip| contains(&self.allowed, ip))
    }

    fn challenge(&self) -> Option<&'static str> {
        None
    }
}

/// Access if any provider grants it.
pub struct AnyOf(pub Vec<Box<dyn AuthProvider>>);

impl AuthProvider for AnyOf {
    fn authenticate(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> bool {
        self.0.iter().any(|p| p.authenticate(headers, peer))
    }

    fn challenge(&self) -> Option<&'static str> {
        self.0.iter().find_map(|p| p.challenge())
    }
}

/// Middleware layer for the management routes.
pub async fn require_auth(
    State(auth): State<Arc<dyn AuthProvider>>,
    req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    if auth.authenticate(req.headers(), peer) {
        return next.run(req).await;
    }
    let mut res = Problem::new(StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    if let Some(c) = auth.challenge() {
        res.headers_mut()
            .insert(header::WWW_AUTHENTICATE, c.parse().expect("static header"));
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, value.parse().unwrap());
        h
    }

    fn basic(creds: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(creds)
        )
    }

    #[test]
    fn accepts_correct_credentials() {
        let a = BasicAuth::new("admin".into(), "s3cret:with:colons".into());
        assert!(a.authenticate(&headers(&basic("admin:s3cret:with:colons")), None));
    }

    #[test]
    fn rejects_wrong_missing_and_malformed() {
        let a = BasicAuth::new("admin".into(), "pw".into());
        assert!(!a.authenticate(&headers(&basic("admin:nope")), None));
        assert!(!a.authenticate(&headers(&basic("root:pw")), None));
        assert!(!a.authenticate(&headers("Bearer abc"), None));
        assert!(!a.authenticate(&headers("Basic !!!"), None));
        assert!(!a.authenticate(&HeaderMap::new(), None));
    }

    fn nets(list: &[&str]) -> Vec<IpNet> {
        list.iter().map(|n| n.parse().unwrap()).collect()
    }

    fn xff(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", value.parse().unwrap());
        h
    }

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn cidr_matches_peer_directly() {
        let a = CidrAuth::new(nets(&["192.168.1.0/24", "fd00::/8"]), vec![]);
        assert!(a.authenticate(&HeaderMap::new(), ip("192.168.1.50")));
        assert!(a.authenticate(&HeaderMap::new(), ip("fd00::5")));
        assert!(a.authenticate(&HeaderMap::new(), ip("::ffff:192.168.1.50")));
        assert!(!a.authenticate(&HeaderMap::new(), ip("192.168.2.50")));
        assert!(!a.authenticate(&HeaderMap::new(), None));
    }

    #[test]
    fn forwarded_for_is_ignored_from_untrusted_peers() {
        let a = CidrAuth::new(nets(&["192.168.1.0/24"]), nets(&["10.0.0.0/8"]));
        assert!(!a.authenticate(&xff("192.168.1.5"), ip("203.0.113.9")));
    }

    #[test]
    fn forwarded_for_is_used_from_trusted_proxies() {
        let a = CidrAuth::new(nets(&["192.168.1.0/24"]), nets(&["10.0.0.0/8"]));
        assert!(a.authenticate(&xff("192.168.1.5"), ip("10.0.0.2")));
        assert!(!a.authenticate(&xff("203.0.113.9"), ip("10.0.0.2")));
        // A client-supplied left entry cannot override what the proxy appended.
        assert!(!a.authenticate(&xff("192.168.1.5, 203.0.113.9"), ip("10.0.0.2")));
        // Chained trusted proxies are skipped.
        assert!(a.authenticate(&xff("192.168.1.5, 10.1.1.1"), ip("10.0.0.2")));
    }

    #[test]
    fn trusted_proxy_without_usable_header_fails_closed_or_uses_peer() {
        let a = CidrAuth::new(nets(&["10.0.0.0/8"]), nets(&["10.0.0.0/8"]));
        assert!(!a.authenticate(&xff("garbage"), ip("10.0.0.2")));
        // No header: the peer is the proxy itself, which is trusted and allowed.
        assert!(a.authenticate(&HeaderMap::new(), ip("10.0.0.2")));
    }

    #[test]
    fn any_of_grants_when_either_does() {
        let a = AnyOf(vec![
            Box::new(BasicAuth::new("admin".into(), "pw".into())),
            Box::new(CidrAuth::new(nets(&["192.168.1.0/24"]), vec![])),
        ]);
        assert!(a.authenticate(&HeaderMap::new(), ip("192.168.1.5")));
        assert!(a.authenticate(&headers(&basic("admin:pw")), ip("8.8.8.8")));
        assert!(!a.authenticate(&headers(&basic("admin:no")), ip("8.8.8.8")));
    }
}
