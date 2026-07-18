//! [`ApiSession`], the stateful facade that owns the model session, spatial index,
//! selection, cached render model, and command registry.
//!
//! # Synchronization
//!
//! After each `execute`, the index, selection, and render model consume the same
//! committed `ChangeSet`. [`sync_incremental`](ApiSession::sync_incremental) updates
//! only delta IDs and prunes stale selection IDs, keeping all observers aligned
//! with the same post-transaction document.
//!
//! Full [`resync`](ApiSession::resync) is reserved for operations without an exposed
//! `ChangeSet`, such as [`open`](ApiSession::open) and
//! [`import_dxf`](ApiSession::import_dxf). Unit tests verify incremental state
//! against a clean rebuild through randomized execute, undo, and redo sequences.

use std::collections::{HashSet, VecDeque};
use std::io::Cursor;

use serde::Serialize;
use serde_json::Value;

use af_cmd::{CommandRegistry, ParamType, builtin, parse_pgp};
use af_geom::flatten::{flatten_arc, flatten_circle, flatten_ellipse};
use af_math::{BBox, Point2};
use af_model::container::ContainerRef;
use af_model::entity::{
    Color, EntityGeometry, LineTypeRef, Lineweight, PolylineGeo, SegKind, SnapKind,
};
use af_model::extents::{ExtentsFilter, doc_extents};
use af_model::id::{EntityId, LayerId, ObjectId, StyleId};
use af_model::units::{LinearUnit, Units};
use af_model::{ChangeSet, Session, SysvarValue, Transaction};
use af_render::{BatchKey, PrimGeom, RenderBatch, RenderModel, RenderOpts, build_full};
use af_select::{
    EntityKind, SelectionFilter, SelectionState, SnapMask, SnapOpts, SpatialIndex, WindowMode,
    apply_filter, pick, pick_all, select_fence, select_polygon, select_similar, select_window,
    snap,
};

use crate::dto::{
    ApiEvent, BatchKeyView, BatchView, ColorView, CommandInfo, DocInfo, DxfReport, EntityProps,
    ExecuteResult, GroupInfo, HitView, LayerInfo, LineTypeRefView, LineweightView, MarkerView,
    ParamInfo, ParsedPoint, PreviewView, RenderDeltaView, RenderView, SelectionFilterView,
    SnapView, StripView, SysvarValueView,
};
use crate::error::ApiError;

/// Default curve-flattening chord tolerance in world units.
// ponytail: fixed until the camera supplies a per-frame tolerance.
const DEFAULT_CHORD_ERR: f64 = 0.1;

/// The renderable and indexable model-space container.
const CONTAINER: ContainerRef = ContainerRef::ModelSpace;

fn pgp_admin_string_arg<'a>(
    args: &'a Value,
    key: &str,
    operation: &str,
) -> Result<&'a str, ApiError> {
    args.as_object()
        .filter(|fields| fields.len() == 1)
        .and_then(|fields| fields.get(key))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::new(
                "invalid_args",
                format!("{operation} requires exactly one string field '{key}'"),
            )
        })
}

/// Stateful facade used by the UI and plugins.
pub struct ApiSession {
    registry: CommandRegistry,
    session: Session,
    index: SpatialIndex,
    selection: SelectionState,
    /// Current render model, updated after every mutation.
    render: RenderModel,
    /// Model last observed by the consumer, used as the `render_delta` base.
    render_seen: RenderModel,
    chord_err: f64,
    /// Drainable event queue.
    events: VecDeque<ApiEvent>,
}

impl ApiSession {
    /// Creates a session with an empty document and registered built-in commands.
    #[must_use]
    pub fn new(units: Units) -> Self {
        let session = Session::new(units);
        let mut registry = CommandRegistry::new();
        // Built-in name collisions are programming errors, not runtime state.
        builtin::register_builtins(&mut registry)
            .expect("builtin command registration is infallible");
        let render = build_full(session.document(), &RenderOpts::new(DEFAULT_CHORD_ERR));
        let index = SpatialIndex::build(session.document(), CONTAINER);
        Self {
            registry,
            session,
            index,
            selection: SelectionState::new(),
            render_seen: render.clone(),
            render,
            chord_err: DEFAULT_CHORD_ERR,
            events: VecDeque::new(),
        }
    }

    // ---------------------------------------------------------------- document

    /// Opens `.arcf` bytes, replaces the current document, and returns recovery warnings.
    ///
    /// # Errors
    /// Returns `io_error` for unreadable, unsupported, or unrecoverable documents.
    pub fn open(&mut self, bytes: &[u8]) -> Result<Vec<String>, ApiError> {
        let (doc, report) = af_io_native::load_bytes(bytes)?;
        self.session = Session::from_document(doc);
        self.selection.clear();
        self.resync();
        // A replaced document requires a full consumer render refresh.
        self.render_seen = RenderModel::default();
        Ok(report.warnings)
    }

    /// Serializes the current document to `.arcf` bytes.
    ///
    /// # Errors
    /// Returns `io_error` when serialization fails.
    pub fn save(&self) -> Result<Vec<u8>, ApiError> {
        Ok(af_io_native::to_bytes(self.session.document())?)
    }

    // ---------------------------------------------------------------- commands

    /// Executes a named command with JSON arguments. On success, it incrementally
    /// synchronizes the index and render model, prunes selection, and queues events.
    ///
    /// # Errors
    /// Maps command validation and execution failures to [`ApiError`]. Failed
    /// commands leave the document unchanged and queue no events.
    pub fn execute(&mut self, name: &str, args: &Value) -> Result<ExecuteResult, ApiError> {
        match name {
            "__ARCFORGE_PGP_REINIT" => return self.reinitialize_pgp(args),
            "__ARCFORGE_PGP_RESOLVE" => return self.resolve_pgp(args),
            _ => {}
        }

        let outcome = self.registry.execute(&mut self.session, name, args)?;
        let before_sel = self.selection.items();
        self.sync_incremental(&outcome.change_sets);

        let created: Vec<u64> = outcome.created.iter().map(|e| e.raw().0).collect();
        self.events.push_back(ApiEvent::CommandExecuted {
            name: name.to_string(),
            tx_seq: outcome.tx_seq,
            created: created.clone(),
        });
        self.emit_selection_if_changed(before_sel);

        Ok(ExecuteResult {
            tx_seq: outcome.tx_seq,
            created,
            message: outcome.message,
        })
    }

    /// Replaces the PGP table without touching document, history, render,
    /// selection, or events. The existing JSON envelope avoids a new ABI export.
    fn reinitialize_pgp(&mut self, args: &Value) -> Result<ExecuteResult, ApiError> {
        let content = pgp_admin_string_arg(args, "pgp", "__ARCFORGE_PGP_REINIT")?;
        let parsed = parse_pgp(content);
        let mut warnings = parsed.warnings;
        warnings.extend(self.registry.replace_user_aliases(parsed.aliases));

        let count = self.registry.user_alias_count();
        let mut message = format!("PGP: {count} alias(es) activo(s)");
        if !warnings.is_empty() {
            message.push_str("\nwarnings:\n");
            message.push_str(&warnings.join("\n"));
        }
        Ok(ExecuteResult {
            tx_seq: None,
            created: Vec::new(),
            message: Some(message),
        })
    }

