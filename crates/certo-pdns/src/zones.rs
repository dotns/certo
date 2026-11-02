//! Zone service: every DNS fact lives in the certo database; `PowerDNS` is derived from
//! it and converged by [`Zones::sync`].

use std::{sync::Arc, time::Duration};

use certo_core::{Zone, ZoneDnssec, ZoneRrSet, ZoneTsigKey};
use certo_db::{Db, DbError};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use tracing::{error, info, warn};

use crate::{
    ChangeType, Client, Delegation, DelegationChecker, NewZone, PdnsError, RecordData, RrSet,
    names::{self, RrSetChange},
};

/// How often a pending zone is re-checked.
pub const PENDING_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often an active, non-exempt zone is re-checked.
pub const ACTIVE_RECHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Consecutive failed re-checks before an active zone is deactivated.
pub const MAX_FAILED_CHECKS: u32 = 7;
/// Pending zones nobody activates within this time are deleted.
pub const PENDING_EXPIRY: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Minimum interval between manual checks of one zone.
pub const VERIFY_COOLDOWN: Duration = Duration::from_secs(60);

/// TSIG algorithms accepted for transfer keys.
pub const TSIG_ALGORITHMS: [&str; 3] = ["hmac-sha256", "hmac-sha384", "hmac-sha512"];
/// Accepted `nsec_mode` values.
pub const NSEC_MODES: [&str; 2] = ["nsec", "nsec3"];
/// RFC 9276 parameters: SHA-1, no extra iterations, no salt.
const NSEC3_PARAM: &str = "1 0 0 -";
const DNSSEC_ALGORITHM: &str = "ecdsa256";
const CHALLENGE_TTL: u32 = 1;
const ALLOW_AXFR_FROM: &str = "ALLOW-AXFR-FROM";
const TSIG_ALLOW_AXFR: &str = "TSIG-ALLOW-AXFR";
/// Zone metadata carrying the database version a `PowerDNS` zone was built from.
pub const VERSION_METADATA: &str = "X-CERTO-VERSION";

/// Server-wide zone rules.
#[derive(Debug, Clone)]
pub struct ZonePolicy {
    /// The challenge domain (`general.acme_domain`): challenge names live at
    /// `<subdomain>.<base_domain>`.
    pub base_domain: String,
    /// The zone certo itself serves (`general.domain`); contains `base_domain`. User zones
    /// may not be at or under it.
    pub base_zone: String,
    /// The nameserver delegations must point at; also the apex NS of new zones.
    pub nsname: String,
}

/// Zone operation failures.
#[derive(Debug, thiserror::Error)]
pub enum ZoneOpError {
    /// A request field is invalid.
    #[error("{message}")]
    Invalid {
        /// Machine-readable code.
        code: &'static str,
        /// Human-readable detail.
        message: String,
    },
    /// The zone may not be hosted or changed like this.
    #[error("zone not allowed: {0}")]
    NotAllowed(&'static str),
    /// A zone with that name exists.
    #[error("zone already exists")]
    Exists,
    /// The zone, key or record does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// The zone is pending activation.
    #[error("zone is not active")]
    NotActive,
    /// The zone was checked less than [`VERIFY_COOLDOWN`] ago.
    #[error("zone was checked less than a minute ago")]
    RateLimited,
    /// The delegation check could not be completed.
    #[error("delegation check failed: {0}")]
    Check(String),
    /// Storage failure.
    #[error(transparent)]
    Db(DbError),
    /// `PowerDNS` failure.
    #[error(transparent)]
    Pdns(PdnsError),
}

impl From<DbError> for ZoneOpError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound(what) => Self::NotFound(what),
            DbError::Conflict("zone") => Self::Exists,
            DbError::Conflict("zone overlap") => Self::NotAllowed("overlaps another user's zone"),
            DbError::Conflict("tsig key") => {
                invalid("tsig_key_exists", "a key with that name exists")
            }
            other => Self::Db(other),
        }
    }
}

impl From<PdnsError> for ZoneOpError {
    fn from(e: PdnsError) -> Self {
        match e {
            PdnsError::Api { status: 404, .. } => Self::NotFound("zone"),
            PdnsError::Api { status: 409, .. } => Self::Exists,
            PdnsError::Api {
                status: 400 | 422,
                message,
            } => Self::Invalid {
                code: "pdns_rejected",
                message,
            },
            other => Self::Pdns(other),
        }
    }
}

fn invalid(code: &'static str, message: impl Into<String>) -> ZoneOpError {
    ZoneOpError::Invalid {
        code,
        message: message.into(),
    }
}

/// DNSSEC state of a zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnssecInfo {
    /// Signing is on.
    pub enabled: bool,
    /// `nsec` or `nsec3`.
    pub nsec_mode: String,
    /// DNSKEY rdata of the signing key.
    pub dnskey: Option<String>,
    /// DS records to publish at the parent.
    pub ds: Vec<String>,
}

/// A TSIG key allowed to transfer a zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsigKeyInfo {
    /// Key name.
    pub name: String,
    /// Algorithm.
    pub algorithm: String,
}

/// Transfer settings of a zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferInfo {
    /// CIDRs allowed to transfer without TSIG.
    pub allow_from: Vec<String>,
    /// TSIG keys allowed to transfer.
    pub tsig_keys: Vec<TsigKeyInfo>,
}

