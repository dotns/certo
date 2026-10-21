//! Zone service: ownership and activation in the certo database, DNS data in `PowerDNS`.

use std::{sync::Arc, time::Duration};

use base64::Engine as _;
use certo_core::Zone;
use certo_db::{Db, DbError};
use rustc_hash::FxHashMap;
use tracing::{error, info, warn};

use crate::{
    Client, Delegation, DelegationChecker, NewZone, PdnsError, RecordData, RrSet,
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

/// Groups `(name, type, ttl, rdata)` tuples into REPLACE rrsets.
fn group_rrsets(records: impl IntoIterator<Item = (String, u32, String, String)>) -> Vec<RrSet> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut sets: FxHashMap<(String, String), RrSet> = FxHashMap::default();
    for (name, ttl, rtype, rdata) in records {
        let key = (apex(&name), rtype.clone());
        let set = sets.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            RrSet::replace(apex(&name), rtype, ttl, Vec::new())
        });
        set.records.push(RecordData::new(rdata));
    }
    order.into_iter().filter_map(|k| sets.remove(&k)).collect()
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

    /// Whether `PowerDNS` answers its API.
    pub async fn ping(&self) -> bool {
        self.client.ping().await
    }

    // --- Base zone and challenge TXT ---

    /// Creates the server's own zone on first start, seeded from the config record lines;
    /// an existing zone is left untouched.
    pub async fn ensure_base_zone(&self, seed: &[String]) -> Result<(), ZoneOpError> {
        let base = &self.policy.base_zone;
        match self.db.zone(base).await {
            Ok(_) => {}
            Err(DbError::NotFound(_)) => {
                let zone = self.db.create_zone(None, base).await?;
                self.db.activate_zone(zone.id, true).await?;
            }
            Err(e) => return Err(e.into()),
        }
        if self.client.zone(base).await?.is_some() {
            if !seed.is_empty() {
                info!(
                    "general.records was imported into the base zone; it can be removed from the config"
                );
            }
            return Ok(());
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
        self.client
            .create_zone(&NewZone::native(apex(base), Vec::new(), rrsets))
            .await?;
        self.client.flush_cache(base).await?;
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
        Ok(())
    }

    /// Makes the base zone's challenge TXT rrsets match the database (startup).
    pub async fn reconcile_txt(&self) -> Result<usize, ZoneOpError> {
        let base = &self.policy.base_zone;
        let own = format!("_acme-challenge.{}.", self.policy.base_zone);
        let served = self
            .client
            .zone(base)
            .await?
            .ok_or(ZoneOpError::NotFound("zone"))?
            .rrsets;
        let wanted = self.db.txt_by_domain().await?;
        let mut changes: Vec<RrSet> = wanted
            .iter()
            .map(|(name, values)| Self::challenge_rrset(name, values))
            .collect();
        for rr in served {
            let stale = rr.rtype == "TXT"
                && rr.ttl == CHALLENGE_TTL
                && rr.name != own
                && !wanted.iter().any(|(n, _)| apex(n) == rr.name);
            if stale {
                changes.push(RrSet::delete(rr.name, "TXT"));
            }
        }
        let n = changes.len();
        if n > 0 {
            self.client.patch_rrsets(base, &changes).await?;
        }
        Ok(n)
    }

    /// Sets (or clears) the DNS-01 token for certo's own certificate, issued for the
    /// base zone name.
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

    // --- Zone lifecycle ---

    /// Creates a pending zone and runs its first delegation check.
    pub async fn create(&self, user_id: Option<i64>, name: &str) -> Result<Zone, ZoneOpError> {
        let name = names::parse_zone_name(name)?;
        if names::inside(&name, &self.policy.base_zone) {
            return Err(ZoneOpError::NotAllowed("inside the server's own zone"));
        }
        let zone = self.db.create_zone(user_id, &name).await?;
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

    /// Creates the zone in `PowerDNS` (restoring a snapshot if one exists) and marks it active.
    async fn activate(&self, zone: &Zone, exempt: bool) -> Result<Zone, ZoneOpError> {
        if self.client.zone(&zone.name).await?.is_none() {
            let snapshot: Option<Vec<RrSet>> = self
                .db
                .zone_snapshot(zone.id)
                .await?
                .and_then(|s| serde_json::from_str(&s).ok());
            let new = match snapshot {
                Some(rrsets) if !rrsets.is_empty() => {
                    NewZone::native(apex(&zone.name), Vec::new(), rrsets)
                }
                _ => NewZone::native(
                    apex(&zone.name),
                    vec![apex(&self.policy.nsname)],
                    Vec::new(),
                ),
            };
            self.client.create_zone(&new).await?;
            self.client.flush_cache(&zone.name).await?;
        }
        Ok(self.db.activate_zone(zone.id, exempt).await?)
    }

    /// Removes the zone from `PowerDNS`, keeping its records for a later re-activation.
    async fn deactivate(&self, zone: &Zone) -> Result<Zone, ZoneOpError> {
        let rrsets: Vec<RrSet> = self
            .client
            .zone(&zone.name)
            .await?
            .map(|z| {
                z.rrsets
                    .into_iter()
                    .map(|mut rr| {
                        rr.changetype = None;
                        rr
                    })
                    .collect()
            })
            .unwrap_or_default();
        let snapshot = serde_json::to_string(&rrsets).unwrap_or_else(|_| "[]".to_owned());
        self.client.delete_zone(&zone.name).await?;
        Ok(self.db.deactivate_zone(zone.id, &snapshot).await?)
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
        self.client.delete_zone(&zone.name).await?;
        self.db.delete_zone(zone.id).await?;
        Ok(())
    }

    /// Removes a user's zones from `PowerDNS` (the rows go with the user).
    pub async fn remove_user_zones(&self, user_id: i64) -> Result<(), ZoneOpError> {
        for zone in self.db.zones(Some(user_id)).await? {
            self.client.delete_zone(&zone.name).await?;
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

    /// All rrsets of an active zone.
    pub async fn rrsets(&self, zone: &Zone) -> Result<Vec<RrSet>, ZoneOpError> {
        Self::require_active(zone)?;
        let mut rrsets = self
            .client
            .zone(&zone.name)
            .await?
            .ok_or(ZoneOpError::NotFound("zone"))?
            .rrsets;
        for rr in &mut rrsets {
            rr.changetype = None;
        }
        rrsets.sort_by(|a, b| (&a.name, &a.rtype).cmp(&(&b.name, &b.rtype)));
        Ok(rrsets)
    }

    /// Applies validated rrset changes to an active zone.
    pub async fn patch_rrsets(
        &self,
        zone: &Zone,
        changes: &[RrSetChange],
    ) -> Result<(), ZoneOpError> {
        Self::require_active(zone)?;
        let rrsets = names::validate_changes(&zone.name, changes)?;
        self.client.patch_rrsets(&zone.name, &rrsets).await?;
        Ok(())
    }

    // --- DNSSEC ---

    /// DNSSEC state of an active zone.
    pub async fn dnssec(&self, zone: &Zone) -> Result<DnssecInfo, ZoneOpError> {
        Self::require_active(zone)?;
        let keys = self.client.cryptokeys(&zone.name).await?;
        let nsec3 = self
            .client
            .zone(&zone.name)
            .await?
            .is_some_and(|z| !z.nsec3param.is_empty());
        let key = keys.iter().find(|k| k.active);
        Ok(DnssecInfo {
            enabled: key.is_some(),
            nsec_mode: if nsec3 { "nsec3" } else { "nsec" }.to_owned(),
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
        if enabled {
            if !keys.iter().any(|k| k.active) {
                self.client.add_csk(&zone.name, DNSSEC_ALGORITHM).await?;
            }
            self.client
                .set_nsec3param(&zone.name, (nsec_mode == "nsec3").then_some(NSEC3_PARAM))
                .await?;
        } else {
            for key in keys {
                self.client.delete_cryptokey(&zone.name, key.id).await?;
            }
            self.client.set_nsec3param(&zone.name, None).await?;
        }
        self.dnssec(zone).await
    }

    // --- Transfers ---

    /// Transfer settings of an active zone.
    pub async fn transfer(&self, zone: &Zone) -> Result<TransferInfo, ZoneOpError> {
        Self::require_active(zone)?;
        let allow_from = self.client.metadata(&zone.name, ALLOW_AXFR_FROM).await?;
        let names = self.client.metadata(&zone.name, TSIG_ALLOW_AXFR).await?;
        let all = self.client.tsig_keys().await?;
        let tsig_keys = names
            .iter()
            .map(|n| TsigKeyInfo {
                name: n.clone(),
                algorithm: all
                    .iter()
                    .find(|k| k.name.trim_end_matches('.') == n.trim_end_matches('.'))
                    .map(|k| k.algorithm.clone())
                    .unwrap_or_default(),
            })
            .collect();
        Ok(TransferInfo {
            allow_from,
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
            .set_metadata(&zone.name, ALLOW_AXFR_FROM, nets)
            .await?;
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
        let mut allowed = self.client.metadata(&zone.name, TSIG_ALLOW_AXFR).await?;
        if allowed.iter().any(|n| n.trim_end_matches('.') == name) {
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
        allowed.push(name.clone());
        self.client
            .set_metadata(&zone.name, TSIG_ALLOW_AXFR, allowed)
            .await?;
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
        let allowed = self.client.metadata(&zone.name, TSIG_ALLOW_AXFR).await?;
        if !allowed.iter().any(|n| n.trim_end_matches('.') == name) {
            return Err(ZoneOpError::NotFound("tsig key"));
        }
        let remaining = allowed
            .into_iter()
            .filter(|n| n.trim_end_matches('.') != name)
            .collect();
        self.client
            .set_metadata(&zone.name, TSIG_ALLOW_AXFR, remaining)
            .await?;
        if let Some(key) = self
            .client
            .tsig_keys()
            .await?
            .into_iter()
            .find(|k| k.name.trim_end_matches('.') == name)
        {
            self.client.delete_tsig_key(&key.id).await?;
        }
        Ok(())
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

    /// Imports the zones of a version 3 database into `PowerDNS`, then finishes the schema
    /// upgrade. Returns the number of zones imported.
    pub async fn import_legacy(&self) -> Result<usize, ZoneOpError> {
        let legacy = self.db.legacy_zones().await?;
        for z in &legacy {
            if self.client.zone(&z.name).await?.is_none() {
                let mut records: Vec<(String, u32, String, String)> = z
                    .records
                    .iter()
                    .map(|(n, t, ttl, d)| (n.clone(), *ttl, t.clone(), d.clone()))
                    .collect();
                if !records.iter().any(|(n, _, t, _)| *n == z.name && t == "NS") {
                    records.push((
                        z.name.clone(),
                        names::DEFAULT_TTL,
                        "NS".to_owned(),
                        apex(&self.policy.nsname),
                    ));
                }
                self.client
                    .create_zone(&NewZone::native(
                        apex(&z.name),
                        Vec::new(),
                        group_rrsets(records),
                    ))
                    .await?;
            }
            if z.dnssec {
                let key = self.client.add_csk(&z.name, DNSSEC_ALGORITHM).await?;
                self.client
                    .set_nsec3param(&z.name, z.nsec3.then_some(NSEC3_PARAM))
                    .await?;
                warn!(zone = %z.name, ds = ?key.ds, "DNSSEC re-keyed during import: publish the new DS at the registrar");
            }
            if !z.axfr_allow_from.is_empty() {
                self.client
                    .set_metadata(&z.name, ALLOW_AXFR_FROM, z.axfr_allow_from.clone())
                    .await?;
            }
            let mut allowed = Vec::new();
            for (name, algorithm, secret) in &z.tsig_keys {
                let key = crate::TsigKey {
                    id: String::new(),
                    name: name.clone(),
                    algorithm: algorithm.clone(),
                    key: base64::engine::general_purpose::STANDARD.encode(secret),
                };
                match self.client.create_tsig_key(&key).await {
                    Ok(_) | Err(PdnsError::Api { status: 409, .. }) => allowed.push(name.clone()),
                    Err(e) => return Err(e.into()),
                }
            }
            if !allowed.is_empty() {
                self.client
                    .set_metadata(&z.name, TSIG_ALLOW_AXFR, allowed)
                    .await?;
            }
            info!(zone = %z.name, records = z.records.len(), "zone imported into PowerDNS");
        }
        self.db.complete_import().await?;
        Ok(legacy.len())
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
        assert_eq!(rrsets[0].records.len(), 2);
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