    /// Resolves a token to its canonical command name with registry precedence.
    /// Unknown tokens return `message = None`, also without side effects.
    fn resolve_pgp(&self, args: &Value) -> Result<ExecuteResult, ApiError> {
        let token = pgp_admin_string_arg(args, "token", "__ARCFORGE_PGP_RESOLVE")?;
        if token.trim().is_empty() {
            return Err(ApiError::new(
                "invalid_args",
                "__ARCFORGE_PGP_RESOLVE requires a non-empty 'token'",
            ));
        }
        Ok(ExecuteResult {
            tx_seq: None,
            created: Vec::new(),
            message: self
                .registry
                .resolve_canonical_name(token)
                .map(str::to_owned),
        })
    }

    /// Previews a modifying command without a transaction or document change.
    /// The same planning phase powers preview and [`execute`](Self::execute).
    ///
    /// It does not change index, render, selection, or events.
    ///
    /// # Errors
    /// Returns `not_previewable` for unsupported commands and the same validation
    /// errors as `execute`.
    pub fn preview(&self, name: &str, args: &Value) -> Result<PreviewView, ApiError> {
        let geoms = self.registry.preview(&self.session, name, args)?;
        let polylines = geoms
            .iter()
            .map(|g| flatten_preview(g, self.chord_err))
            .collect();
        Ok(PreviewView { polylines })
    }

