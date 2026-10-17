//! Hosted zones: the database is the source of truth; every change rebuilds the zone
//! and swaps it into the serving catalog.
use std::{net::IpAddr, sync::Arc, time::Duration};

use certo_core::{TsigKey, Zone, ZoneKey, ZoneRecord, ids};
use certo_db::{Db, DbError};
use hickory_server::{
    dnssec::NxProofKind,
    proto::{
        dnssec::{
            Algorithm, DigestType, DnssecSigner, Nsec3HashAlgorithm, SigningKey,
            crypto::EcdsaSigningKey,
            rdata::{DNSKEY, DS},
        },
        rr::{
            LowerName, Name, RData, Record, RecordType, TSigner,
            rdata::{NS, SOA, tsig::TsigAlgorithm},
        },
    },
};
use ipnet::IpNet;
use rustls_pki_types::PrivatePkcs8KeyDer;
use secrecy::ExposeSecret;
use tokio::sync::Mutex;

use crate::{
    AxfrAccess, DnssecConfig, DynamicCatalog, ZoneConfig, ZoneError, build_zone,
    records::{
        RecordError, RecordInput, ValidRecord, absolute, parse_zone_name, stored_name, validate,
    },
};

const SOA_TTL: u32 = 3600;
const SOA_REFRESH: i32 = 3600;
const SOA_RETRY: i32 = 900;
const SOA_EXPIRE: i32 = 1_209_600;
const SOA_MINIMUM: u32 = 300;

/// Zones are signed with one combined key of this algorithm.
const KEY_ALGORITHM: Algorithm = Algorithm::ECDSAP256SHA256;
/// RRSIG validity; zones are re-signed well before it runs out.
const SIG_DURATION: Duration = Duration::from_secs(14 * 86_400);
/// How often signed zones are re-signed.
pub const RESIGN_INTERVAL: Duration = Duration::from_secs(86_400);
/// TSIG timestamp tolerance (RFC 8945 recommends 300s).
const TSIG_FUDGE: u16 = 300;

/// `nsec` (default) or `nsec3` with RFC 9276 parameters.
pub const NSEC_MODES: [&str; 2] = ["nsec", "nsec3"];

/// TSIG algorithms accepted for transfer keys, with their secret length in bytes.
pub const TSIG_ALGORITHMS: [(&str, usize); 3] = [
    ("hmac-sha256", 32),
    ("hmac-sha384", 48),
    ("hmac-sha512", 64),
];

/// DNSSEC state of a zone, with what to publish at the parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnssecInfo {
    /// Whether the zone is signed.
    pub enabled: bool,
    /// `nsec` or `nsec3`.
    pub nsec_mode: String,
    /// The DNSKEY RDATA, when signed.
    pub dnskey: Option<String>,
    /// The SHA-256 DS RDATA for the parent zone, when signed.
    pub ds: Option<String>,
}

/// A TSIG key without its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsigKeyInfo {
    /// Key name.
    pub name: String,
    /// Algorithm.
    pub algorithm: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// Who may transfer a zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferInfo {
    /// Networks allowed without TSIG.
    pub allow_from: Vec<String>,
    /// Keys accepted for signed transfers.
    pub tsig_keys: Vec<TsigKeyInfo>,
}

fn nx_proof(mode: &str) -> NxProofKind {
    if mode == "nsec3" {
        NxProofKind::Nsec3 {
            algorithm: Nsec3HashAlgorithm::default(),
            salt: Arc::from([]),
            iterations: 0,
            opt_out: false,
        }
    } else {
        NxProofKind::Nsec
    }
}

fn signing_key(key: &ZoneKey) -> Result<EcdsaSigningKey, ZoneOpError> {
    if key.algorithm != KEY_ALGORITHM.to_string() {
        return Err(ZoneOpError::Dnssec(format!(
            "unsupported key algorithm {}",
            key.algorithm
        )));
    }
    let der = PrivatePkcs8KeyDer::from(key.private_key.expose_secret());
    EcdsaSigningKey::from_pkcs8(&der, KEY_ALGORITHM).map_err(|e| ZoneOpError::Dnssec(e.to_string()))
}

