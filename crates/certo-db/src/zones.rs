//! Zone ownership and activation state; the records themselves live in `PowerDNS`.
use certo_core::Zone;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    Db, DbError,
    store::{normalize, now, parse_list},
};

const ZONE_SELECT: &str = "SELECT z.id, z.user_id, u.username, z.name, z.active, z.exempt, \
     z.verified_at, z.last_check, z.last_error, z.failed_checks, z.created_at \
     FROM zones z LEFT JOIN users u ON u.id = z.user_id";

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
        created_at: r.get(10)?,
    })
}

/// Rejects a zone of another owner that is an ancestor or descendant of `name`.
fn check_overlap(tx: &Transaction<'_>, user_id: Option<i64>, name: &str) -> Result<(), DbError> {
    let zones: Vec<(Option<i64>, String)> = tx
        .prepare("SELECT user_id, name FROM zones")?
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

/// Removes every zone row of a user; part of the user deletion transaction.
pub(crate) fn delete_user_zones(tx: &Transaction<'_>, user_id: i64) -> Result<(), DbError> {
    tx.execute("DELETE FROM zones WHERE user_id=?1", [user_id])?;
    Ok(())
}

fn zone_by_id(c: &Connection, id: i64) -> Result<Zone, DbError> {
    c.query_row(&format!("{ZONE_SELECT} WHERE z.id=?1"), [id], zone_row)
        .optional()?
        .ok_or(DbError::NotFound("zone"))
}

/// A version 3 zone awaiting its import into `PowerDNS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyZone {
    /// Owning user.
    pub user_id: i64,
    /// Zone apex, lowercase without trailing dot.
    pub name: String,
    /// DNSSEC was enabled.
    pub dnssec: bool,
    /// NSEC3 was selected.
    pub nsec3: bool,
    /// CIDRs allowed to transfer the zone.
    pub axfr_allow_from: Vec<String>,
    /// Records in presentation format: `(name, type, ttl, rdata)`.
    pub records: Vec<(String, String, u32, String)>,
    /// TSIG keys: `(name, algorithm, secret)`.
    pub tsig_keys: Vec<(String, String, Vec<u8>)>,
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

    /// Deletes a zone row.
    pub async fn delete_zone(&self, zone_id: i64) -> Result<(), DbError> {
        self.run(move |c| {
            if c.execute("DELETE FROM zones WHERE id=?1", [zone_id])? == 0 {
                return Err(DbError::NotFound("zone"));
            }
            Ok(())
        })
        .await
    }

    /// Marks a zone active after a successful check (or an exemption) and clears any snapshot.
    pub async fn activate_zone(&self, zone_id: i64, exempt: bool) -> Result<Zone, DbError> {
        self.run(move |c| {
            let n = c.execute(
                "UPDATE zones SET active=1, exempt=?2, verified_at=?3, last_check=?3, last_error=NULL,
                    failed_checks=0, snapshot=NULL WHERE id=?1",
                params![zone_id, exempt, now()],
            )?;
            if n == 0 {
                return Err(DbError::NotFound("zone"));
            }
            zone_by_id(c, zone_id)
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

    /// Marks a zone inactive, keeping a JSON snapshot of its records for re-activation.
    pub async fn deactivate_zone(&self, zone_id: i64, snapshot: &str) -> Result<Zone, DbError> {
        let snapshot = snapshot.to_owned();
        self.run(move |c| {
            let n = c.execute(
                "UPDATE zones SET active=0, exempt=0, snapshot=?2 WHERE id=?1",
                params![zone_id, snapshot],
            )?;
            if n == 0 {
                return Err(DbError::NotFound("zone"));
            }
            zone_by_id(c, zone_id)
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

    /// The record snapshot stored when a zone was deactivated.
    pub async fn zone_snapshot(&self, zone_id: i64) -> Result<Option<String>, DbError> {
        self.run(move |c| {
            Ok(
                c.query_row("SELECT snapshot FROM zones WHERE id=?1", [zone_id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten(),
            )
        })
        .await
    }

    /// Zones of a version 3 database that still have to be imported into `PowerDNS`;
    /// empty once the schema is at version 4.
    pub async fn legacy_zones(&self) -> Result<Vec<LegacyZone>, DbError> {
        self.run(|c| {
            if crate::schema::version(c)? != 3 {
                return Ok(Vec::new());
            }
            let zones: Vec<(i64, i64, String, bool, String, String)> = c
                .prepare(
                    "SELECT id, user_id, name, dnssec_enabled, nsec_mode, axfr_allow_from FROM zones ORDER BY name",
                )?
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                })?
                .collect::<Result<_, _>>()?;
            let mut out = Vec::with_capacity(zones.len());
            for (id, user_id, name, dnssec, nsec_mode, allow_from) in zones {
                let records = c
                    .prepare("SELECT name, rtype, ttl, rdata FROM records WHERE zone_id=?1 ORDER BY id")?
                    .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                    .collect::<Result<_, _>>()?;
                let tsig_keys = c
                    .prepare("SELECT name, algorithm, secret FROM tsig_keys WHERE zone_id=?1 ORDER BY id")?
                    .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<Result<_, _>>()?;
                out.push(LegacyZone {
                    user_id,
                    name,
                    dnssec,
                    nsec3: nsec_mode == "nsec3",
                    axfr_allow_from: parse_list(&allow_from),
                    records,
                    tsig_keys,
                });
            }
            Ok(out)
        })
        .await
    }

    /// Finishes the version 3 → 4 upgrade once every legacy zone exists in `PowerDNS`.
    pub async fn complete_import(&self) -> Result<(), DbError> {
        self.run(|c| {
            if crate::schema::version(c)? == 3 {
                crate::schema::upgrade_3_to_4(c)?;
            }
            Ok(())
        })
        .await
    }
}