    /// Panic-safe string entry point for FFI and scripting. It parses `args_json`
    /// and returns a structured `ResultJson` envelope.
    #[must_use]
    pub fn execute_json(&mut self, name: &str, args_json: &str) -> String {
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let args: Value = if args_json.trim().is_empty() {
                Value::Null
            } else {
                serde_json::from_str(args_json)?
            };
            self.execute(name, &args)
        }));
        match caught {
            Ok(res) => result_json(&res),
            Err(_) => error_json(&ApiError::new(
                "panic",
                "internal panic caught at API boundary; reload the document",
            )),
        }
    }

    /// Returns all registered commands and their metadata.
    #[must_use]
    pub fn list_commands(&self) -> Vec<CommandInfo> {
        self.registry
            .commands()
            .iter()
            .map(|spec| CommandInfo {
                name: spec.name().to_string(),
                aliases: spec.aliases().to_vec(),
                label: spec.label().to_string(),
                affects_document: spec.affects_document(),
                params: spec
                    .params()
                    .iter()
                    .map(|p| ParamInfo {
                        name: p.name.clone(),
                        ty: p.ty.name().to_string(),
                        optional: p.optional,
                    })
                    .collect(),
            })
            .collect()
    }

    // ------------------------------------------------------------- properties

    /// Changes common properties for a set of entities in one labeled transaction.
    /// Present fields apply to every entity; absent fields remain unchanged.
    ///
    /// The update is atomic. Unknown IDs or references roll back every change.
    /// Effective changes commit exactly one transaction; no-op updates create none.
    ///
    /// # Errors
    /// Returns typed errors for unknown entities, layers, line types, or invalid ACI.
    pub fn set_entity_props(
        &mut self,
        ids: &[u64],
        props: &EntityProps,
    ) -> Result<ExecuteResult, ApiError> {
        // Validate value shape before the transaction; the transaction validates
        // entity, layer, and style references atomically.
        let color = props.color.map(color_from_view).transpose()?;
        let layer = props.layer.map(|n| LayerId::from(ObjectId(n)));
        let line_type = props.line_type.map(line_type_from_view);
        let lineweight = props.lineweight.map(lineweight_from_view);
        let visible = props.visible;

        let entity_ids: Vec<EntityId> = ids.iter().map(|&n| EntityId::from(ObjectId(n))).collect();
        let before_sel = self.selection.items();

        let outcome =
            self.session
                .transact("Cambiar propiedades", |tx| -> Result<(), ApiError> {
                    for &id in &entity_ids {
                        tx.modify_entity(id, |rec| {
                            if let Some(l) = layer {
                                rec.layer = l;
                            }
                            if let Some(c) = color {
                                rec.color = c;
                            }
                            if let Some(lt) = line_type {
                                rec.line_type = lt;
                            }
                            if let Some(w) = lineweight {
                                rec.lineweight = w;
                            }
                            if let Some(v) = visible {
                                rec.visible = v;
                            }
                        })?;
                    }
                    Ok(())
                })?;

        let tx_seq = outcome.transaction.as_ref().map(Transaction::seq);
        let change_sets: Vec<ChangeSet> = outcome.change_set.into_iter().collect();
        self.sync_incremental(&change_sets);
        // Queue a document-change event only when a transaction committed.
        if tx_seq.is_some() {
            self.events.push_back(ApiEvent::CommandExecuted {
                name: "SETPROPS".to_string(),
                tx_seq,
                created: Vec::new(),
            });
        }
        self.emit_selection_if_changed(before_sel);

        Ok(ExecuteResult {
            tx_seq,
            created: Vec::new(),
            message: None,
        })
    }

    // ----------------------------------------------------------------- system variables

    /// Returns system variable `name`, ignoring case.
    ///
    /// # Errors
    /// Returns `unknown_sysvar` when the name is not registered.
    pub fn get_sysvar(&self, name: &str) -> Result<SysvarValueView, ApiError> {
        // Commands and facade access share the session's single system-variable table.
        self.session
            .sysvar(name)
            .map(sysvar_value_view)
            .ok_or_else(|| ApiError::from(af_model::SysvarError::Unknown(name.to_string())))
    }

    /// Sets a system variable after model type and range validation.
    ///
    /// # Errors
    /// Returns `unknown_sysvar`, `type_mismatch`, or `out_of_range`. Failures retain
    /// the previous value.
    pub fn set_sysvar(&mut self, name: &str, value: SysvarValueView) -> Result<(), ApiError> {
        // Delegate to the session table and preserve event semantics.
        self.session
            .set_sysvar(name, sysvar_value_from_view(value))?;
        // Events use the canonical uppercase name.
        let canonical = self
            .session
            .sysvar_def(name)
            .map_or_else(|| name.to_string(), |d| d.name.to_string());
        self.events.push_back(ApiEvent::SysvarChanged {
            name: canonical,
            value,
        });
        Ok(())
    }

    // ---------------------------------------------------------------- DXF

    /// Imports DXF bytes into the current document in one transaction.
    ///
    /// # Errors
    /// Returns `dxf_error` for unreadable or oversized files. Unsupported entities
    /// are reported as skipped.
    pub fn import_dxf(&mut self, bytes: &[u8]) -> Result<DxfReport, ApiError> {
        let report = af_io_dxf::import_dxf(
            &mut self.session,
            Cursor::new(bytes),
            af_io_dxf::ImportOptions::default(),
        )?;
        self.resync();
        // Import is undoable, but the DXF layer does not expose its transaction sequence.
        self.events.push_back(ApiEvent::CommandExecuted {
            name: "IMPORTDXF".to_string(),
            tx_seq: None,
            created: Vec::new(),
        });
        Ok(DxfReport {
            counts: report.imported,
            skipped: report.skipped,
            warnings: report.warnings,
        })
    }

    /// Exports the document as DXF R2000 bytes with a report.
    ///
    /// # Errors
    /// Returns `dxf_error` on output failure; unsupported geometry is skipped.
    pub fn export_dxf(&self) -> Result<(Vec<u8>, DxfReport), ApiError> {
        let mut buf = Vec::new();
        let report = af_io_dxf::export_dxf(
            self.session.document(),
            &mut buf,
            af_io_dxf::ExportOptions::default(),
        )?;
        Ok((
            buf,
            DxfReport {
                counts: report.exported,
                skipped: report.skipped,
                warnings: report.warnings,
            },
        ))
    }

    // ---------------------------------------------------------------- queries

    /// Returns entities hit near `(x, y)`, best candidate first, without changing selection.
    #[must_use]
    pub fn pick(&self, x: f64, y: f64, tol: f64) -> Vec<HitView> {
        pick(self.session.document(), &self.index, Point2::new(x, y), tol)
            .into_iter()
            .map(|h| HitView {
                id: h.id.raw().0,
                dist: h.dist,
                locked: h.locked,
            })
            .collect()
    }

    /// Queries entities by world-space rectangle without changing selection.
    #[must_use]
    pub fn select_window(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        crossing: bool,
    ) -> Vec<u64> {
        let rect = BBox::new(Point2::new(min_x, min_y), Point2::new(max_x, max_y));
        let mode = if crossing {
            WindowMode::Crossing
        } else {
            WindowMode::Window
        };
        select_window(self.session.document(), &self.index, rect, mode)
            .into_iter()
            .map(|id| id.raw().0)
            .collect()
    }

    /// Returns ranked snap points near `(x, y)` within `radius`.
    /// Optional JSON fields are `kinds` and `pxPerUnit`.
    #[must_use]
    pub fn snap(&self, x: f64, y: f64, radius: f64, opts: &Value) -> Vec<SnapView> {
        let opts = snap_opts_from_json(opts);
        snap(
            self.session.document(),
            &self.index,
            Point2::new(x, y),
            radius,
            opts,
        )
        .into_iter()
        .map(|s| SnapView {
            point: [s.point.x, s.point.y],
            kind: snap_kind_str(s.kind).to_string(),
            entity: s.entity.raw().0,
            dist: s.dist,
        })
        .collect()
    }

    /// Returns a document and session state snapshot.
    #[must_use]
    pub fn doc_info(&self) -> DocInfo {
        let doc = self.session.document();
        DocInfo {
            id: doc.id().as_uuid().to_string(),
            units: linear_unit_str(doc.units().linear).to_string(),
            entity_count: doc.model_space().iter_records().count(),
            layer_count: doc.layers().count(),
            current_layer: doc.current_layer().raw().0,
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
            undo_label: self.session.undo_label().map(str::to_string),
            redo_label: self.session.redo_label().map(str::to_string),
            extents: doc_extents(doc, CONTAINER, ExtentsFilter::Visible)
                .map(|b| [b.min.x, b.min.y, b.max.x, b.max.y]),
        }
    }

    /// Returns catalog layers in stable creation order.
    #[must_use]
    pub fn layers(&self) -> Vec<LayerInfo> {
        let doc = self.session.document();
        let current = doc.current_layer();
        doc.layers()
            .map(|l| LayerInfo {
                id: l.id().raw().0,
                name: l.name().to_string(),
                color: color_view(l.color()),
                line_type: l.line_type().raw().0,
                lineweight: lineweight_view(l.lineweight()),
                off: l.is_off(),
                frozen: l.is_frozen(),
                locked: l.is_locked(),
                plot: l.is_plottable(),
                current: l.id() == current,
            })
            .collect()
    }

    /// Parses command-line coordinate input into an absolute world point. Supported
    /// forms are `x,y`, `@Δx,Δy`, and `@d<a`; commas never separate decimals.
    ///
    /// Relative and polar forms require `base`; absolute input ignores it.
    ///
    /// # Errors
    /// Returns `parse_error` for malformed input and `not_a_point` for valid input
    /// that does not represent a point.
    pub fn parse_input(
        &self,
        input: &str,
        base: Option<[f64; 2]>,
    ) -> Result<ParsedPoint, ApiError> {
        let base_pt = base.map(|[x, y]| Point2::new(x, y));
        let parsed = af_cmd::parse_input(input, &ParamType::Point, base_pt)?;
        parsed
            .resolve_point(base_pt.unwrap_or(Point2::ORIGIN))
            .map(|p| ParsedPoint { point: [p.x, p.y] })
            .ok_or_else(|| {
                ApiError::new(
                    "not_a_point",
                    "la entrada no denota un punto (usa 'x,y', '@Δx,Δy' o '@d<a')",
                )
            })
    }

    // ---------------------------------------------------------------- selection

    /// Returns selected IDs in stable order.
    #[must_use]
    pub fn selection(&self) -> Vec<u64> {
        self.selection.items().iter().map(|e| e.raw().0).collect()
    }

    /// Replaces selection, expands selectable groups, and queues an event if changed.
    pub fn set_selection(&mut self, ids: &[u64]) {
        let ids: Vec<EntityId> = ids.iter().map(|&n| EntityId::from(ObjectId(n))).collect();
        self.commit_selection(ids);
    }

    /// Clears selection and stores the previous live selection when it changes.
    pub fn clear_selection(&mut self) {
        self.commit_selection(Vec::new());
    }

    /// Selects by arbitrary polygon. Crossing mode accepts contained or intersecting
    /// entities; window mode accepts fully contained entities.
    pub fn select_polygon(&mut self, points: &[[f64; 2]], crossing: bool) -> Vec<u64> {
        let poly: Vec<Point2> = points.iter().map(|&[x, y]| Point2::new(x, y)).collect();
        let mode = if crossing {
            WindowMode::Crossing
        } else {
            WindowMode::Window
        };
        let ids = select_polygon(self.session.document(), &self.index, &poly, mode);
        self.commit_selection(ids);
        self.selection()
    }

    /// Selects entities crossing any segment of an open fence polyline.
    pub fn select_fence(&mut self, points: &[[f64; 2]]) -> Vec<u64> {
        let fence: Vec<Point2> = points.iter().map(|&[x, y]| Point2::new(x, y)).collect();
        let ids = select_fence(self.session.document(), &self.index, &fence);
        self.commit_selection(ids);
        self.selection()
    }

    /// Filters model space by type, layer, and color, then replaces selection.
    ///
    /// # Errors
    /// Returns `aci_out_of_range` for invalid filter colors without changing state.
    pub fn select_filter(&mut self, filter: &SelectionFilterView) -> Result<Vec<u64>, ApiError> {
        let filter = selection_filter_from_view(filter)?;
        let ids: Vec<EntityId> = {
            let doc = self.session.document();
            apply_filter(doc.model_space().iter_records().map(|r| r.id), doc, &filter)
        };
        self.commit_selection(ids);
        Ok(self.selection())
    }

    /// Selects entities similar to `id` by type, layer, and explicit color.
    pub fn select_similar(&mut self, id: u64) -> Vec<u64> {
        let ids = select_similar(self.session.document(), EntityId::from(ObjectId(id)));
        self.commit_selection(ids);
        self.selection()
    }

    /// Restores the previous selection and archives the current one in its place.
    pub fn select_previous(&mut self) -> Vec<u64> {
        let prev = self.selection.previous();
        self.commit_selection(prev);
        self.selection()
    }

    /// Returns every entity under `(x, y)` in deterministic cycling order without
    /// changing selection.
    #[must_use]
    pub fn pick_all(&self, x: f64, y: f64, tol: f64) -> Vec<u64> {
        pick_all(self.session.document(), &self.index, Point2::new(x, y), tol)
            .into_iter()
            .map(|id| id.raw().0)
            .collect()
    }

    // ------------------------------------------------------------------ groups

    /// Returns document groups in creation order.
    #[must_use]
    pub fn groups(&self) -> Vec<GroupInfo> {
        self.session
            .document()
            .groups()
            .map(|g| GroupInfo {
                id: g.id().raw().0,
                name: g.name().to_string(),
                members: g.members().iter().map(|e| e.raw().0).collect(),
                selectable: g.is_selectable(),
            })
            .collect()
    }

    // ---------------------------------------------------------------- render

    /// Returns the complete render model with control batches and packed vertices.
    ///
    /// Marks the model as observed so later deltas start from this state.
    pub fn render_full(&mut self) -> RenderView {
        self.render_seen = self.render.clone();
        view_from_model(&self.render)
    }

    /// Returns the render delta since the previous full or delta observation.
    ///
    /// The delta is a diff of cached models: applying it to the observed model
    /// reproduces the current model.
    pub fn render_delta(&mut self) -> RenderDeltaView {
        let delta = diff_models(&self.render_seen, &self.render);
        self.render_seen = self.render.clone();
        delta
    }

    /// Returns packed vertices for [`render_full`](Self::render_full) in deterministic order.
    #[must_use]
    pub fn render_vertices(&self) -> Vec<f32> {
        view_from_model(&self.render).vertices
    }

    /// Sets curve-flattening chord tolerance and rebuilds the render model.
    pub fn set_chord_err(&mut self, chord_err: f64) {
        self.chord_err = chord_err.max(f64::MIN_POSITIVE);
        self.render = build_full(self.session.document(), &RenderOpts::new(self.chord_err));
    }

    // ---------------------------------------------------------------- events

    /// Drains events accumulated since the previous call.
    pub fn poll_events(&mut self) -> Vec<ApiEvent> {
        self.events.drain(..).collect()
    }

    // ---------------------------------------------------------------- internals

    /// Incrementally applies execution changes to index and render, then prunes selection.
    ///
    /// Normal commands produce one `ChangeSet`; the loop also accepts macro commands.
    // ponytail: each change set uses the final document; multi-step commands would
    // need an intermediate document for each step.
    fn sync_incremental(&mut self, change_sets: &[ChangeSet]) {
        let opts = RenderOpts::new(self.chord_err);
        for cs in change_sets {
            self.index.apply_changeset(cs, self.session.document());
            let delta =
                af_render::apply_changeset(&self.render, cs, self.session.document(), &opts);
            self.render = self.render.apply_delta(&delta);
        }
        self.selection.retain_existing(self.session.document());
    }

    /// Fully rebuilds index and render when no `ChangeSet` is available, then prunes selection.
    fn resync(&mut self) {
        self.index = SpatialIndex::build(self.session.document(), CONTAINER);
        self.render = build_full(self.session.document(), &RenderOpts::new(self.chord_err));
        self.selection.retain_existing(self.session.document());
    }

    /// Replaces live selection, expanding selectable groups, archiving the previous
    /// selection, and queuing `SelectionChanged` when needed.
    ///
    /// All facade selection mutations pass through this method, keeping group policy
    /// consistent while af-select remains a read-only query layer.
    fn commit_selection(&mut self, ids: Vec<EntityId>) {
        let before = self.selection.items();
        let expanded = self.expand_groups(ids);
        // "Previous" is the live selection immediately before this change.
        self.selection.set_previous(before.iter().copied());
        self.selection.set(expanded);
        self.emit_selection_if_changed(before);
    }

    /// Expands selectable group members while preserving arrival order and deduplicating.
    // ponytail: O(ids × groups); add an entity-to-group index only if scale requires it.
    fn expand_groups(&self, ids: Vec<EntityId>) -> Vec<EntityId> {
        let doc = self.session.document();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let mut expanded = false;
            for g in doc.groups() {
                if g.is_selectable() && g.contains(id) {
                    out.extend(g.members().iter().copied());
                    expanded = true;
                }
            }
            if !expanded {
                out.push(id);
            }
        }
        out
    }

    /// Queues `SelectionChanged` when selection differs from `before`.
    fn emit_selection_if_changed(&mut self, before: Vec<EntityId>) {
        let after = self.selection.items();
        if before != after {
            self.events.push_back(ApiEvent::SelectionChanged {
                ids: after.iter().map(|e| e.raw().0).collect(),
            });
        }
    }
}