/// Summary of one periodic check run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Zones whose delegation was checked.
    pub checked: usize,
    /// Zones that became active.
    pub activated: usize,
    /// Zones deactivated after repeated failures.
    pub deactivated: usize,
    /// Pending zones deleted after [`PENDING_EXPIRY`].
    pub expired: usize,
}

/// Database and `PowerDNS` versions of one zone that differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ZoneVersions {
    /// Zone name.
    pub name: String,
    /// Version in the database; `None` for a `PowerDNS` zone the database does not serve.
    pub db: Option<i64>,
    /// Version stamped on the `PowerDNS` zone; `None` when the zone or the stamp is missing.
    pub pdns: Option<i64>,
}

/// How far `PowerDNS` is from the database.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    /// Sum of the versions of all active zones in the database.
    pub db_version: i64,
    /// Sum of the versions stamped on the `PowerDNS` zones.
    pub pdns_version: i64,
    /// Zones whose versions differ.
    pub zones: Vec<ZoneVersions>,
}

/// What one [`Zones::sync`] run changed in `PowerDNS`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    /// Zones created in `PowerDNS`.
    pub created: usize,
    /// `PowerDNS` zones removed because the database does not serve them.
    pub removed: usize,
    /// Rrsets replaced or deleted.
    pub rrsets_changed: usize,
    /// DNSSEC keys imported from stored content.
    pub keys_imported: usize,
    /// Rrsets taken over from `PowerDNS` zones that were never stamped.
    pub records_adopted: usize,
    /// Versions after the run.
    #[serde(flatten)]
    pub status: SyncStatus,
}

/// What converging one zone changed.
#[derive(Debug, Default)]
struct Converged {
    created: bool,
    rrsets_changed: usize,
    keys_imported: usize,
}

/// Zone operations shared by the HTTP API and the background checker.
#[derive(Clone)]
pub struct Zones {
    db: Db,
    client: Client,
    policy: Arc<ZonePolicy>,
    checker: Arc<dyn DelegationChecker>,
}

impl std::fmt::Debug for Zones {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Zones")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

fn apex(name: &str) -> String {
    format!("{}.", name.trim_end_matches('.'))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Parses a config record line `<name> [ttl] [IN] <type> <rdata>`.
fn parse_static(line: &str) -> Option<(String, u32, String, String)> {
    let mut fields = line.split_whitespace().peekable();
    let name = fields.next()?.trim_end_matches('.').to_ascii_lowercase();
    let mut ttl = names::DEFAULT_TTL;
    if let Some(t) = fields.peek().and_then(|f| f.parse::<u32>().ok()) {
        ttl = t;
        fields.next();
    }
    if fields.peek().is_some_and(|f| f.eq_ignore_ascii_case("in")) {
        fields.next();
    }
    let rtype = fields.next()?.to_ascii_uppercase();
    let rdata = fields.collect::<Vec<_>>().join(" ");
    (!rdata.is_empty()).then_some((name, ttl, rtype, rdata))
}

/// Groups `(name, ttl, type, rdata)` tuples into rrsets.
fn group_rrsets(
    records: impl IntoIterator<Item = (String, u32, String, String)>,
) -> Vec<ZoneRrSet> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut sets: FxHashMap<(String, String), ZoneRrSet> = FxHashMap::default();
    for (name, ttl, rtype, rdata) in records {
        let key = (apex(&name), rtype.clone());
        let set = sets.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            ZoneRrSet {
                name: apex(&name),
                rtype,
                ttl,
                records: Vec::new(),
            }
        });
        set.records.push(rdata);
    }
    order.into_iter().filter_map(|k| sets.remove(&k)).collect()
}

/// A stored rrset as a `REPLACE` change (`DELETE` when it has no records).
fn to_change(rr: &ZoneRrSet) -> RrSet {
    if rr.records.is_empty() {
        RrSet::delete(rr.name.clone(), rr.rtype.clone())
    } else {
        RrSet::replace(
            rr.name.clone(),
            rr.rtype.clone(),
            rr.ttl,
            rr.records.iter().map(RecordData::new).collect(),
        )
    }
}

/// A `PowerDNS` rrset (or change) as stored data.
fn to_stored(rr: &RrSet) -> ZoneRrSet {
    ZoneRrSet {
        name: rr.name.clone(),
        rtype: rr.rtype.clone(),
        ttl: rr.ttl,
        records: if rr.changetype == Some(ChangeType::Delete) {
            Vec::new()
        } else {
            rr.records.iter().map(|r| r.content.clone()).collect()
        },
    }
}

fn same_records(a: &RrSet, b: &RrSet) -> bool {
    let mut x: Vec<&str> = a.records.iter().map(|r| r.content.as_str()).collect();
    let mut y: Vec<&str> = b.records.iter().map(|r| r.content.as_str()).collect();
    x.sort_unstable();
    y.sort_unstable();
    a.ttl == b.ttl && x == y
}

fn parse_version(values: &[String]) -> Option<i64> {
    values.first().and_then(|v| v.trim().parse().ok())
}

impl Zones {
    /// Builds the service.
    pub fn new(
        db: Db,
        client: Client,
        policy: ZonePolicy,
        checker: Arc<dyn DelegationChecker>,
    ) -> Self {
        Self {
            db,
            client,
            policy: Arc::new(policy),
            checker,
        }
    }

