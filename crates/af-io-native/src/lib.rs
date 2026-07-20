#![forbid(unsafe_code)]
//! Persistence for the native `.arcf` format.
//!
//! An `.arcf` is a standard ZIP containing:
//! - `manifest.json` first and uncompressed for detection and recovery even
//!   when the ZIP is partially damaged;
//! - the complete [`Document`] in a deflated `document.json`, wrapped as
//!   `{ "formatVersion": N, "document": { ... } }` so extracted JSON remains
//!   self-describing.
//!
//! Invariants:
//! - **Atomic writes** use `path.tmp`, `fsync`, and `rename`; failures never
//!   corrupt the previous file.
//! - **Roundtrips** preserve the canonical `document.json` byte for byte,
//!   including exact f64 values through `serde_json`'s `float_roundtrip`.
//! - Versions newer than this reader return [`Error::NewerVersion`].
//! - Resilient loading always runs `validate_full`: unrecoverable corruption
//!   returns [`Error::InvalidDocument`], while recoverable damage is repaired
//!   and reported through [`LoadReport`].

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use af_model::{Document, DocumentId, Issue, Severity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// Latest format version understood by this reader and writer. Newer files are
/// rejected with [`Error::NewerVersion`]; older versions pass through [`migrate`].
pub const FORMAT_VERSION: u32 = 1;

const FORMAT_TAG: &str = "arcforge-cad-document";
const MANIFEST_NAME: &str = "manifest.json";
const DOCUMENT_NAME: &str = "document.json";

/// Hard decompression limit for `document.json` (256 MiB). Larger entries are
/// treated as corrupt or hostile input so memory allocation remains bounded.
const MAX_DOCUMENT_BYTES: u64 = 256 * 1024 * 1024;

/// Hard decompression limit for the small `manifest.json` metadata entry.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Result type for persistence operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Typed persistence errors. No path intentionally panics.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// File system error such as open, write, rename, permissions, or a full disk.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// Malformed internal JSON.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// The bytes are not a readable `.arcf` container.
    #[error("not a readable .arcf container")]
    NotArcf,
    /// `document.json` is missing or unreadable.
    #[error("document.json missing or unreadable (container: {container})")]
    DocumentUnreadable {
        /// Diagnostic derived from `manifest.json` when available.
        container: String,
    },
    /// A ZIP entry expands beyond its hard limit.
    #[error("zip entry '{name}' exceeds the {limit}-byte decompression limit")]
    EntryTooLarge {
        /// Container entry name (`document.json` or `manifest.json`).
        name: String,
        /// Exceeded limit in bytes.
        limit: u64,
    },
    /// The file uses a newer format version than this build supports.
    #[error(
        "file format version {found} is newer than supported ({supported}); save it from a newer ArcCAD"
    )]
    NewerVersion {
        /// Version found in the file.
        found: u32,
        /// Maximum version understood by this build.
        supported: u32,
    },
    /// No migration is registered from `from` to [`FORMAT_VERSION`].
    #[error("no migration path from format version {from} to {FORMAT_VERSION}")]
    NoMigration {
        /// Source version without a registered migration.
        from: u32,
    },
    /// `validate_full` found unrecoverable corruption after loading.
    #[error("document from disk is invalid: {count} unrecoverable issue(s)")]
    InvalidDocument {
        /// Number of [`Severity::Error`] issues.
        count: usize,
        /// Complete validation report.
        issues: Vec<Issue>,
    },
}

/// Recovery level used while loading a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Both `manifest.json` and `document.json` are present and readable.
    Normal,
    /// The manifest was missing or unreadable, but `document.json` was recovered.
    Recovered,
}

/// Load report containing repairs, recovery level, and warnings.
#[derive(Debug, Clone)]
pub struct LoadReport {
    /// Recoverable repairs and warnings returned by `validate_full`.
    pub issues: Vec<Issue>,
    /// How the container was recovered.
    pub recovery: Recovery,
    /// Additional human-readable warnings.
    pub warnings: Vec<String>,
}

