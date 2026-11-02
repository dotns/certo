//! Zone ownership, activation state and the zone data `PowerDNS` is rebuilt from. Every
//! change to what `PowerDNS` serves bumps the zone's `version` in the same transaction.
use certo_core::{Zone, ZoneDnssec, ZoneRrSet, ZoneTsigKey};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    Db, DbError,
    store::{is_unique_violation, normalize, now, parse_list, scope_json},
};

const ZONE_SELECT: &str = "SELECT z.id, z.user_id, u.username, z.name, z.active, z.exempt, \
     z.verified_at, z.last_check, z.last_error, z.failed_checks, z.version, z.axfr_allow_from, \
     z.created_at FROM zones z LEFT JOIN users u ON u.id = z.user_id";

fn zone_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Zone> {
    Ok(Zone {
        id: r.get(0)?,
        user_id: r.get(1)?,
        owner: r.get(2)?,
        name: r.get(3)?,
        active: r.get(4)?,
        exempt: r.get(5)?,
        verified_at: r.get(6)?,
        last_check: r.get(7)?,
        last_error: r.get(8)?,
        failed_checks: r.get(9)?,
        version: r.get(10)?,
        axfr_allow_from: parse_list(&r.get::<_, String>(11)?),
        created_at: r.get(12)?,
    })
}