    /// The zone rules.
    pub fn policy(&self) -> &ZonePolicy {
        &self.policy
    }

    /// The CNAME target currently published for `name`, through the configured resolver.
    pub async fn cname(&self, name: &str) -> Result<Option<String>, String> {
        self.checker.cname(name).await
    }

    /// Whether `PowerDNS` answers its API.
    pub async fn ping(&self) -> bool {
        self.client.ping().await
    }

    /// Writes the version a `PowerDNS` zone was built from.
    async fn stamp(&self, name: &str, version: i64) -> Result<(), ZoneOpError> {
        self.client
            .set_metadata(name, VERSION_METADATA, vec![version.to_string()])
            .await?;
        Ok(())
    }

    // --- Base zone and challenge TXT ---

    /// Creates the server's own zone row on first start, seeded from the config record
    /// lines; an existing row is left untouched.
    pub async fn ensure_base_zone(&self, seed: &[String]) -> Result<(), ZoneOpError> {
        let base = &self.policy.base_zone;
        match self.db.zone(base).await {
            Ok(_) => {
                if !seed.is_empty() {
                    info!(
                        "general.records was imported into the base zone; it can be removed from the config"
                    );
                }
                return Ok(());
            }
            Err(DbError::NotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
        let mut records = Vec::new();
        for line in seed {
            match parse_static(line) {
                Some(rec) if names::inside(&rec.0, base) => records.push(rec),
                Some((name, ..)) => error!(
                    record = %line,
                    "config record {name} is outside the base zone; create a hosted zone for it"
                ),
                None => error!(record = %line, "could not parse config record"),
            }
        }
        let ns = apex(&self.policy.nsname);
        if !records
            .iter()
            .any(|(name, _, rtype, rdata)| name == base && rtype == "NS" && apex(rdata) == ns)
        {
            records.push((base.clone(), names::DEFAULT_TTL, "NS".to_owned(), ns));
        }
        let rrsets = group_rrsets(records);
        info!(zone = %base, rrsets = rrsets.len(), "creating the base zone");
        let zone = self.db.create_zone(None, base).await?;
        self.db.replace_zone_rrsets(zone.id, &rrsets).await?;
        self.db.activate_zone(zone.id, true).await?;
        Ok(())
    }

    fn challenge_rrset(name: &str, values: &[String]) -> RrSet {
        if values.is_empty() {
            RrSet::delete(apex(name), "TXT")
        } else {
            RrSet::replace(
                apex(name),
                "TXT",
                CHALLENGE_TTL,
                values
                    .iter()
                    .map(|v| RecordData::new(names::txt_content(v)))
                    .collect(),
            )
        }
    }

    fn own_challenge_name(&self) -> String {
        format!("_acme-challenge.{}.", self.policy.base_zone)
    }

    /// Pushes the current TXT values of one challenge name into the base zone.
    pub async fn sync_txt(&self, name: &str) -> Result<(), ZoneOpError> {
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        if !names::inside(&name, &self.policy.base_domain) {
            return Err(invalid(
                "name_outside_zone",
                "challenge name outside the base domain",
            ));
        }
        let values = self.db.txt_for_domain(&name).await?;
        self.client
            .patch_rrsets(
                &self.policy.base_zone,
                &[Self::challenge_rrset(&name, &values)],
            )
            .await?;
        let base = self.db.zone(&self.policy.base_zone).await?;
        self.stamp(&base.name, base.version).await
    }

    /// Sets (or clears) the DNS-01 token for certo's own certificate, issued for the
    /// base zone name. Not stored: the token is short-lived and re-issued on demand.
    pub async fn set_own_challenge(&self, value: Option<&str>) -> Result<(), ZoneOpError> {
        let name = format!("_acme-challenge.{}", self.policy.base_zone);
        let values: Vec<String> = value.map(str::to_owned).into_iter().collect();
        self.client
            .patch_rrsets(
                &self.policy.base_zone,
                &[Self::challenge_rrset(&name, &values)],
            )
            .await?;
        Ok(())
    }

    // --- Sync ---

    /// Everything `PowerDNS` should serve for a zone: the stored rrsets, an apex NS when
    /// none is stored, and for the base zone the challenge TXT values.
    async fn desired_rrsets(&self, zone: &Zone) -> Result<Vec<RrSet>, ZoneOpError> {
        let apex_name = apex(&zone.name);
        let mut desired: Vec<RrSet> = self
            .db
            .zone_rrsets(zone.id)
            .await?
            .iter()
            .map(to_change)
            .collect();
        if !desired
            .iter()
            .any(|rr| rr.rtype == "NS" && rr.name == apex_name)
        {
            desired.push(RrSet::replace(
                apex_name,
                "NS",
                names::DEFAULT_TTL,
                vec![RecordData::new(apex(&self.policy.nsname))],
            ));
        }
        if zone.is_system() {
            let challenges: Vec<RrSet> = self
                .db
                .txt_by_domain()
                .await?
                .iter()
                .filter(|(name, _)| names::inside(name, &zone.name))
                .map(|(name, values)| Self::challenge_rrset(name, values))
                .collect();
            desired
                .retain(|rr| !(rr.rtype == "TXT" && challenges.iter().any(|c| c.name == rr.name)));
            desired.extend(challenges);
        }
        Ok(desired)
    }

    /// Rrsets `PowerDNS` maintains itself or that are pushed outside the database.
    fn is_protected(&self, zone: &Zone, rr: &RrSet) -> bool {
        rr.rtype == "SOA"
            || (zone.is_system() && rr.rtype == "TXT" && rr.name == self.own_challenge_name())
    }

    /// Creates the `PowerDNS` zone or patches its rrsets to match `desired`.
    async fn converge_rrsets(
        &self,
        zone: &Zone,
        served: Option<&crate::Zone>,
        desired: Vec<RrSet>,
    ) -> Result<Converged, ZoneOpError> {
        let mut out = Converged::default();
        let Some(z) = served else {
            info!(zone = %zone.name, rrsets = desired.len(), "creating zone in pdns");
            out.rrsets_changed = desired.len();
            self.client
                .create_zone(&NewZone::native(apex(&zone.name), Vec::new(), desired))
                .await?;
            out.created = true;
            return Ok(out);
        };
        let mut changes: Vec<RrSet> = desired
            .iter()
            .filter(|d| {
                !z.rrsets
                    .iter()
                    .any(|s| s.name == d.name && s.rtype == d.rtype && same_records(s, d))
            })
            .cloned()
            .collect();
        for s in &z.rrsets {
            let wanted = desired
                .iter()
                .any(|d| d.name == s.name && d.rtype == s.rtype);
            if !wanted && !self.is_protected(zone, s) {
                changes.push(RrSet::delete(s.name.clone(), s.rtype.clone()));
            }
        }
        if !changes.is_empty() {
            info!(zone = %zone.name, rrsets = changes.len(), "converging zone records");
            self.client.patch_rrsets(&zone.name, &changes).await?;
            out.rrsets_changed = changes.len();
        }
        Ok(out)
    }

    /// Keeps the key whose content is stored, drops every other one, imports a missing
    /// one and sets the NSEC mode; returns the number of keys imported.
    async fn converge_dnssec(
        &self,
        zone: &Zone,
        served: Option<&crate::Zone>,
    ) -> Result<usize, ZoneOpError> {
        let stored = self.db.zone_dnssec(zone.id).await?;
        let mut kept = false;
        for key in self.client.cryptokeys(&zone.name).await? {
            let full = self.client.cryptokey(&zone.name, key.id).await?;
            let matches = stored
                .as_ref()
                .is_some_and(|s| s.private_key == full.privatekey);
            if matches && !kept {
                kept = true;
            } else {
                self.client.delete_cryptokey(&zone.name, key.id).await?;
            }
        }
        let imported = if let Some(s) = stored.as_ref().filter(|_| !kept) {
            info!(zone = %zone.name, "importing the stored DNSSEC key");
            self.client.import_csk(&zone.name, &s.private_key).await?;
            1
        } else {
            0
        };
        let want_nsec3 = stored.as_ref().is_some_and(|s| s.nsec3);
        let has_nsec3 = served.is_some_and(|z| !z.nsec3param.is_empty());
        if want_nsec3 != has_nsec3 {
            self.client
                .set_nsec3param(&zone.name, want_nsec3.then_some(NSEC3_PARAM))
                .await?;
        }
        Ok(imported)
    }

    /// Creates missing TSIG keys (server-wide in pdns) and sets both transfer metadata kinds.
    async fn converge_transfers(&self, zone: &Zone) -> Result<(), ZoneOpError> {
        let tsig = self.db.zone_tsig_keys(zone.id).await?;
        let present = self.client.tsig_keys().await?;
        for key in &tsig {
            if !present
                .iter()
                .any(|p| p.name.trim_end_matches('.') == key.name)
            {
                self.client
                    .create_tsig_key(&crate::TsigKey {
                        id: String::new(),
                        name: key.name.clone(),
                        algorithm: key.algorithm.clone(),
                        key: key.secret.clone(),
                    })
                    .await?;
            }
        }
        self.client
            .set_metadata(
                &zone.name,
                TSIG_ALLOW_AXFR,
                tsig.iter().map(|k| k.name.clone()).collect(),
            )
            .await?;
        self.client
            .set_metadata(&zone.name, ALLOW_AXFR_FROM, zone.axfr_allow_from.clone())
            .await?;
        Ok(())
    }

    /// Makes the `PowerDNS` zone equal to the database and stamps it with the version.
    async fn converge(&self, zone: &Zone) -> Result<Converged, ZoneOpError> {
        let desired = self.desired_rrsets(zone).await?;
        let served = self.client.zone(&zone.name).await?;
        let mut out = self.converge_rrsets(zone, served.as_ref(), desired).await?;
        out.keys_imported = self.converge_dnssec(zone, served.as_ref()).await?;
        self.converge_transfers(zone).await?;
        self.stamp(&zone.name, zone.version).await?;
        if out.created || out.rrsets_changed > 0 {
            self.client.flush_cache(&zone.name).await?;
        }
        Ok(out)
    }

    /// Takes the records, key and allow list of a `PowerDNS` zone that predates the
    /// version stamps into the database; returns the number of rrsets adopted.
    async fn adopt(&self, zone: &Zone, served: &crate::Zone) -> Result<usize, ZoneOpError> {
        let rrsets: Vec<ZoneRrSet> = served
            .rrsets
            .iter()
            .filter(|rr| {
                let challenge = zone.is_system() && rr.rtype == "TXT" && rr.ttl == CHALLENGE_TTL;
                !self.is_protected(zone, rr) && !challenge
            })
            .map(to_stored)
            .collect();
        warn!(zone = %zone.name, rrsets = rrsets.len(), "adopting records from an unstamped pdns zone");
        self.db.replace_zone_rrsets(zone.id, &rrsets).await?;
        let active_key = if self.db.zone_dnssec(zone.id).await?.is_none() {
            self.client
                .cryptokeys(&zone.name)
                .await?
                .into_iter()
                .find(|k| k.active)
        } else {
            None
        };
        if let Some(key) = active_key {
            let full = self.client.cryptokey(&zone.name, key.id).await?;
            self.db
                .set_zone_dnssec(
                    zone.id,
                    Some(&ZoneDnssec {
                        nsec3: !served.nsec3param.is_empty(),
                        algorithm: full.algorithm,
                        private_key: full.privatekey,
                    }),
                )
                .await?;
        }
        if zone.axfr_allow_from.is_empty() {
            let allow = self.client.metadata(&zone.name, ALLOW_AXFR_FROM).await?;
            if !allow.is_empty() {
                self.db.set_zone_axfr_allow_from(zone.id, &allow).await?;
            }
        }
        Ok(rrsets.len())
    }

    /// Removes the given TSIG keys from pdns (unknown ones are ignored).
    async fn remove_pdns_tsig_keys(&self, names: &[String]) -> Result<(), ZoneOpError> {
        if names.is_empty() {
            return Ok(());
        }
        for key in self.client.tsig_keys().await? {
            if names.iter().any(|n| n == key.name.trim_end_matches('.')) {
                self.client.delete_tsig_key(&key.id).await?;
            }
        }
        Ok(())
    }

    /// Brings `PowerDNS` in line with the database: zones it should not serve are removed,
    /// active zones are created or converged. Zones whose stamped version equals the
    /// database version are skipped unless `force` is set; a zone that was never stamped
    /// is adopted first.
    pub async fn sync(&self, force: bool) -> Result<SyncReport, ZoneOpError> {
        let mut report = SyncReport::default();
        let zones = self.db.zones(None).await?;
        let served = self.client.zones().await?;
        let active: FxHashSet<String> = zones
            .iter()
            .filter(|z| z.active)
            .map(|z| apex(&z.name))
            .collect();
        for summary in &served {
            if !active.contains(&summary.name) {
                warn!(zone = %summary.name, "removing pdns zone the database does not serve");
                self.client.delete_zone(&summary.name).await?;
                report.removed += 1;
            }
        }
        let mut wanted_tsig: FxHashSet<String> = FxHashSet::default();
        for zone in zones.iter().filter(|z| z.active) {
            for key in self.db.zone_tsig_keys(zone.id).await? {
                wanted_tsig.insert(key.name);
            }
            let exists = served.iter().any(|s| s.name == apex(&zone.name));
            let stamped = if exists {
                parse_version(&self.client.metadata(&zone.name, VERSION_METADATA).await?)
            } else {
                None
            };
            let mut zone = zone.clone();
            if exists && stamped.is_none() {
                if let Some(current) = self.client.zone(&zone.name).await? {
                    report.records_adopted += self.adopt(&zone, &current).await?;
                    zone = self.db.zone(&zone.name).await?;
                }
            } else if !force && stamped == Some(zone.version) {
                continue;
            }
            let done = self.converge(&zone).await?;
            report.created += usize::from(done.created);
            report.rrsets_changed += done.rrsets_changed;
            report.keys_imported += done.keys_imported;
        }
        for key in self.client.tsig_keys().await? {
            if !wanted_tsig.contains(key.name.trim_end_matches('.')) {
                self.client.delete_tsig_key(&key.id).await?;
            }
        }
        report.status = self.status().await?;
        Ok(report)
    }

    /// Compares the database with `PowerDNS` without changing anything.
    pub async fn status(&self) -> Result<SyncStatus, ZoneOpError> {
        let mut status = SyncStatus::default();
        let zones = self.db.zones(None).await?;
        let served = self.client.zones().await?;
        let mut seen: FxHashSet<String> = FxHashSet::default();
        for zone in zones.iter().filter(|z| z.active) {
            seen.insert(apex(&zone.name));
            status.db_version += zone.version;
            let pdns = if served.iter().any(|s| s.name == apex(&zone.name)) {
                parse_version(&self.client.metadata(&zone.name, VERSION_METADATA).await?)
            } else {
                None
            };
            status.pdns_version += pdns.unwrap_or(0);
            if pdns != Some(zone.version) {
                status.zones.push(ZoneVersions {
                    name: zone.name.clone(),
                    db: Some(zone.version),
                    pdns,
                });
            }
        }
        for summary in served.iter().filter(|s| !seen.contains(&s.name)) {
            let pdns = parse_version(
                &self
                    .client
                    .metadata(&summary.name, VERSION_METADATA)
                    .await?,
            );
            status.pdns_version += pdns.unwrap_or(0);
            status.zones.push(ZoneVersions {
                name: summary.name.trim_end_matches('.').to_owned(),
                db: None,
                pdns,
            });
        }
        Ok(status)
    }

    // --- Zone lifecycle ---

    /// Creates a pending zone (with its apex NS) and runs its first delegation check.
    pub async fn create(&self, user_id: Option<i64>, name: &str) -> Result<Zone, ZoneOpError> {
        let name = names::parse_zone_name(name)?;
        if names::inside(&name, &self.policy.base_zone) {
            return Err(ZoneOpError::NotAllowed("inside the server's own zone"));
        }
        if names::is_public_suffix(&name) {
            return Err(ZoneOpError::NotAllowed("a public suffix cannot be a zone"));
        }
        let zone = self.db.create_zone(user_id, &name).await?;
        self.db
            .apply_zone_rrsets(
                zone.id,
                &[ZoneRrSet {
                    name: apex(&name),
                    rtype: "NS".to_owned(),
                    ttl: names::DEFAULT_TTL,
                    records: vec![apex(&self.policy.nsname)],
                }],
            )
            .await?;
        let zone = self.db.zone(&name).await?;
        match self.run_check(&zone).await {
            Ok(updated) => Ok(updated),
            Err(e) => {
                warn!(zone = %zone.name, error = %e, "initial delegation check inconclusive");
                Ok(zone)
            }
        }
    }

    /// A zone by name.
    pub async fn get(&self, name: &str) -> Result<Zone, ZoneOpError> {
        let name = names::parse_zone_name(name).map_err(|_| ZoneOpError::NotFound("zone"))?;
        Ok(self.db.zone(&name).await?)
    }

    /// Runs a delegation check on request; limited to one per [`VERIFY_COOLDOWN`].
    pub async fn verify(&self, zone: &Zone) -> Result<Zone, ZoneOpError> {
        if zone.is_system() {
            return Err(ZoneOpError::NotAllowed("the base zone is always active"));
        }
        if zone
            .last_check
            .is_some_and(|t| now() - t < i64::try_from(VERIFY_COOLDOWN.as_secs()).unwrap_or(60))
        {
            return Err(ZoneOpError::RateLimited);
        }
        self.run_check(zone).await
    }

    async fn run_check(&self, zone: &Zone) -> Result<Zone, ZoneOpError> {
        match self.checker.check(&zone.name, &self.policy.nsname).await {
            Ok(Delegation::Delegated) => {
                let updated = self.activate(zone, zone.exempt).await?;
                if !zone.active {
                    info!(zone = %zone.name, "zone activated: delegation verified");
                }
                Ok(updated)
            }
            Ok(Delegation::NotDelegated(reason)) => {
                let updated = self.db.record_zone_check_failure(zone.id, &reason).await?;
                if updated.active && updated.failed_checks >= MAX_FAILED_CHECKS {
                    warn!(zone = %zone.name, failures = updated.failed_checks, "zone deactivated: delegation lost");
                    return self.deactivate(&updated).await;
                }
                Ok(updated)
            }
            Err(e) => Err(ZoneOpError::Check(e)),
        }
    }

    /// Marks a zone active and builds it in `PowerDNS` from the stored data.
    async fn activate(&self, zone: &Zone, exempt: bool) -> Result<Zone, ZoneOpError> {
        let updated = self.db.activate_zone(zone.id, exempt).await?;
        if !zone.active || self.client.zone(&zone.name).await?.is_none() {
            self.converge(&updated).await?;
        }
        Ok(updated)
    }

    /// Removes the zone from `PowerDNS`; its data stays for a later re-activation.
    async fn deactivate(&self, zone: &Zone) -> Result<Zone, ZoneOpError> {
        self.remove_from_pdns(zone).await?;
        Ok(self.db.deactivate_zone(zone.id).await?)
    }

    async fn remove_from_pdns(&self, zone: &Zone) -> Result<(), ZoneOpError> {
        self.client.delete_zone(&zone.name).await?;
        let names: Vec<String> = self
            .db
            .zone_tsig_keys(zone.id)
            .await?
            .into_iter()
            .map(|k| k.name)
            .collect();
        self.remove_pdns_tsig_keys(&names).await
    }

    /// Admin exemption: `true` activates without a check and stops re-checks; `false`
    /// returns the zone to the normal cycle.
    pub async fn set_exempt(&self, zone: &Zone, exempt: bool) -> Result<Zone, ZoneOpError> {
        if zone.is_system() {
            return Err(ZoneOpError::NotAllowed("the base zone is always exempt"));
        }
        if exempt {
            return self.activate(zone, true).await;
        }
        if zone.active {
            return Ok(self.db.clear_zone_exemption(zone.id).await?);
        }
        Ok(zone.clone())
    }

    /// Deletes a zone everywhere.
    pub async fn delete(&self, zone: &Zone) -> Result<(), ZoneOpError> {
        if zone.is_system() {
            return Err(ZoneOpError::NotAllowed("the base zone cannot be deleted"));
        }
        self.remove_from_pdns(zone).await?;
        self.db.delete_zone(zone.id).await?;
        Ok(())
    }

    /// Removes a user's zones from `PowerDNS` (the rows go with the user).
    pub async fn remove_user_zones(&self, user_id: i64) -> Result<(), ZoneOpError> {
        for zone in self.db.zones(Some(user_id)).await? {
            self.remove_from_pdns(&zone).await?;
        }
        Ok(())
    }

    const fn require_active(zone: &Zone) -> Result<(), ZoneOpError> {
        if zone.active {
            Ok(())
        } else {
            Err(ZoneOpError::NotActive)
        }
    }

    // --- Records ---

    /// All stored rrsets of an active zone, ordered by name and type.
    pub async fn rrsets(&self, zone: &Zone) -> Result<Vec<RrSet>, ZoneOpError> {
        Self::require_active(zone)?;
        Ok(self
            .db
            .zone_rrsets(zone.id)
            .await?
            .iter()
            .map(|rr| RrSet {
                changetype: None,
                ..to_change(rr)
            })
            .collect())
    }

    /// Applies validated rrset changes to an active zone: `PowerDNS` checks them first,
    /// then they are stored.
    pub async fn patch_rrsets(
        &self,
        zone: &Zone,
        changes: &[RrSetChange],
    ) -> Result<(), ZoneOpError> {
        Self::require_active(zone)?;
        let rrsets = names::validate_changes(&zone.name, changes)?;
        self.client.patch_rrsets(&zone.name, &rrsets).await?;
        let stored: Vec<ZoneRrSet> = rrsets.iter().map(to_stored).collect();
        let version = self.db.apply_zone_rrsets(zone.id, &stored).await?;
        self.stamp(&zone.name, version).await
    }

    // --- DNSSEC ---

    /// DNSSEC state of an active zone; the DNSKEY and DS come from `PowerDNS`.
    pub async fn dnssec(&self, zone: &Zone) -> Result<DnssecInfo, ZoneOpError> {
        Self::require_active(zone)?;
        let Some(stored) = self.db.zone_dnssec(zone.id).await? else {
            return Ok(DnssecInfo {
                enabled: false,
                nsec_mode: "nsec".to_owned(),
                dnskey: None,
                ds: Vec::new(),
            });
        };
        let keys = self.client.cryptokeys(&zone.name).await?;
        let key = keys.iter().find(|k| k.active);
        Ok(DnssecInfo {
            enabled: true,
            nsec_mode: if stored.nsec3 { "nsec3" } else { "nsec" }.to_owned(),
            dnskey: key.map(|k| k.dnskey.clone()).filter(|d| !d.is_empty()),
            ds: key.map(|k| k.ds.clone()).unwrap_or_default(),
        })
    }

    /// Enables (one ECDSAP256SHA256 CSK) or disables signing; `nsec_mode` is `nsec` or `nsec3`.
    pub async fn set_dnssec(
        &self,
        zone: &Zone,
        enabled: bool,
        nsec_mode: &str,
    ) -> Result<DnssecInfo, ZoneOpError> {
        Self::require_active(zone)?;
        if !NSEC_MODES.contains(&nsec_mode) {
            return Err(invalid(
                "invalid_nsec_mode",
                "nsec_mode must be nsec or nsec3",
            ));
        }
        let keys = self.client.cryptokeys(&zone.name).await?;
        let version = if enabled {
            let key = match keys.into_iter().find(|k| k.active) {
                Some(k) => k,
                None => self.client.add_csk(&zone.name, DNSSEC_ALGORITHM).await?,
            };
            self.client
                .set_nsec3param(&zone.name, (nsec_mode == "nsec3").then_some(NSEC3_PARAM))
                .await?;
            let full = self.client.cryptokey(&zone.name, key.id).await?;
            self.db
                .set_zone_dnssec(
                    zone.id,
                    Some(&ZoneDnssec {
                        nsec3: nsec_mode == "nsec3",
                        algorithm: full.algorithm,
                        private_key: full.privatekey,
                    }),
                )
                .await?
        } else {
            for key in keys {
                self.client.delete_cryptokey(&zone.name, key.id).await?;
            }
            self.client.set_nsec3param(&zone.name, None).await?;
            self.db.set_zone_dnssec(zone.id, None).await?
        };
        self.stamp(&zone.name, version).await?;
        self.dnssec(zone).await
    }

    // --- Transfers ---

    /// Transfer settings of an active zone.
    pub async fn transfer(&self, zone: &Zone) -> Result<TransferInfo, ZoneOpError> {
        Self::require_active(zone)?;
        let current = self.db.zone(&zone.name).await?;
        let tsig_keys = self
            .db
            .zone_tsig_keys(zone.id)
            .await?
            .into_iter()
            .map(|k| TsigKeyInfo {
                name: k.name,
                algorithm: k.algorithm,
            })
            .collect();
        Ok(TransferInfo {
            allow_from: current.axfr_allow_from,
            tsig_keys,
        })
    }

    /// Replaces the addresses allowed to transfer without TSIG.
    pub async fn set_transfer_allow_from(
        &self,
        zone: &Zone,
        allow_from: &[String],
    ) -> Result<TransferInfo, ZoneOpError> {
        Self::require_active(zone)?;
        let mut nets = Vec::with_capacity(allow_from.len());
        for entry in allow_from {
            let entry = entry.trim();
            let net: ipnet::IpNet = entry
                .parse()
                .or_else(|_| entry.parse::<std::net::IpAddr>().map(ipnet::IpNet::from))
                .map_err(|_| {
                    invalid("invalid_cidr", format!("invalid address or CIDR {entry:?}"))
                })?;
            nets.push(net.trunc().to_string());
        }
        self.client
            .set_metadata(&zone.name, ALLOW_AXFR_FROM, nets.clone())
            .await?;
        let version = self.db.set_zone_axfr_allow_from(zone.id, &nets).await?;
        self.stamp(&zone.name, version).await?;
        self.transfer(zone).await
    }

    /// Creates a TSIG key for the zone; returns it with the base64 secret.
    pub async fn add_tsig_key(
        &self,
        zone: &Zone,
        name: &str,
        algorithm: &str,
    ) -> Result<(TsigKeyInfo, String), ZoneOpError> {
        Self::require_active(zone)?;
        let name = names::parse_key_name(name)?;
        if !TSIG_ALGORITHMS.contains(&algorithm) {
            return Err(invalid("invalid_algorithm", "unsupported TSIG algorithm"));
        }
        let existing = self.db.zone_tsig_keys(zone.id).await?;
        if existing.iter().any(|k| k.name == name) {
            return Err(invalid("tsig_key_exists", "a key with that name exists"));
        }
        let created = match self
            .client
            .create_tsig_key(&crate::TsigKey {
                id: String::new(),
                name: name.clone(),
                algorithm: algorithm.to_owned(),
                key: String::new(),
            })
            .await
        {
            Ok(k) => k,
            Err(PdnsError::Api { status: 409, .. }) => {
                return Err(invalid("tsig_key_exists", "a key with that name exists"));
            }
            Err(e) => return Err(e.into()),
        };
        let version = self
            .db
            .add_zone_tsig_key(
                zone.id,
                &ZoneTsigKey {
                    name: name.clone(),
                    algorithm: created.algorithm.clone(),
                    secret: created.key.clone(),
                },
            )
            .await?;
        let mut allowed: Vec<String> = existing.into_iter().map(|k| k.name).collect();
        allowed.push(name.clone());
        self.client
            .set_metadata(&zone.name, TSIG_ALLOW_AXFR, allowed)
            .await?;
        self.stamp(&zone.name, version).await?;
        Ok((
            TsigKeyInfo {
                name,
                algorithm: created.algorithm,
            },
            created.key,
        ))
    }

    /// Deletes one of the zone's TSIG keys.
    pub async fn delete_tsig_key(&self, zone: &Zone, name: &str) -> Result<(), ZoneOpError> {
        Self::require_active(zone)?;
        let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
        let version = self.db.delete_zone_tsig_key(zone.id, &name).await?;
        let remaining = self
            .db
            .zone_tsig_keys(zone.id)
            .await?
            .into_iter()
            .map(|k| k.name)
            .collect();
        self.client
            .set_metadata(&zone.name, TSIG_ALLOW_AXFR, remaining)
            .await?;
        self.remove_pdns_tsig_keys(std::slice::from_ref(&name))
            .await?;
        self.stamp(&zone.name, version).await
    }

    // --- Background work ---

    /// One pass of the periodic checks: pending zones are verified, active ones re-checked,
    /// abandoned pending zones deleted.
    pub async fn run_checks(&self) -> Result<CheckReport, ZoneOpError> {
        let now = now();
        let secs = |d: Duration| i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
        let mut report = CheckReport::default();
        for zone in self.db.zones(None).await? {
            if zone.is_system() || zone.exempt {
                continue;
            }
            if !zone.active && now - zone.created_at > secs(PENDING_EXPIRY) {
                info!(zone = %zone.name, "deleting pending zone nobody activated");
                self.db.delete_zone(zone.id).await?;
                report.expired += 1;
                continue;
            }
            let interval = if zone.active {
                ACTIVE_RECHECK_INTERVAL
            } else {
                PENDING_CHECK_INTERVAL
            };
            if zone.last_check.is_some_and(|t| now - t < secs(interval)) {
                continue;
            }
            report.checked += 1;
            match self.run_check(&zone).await {
                Ok(updated) => {
                    if updated.active && !zone.active {
                        report.activated += 1;
                    } else if !updated.active && zone.active {
                        report.deactivated += 1;
                    }
                }
                Err(e) => warn!(zone = %zone.name, error = %e, "delegation check inconclusive"),
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_record_lines() {
        assert_eq!(
            parse_static("dns.example.com. A 198.51.100.1"),
            Some((
                "dns.example.com".into(),
                3600,
                "A".into(),
                "198.51.100.1".into()
            ))
        );
        assert_eq!(
            parse_static("Dns.Example.com 300 IN TXT \"a b\""),
            Some((
                "dns.example.com".into(),
                300,
                "TXT".into(),
                "\"a b\"".into()
            ))
        );
        assert_eq!(parse_static("dns.example.com. A"), None);
        assert_eq!(parse_static(""), None);
    }

    #[test]
    fn grouping_keeps_order_and_merges_values() {
        let rrsets = group_rrsets([
            ("a.test".into(), 60, "A".into(), "192.0.2.1".into()),
            ("a.test".into(), 60, "NS".into(), "ns1.test.".into()),
            ("a.test".into(), 60, "A".into(), "192.0.2.2".into()),
        ]);
        assert_eq!(rrsets.len(), 2);
        assert_eq!(rrsets[0].rtype, "A");
        assert_eq!(rrsets[0].records, ["192.0.2.1", "192.0.2.2"]);
        assert_eq!(rrsets[1].name, "a.test.");
    }

    #[test]
    fn challenge_rrsets() {
        let rr = Zones::challenge_rrset("x.base.test", &["t1".into(), "t2".into()]);
        assert_eq!(rr.ttl, CHALLENGE_TTL);
        assert_eq!(rr.records[0].content, "\"t1\"");
        assert_eq!(
            Zones::challenge_rrset("x.base.test", &[]).changetype,
            Some(crate::ChangeType::Delete)
        );
    }
}
