//! Per-account SQLite index. A reader sees either side of a complete sync.
//! Item and subtree byte counts use canonical unsigned decimal strings because
//! SQLite's native integer range cannot represent the full u64 domain.
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::drive_stats::RootListing;
use super::{EntryKind, FolderStats, FolderStatsError};

const VERSION: i64 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Checkpoint {
    pub cursor: String,
    pub full_at: i64,
    pub checked_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum IndexChange {
    Upsert {
        id: String,
        parent_id: String,
        kind: EntryKind,
        size: u64,
        modified_time: String,
    },
    Remove {
        id: String,
    },
}

/// Changed rows alone cannot establish a complete tree. Keep the previous
/// checkpoint and retry with a complete normal wildcard listing.
#[derive(Debug, thiserror::Error)]
#[error("index reconciliation required: {0}")]
pub(super) struct NeedsFull(&'static str);

pub(super) struct IndexStore(Connection);

const SCHEMA: &str = "
    CREATE TABLE checkpoint (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        cursor TEXT NOT NULL, full_at INTEGER NOT NULL, checked_at INTEGER NOT NULL
    );
    CREATE TABLE items (
        id TEXT PRIMARY KEY NOT NULL,
        parent TEXT, folder INTEGER NOT NULL,
        size TEXT NOT NULL, modified TEXT NOT NULL,
        total TEXT NOT NULL, children INTEGER NOT NULL DEFAULT 0,
        latest TEXT, seconds INTEGER, nanos INTEGER
    ) WITHOUT ROWID;
    CREATE INDEX by_parent_time ON items(parent, seconds DESC, nanos DESC, latest DESC);
    CREATE INDEX by_parent_kind_time ON items(parent, folder, seconds DESC, nanos DESC, latest DESC);
    PRAGMA user_version=1;
";

// Use the account's own volume for scratch rows. In particular, an Android app
// need not have a writable system temp directory. Rows are removed before a
// successful commit, or by rollback. Parent indexes keep deltas local.
const SCRATCH: &str = "
    CREATE TABLE IF NOT EXISTS staged(
        id TEXT PRIMARY KEY, parent TEXT, folder INTEGER, size TEXT, modified TEXT,
        seconds INTEGER, nanos INTEGER
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS work(
        id TEXT PRIMARY KEY, old_parent TEXT, old_total TEXT,
        new_parent TEXT, pending INTEGER
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS work_old ON work(old_parent);
    CREATE INDEX IF NOT EXISTS work_new ON work(new_parent);
    CREATE INDEX IF NOT EXISTS work_pending ON work(pending);
    CREATE TABLE IF NOT EXISTS hydrate_dirs(
        id TEXT PRIMARY KEY, done INTEGER NOT NULL DEFAULT 0
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS hydrate_remaining ON hydrate_dirs(done);
    CREATE TABLE IF NOT EXISTS observed_children(
        parent TEXT, id TEXT, PRIMARY KEY(parent,id)
    ) WITHOUT ROWID;
    DELETE FROM observed_children;
    DELETE FROM hydrate_dirs;
    DELETE FROM staged;
    DELETE FROM work;
";

// During first boot WAL readers can see the new file before its schema is
// committed. This is an empty cache; an unrelated or newer schema is an error.
fn has_schema(db: &Connection) -> Result<bool> {
    let (version, empty): (i64, bool) = db.query_row(
        "SELECT user_version, CASE WHEN user_version=0 THEN
            NOT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*')
         ELSE 0 END FROM pragma_user_version",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        version == VERSION || (version == 0 && empty),
        "unsupported drive index schema {version}"
    );
    Ok(version == VERSION)
}

fn checkpoint(db: &Connection) -> Result<Option<Checkpoint>> {
    Ok(db
        .query_row(
            "SELECT cursor, full_at, checked_at FROM checkpoint WHERE singleton=1",
            [],
            |r| {
                Ok(Checkpoint {
                    cursor: r.get(0)?,
                    full_at: r.get(1)?,
                    checked_at: r.get(2)?,
                })
            },
        )
        .optional()?)
}

// Detached (trashed) folders keep their descendants private. Walking the
// parent chain through the primary key avoids rewriting a subtree on a move.
const GET: &str = "
    WITH RECURSIVE chain(id) AS (
        VALUES(?1) UNION ALL
        SELECT n.parent FROM chain JOIN items n ON n.id=chain.id WHERE n.parent IS NOT NULL
    )
    SELECT total,latest,children=0 FROM items WHERE id=?1 AND folder=1
    AND EXISTS(SELECT 1 FROM chain WHERE id='')";

fn get(db: &Connection, id: &str) -> Result<Option<FolderStats>> {
    let row = db
        .prepare_cached(GET)?
        .query_row([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })
        .optional()?;
    row.map(|(total, latest_modified_time, is_empty)| {
        Ok(FolderStats {
            total_size_bytes: total.parse()?,
            latest_modified_time,
            is_empty,
        })
    })
    .transpose()
}

impl IndexStore {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        // Newly created indexes return pages used by large one-off scans to
        // the filesystem at commit, instead of retaining them after staging.
        db.pragma_update(None, "auto_vacuum", "FULL")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        #[cfg(target_os = "android")]
        db.pragma_update(None, "temp_store", "MEMORY")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !has_schema(&tx)? {
            tx.execute_batch(SCHEMA)?;
        }
        tx.commit()?;
        Ok(Self(db))
    }

    pub(super) fn open_readonly(path: &Path) -> Result<Self> {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(5))?;
        has_schema(&db)?;
        Ok(Self(db))
    }

    pub(super) fn checkpoint(&self) -> Result<Option<Checkpoint>> {
        if has_schema(&self.0)? {
            checkpoint(&self.0)
        } else {
            Ok(None)
        }
    }

    pub(super) fn get(&self, id: &str) -> Result<Option<FolderStats>> {
        if has_schema(&self.0)? {
            get(&self.0, id)
        } else {
            Ok(None)
        }
    }

    pub(super) fn get_many(&self, ids: &[String]) -> Result<HashMap<String, FolderStats>> {
        let tx = self.0.unchecked_transaction()?;
        let mut result = HashMap::new();
        if !has_schema(&tx)? {
            return Ok(result);
        }
        for id in ids {
            if let Some(stats) = get(&tx, id)? {
                result.insert(id.clone(), stats);
            }
        }
        tx.commit()?;
        Ok(result)
    }

    pub(super) fn apply<G>(
        &mut self,
        expected: Option<&Checkpoint>,
        full: bool,
        expected_root: Option<&RootListing>,
        read: impl FnOnce(&mut dyn FnMut(IndexChange) -> Result<()>) -> Result<Checkpoint>,
        hydrate: impl FnMut(&str, &mut dyn FnMut(IndexChange) -> Result<()>) -> Result<()>,
        check: impl Fn(bool) -> Result<G>,
    ) -> Result<()> {
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            checkpoint(&tx)?.as_ref() == expected,
            "drive index checkpoint changed since sync started"
        );
        ensure!(full || expected.is_some(), NeedsFull("missing baseline"));
        tx.execute_batch(SCRATCH)?;
        let next = read(&mut |change| stage(&tx, change))?;
        drop(check(false)?);
        if full {
            tx.execute_batch(
                "DELETE FROM items; INSERT INTO items(id,parent,folder,size,modified,total)
                              VALUES('',NULL,1,'0','','0');",
            )?;
        }
        if !full {
            hydrate_changed_folders(&tx, hydrate, || {
                drop(check(false)?);
                Ok(())
            })?;
        }
        prepare_changes(&tx, full)?;
        aggregate(&tx, || {
            drop(check(false)?);
            Ok(())
        })?;
        if let Some(root) = expected_root {
            validate_root(&tx, root)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO checkpoint VALUES(1,?1,?2,?3)",
            params![next.cursor, next.full_at, next.checked_at],
        )?;
        tx.execute_batch("DELETE FROM staged; DELETE FROM work; DELETE FROM hydrate_dirs; DELETE FROM observed_children;")?;
        // The caller's guard can lock the account/generation through publication.
        let _guard = check(true)?;
        tx.commit()?;
        Ok(())
    }
}

fn stage(db: &Connection, change: IndexChange) -> Result<()> {
    let (id, parent, folder, size, modified) = match change {
        IndexChange::Upsert {
            id,
            parent_id,
            kind,
            size,
            modified_time,
        } => {
            ensure!(
                parent_id != "*" && parent_id != id,
                NeedsFull("invalid parent ID")
            );
            (
                id,
                Some(parent_id),
                Some(kind == EntryKind::Folder),
                Some(size.to_string()),
                Some(modified_time),
            )
        }
        IndexChange::Remove { id } => (id, None, None, None, None),
    };
    ensure!(!id.is_empty() && id != "*", NeedsFull("invalid item ID"));
    let stamp = modified
        .as_deref()
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok());
    let affected = db
        .prepare_cached(
            "
        INSERT INTO staged VALUES(?1,?2,?3,?4,?5,?6,?7)
        ON CONFLICT(id) DO UPDATE SET id=excluded.id WHERE
        staged.parent IS excluded.parent AND staged.folder IS excluded.folder AND
        staged.size IS excluded.size AND staged.modified IS excluded.modified
    ",
        )?
        .execute(params![
            id,
            parent,
            folder,
            size,
            modified,
            stamp.map(|t| t.unix_timestamp()),
            stamp.map(|t| t.nanosecond())
        ])?;
    ensure!(
        affected == 1,
        NeedsFull("conflicting versions of an item in one scan")
    );
    Ok(())
}

/// A newly visible directory may already contain files older than the delta
/// cursor. Enumerate only those branches. A normal directory listing is also
/// the authority for removals which happened while a directory was hidden.
fn hydrate_changed_folders(
    db: &Connection,
    mut list: impl FnMut(&str, &mut dyn FnMut(IndexChange) -> Result<()>) -> Result<()>,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    let mut insert = db.prepare_cached("INSERT OR IGNORE INTO hydrate_dirs(id) VALUES(?1)")?;
    {
        let mut folders = db.prepare("SELECT id FROM staged WHERE folder=1")?;
        let mut rows = folders.query([])?;
        while let Some(row) = rows.next()? {
            let id = row.get_ref(0)?.as_str()?;
            if get(db, id)?.is_none() {
                insert.execute([id])?;
            }
        }
    }
    loop {
        check()?;
        let id: Option<String> = db
            .prepare_cached("SELECT id FROM hydrate_dirs WHERE done=0 LIMIT 1")?
            .query_row([], |r| r.get(0))
            .optional()?;
        let Some(id) = id else { break };
        list(&id, &mut |change| {
            let IndexChange::Upsert {
                id: child,
                parent_id,
                kind,
                ..
            } = &change
            else {
                return Err(NeedsFull("tombstone in scoped normal listing").into());
            };
            ensure!(
                *parent_id == id,
                NeedsFull("wrong parent in scoped listing")
            );
            let added = db
                .prepare_cached("INSERT OR IGNORE INTO observed_children VALUES(?1,?2)")?
                .execute(params![id, child])?;
            ensure!(added == 1, NeedsFull("duplicate child in scoped listing"));
            if *kind == EntryKind::Folder {
                insert.execute([child])?;
            }
            stage(db, change)
        })?;
        db.prepare_cached("UPDATE hydrate_dirs SET done=1 WHERE id=?1")?
            .execute([id])?;
    }
    let disagreement: bool = db.query_row("
        SELECT EXISTS(SELECT 1 FROM staged s JOIN hydrate_dirs h ON h.id=s.parent
                      WHERE NOT EXISTS(SELECT 1 FROM observed_children c WHERE c.parent=s.parent AND c.id=s.id))",
        [], |r| r.get(0))?;
    ensure!(
        !disagreement,
        NeedsFull("incremental and normal directory listings disagree")
    );
    // An independently reported move wins over absence in the old directory.
    db.execute_batch(
        "
        INSERT INTO staged(id)
            SELECT i.id FROM hydrate_dirs h CROSS JOIN items i ON i.parent=h.id
            WHERE NOT EXISTS(SELECT 1 FROM observed_children c WHERE c.parent=h.id AND c.id=i.id)
              AND NOT EXISTS(SELECT 1 FROM staged s WHERE s.id=i.id);
    ",
    )?;
    Ok(())
}

fn prepare_changes(db: &Connection, full: bool) -> Result<()> {
    if !full {
        let type_change: bool = db.query_row(
            "
        SELECT EXISTS(SELECT 1 FROM staged s JOIN items i ON i.id=s.id
                      WHERE s.folder IS NOT NULL AND i.folder!=s.folder)",
            [],
            |r| r.get(0),
        )?;
        ensure!(!type_change, NeedsFull("changed item kind"));
    }
    db.execute_batch(
        "
        DELETE FROM staged WHERE EXISTS(
            SELECT 1 FROM items i WHERE i.id=staged.id AND
                ((staged.folder IS NULL AND i.parent IS NULL)
                 OR (i.parent=staged.parent AND i.folder=staged.folder
                     AND i.size=staged.size AND i.modified=staged.modified))
        ) OR (folder IS NULL AND NOT EXISTS(SELECT 1 FROM items i WHERE i.id=staged.id));
    ",
    )?;
    db.execute_batch(
        "
        INSERT INTO work(id,old_parent,old_total)
            SELECT s.id,i.parent,i.total FROM staged s LEFT JOIN items i ON i.id=s.id;

        UPDATE items SET parent=NULL WHERE id IN (SELECT id FROM staged WHERE folder IS NULL);

        INSERT INTO items(id,parent,folder,size,modified,total,latest,seconds,nanos)
            SELECT id,parent,folder,size,modified,
                   CASE WHEN folder=0 THEN size ELSE '0' END,
                   CASE WHEN folder=0 AND seconds IS NOT NULL THEN modified END,
                   CASE WHEN folder=0 THEN seconds END,
                   CASE WHEN folder=0 THEN nanos END
            FROM staged WHERE folder IS NOT NULL
        ON CONFLICT(id) DO UPDATE SET
            parent=excluded.parent, size=excluded.size, modified=excluded.modified,
            total=CASE WHEN items.folder=1 THEN items.total ELSE excluded.total END,
            latest=CASE WHEN items.folder=1 THEN items.latest ELSE excluded.latest END,
            seconds=CASE WHEN items.folder=1 THEN items.seconds ELSE excluded.seconds END,
            nanos=CASE WHEN items.folder=1 THEN items.nanos ELSE excluded.nanos END;
    ",
    )?;
    // A baseline already staged every node. Only a delta needs to discover
    // additional ancestors whose recursive results must be updated.
    if !full {
        db.execute_batch("
        -- Capture contributions from both ends of moves. Ancestors not in
        -- staged still have their old totals and links until the bottom-up pass.
        WITH RECURSIVE ancestors(id) AS (
            SELECT old_parent FROM work WHERE old_parent IS NOT NULL
            UNION SELECT i.parent FROM work w CROSS JOIN items i ON i.id=w.id WHERE i.parent IS NOT NULL
            UNION SELECT i.parent FROM ancestors a CROSS JOIN items i ON i.id=a.id WHERE i.parent IS NOT NULL
        )
        INSERT OR IGNORE INTO work(id,old_parent,old_total)
            SELECT i.id,i.parent,i.total FROM ancestors a CROSS JOIN items i ON i.id=a.id;
        ")?;
    }
    db.execute_batch(
        "
        INSERT OR IGNORE INTO work(id,old_parent,old_total) VALUES('',NULL,'0');
        UPDATE work SET new_parent=(SELECT parent FROM items i WHERE i.id=work.id);
    ",
    )?;
    let invalid: bool = db.query_row(
        "
        SELECT EXISTS(SELECT 1 FROM work w LEFT JOIN items p ON p.id=w.new_parent
                      WHERE w.new_parent IS NOT NULL AND (p.folder IS NOT 1))",
        [],
        |r| r.get(0),
    )?;
    ensure!(!invalid, NeedsFull("missing or non-folder parent"));
    if !full {
        let hidden: bool = db.query_row("
            WITH RECURSIVE path(id) AS (
                SELECT id FROM staged WHERE folder IS NOT NULL
                UNION SELECT i.parent FROM path JOIN items i ON i.id=path.id WHERE i.parent IS NOT NULL
            )
            SELECT EXISTS(SELECT 1 FROM path JOIN items i ON i.id=path.id
                          WHERE i.id!='' AND i.parent IS NULL)", [], |r| r.get(0))?;
        ensure!(!hidden, NeedsFull("changed item has a detached parent"));
    }
    db.execute_batch(
        "
        UPDATE work SET pending=CASE
            WHEN EXISTS(SELECT 1 FROM items i WHERE i.id=work.id AND i.folder=1) THEN
                (SELECT count(*) FROM work child CROSS JOIN items i ON i.id=child.id
                 WHERE child.new_parent=work.id AND i.folder=1)
            ELSE -1 END;
    ",
    )?;
    Ok(())
}

fn contributions(db: &Connection, sql: &str, id: &str) -> Result<(u64, i64)> {
    let mut stmt = db.prepare_cached(sql)?;
    let mut rows = stmt.query([id])?;
    let (mut bytes, mut count) = (0_u64, 0);
    while let Some(row) = rows.next()? {
        let value: u64 = row.get_ref(0)?.as_str()?.parse()?;
        bytes = bytes
            .checked_add(value)
            .ok_or(FolderStatsError::SizeOverflow)?;
        count += 1;
    }
    Ok((bytes, count))
}

fn aggregate(db: &Connection, check: impl Fn() -> Result<()>) -> Result<()> {
    loop {
        check()?;
        let id: Option<String> = db
            .prepare_cached("SELECT id FROM work WHERE pending=0 LIMIT 1")?
            .query_row([], |r| r.get(0))
            .optional()?;
        let Some(id) = id else { break };
        let (previous, count): (String, i64) = db
            .prepare_cached("SELECT total,children FROM items WHERE id=?1")?
            .query_row([&id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let (removed, lost) =
            contributions(db, "SELECT old_total FROM work WHERE old_parent=?1", &id)?;
        let (added, gained) = contributions(
            db,
            "
            SELECT i.total FROM work w CROSS JOIN items i ON i.id=w.id WHERE w.new_parent=?1",
            &id,
        )?;
        let total = previous
            .parse::<u64>()?
            .checked_sub(removed)
            .context("inconsistent stored subtree size")?
            .checked_add(added)
            .ok_or(FolderStatsError::SizeOverflow)?;
        ensure!(count >= lost, "inconsistent stored child count");
        db.prepare_cached(
            "
            UPDATE items SET total=?2, children=?3, (latest,seconds,nanos)=(
                SELECT latest,seconds,nanos FROM items WHERE parent=?1 AND seconds IS NOT NULL
                ORDER BY seconds DESC,nanos DESC,latest DESC LIMIT 1
            ) WHERE id=?1",
        )?
        .execute(params![id, total.to_string(), count - lost + gained])?;
        db.prepare_cached("UPDATE work SET pending=-1 WHERE id=?1")?
            .execute([&id])?;
        db.prepare_cached(
            "UPDATE work SET pending=pending-1 WHERE id=(SELECT new_parent FROM work WHERE id=?1)",
        )?
        .execute([&id])?;
    }
    let cycle: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM work WHERE pending>=0)",
        [],
        |r| r.get(0),
    )?;
    ensure!(!cycle, NeedsFull("cycle in folder graph"));
    Ok(())
}

fn validate_root(db: &Connection, expected: &RootListing) -> Result<()> {
    let (total, children): (String, usize) =
        db.query_row("SELECT total,children FROM items WHERE id=''", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let mut direct_bytes: u64 = total.parse()?;
    let mut folders = db.prepare("SELECT id,total FROM items WHERE parent='' AND folder=1")?;
    let mut rows = folders.query([])?;
    let mut folder_count = 0;
    while let Some(row) = rows.next()? {
        ensure!(
            expected.folders.contains(row.get_ref(0)?.as_str()?),
            NeedsFull("normal root folder IDs differ")
        );
        direct_bytes = direct_bytes
            .checked_sub(row.get_ref(1)?.as_str()?.parse()?)
            .context("inconsistent root folder size")?;
        folder_count += 1;
    }
    let latest: Option<String> = db
        .query_row(
            "
        SELECT latest FROM items WHERE parent='' AND folder=0 AND seconds IS NOT NULL
        ORDER BY seconds DESC,nanos DESC,latest DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    ensure!(
        folder_count == expected.folders.len()
            && children.checked_sub(folder_count) == Some(expected.file_count)
            && direct_bytes == expected.files.total_size_bytes
            && latest == expected.files.latest_modified_time
            && (children == 0) == expected.files.is_empty,
        NeedsFull("normal root listing differs from index")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::temp_test_dir;
    use super::*;

    fn folder(id: &str, parent: &str) -> IndexChange {
        IndexChange::Upsert {
            id: id.into(),
            parent_id: parent.into(),
            kind: EntryKind::Folder,
            size: u64::MAX,
            modified_time: "2100-01-01T00:00:00Z".into(),
        }
    }

    fn file(id: &str, parent: &str, size: u64, time: &str) -> IndexChange {
        IndexChange::Upsert {
            id: id.into(),
            parent_id: parent.into(),
            kind: EntryKind::File,
            size,
            modified_time: time.into(),
        }
    }

    fn remove(id: &str) -> IndexChange {
        IndexChange::Remove { id: id.into() }
    }

    fn cp(n: i64) -> Checkpoint {
        Checkpoint {
            cursor: n.to_string(),
            full_at: 1,
            checked_at: n,
        }
    }

    fn sync(store: &mut IndexStore, full: bool, changes: Vec<IndexChange>, n: i64) -> Result<()> {
        let prev = store.checkpoint()?;
        store.apply(
            prev.as_ref(),
            full,
            None,
            |sink| {
                for change in changes {
                    sink(change)?;
                }
                Ok(cp(n))
            },
            |_, _| unreachable!("unexpected scoped call"),
            |_| Ok(()),
        )
    }

    fn stats(size: u64, time: Option<&str>, empty: bool) -> FolderStats {
        FolderStats {
            total_size_bytes: size,
            latest_modified_time: time.map(str::to_owned),
            is_empty: empty,
        }
    }

    #[test]
    fn out_of_order_replay_move_and_decreasing_maximum_survive_reopen() -> Result<()> {
        let dir = temp_test_dir("index-moves");
        let path = dir.join("state.sqlite");
        let older = "2024-04-01T10:00:00+08:00";
        let newer = "2024-04-01T02:00:00.001Z";
        {
            let mut db = IndexStore::open(&path)?;
            let leaf = file("large", "b", u64::MAX - 1, newer);
            sync(
                &mut db,
                true,
                vec![
                    leaf.clone(),
                    file("small", "a", 1, older),
                    folder("b", "a"),
                    folder("a", ""),
                    folder("c", ""),
                    leaf.clone(),
                ],
                1,
            )?;
            assert_eq!(db.get("")?, Some(stats(u64::MAX, Some(newer), false)));
            assert_eq!(db.get("c")?, Some(stats(0, None, true)));
            assert_eq!(db.get("small")?, None);
            // Known subtree changes parent without descending into its files.
            sync(&mut db, false, vec![leaf, folder("b", "c")], 2)?;
            assert_eq!(db.get("a")?, Some(stats(1, Some(older), false)));
            assert_eq!(db.get("c")?, Some(stats(u64::MAX - 1, Some(newer), false)));
            // Replace the winner with an invalid stamp while moving the other file.
            sync(
                &mut db,
                false,
                vec![
                    file("large", "b", 0, "no date"),
                    file("small", "", 1, older),
                ],
                3,
            )?;
            assert_eq!(db.get("a")?, Some(stats(0, None, true)));
            assert_eq!(db.get("b")?, Some(stats(0, None, false)));
            assert_eq!(db.get("")?, Some(stats(1, Some(older), false)));
        }
        let db = IndexStore::open_readonly(&path)?;
        assert_eq!(db.checkpoint()?, Some(cp(3)));
        let result = db.get_many(&["".into(), "absent".into(), "a".into(), "small".into()])?;
        assert_eq!(result.len(), 2);
        assert_eq!(result[""], stats(1, Some(older), false));
        drop(db);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn every_folder_matches_full_tree_walk_after_incremental_batches() -> Result<()> {
        type Tree = HashMap<String, (String, EntryKind, u64, String)>;

        // Recursively enumerate the complete file list anew for each subtree.
        // The reference model has no incremental sums, SQL or tombstone logic.
        fn walk<'a>(
            tree: &'a Tree,
            parent: &str,
            out: &mut HashMap<String, FolderStats>,
        ) -> Vec<(u64, &'a str)> {
            let mut files = Vec::new();
            let mut empty = true;
            for (id, (p, kind, bytes, time)) in tree {
                if p != parent {
                    continue;
                }
                empty = false;
                match kind {
                    EntryKind::Folder => files.extend(walk(tree, id, out)),
                    EntryKind::File => files.push((*bytes, time.as_str())),
                }
            }
            let latest = files
                .iter()
                .filter_map(|(_, stamp)| {
                    OffsetDateTime::parse(stamp, &Rfc3339)
                        .ok()
                        .map(|t| (t, *stamp))
                })
                .max();
            out.insert(
                parent.into(),
                stats(
                    files.iter().map(|(bytes, _)| bytes).sum(),
                    latest.map(|(_, stamp)| stamp),
                    empty,
                ),
            );
            files
        }

        let early = "2024-03-31T23:00:00Z";
        let late = "2024-04-01T08:00:00+08:00";
        let newest = "2024-04-01T00:00:00.001Z";
        let batches = [
            vec![
                folder("a", ""),
                folder("b", "a"),
                folder("c", "b"),
                folder("d", ""),
                folder("e", "d"),
                folder("untouched", ""),
                file("f", "c", 7, late),
                file("g", "b", 3, early),
                file("h", "e", 2, newest),
                file("zero", "e", 0, ""),
            ],
            // Three generations move in one batch, while a deep file is resized.
            vec![
                folder("a", "d"),
                folder("b", "e"),
                folder("c", "a"),
                file("f", "c", 12, "invalid"),
            ],
            vec![
                folder("e", "c"),
                folder("c", ""),
                folder("b", "a"),
                file("h", "b", 4, newest),
            ],
            // A surviving branch moves out of its deleted ancestor in the same batch.
            vec![
                remove("a"),
                folder("b", ""),
                remove("h"),
                file("g", "b", 3, late),
            ],
            vec![remove("c"), file("g", "b", 3, early)],
            vec![remove("g")],
        ];
        let mut store = IndexStore::open(Path::new(":memory:"))?;
        let mut tree = Tree::new();
        let mut observed = std::collections::BTreeSet::from([String::new()]);
        for (step, batch) in batches.into_iter().enumerate() {
            for event in &batch {
                match event {
                    IndexChange::Upsert {
                        id,
                        parent_id,
                        kind,
                        size,
                        modified_time,
                    } => {
                        observed.insert(id.clone());
                        tree.insert(
                            id.clone(),
                            (
                                parent_id.clone(),
                                kind.clone(),
                                *size,
                                modified_time.clone(),
                            ),
                        );
                    }
                    IndexChange::Remove { id } => {
                        tree.remove(id);
                    }
                }
            }
            sync(&mut store, step == 0, batch, step as i64 + 1)?;
            let mut expected = HashMap::new();
            walk(&tree, "", &mut expected);
            let ids = observed.iter().cloned().collect::<Vec<_>>();
            assert_eq!(store.get_many(&ids)?, expected, "batch {step}");
        }
        Ok(())
    }

    #[test]
    fn trash_restore_hydrates_only_returned_subtrees_and_preserves_moves() -> Result<()> {
        let mut db = IndexStore::open(Path::new(":memory:"))?;
        sync(
            &mut db,
            true,
            vec![
                folder("parent", ""),
                folder("nested", "parent"),
                file("gone", "nested", 99, ""),
                file("moved", "parent", 4, ""),
                folder("other", ""),
            ],
            1,
        )?;
        sync(
            &mut db,
            false,
            vec![remove("parent"), remove("absent"), remove("parent")],
            2,
        )?;
        assert_eq!(db.get("")?, Some(stats(0, None, false)));
        assert_eq!(db.get("nested")?, None);
        let mut visited = Vec::new();
        db.apply(
            Some(&cp(2)),
            false,
            None,
            |sink| {
                sink(folder("parent", ""))?;
                sink(file("moved", "other", 4, ""))?;
                sink(folder("fresh", "parent"))?;
                Ok(cp(3))
            },
            |id, sink| {
                visited.push(id.to_owned());
                match id {
                    "parent" => {
                        sink(folder("nested", "parent"))?;
                        sink(folder("fresh", "parent"))?;
                    }
                    "nested" => {
                        sink(file("new", "nested", 7, ""))?;
                    }
                    "fresh" => {}
                    _ => panic!("unrelated folder enumerated: {id}"),
                }
                Ok(())
            },
            |_| Ok(()),
        )?;
        visited.sort();
        assert_eq!(visited, ["fresh", "nested", "parent"]);
        assert_eq!(db.get("")?, Some(stats(11, None, false)));
        assert_eq!(db.get("parent")?, Some(stats(7, None, false)));
        assert_eq!(db.get("other")?, Some(stats(4, None, false)));
        assert_eq!(db.get("fresh")?, Some(stats(0, None, true)));
        // A plain mkdir needs exactly one scoped request.
        let mut listed = 0;
        db.apply(
            Some(&cp(3)),
            false,
            None,
            |sink| {
                sink(folder("empty", ""))?;
                Ok(cp(4))
            },
            |id, _| {
                assert_eq!(id, "empty");
                listed += 1;
                Ok(())
            },
            |_| Ok(()),
        )?;
        assert_eq!(listed, 1);
        assert_eq!(db.get("empty")?, Some(stats(0, None, true)));
        Ok(())
    }

    #[test]
    fn graph_conflicts_failures_and_overflow_roll_back_data_and_cursor() -> Result<()> {
        let mut db = IndexStore::open(Path::new(":memory:"))?;
        sync(
            &mut db,
            true,
            vec![folder("a", ""), folder("b", "a"), file("x", "b", 3, "")],
            1,
        )?;
        for (full, changes) in [
            (false, vec![folder("a", "b")]),
            (false, vec![file("x", "no parent", 9, "")]),
            (false, vec![file("y", "x", 0, "")]),
            (true, vec![folder("a", "b"), folder("b", "a")]),
            (false, vec![file("x", "", 5, ""), file("x", "", 9, "")]),
            (false, vec![file("big", "", u64::MAX, "")]),
        ] {
            assert!(sync(&mut db, full, changes, 2).is_err());
            assert_eq!(db.checkpoint()?, Some(cp(1)));
            assert_eq!(db.get("a")?, Some(stats(3, None, false)));
        }
        let failure = db.apply(
            Some(&cp(1)),
            false,
            None,
            |sink| {
                sink(remove("x"))?;
                anyhow::bail!("late page failed")
            },
            |_, _| unreachable!("unexpected scoped call"),
            |_| Ok(()),
        );
        assert!(failure.is_err());
        // Fail at the last guard, after the prospective graph has been updated.
        let failure = db.apply(
            Some(&cp(1)),
            false,
            None,
            |_| Ok(cp(2)),
            |_, _| unreachable!(),
            |committing| {
                ensure!(!committing, "cancelled");
                Ok(())
            },
        );
        assert!(failure.is_err());
        assert_eq!(db.checkpoint()?, Some(cp(1)));
        assert_eq!(db.get("")?, Some(stats(3, None, false)));
        Ok(())
    }

    #[test]
    fn first_schema_creation_is_an_offline_miss() -> Result<()> {
        let dir = temp_test_dir("index-schema");
        let path = dir.join("state.sqlite");
        let mut writer = Connection::open(&path)?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        let tx = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        let early = IndexStore::open_readonly(&path)?;
        assert!(early.checkpoint()?.is_none());
        assert!(early.get("")?.is_none());
        assert!(early.get_many(&["".into()])?.is_empty());
        tx.commit()?;
        assert!(early.get("")?.is_none());
        drop(early);
        drop(writer);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn readers_keep_committed_snapshot_and_root_mismatch_never_publishes() -> Result<()> {
        let dir = temp_test_dir("index-atomic");
        let path = dir.join("state.sqlite");
        let mut db = IndexStore::open(&path)?;
        sync(&mut db, true, vec![file("x", "", 3, "")], 1)?;
        let expected = RootListing {
            folders: Default::default(),
            files: stats(3, None, false),
            file_count: 1,
        };
        let err = db
            .apply(
                Some(&cp(1)),
                false,
                Some(&expected),
                |sink| {
                    sink(file("x", "", 9, ""))?;
                    let reader = IndexStore::open_readonly(&path)?;
                    assert_eq!(reader.get("")?, Some(stats(3, None, false)));
                    Ok(cp(2))
                },
                |_, _| unreachable!("unexpected scoped call"),
                |_| Ok(()),
            )
            .unwrap_err();
        assert!(err.is::<NeedsFull>(), "{err:#}");
        assert!(
            db.apply(
                Some(&cp(0)),
                false,
                None,
                |_| panic!("stale writer must not fetch"),
                |_, _| unreachable!("unexpected scoped call"),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(db.checkpoint()?, Some(cp(1)));
        assert_eq!(db.get("")?, Some(stats(3, None, false)));
        drop(db);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
