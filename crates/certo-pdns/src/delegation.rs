//! Checks whether a zone's parent delegates it to this server.

use std::{fmt, net::SocketAddr, time::Duration};

use async_trait::async_trait;
use hickory_resolver::{
    Resolver, TokioResolver,
    config::{ConnectionConfig, NameServerConfig, ProtocolConfig, ResolverConfig, ResolverOpts},
    net::{DnsError, NetError, runtime::TokioRuntimeProvider},
    proto::rr::{Record, RecordType},
};

/// Outcome of a delegation check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delegation {
    /// The parent delegates the zone to the expected nameserver.
    Delegated,
    /// The parent answers, but the delegation does not point at the expected nameserver.
    NotDelegated(String),
}

/// Decides whether `zone` is delegated to `nsname`.
///
/// `Err` means the check was inconclusive (resolver failure); it must not be counted
/// against the zone.
#[async_trait]
pub trait DelegationChecker: Send + Sync + fmt::Debug {
    /// Runs the check.
    async fn check(&self, zone: &str, nsname: &str) -> Result<Delegation, String>;
}

/// Checker that walks from a recursive resolver to the parent's authoritative servers and
/// asks them (non-recursively) for the zone's NS records.
pub struct DnsDelegationChecker {
    resolver: TokioResolver,
}

impl fmt::Debug for DnsDelegationChecker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DnsDelegationChecker")
            .finish_non_exhaustive()
    }
}

const QUERY_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_PARENT_SERVERS: usize = 3;

fn opts(recursion: bool) -> ResolverOpts {
    let mut o = ResolverOpts::default();
    o.timeout = QUERY_TIMEOUT;
    o.attempts = 2;
    o.recursion_desired = recursion;
    o.cache_size = 0;
    o.use_hosts_file = hickory_resolver::config::ResolveHosts::Never;
    o
}

fn resolver_for(addrs: &[SocketAddr], recursion: bool) -> Result<TokioResolver, NetError> {
    let servers = addrs
        .iter()
        .map(|a| {
            let mut conn = ConnectionConfig::new(ProtocolConfig::Udp);
            conn.port = a.port();
            NameServerConfig::new(a.ip(), true, vec![conn])
        })
        .collect();
    Resolver::builder_with_config(
        ResolverConfig::from_name_servers(servers),
        TokioRuntimeProvider::default(),
    )
    .with_options(opts(recursion))
    .build()
}

impl DnsDelegationChecker {
    /// Uses `resolver` (`host:port`) or, when `None`, the system resolver configuration.
    pub fn new(resolver: Option<SocketAddr>) -> Result<Self, NetError> {
        let resolver = match resolver {
            Some(addr) => resolver_for(&[addr], true)?,
            None => Resolver::builder_tokio()?
                .with_options(opts(true))
                .build()?,
        };
        Ok(Self { resolver })
    }

    /// Nameserver names of the closest enclosing zone above `zone`.
    async fn parent_nameservers(&self, zone: &str) -> Result<Vec<String>, String> {
        let mut parent = zone.split_once('.').map(|(_, rest)| rest);
        while let Some(p) = parent {
            match self.resolver.lookup(format!("{p}."), RecordType::NS).await {
                Ok(lookup) => {
                    let names = ns_names(lookup.answers());
                    if !names.is_empty() {
                        return Ok(names);
                    }
                }
                Err(e) if e.is_no_records_found() => {}
                Err(e) => return Err(format!("looking up the nameservers of {p}: {e}")),
            }
            parent = p.split_once('.').map(|(_, rest)| rest);
        }
        Err(format!("no parent zone found for {zone}"))
    }
}

fn ns_names(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .filter(|r| r.record_type() == RecordType::NS)
        .map(|r| {
            r.data
                .to_string()
                .trim_end_matches('.')
                .to_ascii_lowercase()
        })
        .collect()
}

#[async_trait]
impl DelegationChecker for DnsDelegationChecker {
    async fn check(&self, zone: &str, nsname: &str) -> Result<Delegation, String> {
        let parents = self.parent_nameservers(zone).await?;
        let mut addrs = Vec::new();
        for ns in parents.iter().take(MAX_PARENT_SERVERS) {
            if let Ok(ips) = self.resolver.lookup_ip(format!("{ns}.")).await {
                addrs.extend(ips.iter().map(|ip| SocketAddr::new(ip, 53)));
            }
        }
        if addrs.is_empty() {
            return Err(format!(
                "no address found for the parent nameservers {parents:?}"
            ));
        }
        let parent = resolver_for(&addrs, false).map_err(|e| e.to_string())?;
        let delegated: Vec<String> = match parent.lookup(format!("{zone}."), RecordType::NS).await {
            Ok(lookup) => {
                let mut names = ns_names(lookup.answers());
                names.extend(ns_names(lookup.authorities()));
                names
            }
            Err(NetError::Dns(DnsError::NoRecordsFound(no_records))) => no_records
                .authorities
                .as_deref()
                .map(ns_names)
                .unwrap_or_default(),
            Err(e) => return Err(format!("asking the parent for {zone} NS: {e}")),
        };
        let nsname = nsname.trim_end_matches('.').to_ascii_lowercase();
        if delegated.contains(&nsname) {
            Ok(Delegation::Delegated)
        } else if delegated.is_empty() {
            Ok(Delegation::NotDelegated(format!(
                "the parent zone has no NS records for {zone}"
            )))
        } else {
            Ok(Delegation::NotDelegated(format!(
                "{zone} is delegated to {} instead of {nsname}",
                delegated.join(", ")
            )))
        }
    }
}
