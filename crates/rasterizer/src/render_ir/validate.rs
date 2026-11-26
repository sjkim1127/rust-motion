//! Render graph validation, deterministic topological scheduling, and liveness analysis.

use super::types::*;
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use thiserror::Error;

/// Errors raised when validating or scheduling a [`RenderGraph`].
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphValidationError {
    #[error("invalid render graph dimensions {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("resource {id} byte size overflows u64")]
    ResourceSizeOverflow { id: ResourceId },
    #[error("resource {id} has invalid sample count {sample_count} (expected 1 or 4)")]
    InvalidSampleCount { id: ResourceId, sample_count: u32 },
    #[error("duplicate resource id {0}")]
    DuplicateResourceId(ResourceId),
    #[error("duplicate pass id {0}")]
    DuplicatePassId(PassId),
    #[error("unknown resource {resource} referenced by pass {pass:?}")]
    UnknownResource {
        pass: Option<PassId>,
        resource: ResourceId,
    },
    #[error("pass {pass} references unknown order dependency {dep}")]
    UnknownOrderDep { pass: PassId, dep: PassId },
    #[error("render graph does not declare an output resource")]
    MissingOutput,
    #[error("output resource {0} is never written by any pass")]
    OutputNeverWritten(ResourceId),
    #[error("pass {pass} reads resource {resource} before any prior write")]
    ReadBeforeWrite { pass: PassId, resource: ResourceId },
    #[error("pass {pass} has a self-dependency")]
    SelfDependency { pass: PassId },
    #[error("cycle detected among passes: {involved:?}")]
    CycleDetected { involved: Vec<PassId> },
    #[error("pass {pass} requires {expected} format for resource {resource}, found {actual:?}")]
    IncompatibleFormat {
        pass: PassId,
        resource: ResourceId,
        expected: &'static str,
        actual: PixelFormat,
    },
    #[error("pass {pass} has incompatible dimensions between {src} and {dst}: {reason}")]
    IncompatibleDimensions {
        pass: PassId,
        src: ResourceId,
        dst: ResourceId,
        reason: String,
    },
    #[error("pass {pass} has invalid parameters: {reason}")]
    InvalidPassParams { pass: PassId, reason: String },
    #[error("pass {pass} requires unsupported capabilities: {missing:?}")]
    MissingCapabilities {
        pass: PassId,
        missing: Vec<&'static str>,
    },
}

/// Lifetime interval and pool slot assignment for a resource in a compiled [`ExecutionPlan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ResourceLiveness {
    pub(crate) resource: ResourceId,
    pub(crate) first_step: usize,
    pub(crate) last_step: usize,
    pub(crate) byte_size: u64,
    pub(crate) reusable_pool_slot: Option<usize>,
}

/// Validated, deterministically scheduled render execution plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExecutionPlan {
    pub(crate) graph: RenderGraph,
    pub(crate) pass_order: Vec<PassId>,
    pub(crate) liveness: Vec<ResourceLiveness>,
    pub(crate) peak_transient_bytes: u64,
    pub(crate) transient_pool_slots: usize,
    pub(crate) required_capabilities: CapabilityFlags,
}

#[derive(Serialize)]
struct PlanDump<'a> {
    width: u32,
    height: u32,
    output: Option<ResourceId>,
    pass_order: &'a [PassId],
    required_capabilities: Vec<&'static str>,
    peak_transient_bytes: u64,
    transient_pool_slots: usize,
    resources: Vec<ResourceDump<'a>>,
    passes: Vec<PassDump<'a>>,
    liveness: &'a [ResourceLiveness],
}

#[derive(Serialize)]
struct ResourceDump<'a> {
    id: ResourceId,
    label: &'a str,
    width: u32,
    height: u32,
    format: PixelFormat,
    sample_count: u32,
    lifetime: ResourceLifetime,
    usage: Vec<&'static str>,
    byte_size: u64,
}

#[derive(Serialize)]
struct PassDump<'a> {
    step: usize,
    id: PassId,
    label: &'a str,
    op: &'static str,
    reads: &'a [ResourceId],
    writes: &'a [ResourceId],
    order_deps: &'a [PassId],
    required_capabilities: Vec<&'static str>,
}