// ============================ free helpers ============================

/// Packs batch primitives into `(x, y)` vertices and returns relative offsets.
fn push_batch(b: &RenderBatch, verts: &mut Vec<f32>) -> BatchView {
    let c = b.key.color;
    let mut strips = Vec::new();
    let mut markers = Vec::new();
    for prim in &b.prims {
        match &prim.geom {
            PrimGeom::PolylineStrip {
                points,
                width_class,
                poly_width,
                analytic_length,
            } => {
                let offset = (verts.len() / 2) as u32;
                for p in points {
                    verts.push(p.x as f32);
                    verts.push(p.y as f32);
                }
                strips.push(StripView {
                    entity: prim.entity.raw().0,
                    offset,
                    count: points.len() as u32,
                    width: width_class.0,
                    poly_width: *poly_width,
                    analytic_length: *analytic_length,
                });
            }
            PrimGeom::Marker { at, .. } => {
                markers.push(MarkerView {
                    entity: prim.entity.raw().0,
                    x: at.x as f32,
                    y: at.y as f32,
                });
            }
            // Emit a closed contour for facade previews; the native renderer owns fill.
            PrimGeom::MaskPolygon { points } => {
                let offset = (verts.len() / 2) as u32;
                for p in points {
                    verts.push(p.x as f32);
                    verts.push(p.y as f32);
                }
                let count = if let Some(first) = points.first() {
                    // Close the contour by repeating its first vertex.
                    verts.push(first.x as f32);
                    verts.push(first.y as f32);
                    points.len() as u32 + 1
                } else {
                    0
                };
                strips.push(StripView {
                    entity: prim.entity.raw().0,
                    offset,
                    count,
                    width: 0.0,
                    poly_width: 0.0,
                    analytic_length: None,
                });
            }
        }
    }
    BatchView {
        layer: b.key.layer.raw().0,
        color: [c.r, c.g, c.b, c.a],
        linetype: b.key.linetype.raw().0,
        strips,
        markers,
    }
}

