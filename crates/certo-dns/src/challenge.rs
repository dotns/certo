//! Answers for the base domain, static config records and challenge TXT values.
//!
//! Mirrors the original Go nameserver so existing ACME clients and delegations behave
//! identically: one SOA (for the base domain), static records served as configured,
//! challenge TXT values read from the database on every query with TTL 1.

use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};

use certo_core::sanitize_fqdn;
use certo_db::Db;
use hickory_server::proto::{
    op::{LowerQuery, ResponseCode},
    rr::{
        DNSClass, LowerName, Name, RData, Record, RecordType,
        rdata::{SOA, TXT},
    },
};
use rustc_hash::FxHashMap;
use tracing::{debug, error, warn};

const STATIC_TTL: u32 = 3600;
const TXT_TTL: u32 = 1;

/// Base-domain settings needed to build the challenge responder.
#[derive(Debug, Clone)]
pub struct ChallengeConfig {
    /// Base domain, e.g. `dns.example.com`.
    pub domain: String,
    /// SOA MNAME.
    pub nsname: String,
    /// SOA RNAME (`@` already replaced by `.`).
    pub nsadmin: String,
    /// Extra records in presentation format, e.g. `dns.example.com. A 198.51.100.1`.
    pub records: Vec<String>,
}

/// The answer to one question.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) records: Vec<Record>,
    pub(crate) rcode: ResponseCode,
    pub(crate) authoritative: bool,
}

/// Go-compatible responder for the base domain and static records.
#[derive(Clone)]
pub struct ChallengeResponder {
    own_domain: LowerName,
    own_challenge: LowerName,
    records: Arc<FxHashMap<LowerName, Vec<Record>>>,
    soa: Option<Record>,
    own_auth_key: Arc<RwLock<String>>,
    db: Db,
}

impl std::fmt::Debug for ChallengeResponder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChallengeResponder")
            .field("own_domain", &self.own_domain)
            .field("names", &self.records.len())
            .finish_non_exhaustive()
    }
}

fn fqdn(name: &str) -> Result<Name, String> {
    let mut name = Name::from_str(name).map_err(|e| e.to_string())?;
    name.set_fqdn(true);
    Ok(name)
}

/// Parses `<name> [ttl] [IN] <type> <rdata>`; the whole line is lowercased first.
fn parse_static(line: &str) -> Result<Record, String> {
    let line = line.to_lowercase();
    let mut fields = line.split_whitespace().peekable();
    let name = fqdn(fields.next().ok_or("empty record")?)?;
    let mut ttl = STATIC_TTL;
    if let Some(t) = fields.peek().and_then(|f| f.parse::<u32>().ok()) {
        ttl = t;
        fields.next();
    }
    if fields.peek() == Some(&"in") {
        fields.next();
    }
    let rtype = RecordType::from_str(&fields.next().ok_or("missing type")?.to_uppercase())
        .map_err(|e| e.to_string())?;
    let rdata = fields.collect::<Vec<_>>().join(" ");
    let rdata = RData::try_from_str(rtype, &rdata).map_err(|e| e.to_string())?;
    Ok(Record::from_rdata(name, ttl, rdata))
}

fn build_soa(config: &ChallengeConfig, own: &Name) -> Result<Record, String> {
    let now = time::OffsetDateTime::now_utc();
    let serial = format!(
        "{:04}{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour()
    )
    .parse::<u32>()
    .map_err(|e| e.to_string())?;
    let soa = SOA::new(
        fqdn(&config.nsname.to_lowercase())?,
        fqdn(&config.nsadmin.to_lowercase())?,
        serial,
        28800,
        7200,
        604_800,
        86400,
    );
    Ok(Record::from_rdata(own.clone(), STATIC_TTL, RData::SOA(soa)))
}