/// Save report.
#[derive(Debug, Clone)]
pub struct SaveReport {
    /// Destination path written.
    pub path: PathBuf,
    /// Size of the written `.arcf` in bytes.
    pub size_bytes: u64,
}

/// Container metadata stored first and uncompressed in `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Format type identifier (`"arcforge-cad-document"`).
    pub format: String,
    /// `.arcf` format version.
    pub format_version: u32,
    /// Name of the application that wrote the file.
    pub app_name: String,
    /// Version of the application that wrote the file.
    pub app_version: String,
    /// Creation time in ISO UTC, when recorded by the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_utc: Option<String>,
    /// Last modification time in ISO UTC, when recorded by the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_utc: Option<String>,
    /// Global document identity.
    pub document_id: DocumentId,
    /// Per-file checksums. ZIP entry CRC32 already covers `document.json`.
    // ponytail: SHA-256 would be redundant until cross-tool verification needs it.
    #[serde(default)]
    pub checksums: BTreeMap<String, String>,
}

impl Manifest {
    fn for_document(doc: &Document) -> Self {
        Self {
            format: FORMAT_TAG.to_string(),
            format_version: FORMAT_VERSION,
            app_name: "ArcCAD".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created_utc: doc.metadata().created_utc().map(str::to_string),
            modified_utc: doc.metadata().modified_utc().map(str::to_string),
            document_id: doc.id(),
            checksums: BTreeMap::new(),
        }
    }
}

/// Borrowed document reference used while writing `document.json`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DocFileRef<'a> {
    format_version: u32,
    document: &'a Document,
}

/// Owned `document.json` used by the current-version fast path.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocFileOwned {
    #[allow(dead_code)]
    format_version: u32,
    document: Document,
}

/// Lightweight header used to choose migration or rejection before decoding a
/// potentially newer `Document` schema.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format_version: u32,
}

// ============================ Public API ============================

/// Serializes `doc` into an in-memory `.arcf` ZIP.
///
/// The internal `document.json` is canonical: the same document produces the
/// same bytes. This is also useful for byte-oriented API boundaries.
pub fn to_bytes(doc: &Document) -> Result<Vec<u8>> {
    let manifest_json = serde_json::to_vec(&Manifest::for_document(doc))?;
    let document_json = serde_json::to_vec(&DocFileRef {
        format_version: FORMAT_VERSION,
        document: doc,
    })?;
    pack_arcf(&manifest_json, &document_json)
}

/// Saves `doc` atomically through `path.tmp`, `fsync`, and `rename`.
pub fn save(doc: &Document, path: impl AsRef<Path>) -> Result<SaveReport> {
    let path = path.as_ref();
    let bytes = to_bytes(doc)?;
    write_atomic(path, &bytes)?;
    Ok(SaveReport {
        path: path.to_path_buf(),
        size_bytes: bytes.len() as u64,
    })
}

/// Loads a document from the `.arcf` at `path`.
pub fn load(path: impl AsRef<Path>) -> Result<(Document, LoadReport)> {
    let bytes = std::fs::read(path)?;
    load_bytes(&bytes)
}

