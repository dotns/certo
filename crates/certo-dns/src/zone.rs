use std::sync::Arc;

use hickory_server::{
    dnssec::NxProofKind,
    proto::{
        dnssec::{DnsSecError, DnsSecResult, DnssecSigner},
        op::ResponseCode,
        rr::rdata::tsig::TsigError,
        rr::{LowerName, Name, RData, Record, RecordType, TSigResponseContext, TSigner},
    },
    server::{Request, RequestInfo},
    store::in_memory::InMemoryZoneHandler,
    zone_handler::{
        AuthLookup, AxfrPolicy, DnssecZoneHandler, LookupControlFlow, LookupError, LookupOptions,
        Nsec3QueryInfo, UpdateRequest, ZoneHandler, ZoneTransfer, ZoneType,
    },
};
use ipnet::IpNet;
use tracing::warn;

/// Errors raised while building a zone.
#[derive(Debug, thiserror::Error)]
pub enum ZoneError {
    /// The record set has no SOA at the zone origin.
    #[error("zone {0} has no SOA record at its origin")]
    MissingSoa(String),
    /// A record could not be inserted (wrong class or outside the zone).
    #[error("record {name} {rtype} rejected by zone {origin}")]
    Rejected {
        /// Zone origin.
        origin: String,
        /// Record owner.
        name: String,
        /// Record type.
        rtype: RecordType,
    },
    /// DNSSEC signing failed.
    #[error("dnssec: {0}")]
    Dnssec(#[from] DnsSecError),
}

/// DNSSEC settings for a zone.
pub struct DnssecConfig {
    /// Zone signing key.
    pub signer: DnssecSigner,
    /// Denial-of-existence proof.
    pub nx_proof: NxProofKind,
}

/// Who may transfer a zone. Empty means AXFR is refused.
#[derive(Clone, Default)]
pub struct AxfrAccess {
    /// TSIG keys accepted for signed transfers.
    pub tsig_keys: Vec<TSigner>,
    /// Source networks allowed without TSIG.
    pub allow_from: Vec<IpNet>,
}

impl std::fmt::Debug for DnssecConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DnssecConfig")
            .field("signer", self.signer.signer_name())
            .field("nx_proof", &self.nx_proof)
            .finish()
    }
}

impl std::fmt::Debug for AxfrAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<&Name> = self.tsig_keys.iter().map(TSigner::signer_name).collect();
        f.debug_struct("AxfrAccess")
            .field("tsig_keys", &keys)
            .field("allow_from", &self.allow_from)
            .finish()
    }
}

/// Everything needed to materialize one authoritative zone.
#[derive(Debug)]
pub struct ZoneConfig {
    /// Zone apex.
    pub origin: Name,
    /// All records, including the apex SOA.
    pub records: Vec<Record>,
    /// Online signing, if enabled.
    pub dnssec: Option<DnssecConfig>,
    /// Zone transfer access.
    pub axfr: AxfrAccess,
}

/// Builds a ready-to-serve zone handler, signing it when DNSSEC is configured.
pub fn build_zone(config: ZoneConfig) -> Result<Arc<dyn ZoneHandler>, ZoneError> {
    let ZoneConfig {
        origin,
        records,
        dnssec,
        axfr,
    } = config;

    let serial = records
        .iter()
        .find(|r| r.record_type() == RecordType::SOA && r.name == origin)
        .and_then(|r| match &r.data {
            RData::SOA(soa) => Some(soa.serial),
            _ => None,
        })
        .ok_or_else(|| ZoneError::MissingSoa(origin.to_string()))?;

    let nx_proof = dnssec.as_ref().map(|d| d.nx_proof.clone());
    let mut zone = InMemoryZoneHandler::empty(
        origin.clone(),
        ZoneType::Primary,
        AxfrPolicy::AllowAll,
        nx_proof,
    );
    for record in records {
        let (name, rtype) = (record.name.clone(), record.record_type());
        if !LowerName::new(&origin).zone_of(&LowerName::new(&name))
            || !zone.upsert_mut(record, serial)
        {
            return Err(ZoneError::Rejected {
                origin: origin.to_string(),
                name: name.to_string(),
                rtype,
            });
        }
    }
    if let Some(dnssec) = dnssec {
        zone.add_zone_signing_key_mut(dnssec.signer)?;
        zone.secure_zone_mut()?;
    }

    Ok(Arc::new(CertoZone { inner: zone, axfr }))
}

