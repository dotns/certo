//! Hosted zones and their records.
use certo_core::{TsigKey, Zone, ZoneKey, ZoneRecord};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    Db, DbError,
    store::{is_unique_violation, normalize, now, parse_list, scope_json},
};

/// A record to insert or replace; fields must already be validated and canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecord {
    /// Owner name, lowercase without trailing dot.
    pub name: String,
    /// Type mnemonic.
    pub rtype: String,
    /// TTL in seconds.
    pub ttl: u32,
    /// Canonical RDATA.
    pub data: String,
}

const ZONE_SELECT: &str = "SELECT z.id, z.user_id, u.username, z.name, z.serial, z.dnssec_enabled, \
     z.nsec_mode, z.axfr_allow_from, z.created_at FROM zones z LEFT JOIN users u ON u.id = z.user_id";

fn zone_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Zone> {
    Ok(Zone {
        id: r.get(0)?,
        user_id: r.get(1)?,
        owner: r.get(2)?,
        name: r.get(3)?,
        serial: r.get(4)?,
        dnssec: r.get(5)?,
        nsec_mode: r.get(6)?,
        axfr_allow_from: parse_list(&r.get::<_, String>(7)?),
        created_at: r.get(8)?,
    })
}

fn record_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ZoneRecord> {
    Ok(ZoneRecord {
        id: r.get(0)?,
        name: r.get(1)?,
        rtype: r.get(2)?,
        ttl: r.get(3)?,
        data: r.get(4)?,
    })
}

/// Unix time as a serial, kept strictly increasing.
fn next_serial(old: u32) -> u32 {
    let now = u32::try_from(now()).unwrap_or(u32::MAX);
    old.wrapping_add(1).max(now)
}