impl ExecutionPlan {
    /// Validate a [`RenderGraph`] and compile it into a deterministic [`ExecutionPlan`].
    pub(crate) fn compile(graph: RenderGraph) -> Result<Self, GraphValidationError> {
        if graph.width == 0 || graph.height == 0 {
            return Err(GraphValidationError::InvalidDimensions {
                width: graph.width,
                height: graph.height,
            });
        }

        let mut resources_by_id: BTreeMap<ResourceId, &ResourceDesc> = BTreeMap::new();
        for res in &graph.resources {
            if res.width == 0 || res.height == 0 {
                return Err(GraphValidationError::InvalidDimensions {
                    width: res.width,
                    height: res.height,
                });
            }
            if res.sample_count != 1 && res.sample_count != 4 {
                return Err(GraphValidationError::InvalidSampleCount {
                    id: res.id,
                    sample_count: res.sample_count,
                });
            }
            if res.checked_byte_size().is_none() {
                return Err(GraphValidationError::ResourceSizeOverflow { id: res.id });
            }
            if resources_by_id.insert(res.id, res).is_some() {
                return Err(GraphValidationError::DuplicateResourceId(res.id));
            }
        }

        let output_id = graph.output.ok_or(GraphValidationError::MissingOutput)?;
        if !resources_by_id.contains_key(&output_id) {
            return Err(GraphValidationError::UnknownResource {
                pass: None,
                resource: output_id,
            });
        }

        let mut passes_by_id: BTreeMap<PassId, &PassNode> = BTreeMap::new();
        let mut pass_decl_index: BTreeMap<PassId, usize> = BTreeMap::new();
        for (idx, pass) in graph.passes.iter().enumerate() {
            if passes_by_id.insert(pass.id, pass).is_some() {
                return Err(GraphValidationError::DuplicatePassId(pass.id));
            }
            pass_decl_index.insert(pass.id, idx);
        }

        // Validate individual pass references, formats, aliasing, and parameters.
        let mut writes_to_output = false;
        let mut writers_by_res: BTreeMap<ResourceId, Vec<PassId>> = BTreeMap::new();
        let mut readers_by_res: BTreeMap<ResourceId, Vec<PassId>> = BTreeMap::new();

        for pass in &graph.passes {
            if pass.writes.is_empty() {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "pass must write at least one resource".into(),
                });
            }
            for &r in &pass.reads {
                if !resources_by_id.contains_key(&r) {
                    return Err(GraphValidationError::UnknownResource {
                        pass: Some(pass.id),
                        resource: r,
                    });
                }
                readers_by_res.entry(r).or_default().push(pass.id);
            }
            for &w in &pass.writes {
                if !resources_by_id.contains_key(&w) {
                    return Err(GraphValidationError::UnknownResource {
                        pass: Some(pass.id),
                        resource: w,
                    });
                }
                if w == output_id {
                    writes_to_output = true;
                }
                writers_by_res.entry(w).or_default().push(pass.id);
            }
            for &dep in &pass.order_deps {
                if dep == pass.id {
                    return Err(GraphValidationError::SelfDependency { pass: pass.id });
                }
                if !passes_by_id.contains_key(&dep) {
                    return Err(GraphValidationError::UnknownOrderDep { pass: pass.id, dep });
                }
            }