/// Builds a complete [`RenderView`] from a render model.
fn view_from_model(model: &RenderModel) -> RenderView {
    let mut vertices = Vec::new();
    let batches = model
        .batches
        .iter()
        .map(|b| push_batch(b, &mut vertices))
        .collect();
    RenderView {
        batches,
        vertices,
        ltscale: model.ltscale,
    }
}

/// Diffs render models so `apply(previous, diff(previous, current)) == current`.
fn diff_models(prev: &RenderModel, cur: &RenderModel) -> RenderDeltaView {
    let mut vertices = Vec::new();
    let mut upserts = Vec::new();
    let mut cur_keys: HashSet<BatchKey> = HashSet::with_capacity(cur.batches.len());
    for b in &cur.batches {
        cur_keys.insert(b.key);
        if prev.batch(&b.key) != Some(b) {
            upserts.push(push_batch(b, &mut vertices));
        }
    }
    let removes = prev
        .batches
        .iter()
        .filter(|b| !cur_keys.contains(&b.key))
        .map(|b| BatchKeyView {
            layer: b.key.layer.raw().0,
            color: [b.key.color.r, b.key.color.g, b.key.color.b, b.key.color.a],
            linetype: b.key.linetype.raw().0,
        })
        .collect();
    RenderDeltaView {
        upserts,
        removes,
        vertices,
        // Always report current global LTSCALE, even when no batch changes.
        ltscale: cur.ltscale,
    }
}

/// Flattens preview geometry into `[x, y]` points using render chord tolerance.
// ponytail: this mirrors af-render's private polyline flattener; reuse it if exposed.
fn flatten_preview(g: &EntityGeometry, chord_err: f64) -> Vec<[f32; 2]> {
    let pts: Vec<Point2> = match g {
        EntityGeometry::Line(l) => vec![l.p1, l.p2],
        EntityGeometry::Circle(c) => flatten_circle(c.center, c.radius, chord_err),
        EntityGeometry::Arc(a) => flatten_arc(&a.arc_seg(), chord_err),
        EntityGeometry::Ellipse(e) => flatten_ellipse(&e.ellipse(), chord_err),
        EntityGeometry::Polyline(p) => flatten_preview_polyline(p, chord_err),
        EntityGeometry::Spline(s) => s
            .fit_spline()
            .map_or_else(|| s.fit_points.clone(), |sp| sp.flatten(chord_err)),
        EntityGeometry::Point(p) => vec![p.position],
        // Materialize infinite geometry as the same large segment used by af-render.
        EntityGeometry::Xline(x) => {
            let (a, b) = x.endpoints();
            vec![a, b]
        }
        EntityGeometry::Ray(r) => {
            let (a, b) = r.endpoints();
            vec![a, b]
        }
        // Preview a mask as a closed contour; the renderer handles its fill.
        EntityGeometry::Wipeout(w) => {
            let mut pts = w.points.clone();
            if let Some(&first) = w.points.first() {
                pts.push(first);
            }
            pts
        }
    };
    pts.iter().map(|p| [p.x as f32, p.y as f32]).collect()
}

/// Flattens a polyline, including bulge arcs, without duplicating shared vertices.
fn flatten_preview_polyline(g: &PolylineGeo, chord_err: f64) -> Vec<Point2> {
    let mut pts: Vec<Point2> = Vec::new();
    let mut first = true;
    for seg in g.segments() {
        match seg {
            SegKind::Line { a, b } => {
                if first {
                    pts.push(a);
                    first = false;
                }
                pts.push(b);
            }
            SegKind::Arc(arc) => {
                let ap = flatten_arc(&arc, chord_err); // [start, …, end]
                if first {
                    pts.push(ap[0]);
                    first = false;
                }
                pts.extend_from_slice(&ap[1..]);
            }
        }
    }
    if pts.is_empty() {
        pts.extend(g.vertices.iter().map(|v| v.pt));
    }
    pts
}

/// Stable snap-kind name used in JSON output.
fn snap_kind_str(kind: SnapKind) -> &'static str {
    match kind {
        SnapKind::Endpoint => "endpoint",
        SnapKind::Midpoint => "midpoint",
        SnapKind::Center => "center",
        SnapKind::Node => "node",
        SnapKind::Quadrant => "quadrant",
        SnapKind::Insertion => "insertion",
        SnapKind::Intersection => "intersection",
        SnapKind::Perpendicular => "perpendicular",
        SnapKind::Nearest => "nearest",
        SnapKind::Tangent => "tangent",
        SnapKind::Extension => "extension",
        SnapKind::GeometricCenter => "geometricCenter",
    }
}

/// Parses a snap kind name, returning `None` when unknown.
fn snap_kind_from_str(s: &str) -> Option<SnapKind> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "endpoint" => SnapKind::Endpoint,
        "midpoint" => SnapKind::Midpoint,
        "center" => SnapKind::Center,
        "node" => SnapKind::Node,
        "quadrant" => SnapKind::Quadrant,
        "insertion" => SnapKind::Insertion,
        "intersection" => SnapKind::Intersection,
        "perpendicular" => SnapKind::Perpendicular,
        "nearest" => SnapKind::Nearest,
        "tangent" => SnapKind::Tangent,
        "extension" => SnapKind::Extension,
        "geometriccenter" => SnapKind::GeometricCenter,
        _ => return None,
    })
}

/// Parses snap options, defaulting to all kinds and `pxPerUnit = 1.0`.
fn snap_opts_from_json(v: &Value) -> SnapOpts {
    let mut opts = SnapOpts::default();
    if let Some(px) = v.get("pxPerUnit").and_then(Value::as_f64) {
        opts.px_per_unit = px;
    }
    // `lastPoint` supplies the base for perpendicular and tangent snaps.
    if let Some(arr) = v.get("lastPoint").and_then(Value::as_array)
        && let (Some(x), Some(y)) = (
            arr.first().and_then(Value::as_f64),
            arr.get(1).and_then(Value::as_f64),
        )
    {
        opts.last_point = Some(Point2::new(x, y));
    }
    if let Some(arr) = v.get("kinds").and_then(Value::as_array) {
        let mut mask = SnapMask::NONE;
        for k in arr {
            if let Some(kind) = k.as_str().and_then(snap_kind_from_str) {
                mask = mask.with(kind);
            }
        }
        opts.kinds = mask;
    }
    opts
}

/// Maps model [`Color`] to stable [`ColorView`].
fn color_view(c: Color) -> ColorView {
    match c {
        Color::ByLayer => ColorView::ByLayer,
        Color::ByBlock => ColorView::ByBlock,
        Color::Aci(a) => ColorView::Aci(a.get()),
        Color::Rgb(r, g, b) => ColorView::Rgb([r, g, b]),
    }
}