impl ChallengeResponder {
    /// Builds the responder; unparsable static records are logged and skipped.
    pub fn new(config: &ChallengeConfig, db: Db) -> Result<Self, String> {
        let own = fqdn(&config.domain.to_lowercase())?;
        let mut records: FxHashMap<LowerName, Vec<Record>> = FxHashMap::default();
        for line in &config.records {
            match parse_static(line) {
                Ok(rr) => {
                    debug!(name = %rr.name, rtype = %rr.record_type(), "adding static record");
                    records
                        .entry(LowerName::new(&rr.name))
                        .or_default()
                        .push(rr);
                }
                Err(e) => error!(record = %line, error = %e, "could not parse static record"),
            }
        }
        let soa = match build_soa(config, &own) {
            Ok(soa) => {
                records
                    .entry(LowerName::new(&own))
                    .or_default()
                    .push(soa.clone());
                Some(soa)
            }
            Err(e) => {
                error!(error = %e, "error while adding SOA record");
                None
            }
        };
        let own_challenge = LowerName::new(&fqdn(&format!("_acme-challenge.{own}"))?);
        Ok(Self {
            own_domain: LowerName::new(&own),
            own_challenge,
            records: Arc::new(records),
            soa,
            own_auth_key: Arc::new(RwLock::new(String::new())),
            db,
        })
    }

    /// Sets the DNS-01 token for certo's own certificate (empty clears it).
    pub fn set_own_auth_key(&self, key: &str) {
        match self.own_auth_key.write() {
            Ok(mut k) => key.clone_into(&mut k),
            Err(e) => key.clone_into(&mut e.into_inner()),
        }
    }

    fn own_auth_key(&self) -> String {
        self.own_auth_key
            .read()
            .map_or_else(|e| e.into_inner().clone(), |k| k.clone())
    }

    /// The base domain.
    pub fn domain(&self) -> Name {
        Name::from(self.own_domain.clone())
    }

    /// Owner names of the static records.
    pub fn static_owners(&self) -> Vec<Name> {
        self.records.keys().cloned().map(Name::from).collect()
    }

    /// The base domain SOA, used in NXDOMAIN authority sections.
    pub(crate) const fn soa(&self) -> Option<&Record> {
        self.soa.as_ref()
    }

    /// Whether `name` is the base domain or one of its subdomains.
    pub(crate) fn owns(&self, name: &LowerName) -> bool {
        self.own_domain.zone_of(name)
    }

    fn answering_for(&self, name: &LowerName) -> bool {
        *name == self.own_domain || self.records.contains_key(name)
    }

    fn is_authoritative(&self, name: &LowerName) -> bool {
        let mut name = name.clone();
        loop {
            if self.answering_for(&name) {
                return true;
            }
            if name.is_root() {
                return false;
            }
            name = name.base_name();
            if name.is_root() {
                return false;
            }
        }
    }

    fn static_records(&self, name: &LowerName, qtype: RecordType) -> Vec<Record> {
        let Some(all) = self.records.get(name) else {
            return Vec::new();
        };
        let exact: Vec<Record> = all
            .iter()
            .filter(|r| r.record_type() == qtype)
            .cloned()
            .collect();
        if exact.is_empty() {
            all.iter()
                .filter(|r| r.record_type() == RecordType::CNAME)
                .cloned()
                .collect()
        } else {
            exact
        }
    }

    fn txt(owner: &Name, value: String) -> Record {
        let mut rr = Record::from_rdata(owner.clone(), TXT_TTL, RData::TXT(TXT::new(vec![value])));
        rr.dns_class = DNSClass::IN;
        rr
    }

    /// Answers one question the way the Go nameserver did.
    pub(crate) async fn answer(&self, query: &LowerQuery) -> Answer {
        let name = query.name();
        let qtype = query.query_type();
        let authoritative = self.is_authoritative(name);
        let own_challenge = *name == self.own_challenge;
        let mut rcode = if own_challenge || self.answering_for(name) {
            ResponseCode::NoError
        } else {
            ResponseCode::NXDomain
        };
        let mut records = self.static_records(name, qtype);
        if qtype == RecordType::TXT {
            let owner = query.original().name();
            if own_challenge {
                records.push(Self::txt(owner, self.own_auth_key()));
            } else {
                match self
                    .db
                    .txt_for_domain(&sanitize_fqdn(&name.to_string()))
                    .await
                {
                    Ok(values) => records.extend(
                        values
                            .into_iter()
                            .filter(|v| !v.is_empty())
                            .map(|v| Self::txt(owner, v)),
                    ),
                    Err(e) => warn!(error = %e, "error while trying to get record"),
                }
            }
        }
        if !records.is_empty() {
            rcode = ResponseCode::NoError;
        }
        debug!(qtype = %qtype, domain = %name, rcode = %rcode, "answering question for domain");
        Answer {
            records,
            rcode,
            authoritative,
        }
    }
}
