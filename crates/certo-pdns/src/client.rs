//! Typed subset of the pdns HTTP API (`/api/v1/servers/localhost`).

use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::PdnsError;

/// Operation of an rrset in a `PATCH` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChangeType {
    /// Replace the whole rrset (or create it).
    Replace,
    /// Remove the whole rrset.
    Delete,
}

/// One record inside an rrset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordData {
    /// Presentation-format rdata.
    pub content: String,
    /// Disabled records are stored but not served.
    #[serde(default)]
    pub disabled: bool,
}

impl RecordData {
    /// An enabled record.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            disabled: false,
        }
    }
}

/// All records of one name and type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RrSet {
    /// Owner name with trailing dot.
    pub name: String,
    /// Record type, e.g. `TXT`.
    #[serde(rename = "type")]
    pub rtype: String,
    /// TTL in seconds.
    #[serde(default)]
    pub ttl: u32,
    /// Only used in `PATCH` requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changetype: Option<ChangeType>,
    /// The records; empty for `Delete`.
    #[serde(default)]
    pub records: Vec<RecordData>,
}

impl RrSet {
    /// A `REPLACE` change.
    pub fn replace(
        name: impl Into<String>,
        rtype: impl Into<String>,
        ttl: u32,
        records: Vec<RecordData>,
    ) -> Self {
        Self {
            name: name.into(),
            rtype: rtype.into(),
            ttl,
            changetype: Some(ChangeType::Replace),
            records,
        }
    }

    /// A `DELETE` change.
    pub fn delete(name: impl Into<String>, rtype: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            rtype: rtype.into(),
            ttl: 0,
            changetype: Some(ChangeType::Delete),
            records: Vec::new(),
        }
    }
}

/// A zone as listed by `GET /zones`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ZoneSummary {
    /// Zone name with trailing dot.
    pub name: String,
    /// `Native`, `Master`, ...
    pub kind: String,
    /// Whether the zone has active keys.
    #[serde(default)]
    pub dnssec: bool,
    /// Current SOA serial.
    #[serde(default)]
    pub serial: u32,
}

/// A zone with its rrsets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Zone {
    /// Zone name with trailing dot.
    pub name: String,
    /// `Native`, `Master`, ...
    pub kind: String,
    /// Whether the zone has active keys.
    #[serde(default)]
    pub dnssec: bool,
    /// Current SOA serial.
    #[serde(default)]
    pub serial: u32,
    /// NSEC3 parameters, empty for NSEC.
    #[serde(default)]
    pub nsec3param: String,
    /// All rrsets.
    #[serde(default)]
    pub rrsets: Vec<RrSet>,
}

/// Body of `POST /zones`.
#[derive(Debug, Clone, Serialize)]
pub struct NewZone {
    /// Zone name with trailing dot.
    pub name: String,
    /// Always `Native` for certo.
    pub kind: &'static str,
    /// Apex NS names; pdns creates the NS rrset.
    pub nameservers: Vec<String>,
    /// `DEFAULT`: pdns bumps the serial on every API change.
    pub soa_edit_api: &'static str,
    /// Initial rrsets.
    pub rrsets: Vec<RrSet>,
}

impl NewZone {
    /// A native zone with the given apex nameservers.
    pub fn native(name: impl Into<String>, nameservers: Vec<String>, rrsets: Vec<RrSet>) -> Self {
        Self {
            name: name.into(),
            kind: "Native",
            nameservers,
            soa_edit_api: "DEFAULT",
            rrsets,
        }
    }
}

/// A DNSSEC key of a zone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CryptoKey {
    /// Key id within the zone.
    pub id: u32,
    /// `csk`, `ksk` or `zsk`.
    pub keytype: String,
    /// Used for signing.
    pub active: bool,
    /// Published in the DNSKEY rrset.
    #[serde(default = "default_true")]
    pub published: bool,
    /// Algorithm mnemonic, e.g. `ECDSAP256SHA256`.
    pub algorithm: String,
    /// DNSKEY rdata.
    #[serde(default)]
    pub dnskey: String,
    /// DS rdata, one per digest type.
    #[serde(default)]
    pub ds: Vec<String>,
}

const fn default_true() -> bool {
    true
}

/// A zone metadata item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// Kind, e.g. `ALLOW-AXFR-FROM`.
    pub kind: String,
    /// Values.
    pub metadata: Vec<String>,
}