/// Maps model [`Lineweight`] to stable [`LineweightView`].
fn lineweight_view(w: Lineweight) -> LineweightView {
    match w {
        Lineweight::ByLayer => LineweightView::ByLayer,
        Lineweight::ByBlock => LineweightView::ByBlock,
        Lineweight::Mm(mm) => LineweightView::Mm(mm),
    }
}

/// Builds model [`Color`] from input [`ColorView`].
///
/// # Errors
/// Returns `aci_out_of_range` when ACI is outside `1..=255`.
fn color_from_view(c: ColorView) -> Result<Color, ApiError> {
    Ok(match c {
        ColorView::ByLayer => Color::ByLayer,
        ColorView::ByBlock => Color::ByBlock,
        ColorView::Aci(a) => {
            Color::aci(a).map_err(|e| ApiError::new("aci_out_of_range", e.to_string()))?
        }
        ColorView::Rgb([r, g, b]) => Color::Rgb(r, g, b),
    })
}

/// Parses an entity class name, returning `None` for unknown classes.
fn entity_kind_from_str(s: &str) -> Option<EntityKind> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "line" => EntityKind::Line,
        "point" => EntityKind::Point,
        "circle" => EntityKind::Circle,
        "arc" => EntityKind::Arc,
        "polyline" => EntityKind::Polyline,
        _ => return None,
    })
}

/// Builds an af-select [`SelectionFilter`] and ignores unknown entity types.
///
/// # Errors
/// Returns `aci_out_of_range` for an invalid filter color.
fn selection_filter_from_view(v: &SelectionFilterView) -> Result<SelectionFilter, ApiError> {
    let kinds = v
        .kinds
        .as_ref()
        .map(|ks| ks.iter().filter_map(|s| entity_kind_from_str(s)).collect());
    let layers = v.layers.as_ref().map(|ls| {
        ls.iter()
            .map(|&n| LayerId::from(ObjectId(n)))
            .collect::<Vec<_>>()
    });
    let colors = v
        .colors
        .as_ref()
        .map(|cs| cs.iter().map(|&c| color_from_view(c)).collect())
        .transpose()?;
    Ok(SelectionFilter {
        kinds,
        layers,
        colors,
    })
}

/// Builds model [`Lineweight`] from input [`LineweightView`].
fn lineweight_from_view(w: LineweightView) -> Lineweight {
    match w {
        LineweightView::ByLayer => Lineweight::ByLayer,
        LineweightView::ByBlock => Lineweight::ByBlock,
        LineweightView::Mm(mm) => Lineweight::Mm(mm),
    }
}

/// Builds model [`LineTypeRef`] from input [`LineTypeRefView`]. The transaction
/// validates referenced style IDs.
fn line_type_from_view(lt: LineTypeRefView) -> LineTypeRef {
    match lt {
        LineTypeRefView::ByLayer => LineTypeRef::ByLayer,
        LineTypeRefView::ByBlock => LineTypeRef::ByBlock,
        LineTypeRefView::Style(id) => LineTypeRef::Style(StyleId::from(ObjectId(id))),
    }
}

/// Maps model [`SysvarValue`] to stable [`SysvarValueView`].
fn sysvar_value_view(v: SysvarValue) -> SysvarValueView {
    match v {
        SysvarValue::Int(n) => SysvarValueView::Int(n),
        SysvarValue::Real(x) => SysvarValueView::Real(x),
        SysvarValue::Real2(x, y) => SysvarValueView::Real2([x, y]),
    }
}

/// Builds model [`SysvarValue`] from input [`SysvarValueView`].
fn sysvar_value_from_view(v: SysvarValueView) -> SysvarValue {
    match v {
        SysvarValueView::Int(n) => SysvarValue::Int(n),
        SysvarValueView::Real(x) => SysvarValue::Real(x),
        SysvarValueView::Real2([x, y]) => SysvarValue::Real2(x, y),
    }
}

/// Returns the lowercase short name of a linear unit.
fn linear_unit_str(u: LinearUnit) -> &'static str {
    match u {
        LinearUnit::Mm => "mm",
        LinearUnit::Cm => "cm",
        LinearUnit::M => "m",
        LinearUnit::In => "in",
        LinearUnit::Ft => "ft",
        LinearUnit::Unitless => "unitless",
    }
}

/// Parses a short unit name, defaulting to millimeters for the WASM layer.
#[cfg(feature = "wasm")]
pub(crate) fn units_from_str(s: &str) -> Units {
    let linear = match s.trim().to_ascii_lowercase().as_str() {
        "cm" => LinearUnit::Cm,
        "m" => LinearUnit::M,
        "in" => LinearUnit::In,
        "ft" => LinearUnit::Ft,
        "unitless" => LinearUnit::Unitless,
        _ => LinearUnit::Mm,
    };
    Units { linear }
}

/// Serializes an endpoint result as `{"ok":T}` or `{"error":E}`.
pub(crate) fn result_json<T: Serialize>(r: &Result<T, ApiError>) -> String {
    let mut m = serde_json::Map::new();
    match r {
        Ok(t) => {
            m.insert(
                "ok".to_string(),
                serde_json::to_value(t).unwrap_or(Value::Null),
            );
        }
        Err(e) => {
            m.insert(
                "error".to_string(),
                serde_json::to_value(e).unwrap_or(Value::Null),
            );
        }
    }
    serde_json::to_string(&Value::Object(m)).unwrap_or_else(|_| {
        r#"{"error":{"code":"serialize","message":"failed to serialize result"}}"#.to_string()
    })
}

/// Serializes an error-only `ResultJson` envelope.
pub(crate) fn error_json(e: &ApiError) -> String {
    result_json::<()>(&Err(e.clone()))
}

#[cfg(test)]
mod pgp_aliases {
    use super::*;
    use af_model::units::Units;
    use serde_json::json;

    fn reinit(session: &mut ApiSession, content: &str) -> ExecuteResult {
        session
            .execute("__ARCFORGE_PGP_REINIT", &json!({ "pgp": content }))
            .expect("PGP reinit")
    }

    fn resolve(session: &mut ApiSession, token: &str) -> Option<String> {
        session
            .execute("__ARCFORGE_PGP_RESOLVE", &json!({ "token": token }))
            .expect("PGP resolve")
            .message
    }