fn bump_serial(tx: &Transaction<'_>, zone_id: i64) -> Result<(), DbError> {
    let serial: u32 = tx
        .query_row("SELECT serial FROM zones WHERE id=?1", [zone_id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(DbError::NotFound("zone"))?;
    tx.execute(
        "UPDATE zones SET serial=?1 WHERE id=?2",
        params![next_serial(serial), zone_id],
    )?;
    Ok(())
}

/// Rejects a zone of another user that is an ancestor or descendant of `name`.
fn check_overlap(tx: &Transaction<'_>, user_id: i64, name: &str) -> Result<(), DbError> {
    let zones: Vec<(i64, String)> = tx
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

/// Rejects `rec` when a CNAME would share its name with any other record
/// (`excluding` is the id of the record being replaced).
fn check_cname(
    tx: &Transaction<'_>,
    zone_id: i64,
    rec: &NewRecord,
    excluding: i64,
) -> Result<(), DbError> {
    let clash: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM records WHERE zone_id=?1 AND name=?2 AND id<>?3
            AND (?4='CNAME' OR rtype='CNAME'))",
        params![zone_id, rec.name, excluding, rec.rtype],
        |r| r.get(0),
    )?;
    if clash {
        return Err(DbError::Conflict("cname"));
    }
    Ok(())
}

fn insert_record(tx: &Transaction<'_>, zone_id: i64, rec: &NewRecord) -> Result<i64, DbError> {
    tx.execute(
        "INSERT INTO records (zone_id, name, rtype, ttl, rdata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![zone_id, rec.name, rec.rtype, rec.ttl, rec.data, now()],
    )?;
    Ok(tx.last_insert_rowid())
}

fn delete_zone_rows(tx: &Transaction<'_>, zone_id: i64) -> Result<(), DbError> {
    for table in ["records", "zone_keys", "tsig_keys"] {
        tx.execute(&format!("DELETE FROM {table} WHERE zone_id=?1"), [zone_id])?;
    }
    tx.execute("DELETE FROM zones WHERE id=?1", [zone_id])?;
    Ok(())
}

/// Removes every zone of a user; part of the user deletion transaction.
pub(crate) fn delete_user_zones(tx: &Transaction<'_>, user_id: i64) -> Result<(), DbError> {
    let ids: Vec<i64> = tx
        .prepare("SELECT id FROM zones WHERE user_id=?1")?
        .query_map([user_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        delete_zone_rows(tx, id)?;
    }
    Ok(())
}

fn zone_by_name(c: &Connection, name: &str) -> Result<Zone, DbError> {
    c.query_row(&format!("{ZONE_SELECT} WHERE z.name=?1"), [name], zone_row)
        .optional()?
        .ok_or(DbError::NotFound("zone"))
}

fn record_by_id(c: &Connection, zone_id: i64, id: i64) -> Result<ZoneRecord, DbError> {
    c.query_row(
        "SELECT id, name, rtype, ttl, rdata FROM records WHERE zone_id=?1 AND id=?2",
        [zone_id, id],
        record_row,
    )
    .optional()?
    .ok_or(DbError::NotFound("record"))
}

impl Db {
    /// Creates a zone with its initial records; the name may not overlap another
    /// user's zone.
    pub async fn create_zone(
        &self,
        user_id: i64,
        name: &str,
        records: Vec<NewRecord>,
    ) -> Result<Zone, DbError> {
        let name = normalize(name);
        self.run(move |c| {
            let tx = c.transaction()?;
            check_overlap(&tx, user_id, &name)?;
            match tx.execute(
                "INSERT INTO zones (user_id, name, serial, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![user_id, name, next_serial(0), now()],
            ) {
                Ok(_) => {}
                Err(e) if is_unique_violation(&e) => return Err(DbError::Conflict("zone")),
                Err(e) => return Err(e.into()),
            }
            let zone_id = tx.last_insert_rowid();
            for rec in &records {
                insert_record(&tx, zone_id, rec)?;
            }
            let zone = zone_by_name(&tx, &name)?;
            tx.commit()?;
            Ok(zone)
        })
        .await
    }

    /// Looks up a zone by apex name.
    pub async fn zone(&self, name: &str) -> Result<Zone, DbError> {
        let name = normalize(name);
        self.run(move |c| zone_by_name(c, &name)).await
    }

    /// Zones ordered by name, all of them or one user's.
    pub async fn zones(&self, user_id: Option<i64>) -> Result<Vec<Zone>, DbError> {
        self.run(move |c| {
            let rows = match user_id {
                Some(uid) => c
                    .prepare(&format!("{ZONE_SELECT} WHERE z.user_id=?1 ORDER BY z.name"))?
                    .query_map([uid], zone_row)?
                    .collect::<Result<_, _>>()?,
                None => c
                    .prepare(&format!("{ZONE_SELECT} ORDER BY z.name"))?
                    .query_map([], zone_row)?
                    .collect::<Result<_, _>>()?,
            };
            Ok(rows)
        })
        .await
    }

    /// Deletes a zone with its records and keys.
    pub async fn delete_zone(&self, zone_id: i64) -> Result<(), DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            delete_zone_rows(&tx, zone_id)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// A zone's records ordered by name, type and id.
    pub async fn zone_records(&self, zone_id: i64) -> Result<Vec<ZoneRecord>, DbError> {
        self.run(move |c| {
            let rows = c
                .prepare(
                    "SELECT id, name, rtype, ttl, rdata FROM records WHERE zone_id=?1 ORDER BY name, rtype, id",
                )?
                .query_map([zone_id], record_row)?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// Adds a record and bumps the zone serial; CNAMEs are exclusive at their name.
    pub async fn add_record(&self, zone_id: i64, rec: NewRecord) -> Result<ZoneRecord, DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            check_cname(&tx, zone_id, &rec, 0)?;
            bump_serial(&tx, zone_id)?;
            let id = insert_record(&tx, zone_id, &rec)?;
            let rec = record_by_id(&tx, zone_id, id)?;
            tx.commit()?;
            Ok(rec)
        })
        .await
    }

    /// Replaces a record and bumps the zone serial; CNAMEs are exclusive at their name.
    pub async fn update_record(
        &self,
        zone_id: i64,
        id: i64,
        rec: NewRecord,
    ) -> Result<ZoneRecord, DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            check_cname(&tx, zone_id, &rec, id)?;
            let changed = tx.execute(
                "UPDATE records SET name=?1, rtype=?2, ttl=?3, rdata=?4 WHERE zone_id=?5 AND id=?6",
                params![rec.name, rec.rtype, rec.ttl, rec.data, zone_id, id],
            )?;
            if changed == 0 {
                return Err(DbError::NotFound("record"));
            }
            bump_serial(&tx, zone_id)?;
            let rec = record_by_id(&tx, zone_id, id)?;
            tx.commit()?;
            Ok(rec)
        })
        .await
    }

    /// Deletes a record and bumps the zone serial.
    pub async fn delete_record(&self, zone_id: i64, id: i64) -> Result<(), DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            let changed = tx.execute(
                "DELETE FROM records WHERE zone_id=?1 AND id=?2",
                [zone_id, id],
            )?;
            if changed == 0 {
                return Err(DbError::NotFound("record"));
            }
            bump_serial(&tx, zone_id)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