/// Rejects a zone of another user that is an ancestor or descendant of `name`. The base
/// zone (no owner) is not considered: names under it are refused by the zone service and
/// an ancestor of it is only ever activated by whoever controls its delegation.
fn check_overlap(tx: &Transaction<'_>, user_id: Option<i64>, name: &str) -> Result<(), DbError> {
    let zones: Vec<(Option<i64>, String)> = tx
        .prepare("SELECT user_id, name FROM zones WHERE user_id IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let inside = |child: &str, parent: &str| child.ends_with(&format!(".{parent}"));
    if zones
        .iter()
        .any(|(uid, other)| *uid != user_id && (inside(name, other) || inside(other, name)))
    {
        return Err(DbError::Conflict("zone overlap"));
    }
    Ok(())
}

/// Removes the zone data rows of one zone.
fn delete_zone_data(tx: &Transaction<'_>, zone_id: i64) -> Result<(), DbError> {
    for table in ["zone_rrsets", "zone_dnssec", "zone_tsig_keys"] {
        tx.execute(&format!("DELETE FROM {table} WHERE zone_id=?1"), [zone_id])?;
    }
    Ok(())
}

/// Removes every zone row (and zone data) of a user; part of the user deletion transaction.
pub(crate) fn delete_user_zones(tx: &Transaction<'_>, user_id: i64) -> Result<(), DbError> {
    let ids: Vec<i64> = tx
        .prepare("SELECT id FROM zones WHERE user_id=?1")?
        .query_map([user_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        delete_zone_data(tx, id)?;
    }
    tx.execute("DELETE FROM zones WHERE user_id=?1", [user_id])?;
    Ok(())
}

/// Bumps the version of the server's own zone; challenge TXT values are part of it.
pub(crate) fn bump_base_zone(tx: &Transaction<'_>) -> Result<(), DbError> {
    tx.execute(
        "UPDATE zones SET version=version+1 WHERE user_id IS NULL",
        [],
    )?;
    Ok(())
}

/// Bumps a zone's version and returns the new one.
fn bump(tx: &Transaction<'_>, zone_id: i64) -> Result<i64, DbError> {
    if tx.execute("UPDATE zones SET version=version+1 WHERE id=?1", [zone_id])? == 0 {
        return Err(DbError::NotFound("zone"));
    }
    Ok(
        tx.query_row("SELECT version FROM zones WHERE id=?1", [zone_id], |r| {
            r.get(0)
        })?,
    )
}

fn zone_by_id(c: &Connection, id: i64) -> Result<Zone, DbError> {
    c.query_row(&format!("{ZONE_SELECT} WHERE z.id=?1"), [id], zone_row)
        .optional()?
        .ok_or(DbError::NotFound("zone"))
}

fn insert_rrsets(tx: &Transaction<'_>, zone_id: i64, rrsets: &[ZoneRrSet]) -> Result<(), DbError> {
    for rr in rrsets {
        tx.execute(
            "DELETE FROM zone_rrsets WHERE zone_id=?1 AND name=?2 AND rtype=?3",
            params![zone_id, rr.name, rr.rtype],
        )?;
        if !rr.records.is_empty() {
            tx.execute(
                "INSERT INTO zone_rrsets (zone_id, name, rtype, ttl, records) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![zone_id, rr.name, rr.rtype, rr.ttl, scope_json(&rr.records)],
            )?;
        }
    }
    Ok(())
}

impl Db {
    /// Creates a pending zone owned by `user_id` (`None` for the server's base zone).
    pub async fn create_zone(&self, user_id: Option<i64>, name: &str) -> Result<Zone, DbError> {
        let name = normalize(name);
        self.run(move |c| {
            let tx = c.transaction()?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM zones WHERE name=?1)",
                [&name],
                |r| r.get(0),
            )?;
            if exists {
                return Err(DbError::Conflict("zone"));
            }
            check_overlap(&tx, user_id, &name)?;
            tx.execute(
                "INSERT INTO zones (user_id, name, created_at) VALUES (?1, ?2, ?3)",
                params![user_id, name, now()],
            )?;
            let id = tx.last_insert_rowid();
            let zone = zone_by_id(&tx, id)?;
            tx.commit()?;
            Ok(zone)
        })
        .await
    }

    /// A zone by name.
    pub async fn zone(&self, name: &str) -> Result<Zone, DbError> {
        let name = normalize(name);
        self.run(move |c| {
            c.query_row(&format!("{ZONE_SELECT} WHERE z.name=?1"), [name], zone_row)
                .optional()?
                .ok_or(DbError::NotFound("zone"))
        })
        .await
    }

    /// Zones of one user, or every zone (including the base zone) for `None`.
    pub async fn zones(&self, user_id: Option<i64>) -> Result<Vec<Zone>, DbError> {
        self.run(move |c| {
            let rows = match user_id {
                Some(uid) => c
                    .prepare(&format!("{ZONE_SELECT} WHERE z.user_id=?1 ORDER BY z.name"))?
                    .query_map([uid], zone_row)?
                    .collect::<Result<Vec<_>, _>>()?,
                None => c
                    .prepare(&format!("{ZONE_SELECT} ORDER BY z.name"))?
                    .query_map([], zone_row)?
                    .collect::<Result<Vec<_>, _>>()?,
            };
            Ok(rows)
        })
        .await
    }

    /// Deletes a zone with its data.
    pub async fn delete_zone(&self, zone_id: i64) -> Result<(), DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            delete_zone_data(&tx, zone_id)?;
            if tx.execute("DELETE FROM zones WHERE id=?1", [zone_id])? == 0 {
                return Err(DbError::NotFound("zone"));
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Marks a zone active after a successful check (or an exemption); the version moves
    /// only when the zone was inactive.
    pub async fn activate_zone(&self, zone_id: i64, exempt: bool) -> Result<Zone, DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            let was_active = zone_by_id(&tx, zone_id)?.active;
            tx.execute(
                "UPDATE zones SET active=1, exempt=?2, verified_at=?3, last_check=?3, last_error=NULL,
                    failed_checks=0 WHERE id=?1",
                params![zone_id, exempt, now()],
            )?;
            if !was_active {
                bump(&tx, zone_id)?;
            }
            let zone = zone_by_id(&tx, zone_id)?;
            tx.commit()?;
            Ok(zone)
        })
        .await
    }

    /// Records a failed delegation check; returns the updated zone.
    pub async fn record_zone_check_failure(
        &self,
        zone_id: i64,
        error: &str,
    ) -> Result<Zone, DbError> {
        let error = error.to_owned();
        self.run(move |c| {
            let n = c.execute(
                "UPDATE zones SET last_check=?2, last_error=?3, failed_checks=failed_checks+1 WHERE id=?1",
                params![zone_id, now(), error],
            )?;
            if n == 0 {
                return Err(DbError::NotFound("zone"));
            }
            zone_by_id(c, zone_id)
        })
        .await
    }

    /// Marks a zone inactive; its data stays for a later re-activation.
    pub async fn deactivate_zone(&self, zone_id: i64) -> Result<Zone, DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            tx.execute("UPDATE zones SET active=0, exempt=0 WHERE id=?1", [zone_id])?;
            bump(&tx, zone_id)?;
            let zone = zone_by_id(&tx, zone_id)?;
            tx.commit()?;
            Ok(zone)
        })
        .await
    }

    /// Clears the exemption of an active zone so it is re-checked like any other.
    pub async fn clear_zone_exemption(&self, zone_id: i64) -> Result<Zone, DbError> {
        self.run(move |c| {
            let n = c.execute(
                "UPDATE zones SET exempt=0, failed_checks=0 WHERE id=?1",
                [zone_id],
            )?;
            if n == 0 {
                return Err(DbError::NotFound("zone"));
            }
            zone_by_id(c, zone_id)
        })
        .await
    }

    /// Sum of the versions of all active zones.
    pub async fn dns_version(&self) -> Result<i64, DbError> {
        self.run(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(version), 0) FROM zones WHERE active=1",
                [],
                |r| r.get(0),
            )?)
        })
        .await
    }

    // --- Zone data ---

    /// Stored rrsets of a zone, ordered by name and type.
    pub async fn zone_rrsets(&self, zone_id: i64) -> Result<Vec<ZoneRrSet>, DbError> {
        self.run(move |c| {
            let mut stmt = c.prepare(
                "SELECT name, rtype, ttl, records FROM zone_rrsets WHERE zone_id=?1 ORDER BY name, rtype",
            )?;
            let rows = stmt.query_map([zone_id], |r| {
                Ok(ZoneRrSet {
                    name: r.get(0)?,
                    rtype: r.get(1)?,
                    ttl: r.get(2)?,
                    records: parse_list(&r.get::<_, String>(3)?),
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// Replaces the given rrsets (an empty record list removes one); returns the new version.
    pub async fn apply_zone_rrsets(
        &self,
        zone_id: i64,
        rrsets: &[ZoneRrSet],
    ) -> Result<i64, DbError> {
        let rrsets = rrsets.to_vec();
        self.run(move |c| {
            let tx = c.transaction()?;
            let version = bump(&tx, zone_id)?;
            insert_rrsets(&tx, zone_id, &rrsets)?;
            tx.commit()?;
            Ok(version)
        })
        .await
    }

    /// Replaces all rrsets of a zone; returns the new version.
    pub async fn replace_zone_rrsets(
        &self,
        zone_id: i64,
        rrsets: &[ZoneRrSet],
    ) -> Result<i64, DbError> {
        let rrsets = rrsets.to_vec();
        self.run(move |c| {
            let tx = c.transaction()?;
            let version = bump(&tx, zone_id)?;
            tx.execute("DELETE FROM zone_rrsets WHERE zone_id=?1", [zone_id])?;
            insert_rrsets(&tx, zone_id, &rrsets)?;
            tx.commit()?;
            Ok(version)
        })
        .await
    }

    /// The stored signing key of a zone.
    pub async fn zone_dnssec(&self, zone_id: i64) -> Result<Option<ZoneDnssec>, DbError> {
        self.run(move |c| {
            Ok(c.query_row(
                "SELECT nsec3, algorithm, private_key FROM zone_dnssec WHERE zone_id=?1",
                [zone_id],
                |r| {
                    Ok(ZoneDnssec {
                        nsec3: r.get(0)?,
                        algorithm: r.get(1)?,
                        private_key: r.get(2)?,
                    })
                },
            )
            .optional()?)
        })
        .await
    }

    /// Stores (or, with `None`, removes) the signing key; returns the new version.
    pub async fn set_zone_dnssec(
        &self,
        zone_id: i64,
        key: Option<&ZoneDnssec>,
    ) -> Result<i64, DbError> {
        let key = key.cloned();
        self.run(move |c| {
            let tx = c.transaction()?;
            let version = bump(&tx, zone_id)?;
            tx.execute("DELETE FROM zone_dnssec WHERE zone_id=?1", [zone_id])?;
            if let Some(key) = key {
                tx.execute(
                    "INSERT INTO zone_dnssec (zone_id, nsec3, algorithm, private_key) VALUES (?1, ?2, ?3, ?4)",
                    params![zone_id, key.nsec3, key.algorithm, key.private_key],
                )?;
            }
            tx.commit()?;
            Ok(version)
        })
        .await
    }

    /// TSIG keys of a zone, ordered by name.
    pub async fn zone_tsig_keys(&self, zone_id: i64) -> Result<Vec<ZoneTsigKey>, DbError> {
        self.run(move |c| {
            let mut stmt = c.prepare(
                "SELECT name, algorithm, secret FROM zone_tsig_keys WHERE zone_id=?1 ORDER BY name",
            )?;
            let rows = stmt.query_map([zone_id], |r| {
                Ok(ZoneTsigKey {
                    name: r.get(0)?,
                    algorithm: r.get(1)?,
                    secret: r.get(2)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// Adds a TSIG key; returns the new version.
    pub async fn add_zone_tsig_key(&self, zone_id: i64, key: &ZoneTsigKey) -> Result<i64, DbError> {
        let key = key.clone();
        self.run(move |c| {
            let tx = c.transaction()?;
            let version = bump(&tx, zone_id)?;
            tx.execute(
                "INSERT INTO zone_tsig_keys (zone_id, name, algorithm, secret) VALUES (?1, ?2, ?3, ?4)",
                params![zone_id, key.name, key.algorithm, key.secret],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    DbError::Conflict("tsig key")
                } else {
                    e.into()
                }
            })?;
            tx.commit()?;
            Ok(version)
        })
        .await
    }

    /// Removes a TSIG key; returns the new version.
    pub async fn delete_zone_tsig_key(&self, zone_id: i64, name: &str) -> Result<i64, DbError> {
        let name = name.to_owned();
        self.run(move |c| {
            let tx = c.transaction()?;
            if tx.execute(
                "DELETE FROM zone_tsig_keys WHERE zone_id=?1 AND name=?2",
                params![zone_id, name],
            )? == 0
            {
                return Err(DbError::NotFound("tsig key"));
            }
            let version = bump(&tx, zone_id)?;
            tx.commit()?;
            Ok(version)
        })
        .await
    }

    /// Replaces the addresses allowed to transfer the zone; returns the new version.
    pub async fn set_zone_axfr_allow_from(
        &self,
        zone_id: i64,
        allow_from: &[String],
    ) -> Result<i64, DbError> {
        let json = scope_json(allow_from);
        self.run(move |c| {
            let tx = c.transaction()?;
            let version = bump(&tx, zone_id)?;
            tx.execute(
                "UPDATE zones SET axfr_allow_from=?2 WHERE id=?1",
                params![zone_id, json],
            )?;
            tx.commit()?;
            Ok(version)
        })
        .await
    }
}