fn dnssec_key(key: &ZoneKey) -> Result<(EcdsaSigningKey, DNSKEY), ZoneOpError> {
    let signing = signing_key(key)?;
    let public = signing
        .to_public_key()
        .map_err(|e| ZoneOpError::Dnssec(e.to_string()))?;
    let dnskey = DNSKEY::from_key(&public);
    Ok((signing, dnskey))
}

fn tsig_algorithm(name: &str) -> Option<TsigAlgorithm> {
    match name {
        "hmac-sha256" => Some(TsigAlgorithm::HmacSha256),
        "hmac-sha384" => Some(TsigAlgorithm::HmacSha384),
        "hmac-sha512" => Some(TsigAlgorithm::HmacSha512),
        _ => None,
    }
}

fn tsig_signer(key: &TsigKey) -> Option<TSigner> {
    TSigner::new(
        key.secret.expose_secret().to_vec(),
        tsig_algorithm(&key.algorithm)?,
        absolute(&key.name).ok()?,
        TSIG_FUDGE,
    )
    .ok()
}

/// Server-wide rules for hosted zones.
#[derive(Debug, Clone)]
pub struct ZonePolicy {
    /// The server's own (challenge) domain; zones may not be at or under it.
    pub base_domain: Name,
    /// SOA MNAME and default apex NS of new zones.
    pub nsname: Name,
    /// SOA RNAME.
    pub nsadmin: Name,
    /// Names served from the static config records, which zones may not cover.
    pub reserved: Vec<Name>,
}

impl ZonePolicy {
    /// Builds the policy from the config names; `static_owners` from the challenge responder.
    pub fn new(
        base_domain: Name,
        nsname: &str,
        nsadmin: &str,
        static_owners: Vec<Name>,
    ) -> Result<Self, RecordError> {
        Ok(Self {
            reserved: static_owners
                .into_iter()
                .filter(|n| !base_domain.zone_of(n))
                .collect(),
            base_domain,
            nsname: absolute(nsname)?,
            nsadmin: absolute(nsadmin)?,
        })
    }
}

/// Zone and record operation failures.
#[derive(Debug, thiserror::Error)]
pub enum ZoneOpError {
    /// The name or record is invalid.
    #[error(transparent)]
    Record(#[from] RecordError),
    /// The zone may not be hosted here.
    #[error("zone not allowed: {0}")]
    NotAllowed(&'static str),
    /// A zone with that name exists.
    #[error("zone already exists")]
    Exists,
    /// The zone or record does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// Storage failure.
    #[error(transparent)]
    Db(DbError),
    /// The zone could not be built for serving.
    #[error(transparent)]
    Build(#[from] ZoneError),
    /// A request field is invalid.
    #[error("{message}")]
    Invalid {
        /// Machine-readable code.
        code: &'static str,
        /// Explanation.
        message: String,
    },
    /// Key generation or loading failed.
    #[error("dnssec: {0}")]
    Dnssec(String),
    /// The blocking build task was cancelled or panicked.
    #[error("zone build task failed")]
    Task,
}

impl From<DbError> for ZoneOpError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound(what) => Self::NotFound(what),
            DbError::Conflict("zone") => Self::Exists,
            DbError::Conflict("zone overlap") => Self::NotAllowed("overlaps another user's zone"),
            DbError::Conflict("cname") => Self::Record(RecordError::CnameConflict),
            DbError::Conflict("tsig key") => Self::Invalid {
                code: "tsig_key_exists",
                message: "a TSIG key with that name exists".into(),
            },
            other => Self::Db(other),
        }
    }
}

/// Hosted zone management, shared by the API and startup.
#[derive(Clone)]
pub struct Zones {
    db: Db,
    catalog: DynamicCatalog,
    policy: Arc<ZonePolicy>,
    /// Serializes rebuilds so a stale snapshot never replaces a newer one.
    publish: Arc<Mutex<()>>,
}

impl std::fmt::Debug for Zones {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Zones")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

fn apex(zone: &Zone) -> Result<Name, ZoneOpError> {
    Ok(absolute(&zone.name)?)
}

impl Zones {
    /// Wraps the database and serving catalog.
    pub fn new(db: Db, catalog: DynamicCatalog, policy: ZonePolicy) -> Self {
        Self {
            db,
            catalog,
            policy: Arc::new(policy),
            publish: Arc::new(Mutex::new(())),
        }
    }