/// Loads a document from `.arcf` bytes and runs `validate_full` before return.
/// Arbitrary bytes produce `Ok` or `Err`, never an intentional panic.
pub fn load_bytes(bytes: &[u8]) -> Result<(Document, LoadReport)> {
    // Reject input that is not even a readable ZIP.
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::NotArcf)?;

    // The manifest is optional. Oversized, missing, or damaged manifests degrade
    // to recovered mode; only document.json has a hard failure path.
    let manifest: Option<Manifest> =
        read_zip_entry(&mut archive, MANIFEST_NAME, MAX_MANIFEST_BYTES)
            .unwrap_or(None)
            .and_then(|b| serde_json::from_slice(&b).ok());
    let container_desc = match &manifest {
        Some(m) => format!("v{} id {}", m.format_version, m.document_id.as_uuid()),
        None => "manifest missing/unreadable".to_string(),
    };

    // The document is required because no drawing can be recovered without it.
    let Some(doc_bytes) = read_zip_entry(&mut archive, DOCUMENT_NAME, MAX_DOCUMENT_BYTES)? else {
        // A readable manifest cannot compensate for a missing document.
        return Err(Error::DocumentUnreadable {
            container: container_desc,
        });
    };

    // A missing manifest loses disposable container metadata, not drawing data.
    let recovery = if manifest.is_some() {
        Recovery::Normal
    } else {
        Recovery::Recovered
    };

    // Read only the version before deserializing the full document.
    let Ok(header) = serde_json::from_slice::<Header>(&doc_bytes) else {
        return Err(Error::DocumentUnreadable {
            container: container_desc,
        });
    };
    if header.format_version > FORMAT_VERSION {
        return Err(Error::NewerVersion {
            found: header.format_version,
            supported: FORMAT_VERSION,
        });
    }

    let mut doc: Document = if header.format_version == FORMAT_VERSION {
        // Current schema fast path with bit-exact f64 deserialization.
        serde_json::from_slice::<DocFileOwned>(&doc_bytes)?.document
    } else {
        // Migrate JSON to the current version before deserializing.
        let file: Value = serde_json::from_slice(&doc_bytes)?;
        let doc_val = file
            .get("document")
            .cloned()
            .ok_or_else(|| Error::DocumentUnreadable {
                container: container_desc.clone(),
            })?;
        let migrated = migrate(doc_val, header.format_version)?;
        serde_json::from_value(migrated)?
    };

    // Repair recoverable issues and report unrecoverable corruption.
    let issues = doc.validate_full();
    let error_count = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    if error_count > 0 {
        return Err(Error::InvalidDocument {
            count: error_count,
            issues,
        });
    }

    let mut warnings = Vec::new();
    if recovery == Recovery::Recovered {
        warnings
            .push("manifest.json missing or unreadable; recovered from document.json".to_string());
    }

    Ok((
        doc,
        LoadReport {
            issues,
            recovery,
            warnings,
        },
    ))
}

/// Migrates a `document` JSON tree from `from` to [`FORMAT_VERSION`] by applying
/// the pure functions registered in [`MIGRATIONS`] in order.
///
/// Version 1 is currently the only format, so the current version is a no-op and
/// older versions return [`Error::NoMigration`].
pub fn migrate(mut document: Value, from: u32) -> Result<Value> {
    let mut v = from;
    while v < FORMAT_VERSION {
        // Step i upgrades version i + 1 to i + 2, so its index is v - 1.
        let step = v
            .checked_sub(1)
            .and_then(|i| MIGRATIONS.get(i as usize))
            .ok_or(Error::NoMigration { from })?;
        document = step(document)?;
        v += 1;
    }
    Ok(document)
}

/// Pure JSON-to-JSON migrations in version order. This remains empty while
/// [`FORMAT_VERSION`] is 1. Every future migration needs an old-version golden
/// file and a validation test.
const MIGRATIONS: &[fn(Value) -> Result<Value>] = &[];

// ============================ Internals ============================