/// In-memory zone with certo's AXFR access rules.
struct CertoZone {
    inner: InMemoryZoneHandler,
    axfr: AxfrAccess,
}

impl CertoZone {
    fn authorize_axfr(
        &self,
        request: &Request,
        now: u64,
    ) -> (Result<(), ResponseCode>, Option<TSigResponseContext>) {
        if let Some(tsig) = request.signature() {
            let req_id = request.id();
            let Some(signer) = self
                .axfr
                .tsig_keys
                .iter()
                .find(|s| s.signer_name() == &tsig.name)
            else {
                warn!(req_id, "AXFR with unknown TSIG key");
                return (
                    Err(ResponseCode::NotAuth),
                    Some(TSigResponseContext::unknown_key(
                        req_id,
                        now,
                        tsig.name.clone(),
                    )),
                );
            };
            let Ok((_, _, range)) = signer.verify_message_byte(request.as_slice(), None, true)
            else {
                warn!(req_id, "AXFR with bad TSIG signature");
                return (
                    Err(ResponseCode::NotAuth),
                    Some(TSigResponseContext::bad_signature(
                        req_id,
                        now,
                        signer.clone(),
                    )),
                );
            };
            let (result, error) = if range.contains(&now) {
                (Ok(()), None)
            } else {
                warn!(req_id, "AXFR with expired TSIG signature");
                (Err(ResponseCode::NotAuth), Some(TsigError::BadTime))
            };
            let context =
                TSigResponseContext::new(req_id, now, signer.clone(), tsig.data.mac.clone(), error);
            return (result, Some(context));
        }

        let ip = request.src().ip();
        if self.axfr.allow_from.iter().any(|net| net.contains(&ip)) {
            (Ok(()), None)
        } else {
            (Err(ResponseCode::Refused), None)
        }
    }
}

#[async_trait::async_trait]
impl ZoneHandler for CertoZone {
    fn zone_type(&self) -> ZoneType {
        ZoneType::Primary
    }

    fn axfr_policy(&self) -> AxfrPolicy {
        if self.axfr.tsig_keys.is_empty() && self.axfr.allow_from.is_empty() {
            AxfrPolicy::Deny
        } else {
            AxfrPolicy::AllowSigned
        }
    }

    fn can_validate_dnssec(&self) -> bool {
        self.inner.can_validate_dnssec()
    }

    fn origin(&self) -> &LowerName {
        self.inner.origin()
    }

    async fn lookup(
        &self,
        name: &LowerName,
        rtype: RecordType,
        request_info: Option<&RequestInfo<'_>>,
        lookup_options: LookupOptions,
    ) -> LookupControlFlow<AuthLookup> {
        self.inner
            .lookup(name, rtype, request_info, lookup_options)
            .await
    }

    async fn search(
        &self,
        request: &Request,
        lookup_options: LookupOptions,
    ) -> (LookupControlFlow<AuthLookup>, Option<TSigResponseContext>) {
        self.inner.search(request, lookup_options).await
    }

    async fn nsec_records(
        &self,
        name: &LowerName,
        lookup_options: LookupOptions,
    ) -> LookupControlFlow<AuthLookup> {
        self.inner.nsec_records(name, lookup_options).await
    }

    async fn nsec3_records(
        &self,
        info: Nsec3QueryInfo<'_>,
        lookup_options: LookupOptions,
    ) -> LookupControlFlow<AuthLookup> {
        self.inner.nsec3_records(info, lookup_options).await
    }

    async fn zone_transfer(
        &self,
        request: &Request,
        lookup_options: LookupOptions,
        now: u64,
    ) -> Option<(
        Result<ZoneTransfer, LookupError>,
        Option<TSigResponseContext>,
    )> {
        let (auth, context) = self.authorize_axfr(request, now);
        if let Err(code) = auth {
            return Some((Err(LookupError::ResponseCode(code)), context));
        }
        let (transfer, _) = self
            .inner
            .zone_transfer(request, lookup_options, now)
            .await?;
        Some((transfer, context))
    }

    fn nx_proof_kind(&self) -> Option<&NxProofKind> {
        self.inner.nx_proof_kind()
    }
}

#[async_trait::async_trait]
impl DnssecZoneHandler for CertoZone {
    async fn add_zone_signing_key(&self, signer: DnssecSigner) -> DnsSecResult<()> {
        self.inner.add_zone_signing_key(signer).await
    }

    async fn secure_zone(&self) -> DnsSecResult<()> {
        self.inner.secure_zone().await
    }
}