    #[test]
    fn typed_and_json_execution_share_the_pgp_table() {
        let mut session = ApiSession::new(Units::default());
        let admin = reinit(&mut session, "línea,*LINE\n");
        assert_eq!(admin.tx_seq, None);
        assert!(admin.created.is_empty());
        assert!(
            admin
                .message
                .as_deref()
                .is_some_and(|m| m.starts_with("PGP: 1 "))
        );
        assert!(session.poll_events().is_empty());

        let typed = session
            .execute("LÍNEA", &json!({ "p1": [0, 0], "p2": [2, 0] }))
            .expect("typed alias execution");
        assert_eq!(typed.created.len(), 1);

        let json_result = session.execute_json("línea", r#"{"p1":[0,1],"p2":[2,1]}"#);
        let envelope: Value = serde_json::from_str(&json_result).expect("result envelope");
        assert_eq!(
            envelope["ok"]["created"]
                .as_array()
                .expect("created array")
                .len(),
            1
        );
        assert_eq!(session.doc_info().entity_count, 2);
    }

    #[test]
    fn malformed_reinit_keeps_the_previous_table() {
        let mut session = ApiSession::new(Units::default());
        reinit(&mut session, "KEEP,*LINE\n");

        let error = session
            .execute(
                "__ARCFORGE_PGP_REINIT",
                &json!({ "pgp": "NEW,*LINE", "extra": true }),
            )
            .expect_err("extra fields must be rejected");
        assert_eq!(error.code, "invalid_args");
        assert_eq!(resolve(&mut session, "KEEP").as_deref(), Some("LINE"));
        assert_eq!(resolve(&mut session, "NEW"), None);

        let malformed_json = session.execute_json("__ARCFORGE_PGP_REINIT", r#"{"pgp":7}"#);
        let envelope: Value = serde_json::from_str(&malformed_json).expect("error envelope");
        assert_eq!(envelope["error"]["code"], "invalid_args");
        assert_eq!(resolve(&mut session, "KEEP").as_deref(), Some("LINE"));
        assert_eq!(session.doc_info().entity_count, 0);
        assert!(session.poll_events().is_empty());
    }

    #[test]
    fn resolve_obeys_canonical_user_and_builtin_precedence() {
        let mut session = ApiSession::new(Units::default());
        let admin = reinit(
            &mut session,
            "C,*COPY\nCIRCLE,*LINE\nBROKEN,*NO_SUCH_COMMAND\n",
        );

        assert_eq!(resolve(&mut session, "C").as_deref(), Some("COPY"));
        assert_eq!(resolve(&mut session, "CIRCLE").as_deref(), Some("CIRCLE"));
        assert_eq!(resolve(&mut session, "BROKEN"), None);
        assert!(
            admin
                .message
                .as_deref()
                .is_some_and(|m| m.contains("warnings"))
        );
        assert!(session.poll_events().is_empty());
    }
}

// ============================ synchronization oracle ============================
//
// These unit tests compare internal incremental index and render state against a
// clean rebuild of the current document.
#[cfg(test)]
mod sync_oracle {
    use super::*;
    use af_model::units::Units;
    use serde_json::json;

    /// Deterministic xorshift64 PRNG for table-driven fuzzing without `rand`.
    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        /// Integer in `[0, n)` for `n > 0`.
        fn below(&mut self, n: u64) -> u64 {
            self.next_u64() % n
        }
        /// Coordinate in the test work area `[-50, 50]`.
        fn coord(&mut self) -> f64 {
            self.below(101) as f64 - 50.0
        }
    }

    /// Compares render models structurally by batch key, independent of ordering.
    fn render_eq(a: &RenderModel, b: &RenderModel) -> bool {
        a.batches.len() == b.batches.len()
            && a.batches.iter().all(|ba| b.batch(&ba.key) == Some(ba))
    }

    /// Verifies incremental index and render state against a clean rebuild.
    fn assert_synced(s: &ApiSession) {
        let doc = s.session.document();

        // Probe both IDs and spatial answers to catch stale bounds after MOVE.
        let fresh = SpatialIndex::build(doc, CONTAINER);
        assert_eq!(
            s.index.ids(),
            fresh.ids(),
            "ids del índice divergen del rebuild"
        );
        let mut cx = -60.0;
        while cx <= 60.0 {
            let mut cy = -60.0;
            while cy <= 60.0 {
                let r = BBox::new(
                    Point2::new(cx - 8.0, cy - 8.0),
                    Point2::new(cx + 8.0, cy + 8.0),
                );
                let sort = |mut v: Vec<EntityId>| {
                    v.sort_unstable_by_key(|id| id.raw().0);
                    v
                };
                assert_eq!(
                    sort(s.index.query_rect(r)),
                    sort(fresh.query_rect(r)),
                    "query_rect diverge (AABB obsoleto) en ({cx},{cy})"
                );
                cy += 12.0;
            }
            cx += 12.0;
        }

        // Render must match a full rebuild structurally.
        let fresh_render = build_full(doc, &RenderOpts::new(s.chord_err));
        assert!(
            render_eq(&s.render, &fresh_render),
            "render incremental diverge de build_full"
        );
    }

    #[test]
    fn incremental_equals_build_over_random_do_undo_redo() {
        let mut s = ApiSession::new(Units::default());
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut draws = 0u32;
        assert_synced(&s);

        for _ in 0..200 {
            match rng.below(4) {
                0 => {
                    // LINE with random endpoints in the work area.
                    let (x1, y1, x2, y2) = (rng.coord(), rng.coord(), rng.coord(), rng.coord());
                    if s.execute("LINE", &json!({ "p1": [x1, y1], "p2": [x2, y2] }))
                        .is_ok()
                    {
                        draws += 1;
                    }
                }
                1 => {
                    // MOVE an entity selected from the current index.
                    let ids = s.index.ids();
                    if !ids.is_empty() {
                        let id = ids[rng.below(ids.len() as u64) as usize].raw().0;
                        let (dx, dy) = (rng.coord(), rng.coord());
                        let _ = s.execute(
                            "MOVE",
                            &json!({ "entities": [id], "from": [0.0, 0.0], "to": [dx, dy] }),
                        );
                    }
                }
                2 => {
                    let _ = s.execute("UNDO", &Value::Null);
                }
                _ => {
                    let _ = s.execute("REDO", &Value::Null);
                }
            }
            assert_synced(&s);
        }
        assert!(draws > 0, "la secuencia debe ejercitar creaciones reales");
    }

    #[test]
    fn pick_sees_line_then_gone_after_undo_incremental() {
        let mut s = ApiSession::new(Units::default());
        let out = s
            .execute("LINE", &json!({ "p1": [0.0, 0.0], "p2": [10.0, 0.0] }))
            .expect("LINE");
        let id = out.created[0];
        assert!(
            s.pick(0.0, 0.0, 0.5).iter().any(|h| h.id == id),
            "pick ve la línea recién dibujada (índice incremental)"
        );
        assert_synced(&s);

        s.execute("UNDO", &Value::Null).expect("undo");
        assert!(
            s.pick(0.0, 0.0, 0.5).is_empty(),
            "pick deja de ver la línea tras UNDO (índice incremental)"
        );
        assert_synced(&s);

        s.execute("REDO", &Value::Null).expect("redo");
        assert!(
            s.pick(0.0, 0.0, 0.5).iter().any(|h| h.id == id),
            "pick la vuelve a ver tras REDO"
        );
        assert_synced(&s);
    }

    #[test]
    fn render_delta_keeps_lineweight_and_poly_width_separate() {
        let mut s = ApiSession::new(Units::default());
        let donut = s
            .execute(
                "DONUT",
                &json!({ "center": [0, 0], "diam_ext": 10, "diam_int": 6 }),
            )
            .expect("DONUT")
            .created[0];
        let line = s
            .execute("LINE", &json!({ "p1": [20, 0], "p2": [30, 0] }))
            .expect("LINE")
            .created[0];

        let delta = s.render_delta();
        let mut strips = delta.upserts.iter().flat_map(|batch| batch.strips.iter());
        let donut_strip = strips
            .clone()
            .find(|strip| strip.entity == donut)
            .expect("DONUT strip");
        let line_strip = strips
            .find(|strip| strip.entity == line)
            .expect("LINE strip");
        assert_eq!(donut_strip.width, 0.25);
        assert_eq!(donut_strip.poly_width, 2.0);
        assert!(donut_strip.analytic_length.is_some());
        assert_eq!(line_strip.width, 0.25);
        assert_eq!(line_strip.poly_width, 0.0);
        assert_eq!(line_strip.analytic_length, Some(10.0));
    }
}

// ===================== advanced selection and groups =====================
//
// These tests need internal session access because no built-in GROUP command exists.
#[cfg(test)]
mod selection_ext {
    use super::*;
    use af_model::Group;
    use af_model::id::GroupId;
    use af_model::units::Units;
    use serde_json::json;

