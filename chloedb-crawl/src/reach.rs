//! Which addresses the crawler may reach (audit G-B3).
//!
//! A page on a crawled site can link or redirect anywhere, and the crawler
//! fetches from where it runs: beside a cluster, inside its network. So by
//! default it reaches the public internet only. An address on this machine,
//! a private network, a link-local range (cloud metadata at 169.254.169.254)
//! or another reserved range is refused, checked on every request a crawl
//! makes (each redirect is a request of its own, [`crate::fetch`]) and at
//! connect time, on every address a name resolves to ([`Resolver`]), so a
//! name that turns private between a check and its connection is refused
//! too.
//!
//! An intranet crawl says so: `--allow-private` lets everything through,
//! `--allow-host HOST[:PORT]` one host (and port). The hosts of the
//! HTTP(S)_PROXY settings are always reachable: a proxy is the operator's,
//! and it resolves the names it is asked for.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, RwLock};

use url::{Host, Url};

/// What a request may reach.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    /// Any address, private ones too.
    pub private: bool,
    /// Hosts reachable whatever their addresses: a name or an address, and
    /// a port (any port when none).
    pub hosts: Vec<(String, Option<u16>)>,
}

/// The words a refusal starts with, so that an error from the resolver is
/// told apart from other failures to connect.
pub const REFUSED: &str = "not on the public internet";

static CURRENT: RwLock<Option<Arc<Reach>>> = RwLock::new(None);

/// Sets what the crawls of this process may reach (from `--allow-private`
/// and `--allow-host`). Crawls started later take it.
pub fn set(r: Reach) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(r));
}

/// What the crawls of this process may reach: the public internet only,
/// unless [`set`] said more.
pub fn current() -> Arc<Reach> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

impl Reach {
    /// Everything: for tests against sites on this machine.
    pub fn any() -> Reach {
        Reach { private: true, hosts: Vec::new() }
    }

    /// Parses `--allow-host` values: `host`, `host:port`, `[v6]:port`.
    pub fn parse_hosts(list: &[String]) -> Result<Vec<(String, Option<u16>)>, String> {
        list.iter()
            .map(|h| {
                let h = h.trim().to_ascii_lowercase();
                let u = Url::parse(&format!("http://{h}/")).map_err(|_| format!("--allow-host {h:?}: a host, or host:port"))?;
                let host = u.host_str().ok_or_else(|| format!("--allow-host {h:?}: a host, or host:port"))?.trim_matches(['[', ']']).to_string();
                // The url crate drops port 80 as http's default: say it again.
                let port = u.port().or_else(|| h.ends_with(":80").then_some(80));
                Ok((host, port))
            })
            .collect()
    }

    /// The proxies' hosts (HTTP_PROXY, HTTPS_PROXY, ALL_PROXY, either case),
    /// added to the hosts reachable whatever their addresses.
    pub fn with_proxies(mut self) -> Reach {
        for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            let Ok(v) = std::env::var(k) else { continue };
            let v = if v.contains("://") { v } else { format!("http://{v}") };
            // Its host at its port: a proxy on this machine is not this
            // machine's every service.
            if let Some((h, p)) = Url::parse(&v).ok().and_then(|u| Some((u.host_str()?.trim_matches(['[', ']']).to_ascii_lowercase(), u.port_or_known_default()))) {
                self.hosts.push((h, p));
            }
        }
        self
    }

    fn listed(&self, host: &str, port: Option<u16>) -> bool {
        let host = host.trim_matches(['[', ']']).trim_end_matches('.');
        self.hosts.iter().any(|(h, p)| h.eq_ignore_ascii_case(host) && (p.is_none() || *p == port))
    }

    /// Whether a request may go to `u`: http or https, and a public address
    /// unless allowed. A name is checked again when it resolves.
    pub fn check(&self, u: &Url) -> Result<(), String> {
        if u.scheme() != "http" && u.scheme() != "https" {
            return Err(format!("{}: only http and https addresses", u.scheme()));
        }
        if self.private {
            return Ok(());
        }
        let host = u.host_str().unwrap_or("");
        if self.listed(host, u.port_or_known_default()) {
            return Ok(());
        }
        let ip = match u.host() {
            Some(Host::Ipv4(a)) => IpAddr::V4(a),
            Some(Host::Ipv6(a)) => IpAddr::V6(a),
            Some(Host::Domain(d)) => {
                let d = d.trim_end_matches('.').to_ascii_lowercase();
                if d == "localhost" || d.ends_with(".localhost") {
                    return Err(format!("{d} is {REFUSED} (give --allow-host {d} to crawl it)"));
                }
                return Ok(());
            }
            None => return Err(format!("{u}: no host")),
        };
        match private(ip) {
            true => Err(format!("{ip} is {REFUSED} (give --allow-host {host} or --allow-private to crawl it)")),
            false => Ok(()),
        }
    }

    /// Whether a name may connect to these of its addresses; it is the
    /// name, not the URL, that a resolver sees, so a listed host counts
    /// with any port here (its port was checked with the URL, and a
    /// proxy's is the proxy's).
    fn addresses(&self, name: &str, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, String> {
        if self.private || self.hosts.iter().any(|(h, _)| h.eq_ignore_ascii_case(name.trim_end_matches('.'))) {
            return Ok(addrs);
        }
        let public: Vec<SocketAddr> = addrs.iter().copied().filter(|a| !private(a.ip())).collect();
        if public.is_empty() && !addrs.is_empty() {
            return Err(format!("{name} resolves to {}, {REFUSED} (give --allow-host {name} or --allow-private to crawl it)", addrs[0].ip()));
        }
        Ok(public)
    }
}