impl Db {
    /// Bumps a zone's serial without other changes (e.g. after re-signing).
    pub async fn touch_zone(&self, zone_id: i64) -> Result<(), DbError> {
        self.run(move |c| {
            let tx = c.transaction()?;
            bump_serial(&tx, zone_id)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Turns signing on (storing `new_key` if the zone has none) or off (deleting the
    /// key), and bumps the serial.
    pub async fn set_dnssec(
        &self,
        zone_id: i64,
        enabled: bool,
        nsec_mode: &str,
        new_key: Option<(String, Vec<u8>)>,
    ) -> Result<(), DbError> {
        let nsec_mode = nsec_mode.to_owned();
        self.run(move |c| {
            let tx = c.transaction()?;
            bump_serial(&tx, zone_id)?;
            tx.execute(
                "UPDATE zones SET dnssec_enabled=?1, nsec_mode=?2 WHERE id=?3",
                params![enabled, nsec_mode, zone_id],
            )?;
            if enabled {
                let has_key = tx
                    .query_row("SELECT 1 FROM zone_keys WHERE zone_id=?1", [zone_id], |_| {
                        Ok(())
                    })
                    .optional()?
                    .is_some();
                if let (false, Some((algorithm, pkcs8))) = (has_key, new_key) {
                    tx.execute(
                        "INSERT INTO zone_keys (zone_id, algorithm, private_key, created_at) VALUES (?1, ?2, ?3, ?4)",
                        params![zone_id, algorithm, pkcs8, now()],
                    )?;
                }
            } else {
                tx.execute("DELETE FROM zone_keys WHERE zone_id=?1", [zone_id])?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// The zone's signing key, if any.
    pub async fn zone_key(&self, zone_id: i64) -> Result<Option<ZoneKey>, DbError> {
        self.run(move |c| {
            Ok(c.query_row(
                "SELECT algorithm, private_key FROM zone_keys WHERE zone_id=?1 ORDER BY id LIMIT 1",
                [zone_id],
                |r| {
                    Ok(ZoneKey {
                        algorithm: r.get(0)?,
                        private_key: r.get::<_, Vec<u8>>(1)?.into(),
                    })
                },
            )
            .optional()?)
        })
        .await
    }

    /// Replaces the networks allowed to transfer the zone without TSIG.
    pub async fn set_axfr_allow_from(
        &self,
        zone_id: i64,
        allow_from: Vec<String>,
    ) -> Result<(), DbError> {
        self.run(move |c| {
            let changed = c.execute(
                "UPDATE zones SET axfr_allow_from=?1 WHERE id=?2",
                params![scope_json(&allow_from), zone_id],
            )?;
            if changed == 0 {
                return Err(DbError::NotFound("zone"));
            }
            Ok(())
        })
        .await
    }

    /// The zone's TSIG keys ordered by name.
    pub async fn tsig_keys(&self, zone_id: i64) -> Result<Vec<TsigKey>, DbError> {
        self.run(move |c| {
            let rows = c
                .prepare(
                    "SELECT name, algorithm, secret, created_at FROM tsig_keys WHERE zone_id=?1 ORDER BY name",
                )?
                .query_map([zone_id], |r| {
                    Ok(TsigKey {
                        name: r.get(0)?,
                        algorithm: r.get(1)?,
                        secret: r.get::<_, Vec<u8>>(2)?.into(),
                        created_at: r.get(3)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// Stores a TSIG key; names are unique per zone.
    pub async fn add_tsig_key(
        &self,
        zone_id: i64,
        name: &str,
        algorithm: &str,
        secret: Vec<u8>,
    ) -> Result<(), DbError> {
        let (name, algorithm) = (normalize(name), algorithm.to_owned());
        self.run(move |c| {
            match c.execute(
                "INSERT INTO tsig_keys (zone_id, name, algorithm, secret, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![zone_id, name, algorithm, secret, now()],
            ) {
                Ok(_) => Ok(()),
                Err(e) if is_unique_violation(&e) => Err(DbError::Conflict("tsig key")),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// Deletes a TSIG key by name.
    pub async fn delete_tsig_key(&self, zone_id: i64, name: &str) -> Result<(), DbError> {
        let name = normalize(name);
        self.run(move |c| {
            let changed = c.execute(
                "DELETE FROM tsig_keys WHERE zone_id=?1 AND name=?2",
                params![zone_id, name],
            )?;
            if changed == 0 {
                return Err(DbError::NotFound("tsig key"));
            }
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_is_monotonic() {
        let now = u32::try_from(now()).unwrap_or(u32::MAX);
        assert!(next_serial(0) >= now);
        assert_eq!(next_serial(u32::MAX - 1), u32::MAX);
        let ahead = now + 1000;
        assert_eq!(next_serial(ahead), ahead + 1);
    }
}