/// Packs `manifest.json` first and uncompressed, followed by a deflated
/// `document.json`, and returns the ZIP bytes.
fn pack_arcf(manifest_json: &[u8], document_json: &[u8]) -> Result<Vec<u8>> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));

    zw.start_file(
        MANIFEST_NAME,
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .map_err(zip_to_io)?;
    zw.write_all(manifest_json)?;

    zw.start_file(
        DOCUMENT_NAME,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .map_err(zip_to_io)?;
    zw.write_all(document_json)?;

    let cursor = zw.finish().map_err(zip_to_io)?;
    Ok(cursor.into_inner())
}

/// Reads a ZIP entry with a hard `max` decompression limit.
///
/// Missing or unreadable entries return `Ok(None)`. Oversized entries return
/// [`Error::EntryTooLarge`]. `take(max + 1)` enforces the bound even if the
/// declared decompressed size is false.
fn read_zip_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    max: u64,
) -> Result<Option<Vec<u8>>> {
    let Ok(f) = archive.by_name(name) else {
        return Ok(None); // Missing or unreadable entry; recovery can continue.
    };
    // The declared size is only a cheap early exit; `take` enforces the limit.
    if f.size() > max {
        return Err(Error::EntryTooLarge {
            name: name.to_string(),
            limit: max,
        });
    }
    let mut buf = Vec::new();
    if f.take(max + 1).read_to_end(&mut buf).is_err() {
        return Ok(None); // Treat a damaged stream as an unreadable entry.
    }
    if buf.len() as u64 > max {
        return Err(Error::EntryTooLarge {
            name: name.to_string(),
            limit: max,
        });
    }
    Ok(Some(buf))
}

/// Writes atomically through a sibling `path.tmp`, `fsync`, and `rename`.
/// Failures after temporary-file creation remove it on a best-effort basis and
/// preserve the previous destination.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);

    // Creation stays outside the block because a failure leaves nothing to clean.
    let mut f = std::fs::File::create(&tmp)?;

    // Keep post-creation work together so one cleanup path handles every failure.
    let result = (|| -> Result<()> {
        f.write_all(bytes)?;
        f.sync_all()?; // Ensure durability before the rename.
        drop(f);
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&tmp); // Best effort; preserve the original error.
    }
    result
}