/// Whether an address is on this machine, a private network or another
/// range that is not the public internet.
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => private_v4(a),
        IpAddr::V6(a) => private_v6(a),
    }
}

fn private_v4(a: Ipv4Addr) -> bool {
    let [x, y, z, _] = a.octets();
    x == 0 // "this network"
        || x == 10
        || (x == 100 && (64..128).contains(&y)) // carrier-grade NAT
        || x == 127
        || (x == 169 && y == 254) // link local, cloud metadata
        || (x == 172 && (16..32).contains(&y))
        || (x == 192 && y == 0 && z == 0) // IETF protocol assignments
        || (x == 192 && y == 0 && z == 2) // documentation
        || (x == 192 && y == 88 && z == 99) // 6to4 relays
        || (x == 192 && y == 168)
        || (x == 198 && (18..20).contains(&y)) // benchmarking
        || (x == 198 && y == 51 && z == 100) // documentation
        || (x == 203 && y == 0 && z == 113) // documentation
        || x >= 224 // multicast, reserved, broadcast
}

fn private_v6(a: Ipv6Addr) -> bool {
    let w = a.segments();
    let v4 = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    if w[..7].iter().all(|&x| x == 0) {
        return true; // :: and ::1
    }
    if w[..5].iter().all(|&x| x == 0) && w[5] == 0xffff {
        return private_v4(v4(w[6], w[7])); // ::ffff:a.b.c.d
    }
    if w[..6].iter().all(|&x| x == 0) {
        return true; // ::a.b.c.d, deprecated
    }
    if w[0] == 0x64 && w[1] == 0xff9b {
        return private_v4(v4(w[6], w[7])); // NAT64
    }
    if w[0] == 0x2002 {
        return private_v4(v4(w[1], w[2])); // 6to4
    }
    (w[0] & 0xfe00) == 0xfc00 // unique local
        || (w[0] & 0xffc0) == 0xfe80 // link local
        || (w[0] & 0xff00) == 0xff00 // multicast
        || (w[0] == 0x2001 && w[1] == 0x0db8) // documentation
        || (w[0] == 0x0100 && w[1] == 0 && w[2] == 0 && w[3] == 0) // discard
}

/// The client's resolver: the system's, and then only the addresses the
/// reach allows.
pub struct Resolver(pub Arc<Reach>);

impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let reach = self.0.clone();
        let name = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            let ok = reach.addresses(&name, addrs).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            Ok(Box::new(ok.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn private_and_reserved_addresses_are_refused() {
        let r = Reach::default();
        for u in [
            "http://127.0.0.1/",
            "http://localhost:7700/v1/tables",
            "http://api.localhost/",
            "http://10.1.2.3/",
            "http://172.20.0.1/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fd00::1]/",
            "http://[fe80::1]/",
            "http://2130706433/", // 127.0.0.1 as one number
            "http://0x7f.1/",
            "http://[64:ff9b::a9fe:a9fe]/",
            "http://[2002:a9fe:a9fe::]/",
        ] {
            assert!(r.check(&url(u)).is_err(), "{u}");
        }
        for u in ["https://example.com/", "http://93.184.215.14/", "http://[2606:4700::1111]/"] {
            assert!(r.check(&url(u)).is_ok(), "{u}");
        }
        assert!(r.check(&url("file:///etc/passwd")).is_err());
        assert!(r.check(&url("ftp://example.com/")).is_err());
    }

    #[test]
    fn allowed_hosts_and_private_crawls() {
        let r = Reach { private: false, hosts: Reach::parse_hosts(&["127.0.0.1:8080".into(), "intranet.local".into(), "[::1]:80".into()]).unwrap() };
        assert!(r.check(&url("http://127.0.0.1:8080/a")).is_ok());
        assert!(r.check(&url("http://127.0.0.1:8081/a")).is_err(), "another port of a listed host");
        assert!(r.check(&url("http://[::1]/")).is_ok(), "port 80, the default");
        assert!(r.addresses("intranet.local", vec!["10.0.0.5:0".parse().unwrap()]).is_ok());
        assert!(r.addresses("other.example", vec!["10.0.0.5:0".parse().unwrap()]).is_err());
        assert_eq!(r.addresses("mixed.example", vec!["10.0.0.5:0".parse().unwrap(), "93.184.215.14:0".parse().unwrap()]).unwrap(), vec!["93.184.215.14:0".parse().unwrap()]);
        assert!(Reach::any().check(&url("http://127.0.0.1:1/")).is_ok());
        assert!(Reach::parse_hosts(&["a b".into()]).is_err());
    }
}