/// A TSIG key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TsigKey {
    /// Server-side id (name, URL-safe), absent when creating.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Key name.
    pub name: String,
    /// `hmac-sha256` etc.
    pub algorithm: String,
    /// Base64 secret; omitted when creating lets pdns generate one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    error: String,
}

/// Client for one pdns server.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    api_key: String,
}

impl Client {
    /// `base` is the server root, e.g. `http://127.0.0.1:8081`.
    pub fn new(base: &str, api_key: &str) -> Result<Self, PdnsError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            base: format!("{}/api/v1/servers/localhost", base.trim_end_matches('/')),
            api_key: api_key.to_owned(),
        })
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .header("X-API-Key", &self.api_key)
    }

    async fn check(res: Response) -> Result<Response, PdnsError> {
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        let message = match res.json::<ApiErrorBody>().await {
            Ok(body) if !body.error.is_empty() => body.error,
            _ => status
                .canonical_reason()
                .unwrap_or("request failed")
                .to_owned(),
        };
        Err(PdnsError::Api {
            status: status.as_u16(),
            message,
        })
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, PdnsError> {
        let res = Self::check(self.req(reqwest::Method::GET, path).send().await?).await?;
        Ok(res.json().await?)
    }

    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &(impl Serialize + Sync),
    ) -> Result<T, PdnsError> {
        let res = Self::check(self.req(method, path).json(body).send().await?).await?;
        Ok(res.json().await?)
    }

    async fn send_empty(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &(impl Serialize + Sync),
    ) -> Result<(), PdnsError> {
        Self::check(self.req(method, path).json(body).send().await?).await?;
        Ok(())
    }

    /// Whether the server answers; used for readiness.
    pub async fn ping(&self) -> bool {
        matches!(
            self.req(reqwest::Method::GET, "").send().await,
            Ok(res) if res.status() == StatusCode::OK
        )
    }

    /// Server version string.
    pub async fn version(&self) -> Result<String, PdnsError> {
        #[derive(Deserialize)]
        struct Server {
            version: String,
        }
        Ok(self.get_json::<Server>("").await?.version)
    }

    /// All zones.
    pub async fn zones(&self) -> Result<Vec<ZoneSummary>, PdnsError> {
        self.get_json("/zones").await
    }

    /// One zone with its rrsets, `None` when unknown.
    pub async fn zone(&self, name: &str) -> Result<Option<Zone>, PdnsError> {
        match self.get_json(&format!("/zones/{}", zone_id(name))).await {
            Ok(z) => Ok(Some(z)),
            Err(e) if e.is_not_found() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Creates a zone.
    pub async fn create_zone(&self, zone: &NewZone) -> Result<Zone, PdnsError> {
        self.send_json(reqwest::Method::POST, "/zones", zone).await
    }

    /// Deletes a zone; unknown zones are not an error.
    pub async fn delete_zone(&self, name: &str) -> Result<(), PdnsError> {
        match Self::check(
            self.req(
                reqwest::Method::DELETE,
                &format!("/zones/{}", zone_id(name)),
            )
            .send()
            .await?,
        )
        .await
        {
            Ok(_) => Ok(()),
            Err(e) if e.is_not_found() => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Drops cached answers for a zone; a zone created after a negative lookup is otherwise
    /// refused until the packet cache expires.
    pub async fn flush_cache(&self, zone: &str) -> Result<(), PdnsError> {
        Self::check(
            self.req(
                reqwest::Method::PUT,
                &format!("/cache/flush?domain={}", zone_id(zone)),
            )
            .send()
            .await?,
        )
        .await?;
        Ok(())
    }

    /// Applies rrset changes to a zone.
    pub async fn patch_rrsets(&self, zone: &str, rrsets: &[RrSet]) -> Result<(), PdnsError> {
        #[derive(Serialize)]
        struct Body<'a> {
            rrsets: &'a [RrSet],
        }
        self.send_empty(
            reqwest::Method::PATCH,
            &format!("/zones/{}", zone_id(zone)),
            &Body { rrsets },
        )
        .await
    }

    /// Sets (`Some("1 0 0 -")`) or clears NSEC3 parameters of a zone.
    pub async fn set_nsec3param(&self, zone: &str, param: Option<&str>) -> Result<(), PdnsError> {
        self.send_empty(
            reqwest::Method::PUT,
            &format!("/zones/{}", zone_id(zone)),
            &serde_json::json!({ "nsec3param": param.unwrap_or(""), "nsec3narrow": false, "api_rectify": true }),
        )
        .await
    }

    /// DNSSEC keys of a zone.
    pub async fn cryptokeys(&self, zone: &str) -> Result<Vec<CryptoKey>, PdnsError> {
        self.get_json(&format!("/zones/{}/cryptokeys", zone_id(zone)))
            .await
    }

    /// Adds an active, published CSK (`algorithm` is a pdns mnemonic such as `ecdsa256`).
    pub async fn add_csk(&self, zone: &str, algorithm: &str) -> Result<CryptoKey, PdnsError> {
        self.send_json(
            reqwest::Method::POST,
            &format!("/zones/{}/cryptokeys", zone_id(zone)),
            &serde_json::json!({ "keytype": "csk", "active": true, "published": true, "algorithm": algorithm }),
        )
        .await
    }

    /// Removes a DNSSEC key.
    pub async fn delete_cryptokey(&self, zone: &str, id: u32) -> Result<(), PdnsError> {
        Self::check(
            self.req(
                reqwest::Method::DELETE,
                &format!("/zones/{}/cryptokeys/{id}", zone_id(zone)),
            )
            .send()
            .await?,
        )
        .await?;
        Ok(())
    }

    /// Values of one metadata kind (empty when unset).
    pub async fn metadata(&self, zone: &str, kind: &str) -> Result<Vec<String>, PdnsError> {
        match self
            .get_json::<Metadata>(&format!("/zones/{}/metadata/{kind}", zone_id(zone)))
            .await
        {
            Ok(m) => Ok(m.metadata),
            Err(e) if e.is_not_found() => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Sets (or clears, with an empty list) one metadata kind.
    pub async fn set_metadata(
        &self,
        zone: &str,
        kind: &str,
        values: Vec<String>,
    ) -> Result<(), PdnsError> {
        let path = format!("/zones/{}/metadata/{kind}", zone_id(zone));
        if values.is_empty() {
            return match Self::check(self.req(reqwest::Method::DELETE, &path).send().await?).await {
                Ok(_) => Ok(()),
                Err(e) if e.is_not_found() => Ok(()),
                Err(e) => Err(e),
            };
        }
        let body = Metadata {
            kind: kind.to_owned(),
            metadata: values,
        };
        Self::check(
            self.req(reqwest::Method::PUT, &path)
                .json(&body)
                .send()
                .await?,
        )
        .await?;
        Ok(())
    }

    /// All TSIG keys.
    pub async fn tsig_keys(&self) -> Result<Vec<TsigKey>, PdnsError> {
        self.get_json("/tsigkeys").await
    }

    /// Creates a TSIG key; an empty `key` lets pdns generate the secret.
    pub async fn create_tsig_key(&self, key: &TsigKey) -> Result<TsigKey, PdnsError> {
        self.send_json(reqwest::Method::POST, "/tsigkeys", key)
            .await
    }

    /// Deletes a TSIG key by its server id.
    pub async fn delete_tsig_key(&self, id: &str) -> Result<(), PdnsError> {
        Self::check(
            self.req(reqwest::Method::DELETE, &format!("/tsigkeys/{id}"))
                .send()
                .await?,
        )
        .await?;
        Ok(())
    }
}

/// pdns zone ids are the canonical name with the trailing dot.
fn zone_id(name: &str) -> String {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    format!("{name}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrset_json_matches_the_pdns_shape() {
        let rr = RrSet::replace("a.example.com.", "TXT", 1, vec![RecordData::new("\"x\"")]);
        let json = serde_json::to_value(&rr).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "name": "a.example.com.", "type": "TXT", "ttl": 1, "changetype": "REPLACE",
                "records": [{ "content": "\"x\"", "disabled": false }]
            })
        );
        let del = serde_json::to_value(RrSet::delete("a.example.com.", "TXT")).unwrap();
        assert_eq!(del["changetype"], "DELETE");
        assert_eq!(del["records"], serde_json::json!([]));
    }

    #[test]
    fn zone_ids_are_canonical() {
        assert_eq!(zone_id("Example.COM"), "example.com.");
        assert_eq!(zone_id("example.com."), "example.com.");
    }
}