    /// Draws a line through the public API and returns its ID.
    fn line(s: &mut ApiSession, x1: f64, y1: f64, x2: f64, y2: f64) -> u64 {
        s.execute("LINE", &json!({ "p1": [x1, y1], "p2": [x2, y2] }))
            .expect("LINE")
            .created[0]
    }

    /// Creates a group by direct transaction and returns its ID.
    fn add_group(s: &mut ApiSession, name: &str, members: &[u64], selectable: bool) -> u64 {
        let members: Vec<EntityId> = members
            .iter()
            .map(|&n| EntityId::from(ObjectId(n)))
            .collect();
        let gid: GroupId = ObjectId::NIL.into();
        let out = s
            .session
            .transact("grupo", |tx| {
                tx.add_group_raw(
                    Group::new(gid, name)
                        .with_members(members)
                        .with_selectable(selectable),
                )
            })
            .expect("add_group_raw");
        out.value.raw().0
    }

    #[test]
    fn select_polygon_window_contains_and_excludes() {
        let mut s = ApiSession::new(Units::default());
        let id = line(&mut s, 1.0, 1.0, 3.0, 3.0);
        // A window polygon surrounding the line contains it.
        let poly = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert_eq!(s.select_polygon(&poly, false), vec![id]);
        assert_eq!(s.selection(), vec![id], "muta la selección");
        // A distant polygon selects nothing.
        let far = [[100.0, 100.0], [110.0, 100.0], [110.0, 110.0]];
        assert!(s.select_polygon(&far, false).is_empty());
    }

    #[test]
    fn select_fence_crosses_line() {
        let mut s = ApiSession::new(Units::default());
        let id = line(&mut s, 0.0, 0.0, 10.0, 0.0);
        // A vertical fence crosses the line at x=5.
        let fence = [[5.0, -5.0], [5.0, 5.0]];
        assert_eq!(s.select_fence(&fence), vec![id]);
    }

    #[test]
    fn select_filter_by_kind_and_bad_aci() {
        let mut s = ApiSession::new(Units::default());
        let l1 = line(&mut s, 0.0, 0.0, 1.0, 0.0);
        line(&mut s, 2.0, 0.0, 3.0, 0.0);
        // Filtering by `line` returns both entities.
        let f = SelectionFilterView {
            kinds: Some(vec!["line".into()]),
            ..Default::default()
        };
        let sel = s.select_filter(&f).expect("filtro válido");
        assert_eq!(sel.len(), 2);
        assert!(sel.contains(&l1));
        // Filtering by `circle` returns neither entity.
        let f = SelectionFilterView {
            kinds: Some(vec!["circle".into()]),
            ..Default::default()
        };
        assert!(s.select_filter(&f).expect("filtro válido").is_empty());
        // Invalid ACI produces a typed error and preserves selection.
        let f = SelectionFilterView {
            colors: Some(vec![ColorView::Aci(0)]),
            ..Default::default()
        };
        assert_eq!(s.select_filter(&f).unwrap_err().code, "aci_out_of_range");
    }

    #[test]
    fn select_similar_same_kind_and_layer() {
        let mut s = ApiSession::new(Units::default());
        let l1 = line(&mut s, 0.0, 0.0, 1.0, 0.0);
        let l2 = line(&mut s, 2.0, 0.0, 3.0, 0.0);
        let sel = s.select_similar(l1);
        assert!(sel.contains(&l1) && sel.contains(&l2));
    }

    #[test]
    fn pick_all_is_a_query_not_a_mutation() {
        let mut s = ApiSession::new(Units::default());
        let id = line(&mut s, 0.0, 0.0, 10.0, 0.0);
        assert!(s.pick_all(5.0, 0.0, 0.5).contains(&id));
        assert!(s.selection().is_empty(), "pick_all NO muta la selección");
    }

    #[test]
    fn select_previous_restores_prior_selection() {
        let mut s = ApiSession::new(Units::default());
        let a = line(&mut s, 0.0, 0.0, 1.0, 0.0);
        let b = line(&mut s, 2.0, 0.0, 3.0, 0.0);
        s.set_selection(&[a]);
        s.set_selection(&[b]);
        // Previous is the live selection immediately before the last change.
        assert_eq!(s.select_previous(), vec![a]);
        // Restoring previous swaps the archived selection.
        assert_eq!(s.select_previous(), vec![b]);
    }

    #[test]
    fn pick_of_selectable_group_member_expands_to_whole_group() {
        let mut s = ApiSession::new(Units::default());
        let a = line(&mut s, 0.0, 0.0, 1.0, 0.0);
        let b = line(&mut s, 2.0, 0.0, 3.0, 0.0);
        add_group(&mut s, "G", &[a, b], true);
        // Selecting one member selects the entire selectable group.
        s.set_selection(&[a]);
        let sel = s.selection();
        assert!(
            sel.contains(&a) && sel.contains(&b),
            "grupo selectable se expande: {sel:?}"
        );
    }

    #[test]
    fn non_selectable_group_does_not_expand() {
        let mut s = ApiSession::new(Units::default());
        let a = line(&mut s, 0.0, 0.0, 1.0, 0.0);
        let b = line(&mut s, 2.0, 0.0, 3.0, 0.0);
        add_group(&mut s, "G", &[a, b], false);
        s.set_selection(&[a]);
        assert_eq!(s.selection(), vec![a], "grupo no selectable no expande");
    }

    #[test]
    fn groups_lists_group_with_members() {
        let mut s = ApiSession::new(Units::default());
        let a = line(&mut s, 0.0, 0.0, 1.0, 0.0);
        let b = line(&mut s, 2.0, 0.0, 3.0, 0.0);
        let gid = add_group(&mut s, "G", &[a, b], true);
        let groups = s.groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, gid);
        assert_eq!(groups[0].name, "G");
        assert_eq!(groups[0].members, vec![a, b]);
        assert!(groups[0].selectable);
    }
}