/// Converts a `zip` crate error, which may wrap an `io::Error`, into [`Error`].
fn zip_to_io(e: zip::result::ZipError) -> Error {
    match e {
        zip::result::ZipError::Io(io) => Error::Io(io),
        other => Error::Io(std::io::Error::other(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::entity::{
        ArcGeo, CircleGeo, Color, EllipseGeo, EntityGeometry, EntityRecord, LineGeo, LineTypeRef,
        Lineweight,
    };
    use af_model::id::{EntityId, ObjectId};
    use af_model::units::Units;
    use af_model::{ContainerRef, Group, Session, TxError};

    /// Document with an extra layer and awkward coordinates to exercise exact
    /// f64 roundtrips through the public transaction API.
    fn sample_doc() -> Document {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        session
            .transact("seed", |tx| -> std::result::Result<(), TxError> {
                let rec = EntityRecord::new(
                    ObjectId::NIL.into(),
                    l0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.1, -3.333_333_333_333_333),
                        Point2::new(1e-9, 42.000_000_000_000_01),
                    )),
                );
                tx.add_entity(ContainerRef::ModelSpace, rec)?;
                Ok(())
            })
            .expect("commit");
        session.document().clone()
    }

    #[test]
    fn roundtrip_bytes_identical_and_equal_doc() {
        let doc = sample_doc();
        let bytes = to_bytes(&doc).unwrap();
        let (loaded, report) = load_bytes(&bytes).unwrap();
        assert_eq!(report.recovery, Recovery::Normal);
        assert!(report.issues.is_empty(), "doc limpio no produce issues");
        assert_eq!(loaded, doc, "load(save(doc)) == doc");

        // Canonical document.json produces identical bytes when serialized again.
        let dj1 = serde_json::to_vec(&DocFileRef {
            format_version: FORMAT_VERSION,
            document: &doc,
        })
        .unwrap();
        let dj2 = serde_json::to_vec(&DocFileRef {
            format_version: FORMAT_VERSION,
            document: &loaded,
        })
        .unwrap();
        assert_eq!(dj1, dj2, "document.json byte-estable tras roundtrip");
    }

    #[test]
    fn circular_shapes_roundtrip_exactly_and_reopen_with_clean_history() {
        let geometries = [
            EntityGeometry::Circle(CircleGeo::new(Point2::new(1.0, 2.0), 4.0)),
            EntityGeometry::Arc(ArcGeo::new(
                Point2::new(10.0, 0.0),
                2.0,
                0.0,
                core::f64::consts::FRAC_PI_2,
            )),
            EntityGeometry::Ellipse(EllipseGeo::new(
                Point2::new(20.0, 5.0),
                6.0,
                0.5,
                0.0,
                0.25,
                2.5,
            )),
        ];
        let mut session = Session::new(Units::default());
        let layer = session.document().current_layer();
        let ids = session
            .transact(
                "circular shapes",
                |tx| -> std::result::Result<Vec<EntityId>, TxError> {
                    let mut ids = Vec::new();
                    for geometry in &geometries {
                        ids.push(tx.add_entity(
                            ContainerRef::ModelSpace,
                            EntityRecord::new(
                                ObjectId::NIL.into(),
                                layer,
                                Color::ByLayer,
                                LineTypeRef::ByLayer,
                                Lineweight::ByLayer,
                                geometry.clone(),
                            ),
                        )?);
                    }
                    Ok(ids)
                },
            )
            .unwrap()
            .value;
        let expected = session.document().clone();
        let expected_next_id = expected.next_object_id();
        let mut validated = expected.clone();
        assert!(validated.validate_full().is_empty());
        assert_eq!(validated, expected);

        let bytes = to_bytes(&expected).unwrap();
        let (loaded, report) = load_bytes(&bytes).unwrap();
        assert_eq!(report.recovery, Recovery::Normal);
        assert!(report.issues.is_empty());
        assert_eq!(loaded, expected);
        assert_eq!(to_bytes(&loaded).unwrap(), bytes);
        for (&id, geometry) in ids.iter().zip(&geometries) {
            assert_eq!(loaded.entity(id).unwrap().0.geometry, *geometry);
        }

        let mut corrupt_file = serde_json::to_value(DocFileRef {
            format_version: FORMAT_VERSION,
            document: &expected,
        })
        .unwrap();
        let ellipse = corrupt_file["document"]["modelSpace"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|record| record["id"].as_u64() == Some(ids[2].raw().0))
            .unwrap();
        assert_eq!(ellipse["geometry"]["type"], "ellipse");
        ellipse["geometry"]["ratio"] = serde_json::json!(1.25);
        let corrupt = pack_arcf(
            &serde_json::to_vec(&Manifest::for_document(&expected)).unwrap(),
            &serde_json::to_vec(&corrupt_file).unwrap(),
        )
        .unwrap();
        let (repaired, report) = load_bytes(&corrupt).unwrap();
        assert!(repaired.entity(ids[2]).is_none());
        assert!(report.issues.iter().any(|issue| {
            issue.severity == Severity::Repaired && issue.object == Some(ids[2].raw())
        }));

        let mut reopened = Session::from_document(loaded);
        assert_eq!(reopened.history().undo_depth(), 0);
        assert_eq!(reopened.history().redo_depth(), 0);
        let layer = reopened.document().current_layer();
        let next = reopened
            .transact("continued circle", |tx| {
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        layer,
                        Color::ByLayer,
                        LineTypeRef::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Circle(CircleGeo::new(Point2::new(100.0, 100.0), 1.0)),
                    ),
                )
            })
            .unwrap();
        assert_eq!(next.value.raw().0, expected_next_id);
        assert_eq!(next.transaction.unwrap().seq(), 0);
    }

    #[test]
    fn save_load_via_file_roundtrips() {
        let doc = sample_doc();
        let path = tmp_path("roundtrip.arcf");
        let report = save(&doc, &path).unwrap();
        assert!(report.size_bytes > 0);
        // The rename consumes the temporary file and leaves no residue.
        assert!(!with_suffix(&path, ".tmp").exists());

        let (loaded, _) = load(&path).unwrap();
        assert_eq!(loaded, doc);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_close_load_roundtrips_commit_undo_redo_checkpoints() {
        let mut session = Session::new(Units::default());
        let add_line = |session: &mut Session, x: f64| {
            let layer = session.document().current_layer();
            let rec = EntityRecord::new(
                ObjectId::NIL.into(),
                layer,
                Color::ByLayer,
                LineTypeRef::ByLayer,
                Lineweight::ByLayer,
                EntityGeometry::Line(LineGeo::new(Point2::new(x, 0.0), Point2::new(x + 1.0, 1.0))),
            );
            session
                .transact("line", |tx| -> std::result::Result<EntityId, TxError> {
                    tx.add_entity(ContainerRef::ModelSpace, rec)
                })
                .unwrap()
        };

        let first = add_line(&mut session, 0.0).value;
        let second = add_line(&mut session, 2.0).value;
        let members = vec![second, first, second];
        let group_id = session
            .transact("ordered group", |tx| {
                tx.add_group_raw(
                    Group::new(ObjectId::NIL.into(), "checkpoint").with_members(members.clone()),
                )
            })
            .unwrap()
            .value;
        add_line(&mut session, 4.0);
        let commit = session.document().clone();
        let commit_path = tmp_path("checkpoint-commit.arcf");
        save(&commit, &commit_path).unwrap();

        session.undo().unwrap();
        let undo = session.document().clone();
        let undo_path = tmp_path("checkpoint-undo.arcf");
        save(&undo, &undo_path).unwrap();

        session.redo().unwrap();
        let redo = session.document().clone();
        let redo_path = tmp_path("checkpoint-redo.arcf");
        save(&redo, &redo_path).unwrap();
        assert_ne!(commit, undo);
        assert_eq!(redo, commit);
        for checkpoint in [&commit, &undo, &redo] {
            assert_eq!(checkpoint.group(group_id).unwrap().members(), members);
        }
        drop(session);

        for (label, path, expected) in [
            ("commit", commit_path, commit),
            ("undo", undo_path, undo),
            ("redo", redo_path, redo),
        ] {
            let (loaded, report) = load(&path).unwrap();
            assert_eq!(report.recovery, Recovery::Normal, "{label}");
            assert!(report.issues.is_empty(), "{label}: {:?}", report.issues);
            assert_eq!(loaded, expected, "{label}");
            assert_eq!(
                loaded.group(group_id).unwrap().members(),
                members,
                "{label} ordered group membership"
            );
            assert_eq!(
                loaded.next_object_id(),
                expected.next_object_id(),
                "{label} nextObjectId"
            );

            let expected_id = loaded.next_object_id();
            let mut reopened = Session::from_document(loaded);
            assert_eq!(reopened.history().undo_depth(), 0, "{label}");
            assert_eq!(reopened.history().redo_depth(), 0, "{label}");
            let first = add_line(&mut reopened, 10.0);
            assert_eq!(first.value.raw().0, expected_id, "{label}");
            assert_eq!(first.transaction.unwrap().seq(), 0, "{label}");
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn atomic_write_leaves_previous_file_intact_on_crash_before_rename() {
        // Save a valid initial document.
        let doc_a = Document::new(Units::default());
        let path = tmp_path("atomic.arcf");
        save(&doc_a, &path).unwrap();

        // Simulate a crash between writing the temporary file and renaming it.
        let tmp = with_suffix(&path, ".tmp");
        std::fs::write(&tmp, b"garbage half-written file").unwrap();

        // The original document remains intact and readable.
        let (loaded, _) = load(&path).unwrap();
        assert_eq!(
            loaded, doc_a,
            "el archivo previo queda intacto tras el fallo"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn newer_version_is_rejected_without_touching_anything() {
        // Build a document.json that declares a future version.
        let doc = Document::new(Units::default());
        let doc_val = serde_json::to_value(&doc).unwrap();
        let document_json = serde_json::to_vec(&serde_json::json!({
            "formatVersion": FORMAT_VERSION + 1,
            "document": doc_val,
        }))
        .unwrap();
        let manifest_json = serde_json::to_vec(&Manifest::for_document(&doc)).unwrap();
        let bytes = pack_arcf(&manifest_json, &document_json).unwrap();

        match load_bytes(&bytes) {
            Err(Error::NewerVersion { found, supported }) => {
                assert_eq!(found, FORMAT_VERSION + 1);
                assert_eq!(supported, FORMAT_VERSION);
            }
            other => panic!("esperaba NewerVersion, fue {other:?}"),
        }
    }

    #[test]
    fn recovery_level_2_manifest_missing_document_present() {
        // An .arcf with only document.json is still recoverable.
        let doc = sample_doc();
        let document_json = serde_json::to_vec(&DocFileRef {
            format_version: FORMAT_VERSION,
            document: &doc,
        })
        .unwrap();
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file(DOCUMENT_NAME, SimpleFileOptions::default())
            .unwrap();
        zw.write_all(&document_json).unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let (loaded, report) = load_bytes(&bytes).unwrap();
        assert_eq!(report.recovery, Recovery::Recovered);
        assert!(!report.warnings.is_empty());
        assert_eq!(loaded, doc);
    }

    /// An oversized manifest must not prevent recovery when document.json is
    /// intact. Hard size failures apply only to the required document entry.
    #[test]
    fn oversized_manifest_degrades_to_recovered_not_error() {
        let doc = sample_doc();
        let document_json = serde_json::to_vec(&DocFileRef {
            format_version: FORMAT_VERSION,
            document: &doc,
        })
        .unwrap();
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        // Compressible content exceeds 1 MiB while keeping the fixture small.
        zw.start_file(
            MANIFEST_NAME,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zw.write_all(&vec![b' '; (MAX_MANIFEST_BYTES + 1024) as usize])
            .unwrap();
        zw.start_file(DOCUMENT_NAME, SimpleFileOptions::default())
            .unwrap();
        zw.write_all(&document_json).unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let (loaded, report) = load_bytes(&bytes).expect("document íntegro: carga recuperada");
        assert_eq!(report.recovery, Recovery::Recovered);
        assert!(!report.warnings.is_empty());
        assert_eq!(loaded, doc);
    }

    #[test]
    fn recovery_level_3_manifest_only_document_missing() {
        // A manifest without document.json returns a diagnostic error.
        let doc = Document::new(Units::default());
        let manifest_json = serde_json::to_vec(&Manifest::for_document(&doc)).unwrap();
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file(MANIFEST_NAME, SimpleFileOptions::default())
            .unwrap();
        zw.write_all(&manifest_json).unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        match load_bytes(&bytes) {
            Err(Error::DocumentUnreadable { container }) => {
                assert!(
                    container.contains("id "),
                    "diagnóstico del manifest: {container}"
                );
            }
            other => panic!("esperaba DocumentUnreadable, fue {other:?}"),
        }
    }

    #[test]
    fn recovery_level_4_garbage_is_not_arcf() {
        assert!(matches!(
            load_bytes(b"this is definitely not a zip file"),
            Err(Error::NotArcf)
        ));
    }

    #[test]
    fn truncated_arcf_errors_without_panic() {
        let doc = sample_doc();
        let bytes = to_bytes(&doc).unwrap();
        // Truncating the ZIP must return an error, not panic.
        let half = &bytes[..bytes.len() / 2];
        assert!(load_bytes(half).is_err());
    }

    /// A layer whose default `line_type` points to a missing style can only come
    /// from corrupt disk data. Loading repairs and reports it without panicking.
    #[test]
    fn corrupt_layer_line_type_is_repaired_on_load_not_panic() {
        // Start with a valid document and corrupt layer 0's lineType.
        let doc = Document::new(Units::default());
        let mut doc_val = serde_json::to_value(&doc).unwrap();
        let layers = doc_val
            .get_mut("layers")
            .and_then(Value::as_object_mut)
            .expect("layers es un objeto");
        for (_id, layer) in layers.iter_mut() {
            layer["lineType"] = serde_json::json!(9_999_999u64); // Missing style.
        }
        let document_json = serde_json::to_vec(&serde_json::json!({
            "formatVersion": FORMAT_VERSION,
            "document": doc_val,
        }))
        .unwrap();
        let manifest_json = serde_json::to_vec(&Manifest::for_document(&doc)).unwrap();
        let bytes = pack_arcf(&manifest_json, &document_json).unwrap();

        let (loaded, report) = load_bytes(&bytes).expect("se carga reparando, sin error");
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.severity == Severity::Repaired),
            "esperaba una reparación reportada: {:?}",
            report.issues
        );
        // Every repaired layer references an existing line type.
        for layer in loaded.layers() {
            assert!(
                loaded.line_type(layer.line_type()).is_some(),
                "capa {} quedó con lineType colgante",
                layer.name()
            );
        }
    }

    #[test]
    fn migrate_current_version_is_noop() {
        let v = serde_json::json!({"anything": 1});
        assert_eq!(migrate(v.clone(), FORMAT_VERSION).unwrap(), v);
    }

    /// A document entry above the decompression limit returns a typed error
    /// without allocating its full expanded size. Repetitive content keeps this
    /// test fixture small and fast.
    #[test]
    fn decompression_bomb_document_is_rejected_without_oom() {
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file(
            DOCUMENT_NAME,
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(1)), // Fast level keeps the debug test under two seconds.
        )
        .unwrap();
        // Write in 1 MiB chunks until the limit is exceeded.
        let chunk = vec![b' '; 1024 * 1024];
        let target = MAX_DOCUMENT_BYTES + 1024;
        let mut written: u64 = 0;
        while written < target {
            let n = (chunk.len() as u64).min(target - written) as usize;
            zw.write_all(&chunk[..n]).unwrap();
            written += n as u64;
        }
        let bytes = zw.finish().unwrap().into_inner();
        // The compressed container remains tiny compared with its expanded size.
        assert!(
            (bytes.len() as u64) < MAX_DOCUMENT_BYTES / 16,
            "el .arcf bomba debe pesar poco: {} bytes",
            bytes.len()
        );

        match load_bytes(&bytes) {
            Err(Error::EntryTooLarge { name, limit }) => {
                assert_eq!(name, DOCUMENT_NAME);
                assert_eq!(limit, MAX_DOCUMENT_BYTES);
            }
            other => panic!("esperaba EntryTooLarge, fue {other:?}"),
        }
    }

    // Lightweight loader fuzzing: arbitrary bytes must return without panic,
    // out-of-memory allocation, or an infinite loop.
    proptest::proptest! {
        #[test]
        fn loader_never_panics_on_arbitrary_bytes(
            bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)
        ) {
            let _ = load_bytes(&bytes);
        }

        // Random bytes rarely form a valid ZIP, so mutate a valid `.arcf` to cover
        // decompression, deserialization, and validation paths as well.
        #[test]
        fn loader_never_panics_on_mutated_valid_arcf(
            flips in proptest::collection::vec(
                (proptest::prelude::any::<proptest::sample::Index>(), proptest::prelude::any::<u8>()),
                0..64,
            ),
            insert_at in proptest::prelude::any::<proptest::sample::Index>(),
            insert in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..48),
            cut in proptest::prelude::any::<proptest::sample::Index>(),
        ) {
            let mut m = to_bytes(&sample_doc()).unwrap();
            for (idx, b) in &flips {
                if !m.is_empty() {
                    let i = idx.index(m.len());
                    m[i] = *b;
                }
            }
            // Splice arbitrary data to break ZIP offsets and CRC values.
            let at = insert_at.index(m.len() + 1);
            m.splice(at..at, insert.iter().copied());
            // Truncate to an arbitrary length.
            let to = cut.index(m.len() + 1);
            m.truncate(to);
            let _ = load_bytes(&m);
        }
    }

    // --- Temporary-file helpers (standard library only) ----------------------

    fn tmp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("arcf-{}-{}-{}", std::process::id(), nanos, name))
    }

    fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
        let mut s = path.as_os_str().to_owned();
        s.push(suffix);
        PathBuf::from(s)
    }
}