    /// The server-wide zone rules.
    pub fn policy(&self) -> &ZonePolicy {
        &self.policy
    }

    /// Serves every stored zone; zones that fail to build are logged and skipped.
    pub async fn load_all(&self) -> Result<usize, ZoneOpError> {
        let mut loaded = 0;
        for zone in self.db.zones(None).await? {
            match self.publish(&zone.name).await {
                Ok(()) => loaded += 1,
                Err(e) => tracing::error!(zone = %zone.name, error = %e, "cannot serve zone"),
            }
        }
        Ok(loaded)
    }

    /// Rebuilds a zone from the database, or stops serving it if it is gone.
    pub async fn publish(&self, name: &str) -> Result<(), ZoneOpError> {
        let _guard = self.publish.lock().await;
        let zone = match self.db.zone(name).await {
            Ok(z) => z,
            Err(DbError::NotFound(_)) => {
                self.catalog.remove(&LowerName::new(&absolute(name)?));
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        let records = self.db.zone_records(zone.id).await?;
        let key = if zone.dnssec {
            self.db.zone_key(zone.id).await?
        } else {
            None
        };
        let tsig = self.db.tsig_keys(zone.id).await?;
        let policy = Arc::clone(&self.policy);
        // Parsing and signing are CPU-bound; keep them off the async workers.
        let handler = tokio::task::spawn_blocking(move || {
            let config = materialize(&policy, &zone, &records, key.as_ref(), &tsig)?;
            Ok::<_, ZoneOpError>(build_zone(config)?)
        })
        .await
        .map_err(|_| ZoneOpError::Task)??;
        self.catalog.upsert(handler);
        Ok(())
    }
}

fn materialize(
    policy: &ZonePolicy,
    zone: &Zone,
    records: &[ZoneRecord],
    key: Option<&ZoneKey>,
    tsig: &[TsigKey],
) -> Result<ZoneConfig, ZoneOpError> {
    let origin = apex(zone)?;
    let soa = SOA::new(
        policy.nsname.clone(),
        policy.nsadmin.clone(),
        zone.serial,
        SOA_REFRESH,
        SOA_RETRY,
        SOA_EXPIRE,
        SOA_MINIMUM,
    );
    let mut rrs = vec![Record::from_rdata(origin.clone(), SOA_TTL, RData::SOA(soa))];
    for rec in records {
        match ValidRecord::from_stored(rec) {
            Some(v) => rrs.push(Record::from_rdata(v.name, v.ttl, v.data)),
            None => {
                tracing::warn!(zone = %zone.name, id = rec.id, "skipping unparsable record");
            }
        }
    }
    let dnssec = match key {
        Some(key) => {
            let (signing, dnskey) = dnssec_key(key)?;
            Some(DnssecConfig {
                signer: DnssecSigner::new(dnskey, Box::new(signing), origin.clone(), SIG_DURATION),
                nx_proof: nx_proof(&zone.nsec_mode),
            })
        }
        None if zone.dnssec => {
            tracing::error!(zone = %zone.name, "DNSSEC enabled but no key stored; serving unsigned");
            None
        }
        None => None,
    };
    let axfr = AxfrAccess {
        tsig_keys: tsig.iter().filter_map(tsig_signer).collect(),
        allow_from: zone
            .axfr_allow_from
            .iter()
            .filter_map(|c| c.parse().ok())
            .collect(),
    };
    Ok(ZoneConfig {
        origin,
        records: rrs,
        dnssec,
        axfr,
    })
}

impl Zones {
    /// Server-side rules for a new zone name; uniqueness and overlap with other users'
    /// zones are enforced by the store.
    fn check_new_zone(&self, name: &Name) -> Result<(), ZoneOpError> {
        if self.policy.base_domain.zone_of(name) {
            return Err(ZoneOpError::NotAllowed("inside the server's own domain"));
        }
        if self.policy.reserved.iter().any(|r| name.zone_of(r)) {
            return Err(ZoneOpError::NotAllowed("covers static records"));
        }
        Ok(())
    }

    /// Creates a zone for `user_id` with an apex NS pointing at this server.
    pub async fn create(&self, user_id: i64, name: &str) -> Result<Zone, ZoneOpError> {
        let origin = parse_zone_name(name)?;
        self.check_new_zone(&origin)?;
        let ns = ValidRecord {
            name: origin.clone(),
            rtype: RecordType::NS,
            ttl: SOA_TTL,
            data: RData::NS(NS(self.policy.nsname.clone())),
        };
        let zone = self
            .db
            .create_zone(user_id, &stored_name(&origin), vec![ns.to_new_record()])
            .await?;
        self.publish(&zone.name).await?;
        Ok(zone)
    }

    /// Looks up a zone by name (relative forms and trailing dots accepted).
    pub async fn get(&self, name: &str) -> Result<Zone, ZoneOpError> {
        let name = absolute(name).map_err(|_| ZoneOpError::NotFound("zone"))?;
        Ok(self.db.zone(&stored_name(&name)).await?)
    }

    /// Deletes a zone and stops serving it.
    pub async fn delete(&self, zone: &Zone) -> Result<(), ZoneOpError> {
        self.db.delete_zone(zone.id).await?;
        self.publish(&zone.name).await
    }

    /// A zone's records.
    pub async fn records(&self, zone: &Zone) -> Result<Vec<ZoneRecord>, ZoneOpError> {
        Ok(self.db.zone_records(zone.id).await?)
    }

    fn checked(&self, zone: &Zone, input: RecordInput<'_>) -> Result<ValidRecord, ZoneOpError> {
        Ok(validate(&apex(zone)?, &self.policy.base_domain, input)?)
    }

    /// Validates and adds a record.
    pub async fn add_record(
        &self,
        zone: &Zone,
        input: RecordInput<'_>,
    ) -> Result<ZoneRecord, ZoneOpError> {
        let rec = self.checked(zone, input)?;
        let stored = self.db.add_record(zone.id, rec.to_new_record()).await?;
        self.publish(&zone.name).await?;
        Ok(stored)
    }

    /// Validates and replaces a record.
    pub async fn update_record(
        &self,
        zone: &Zone,
        id: i64,
        input: RecordInput<'_>,
    ) -> Result<ZoneRecord, ZoneOpError> {
        let rec = self.checked(zone, input)?;
        let stored = self
            .db
            .update_record(zone.id, id, rec.to_new_record())
            .await?;
        self.publish(&zone.name).await?;
        Ok(stored)
    }

    /// Deletes a record.
    pub async fn delete_record(&self, zone: &Zone, id: i64) -> Result<(), ZoneOpError> {
        self.db.delete_record(zone.id, id).await?;
        self.publish(&zone.name).await
    }

    /// DNSSEC state and the DS record to publish at the parent.
    pub async fn dnssec(&self, zone: &Zone) -> Result<DnssecInfo, ZoneOpError> {
        let zone = self.db.zone(&zone.name).await?;
        let mut info = DnssecInfo {
            enabled: zone.dnssec,
            nsec_mode: zone.nsec_mode.clone(),
            dnskey: None,
            ds: None,
        };
        if let (true, Some(key)) = (zone.dnssec, self.db.zone_key(zone.id).await?) {
            let (signing, dnskey) = dnssec_key(&key)?;
            let public = signing
                .to_public_key()
                .map_err(|e| ZoneOpError::Dnssec(e.to_string()))?;
            let ds = DS::from_key(&public, &apex(&zone)?, DigestType::SHA256)
                .map_err(|e| ZoneOpError::Dnssec(e.to_string()))?;
            info.dnskey = Some(dnskey.to_string());
            info.ds = Some(ds.to_string());
        }
        Ok(info)
    }

    /// Turns signing on (creating a key on first use) or off (discarding the key).
    pub async fn set_dnssec(
        &self,
        zone: &Zone,
        enabled: bool,
        nsec_mode: &str,
    ) -> Result<DnssecInfo, ZoneOpError> {
        if !NSEC_MODES.contains(&nsec_mode) {
            return Err(ZoneOpError::Invalid {
                code: "invalid_nsec_mode",
                message: format!("nsec_mode must be one of {NSEC_MODES:?}"),
            });
        }
        let new_key = if enabled {
            let pkcs8 = EcdsaSigningKey::generate_pkcs8(KEY_ALGORITHM)
                .map_err(|e| ZoneOpError::Dnssec(e.to_string()))?;
            Some((KEY_ALGORITHM.to_string(), pkcs8.secret_pkcs8_der().to_vec()))
        } else {
            None
        };
        self.db
            .set_dnssec(zone.id, enabled, nsec_mode, new_key)
            .await?;
        self.publish(&zone.name).await?;
        self.dnssec(zone).await
    }

    /// Re-signs every DNSSEC zone, bumping serials so secondaries pick up the signatures.
    pub async fn resign_all(&self) -> Result<usize, ZoneOpError> {
        let mut signed = 0;
        for zone in self.db.zones(None).await?.into_iter().filter(|z| z.dnssec) {
            self.db.touch_zone(zone.id).await?;
            match self.publish(&zone.name).await {
                Ok(()) => signed += 1,
                Err(e) => tracing::error!(zone = %zone.name, error = %e, "re-signing failed"),
            }
        }
        Ok(signed)
    }

    /// Transfer access of a zone.
    pub async fn transfer(&self, zone: &Zone) -> Result<TransferInfo, ZoneOpError> {
        let zone = self.db.zone(&zone.name).await?;
        let tsig_keys = self
            .db
            .tsig_keys(zone.id)
            .await?
            .into_iter()
            .map(|k| TsigKeyInfo {
                name: k.name,
                algorithm: k.algorithm,
                created_at: k.created_at,
            })
            .collect();
        Ok(TransferInfo {
            allow_from: zone.axfr_allow_from,
            tsig_keys,
        })
    }

    /// Replaces the networks (or single addresses) allowed to transfer without TSIG; an
    /// empty list denies all.
    pub async fn set_transfer_allow_from(
        &self,
        zone: &Zone,
        allow_from: &[String],
    ) -> Result<TransferInfo, ZoneOpError> {
        let mut nets = Vec::with_capacity(allow_from.len());
        for cidr in allow_from {
            let cidr = cidr.trim();
            let net = cidr
                .parse::<IpNet>()
                .or_else(|_| cidr.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| ZoneOpError::Invalid {
                    code: "invalid_cidr",
                    message: format!("{cidr:?} is not an IP address or CIDR network"),
                })?;
            nets.push(net.trunc().to_string());
        }
        self.db.set_axfr_allow_from(zone.id, nets).await?;
        self.publish(&zone.name).await?;
        self.transfer(zone).await
    }

    /// Creates a TSIG key with a random secret, returned only here.
    pub async fn add_tsig_key(
        &self,
        zone: &Zone,
        name: &str,
        algorithm: &str,
    ) -> Result<(TsigKeyInfo, Vec<u8>), ZoneOpError> {
        let name = absolute(name)?;
        let algorithm = algorithm.trim().to_lowercase();
        let (_, len) = TSIG_ALGORITHMS
            .iter()
            .find(|(a, _)| *a == algorithm)
            .ok_or_else(|| ZoneOpError::Invalid {
                code: "invalid_algorithm",
                message: format!(
                    "algorithm must be one of {:?}",
                    TSIG_ALGORITHMS.map(|(a, _)| a)
                ),
            })?;
        let secret = ids::random_bytes(*len)
            .ok_or_else(|| ZoneOpError::Dnssec("system random generator failed".into()))?;
        let name = stored_name(&name);
        self.db
            .add_tsig_key(zone.id, &name, &algorithm, secret.clone())
            .await?;
        self.publish(&zone.name).await?;
        let created = self
            .transfer(zone)
            .await?
            .tsig_keys
            .into_iter()
            .find(|k| k.name == name)
            .ok_or(ZoneOpError::NotFound("tsig key"))?;
        Ok((created, secret))
    }

    /// Deletes a TSIG key.
    pub async fn delete_tsig_key(&self, zone: &Zone, name: &str) -> Result<(), ZoneOpError> {
        let name = absolute(name).map_err(|_| ZoneOpError::NotFound("tsig key"))?;
        self.db
            .delete_tsig_key(zone.id, &stored_name(&name))
            .await?;
        self.publish(&zone.name).await
    }
}