            validate_pass_semantics(pass, &resources_by_id)?;
        }

        if !writes_to_output {
            return Err(GraphValidationError::OutputNeverWritten(output_id));
        }

        // Build deterministic dependency edges (explicit order_deps + RAW/WAW/WAR).
        let mut adj: BTreeMap<PassId, BTreeSet<PassId>> = BTreeMap::new();
        let mut indegree: BTreeMap<PassId, usize> = BTreeMap::new();
        for pass in &graph.passes {
            adj.insert(pass.id, BTreeSet::new());
            indegree.insert(pass.id, 0);
        }

        let mut add_edge = |from: PassId, to: PassId| -> Result<(), GraphValidationError> {
            if from == to {
                return Err(GraphValidationError::SelfDependency { pass: from });
            }
            if adj.get_mut(&from).expect("known pass").insert(to) {
                *indegree.get_mut(&to).expect("known pass") += 1;
            }
            Ok(())
        };

        for pass in &graph.passes {
            for &dep in &pass.order_deps {
                add_edge(dep, pass.id)?;
            }
        }

        for (&res_id, res) in &resources_by_id {
            let writers = writers_by_res
                .get(&res_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let readers = readers_by_res
                .get(&res_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            if writers.is_empty() {
                if res.lifetime != ResourceLifetime::External {
                    if let Some(&first_reader) = readers.first() {
                        return Err(GraphValidationError::ReadBeforeWrite {
                            pass: first_reader,
                            resource: res_id,
                        });
                    }
                }
                continue;
            }

            // First writer must not require prior contents on an uninitialized non-external resource.
            let first_writer = passes_by_id[&writers[0]];
            if res.lifetime != ResourceLifetime::External
                && pass_reads_existing_target(first_writer, res_id)
            {
                return Err(GraphValidationError::ReadBeforeWrite {
                    pass: first_writer.id,
                    resource: res_id,
                });
            }

            // WAW chain across multiple writers in declaration order.
            for pair in writers.windows(2) {
                add_edge(pair[0], pair[1])?;
            }

            // RAW and WAR edges for readers of this resource.
            for &reader_id in readers {
                if writers.len() == 1 {
                    let writer_id = writers[0];
                    if reader_id != writer_id {
                        add_edge(writer_id, reader_id)?;
                    }
                    continue;
                }

                let reader_decl = pass_decl_index[&reader_id];
                // Find the latest writer declared before (or equal to) the reader.
                let prior_pos = writers
                    .iter()
                    .rposition(|w| pass_decl_index[w] <= reader_decl && *w != reader_id);

                let Some(writer_idx) = prior_pos else {
                    if res.lifetime != ResourceLifetime::External {
                        return Err(GraphValidationError::ReadBeforeWrite {
                            pass: reader_id,
                            resource: res_id,
                        });
                    }
                    // External resource read before first overwrite: add WAR to first writer.
                    if writers[0] != reader_id {
                        add_edge(reader_id, writers[0])?;
                    }
                    continue;
                };

                add_edge(writers[writer_idx], reader_id)?;
                if let Some(&next_writer) = writers.get(writer_idx + 1) {
                    if reader_id != next_writer {
                        add_edge(reader_id, next_writer)?;
                    }
                }
            }
        }

        // Kahn's topological sort with min-heap by PassId for stable deterministic order.
        let mut ready: BinaryHeap<Reverse<PassId>> = BinaryHeap::new();
        for (&id, &deg) in &indegree {
            if deg == 0 {
                ready.push(Reverse(id));
            }
        }

        let mut pass_order = Vec::with_capacity(graph.passes.len());
        while let Some(Reverse(id)) = ready.pop() {
            pass_order.push(id);
            if let Some(neighbors) = adj.get(&id) {
                for &next in neighbors {
                    let deg = indegree.get_mut(&next).expect("known pass");
                    *deg -= 1;
                    if *deg == 0 {
                        ready.push(Reverse(next));
                    }
                }
            }
        }

        if pass_order.len() != graph.passes.len() {
            let scheduled: BTreeSet<PassId> = pass_order.iter().copied().collect();
            let involved: Vec<PassId> = passes_by_id
                .keys()
                .copied()
                .filter(|id| !scheduled.contains(id))
                .collect();
            return Err(GraphValidationError::CycleDetected { involved });
        }

        // Verify post-schedule that every non-external read has an earlier write in pass_order.
        let mut written_so_far: BTreeSet<ResourceId> = graph
            .resources
            .iter()
            .filter(|r| r.lifetime == ResourceLifetime::External)
            .map(|r| r.id)
            .collect();
        for &pid in &pass_order {
            let pass = passes_by_id[&pid];
            for &r in &pass.reads {
                if !pass.writes.contains(&r) && !written_so_far.contains(&r) {
                    return Err(GraphValidationError::ReadBeforeWrite {
                        pass: pid,
                        resource: r,
                    });
                }
            }
            for &w in &pass.writes {
                written_so_far.insert(w);
            }
        }

        // Compute liveness intervals across pass_order steps.
        let mut first_use: BTreeMap<ResourceId, usize> = BTreeMap::new();
        let mut last_use: BTreeMap<ResourceId, usize> = BTreeMap::new();
        let last_step_index = pass_order.len().saturating_sub(1);

        for (step, &pid) in pass_order.iter().enumerate() {
            let pass = passes_by_id[&pid];
            for &id in pass.reads.iter().chain(pass.writes.iter()) {
                first_use.entry(id).or_insert(step);
                last_use.insert(id, step);
            }
        }
        // Output stays live until end of graph.
        last_use.insert(output_id, last_step_index);

        // Assign transient pool slots greedily across compatible descriptors.
        #[derive(Clone, Copy, PartialEq, Eq)]
        struct PoolSlot {
            width: u32,
            height: u32,
            format: PixelFormat,
            sample_count: u32,
            free_after_step: usize,
        }

        let mut pool_slots: Vec<PoolSlot> = Vec::new();
        let mut liveness = Vec::with_capacity(resources_by_id.len());
        let mut transient_intervals: Vec<(usize, usize, u64)> = Vec::new();

        for (&res_id, &res) in &resources_by_id {
            let byte_size = res
                .checked_byte_size()
                .ok_or(GraphValidationError::ResourceSizeOverflow { id: res_id })?;
            let (first_step, last_step) = match (first_use.get(&res_id), last_use.get(&res_id)) {
                (Some(&f), Some(&l)) => (f, l),
                _ => (0, 0),
            };
            let reusable_pool_slot =
                if res.lifetime == ResourceLifetime::Transient && first_use.contains_key(&res_id) {
                    transient_intervals.push((first_step, last_step, byte_size));
                    let found = pool_slots.iter().position(|slot| {
                        slot.width == res.width
                            && slot.height == res.height
                            && slot.format == res.format
                            && slot.sample_count == res.sample_count
                            && slot.free_after_step < first_step
                    });
                    if let Some(slot_idx) = found {
                        pool_slots[slot_idx].free_after_step = last_step;
                        Some(slot_idx)
                    } else {
                        let slot_idx = pool_slots.len();
                        pool_slots.push(PoolSlot {
                            width: res.width,
                            height: res.height,
                            format: res.format,
                            sample_count: res.sample_count,
                            free_after_step: last_step,
                        });
                        Some(slot_idx)
                    }
                } else {
                    None
                };

            liveness.push(ResourceLiveness {
                resource: res_id,
                first_step,
                last_step,
                byte_size,
                reusable_pool_slot,
            });
        }

        let mut peak_transient_bytes = 0u64;
        for step in 0..pass_order.len() {
            let live_at_step: u64 = transient_intervals
                .iter()
                .filter(|(first, last, _)| *first <= step && step <= *last)
                .map(|(_, _, bytes)| *bytes)
                .fold(0u64, u64::saturating_add);
            peak_transient_bytes = peak_transient_bytes.max(live_at_step);
        }

        let required_capabilities = graph.passes.iter().fold(CapabilityFlags::NONE, |acc, p| {
            acc.union(p.required_capabilities)
        });

        Ok(Self {
            graph,
            pass_order,
            liveness,
            peak_transient_bytes,
            transient_pool_slots: pool_slots.len(),
            required_capabilities,
        })
    }

    /// Check whether an executor capability set satisfies every pass in the plan.
    pub(crate) fn check_capabilities(
        &self,
        available: CapabilityFlags,
    ) -> Result<(), GraphValidationError> {
        for &pid in &self.pass_order {
            if let Some(pass) = self.graph.pass(pid) {
                let missing = pass.required_capabilities.missing(available);
                if !missing.is_empty() {
                    return Err(GraphValidationError::MissingCapabilities {
                        pass: pid,
                        missing: missing.names(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Render a deterministic human-readable inspection dump of the compiled plan.
    pub(crate) fn dump_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "RenderPlan {}x{} output={:?} peak_transient_bytes={} pool_slots={}\n",
            self.graph.width,
            self.graph.height,
            self.graph.output.map(|r| r.to_string()),
            self.peak_transient_bytes,
            self.transient_pool_slots,
        ));
        out.push_str(&format!(
            "Capabilities: {:?}\n",
            self.required_capabilities.names()
        ));
        out.push_str("Resources:\n");
        for live in &self.liveness {
            if let Some(res) = self.graph.resource(live.resource) {
                out.push_str(&format!(
                    "  {} {:?} {}x{} {:?}x{} {:?} live=[{}..{}] slot={:?} bytes={}\n",
                    res.id,
                    res.label,
                    res.width,
                    res.height,
                    res.format,
                    res.sample_count,
                    res.lifetime,
                    live.first_step,
                    live.last_step,
                    live.reusable_pool_slot,
                    live.byte_size,
                ));
            }
        }
        out.push_str("Schedule:\n");
        for (step, &pid) in self.pass_order.iter().enumerate() {
            if let Some(pass) = self.graph.pass(pid) {
                out.push_str(&format!(
                    "  [{step}] {} {:?} op={} reads={:?} writes={:?} deps={:?}\n",
                    pass.id,
                    pass.label,
                    pass.op.kind_name(),
                    pass.reads,
                    pass.writes,
                    pass.order_deps,
                ));
            }
        }
        out
    }

    /// Serialize a deterministic JSON inspection dump of the compiled plan.
    pub(crate) fn dump_json(&self) -> Result<String, serde_json::Error> {
        let mut resources = Vec::with_capacity(self.liveness.len());
        for live in &self.liveness {
            if let Some(res) = self.graph.resource(live.resource) {
                resources.push(ResourceDump {
                    id: res.id,
                    label: &res.label,
                    width: res.width,
                    height: res.height,
                    format: res.format,
                    sample_count: res.sample_count,
                    lifetime: res.lifetime,
                    usage: res.usage.names(),
                    byte_size: live.byte_size,
                });
            }
        }
        let mut passes = Vec::with_capacity(self.pass_order.len());
        for (step, &pid) in self.pass_order.iter().enumerate() {
            if let Some(pass) = self.graph.pass(pid) {
                passes.push(PassDump {
                    step,
                    id: pass.id,
                    label: &pass.label,
                    op: pass.op.kind_name(),
                    reads: &pass.reads,
                    writes: &pass.writes,
                    order_deps: &pass.order_deps,
                    required_capabilities: pass.required_capabilities.names(),
                });
            }
        }
        let dump = PlanDump {
            width: self.graph.width,
            height: self.graph.height,
            output: self.graph.output,
            pass_order: &self.pass_order,
            required_capabilities: self.required_capabilities.names(),
            peak_transient_bytes: self.peak_transient_bytes,
            transient_pool_slots: self.transient_pool_slots,
            resources,
            passes,
            liveness: &self.liveness,
        };
        serde_json::to_string_pretty(&dump)
    }
}

fn pass_reads_existing_target(pass: &PassNode, target: ResourceId) -> bool {
    match &pass.op {
        PassOp::Composite { .. } => pass.writes.contains(&target),
        PassOp::Raster2D { clear, .. } | PassOp::Shader { clear, .. } => {
            pass.writes.contains(&target) && matches!(clear, ClearOp::Load)
        }
        _ => false,
    }
}

fn validate_pass_semantics(
    pass: &PassNode,
    resources: &BTreeMap<ResourceId, &ResourceDesc>,
) -> Result<(), GraphValidationError> {
    let dst_id = pass.writes[0];
    let dst = resources[&dst_id];

    match &pass.op {
        PassOp::PathMask { path_d, .. } => {
            if !dst.format.is_mask() {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: dst_id,
                    expected: "mask (R8Unorm)",
                    actual: dst.format,
                });
            }
            if path_d.trim().is_empty() {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "path mask SVG path cannot be empty".into(),
                });
            }
        }
        PassOp::Raster2D { path_mask, .. } => {
            if !dst.format.is_color() {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: dst_id,
                    expected: "color",
                    actual: dst.format,
                });
            }
            if let Some(mask_id) = path_mask {
                let Some(mask_res) = resources.get(mask_id) else {
                    return Err(GraphValidationError::UnknownResource {
                        pass: Some(pass.id),
                        resource: *mask_id,
                    });
                };
                if !mask_res.format.is_mask() {
                    return Err(GraphValidationError::IncompatibleFormat {
                        pass: pass.id,
                        resource: *mask_id,
                        expected: "mask (R8Unorm)",
                        actual: mask_res.format,
                    });
                }
                if !pass.reads.contains(mask_id) {
                    return Err(GraphValidationError::InvalidPassParams {
                        pass: pass.id,
                        reason: format!("path_mask {mask_id} must be listed in pass.reads"),
                    });
                }
            }
        }
        PassOp::Composite {
            source,
            opacity,
            region,
            ..
        } => {
            if !pass.reads.contains(source) {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: format!("composite source {source} must be listed in pass.reads"),
                });
            }
            if *source == dst_id {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "composite source and destination cannot alias".into(),
                });
            }
            let src = resources[source];
            if !src.format.is_color() {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: *source,
                    expected: "color",
                    actual: src.format,
                });
            }
            if !dst.format.is_color() {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: dst_id,
                    expected: "color",
                    actual: dst.format,
                });
            }
            if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: format!("invalid composite opacity {opacity}"),
                });
            }
            if let Some(bounds) = region {
                if !bounds.is_valid() {
                    return Err(GraphValidationError::InvalidPassParams {
                        pass: pass.id,
                        reason: "invalid composite region bounds".into(),
                    });
                }
            }
        }
        PassOp::Copy { source, dst_origin } => {
            if !pass.reads.contains(source) {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: format!("copy source {source} must be listed in pass.reads"),
                });
            }
            if *source == dst_id {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "copy source and destination cannot alias".into(),
                });
            }
            let src = resources[source];
            if src.format != dst.format {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: dst_id,
                    expected: "matching copy source format",
                    actual: dst.format,
                });
            }
            let end_x = dst_origin[0].checked_add(src.width);
            let end_y = dst_origin[1].checked_add(src.height);
            if end_x.is_none_or(|x| x > dst.width) || end_y.is_none_or(|y| y > dst.height) {
                return Err(GraphValidationError::IncompatibleDimensions {
                    pass: pass.id,
                    src: *source,
                    dst: dst_id,
                    reason: format!(
                        "copy {}x{} at {:?} exceeds destination {}x{}",
                        src.width, src.height, dst_origin, dst.width, dst.height
                    ),
                });
            }
        }
        PassOp::Compute {
            kernel, workgroups, ..
        } => {
            if kernel.trim().is_empty() {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "compute kernel name cannot be empty".into(),
                });
            }
            if workgroups.contains(&0) {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "compute workgroup counts must all be non-zero".into(),
                });
            }
        }
        PassOp::Shader {
            x,
            y,
            w,
            h,
            time,
            opacity,
            ..
        } => {
            if !dst.format.is_color() {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: dst_id,
                    expected: "color",
                    actual: dst.format,
                });
            }
            if ![*x, *y, *w, *h, *time, *opacity]
                .iter()
                .all(|v| v.is_finite())
                || *w <= 0.0
                || *h <= 0.0
                || !(0.0..=1.0).contains(opacity)
            {
                return Err(GraphValidationError::InvalidPassParams {
                    pass: pass.id,
                    reason: "invalid shader region or opacity".into(),
                });
            }
        }
        PassOp::CpuIsland { bounds, .. } => {
            if !dst.format.is_color() {
                return Err(GraphValidationError::IncompatibleFormat {
                    pass: pass.id,
                    resource: dst_id,
                    expected: "color",
                    actual: dst.format,
                });
            }
            if let Some(b) = bounds {
                if !b.is_valid() {
                    return Err(GraphValidationError::InvalidPassParams {
                        pass: pass.id,
                        reason: "invalid cpu island bounds".into(),
                    });
                }
            }
        }
    }
    Ok(())
}
