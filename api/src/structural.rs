//! Module 4 — **Structural Integrity & Building Collapse** (`structural`).
//!
//! Analyzes 3D structural stability whenever blocks are broken (`destroy_block`) or
//! destroyed by explosions (`explosion_detonate`):
//! 1. **3D Support & Cantilever Stress Graph** (`analyze_unsupported_cluster`):
//!    - Starting from each of the 6 neighbors of a removed block, runs a bounded 3D Breadth-First
//!      Search (BFS) through connected solid structural voxels.
//!    - Evaluates whether the component is anchored to a valid **Foundation**:
//!      - An unbreakable anchor (`bedrock`, `obsidian`, `reinforced_deepslate`), OR
//!      - A vertical column of natural terrain (`stone`, `deepslate`, `dirt`, etc.) reaching down
//!        to solid ground, OR
//!      - Connected to a load-bearing pillar within the material's `max_overhang` cantilever span
//!        and `support_reach` distance (scaled by `structural.strength_multiplier`).
//!    - If the BFS component exhausts all paths without finding a valid foundation within its
//!      material cantilever/reach limits, the entire cluster of voxels is declared **structurally
//!      unstable**!
//! 2. **Server-Side Collapse Execution & Falling Blocks**:
//!    - Detaches the unstable voxel cluster from the world (`host.set_block` to `minecraft:air`),
//!      spawns authoritative `FallingBlock` entities (`host.spawn_falling_block`) with outward
//!      toppling velocities (`NativeFalling { drop_item: true, hurts: true }`), and queues
//!      client-side **Compound Tilting Collapse Clusters**.
//! 3. **Client-Side Cinematic Tilting Rigid Clusters (`CollapsingStructureCluster`)**:
//!    - Renders the detached building section as a unified 3D multi-block structure rotating
//!      around its fracture pivot point (`host.draw_block` inside `Hooks::world_submit`),
//!      and shatters its blocks into 3D Voronoi debris shards (`DebrisEngine`) upon ground impact!

use std::collections::{HashSet, VecDeque};

use glam::{Quat, Vec3};
use horizon_client::mod_api::{Host, NativeFalling, NativeWorldDraw};

use crate::config::{DebrisConfig, StructuralConfig};
use crate::debris::DebrisEngine;
use crate::materials::{
    classify_material, is_air_or_fluid, is_natural_foundation, is_unbreakable_anchor,
};
use crate::math_phys::{FastRng, integrate_quat, is_client_voxel_solid, quat_to_axis_angle_deg};

/// Six orthogonal voxel neighbor offsets (`-Y`, `+Y`, `-X`, `+X`, `-Z`, `+Z`).
const NEIGHBORS_6: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [-1, 0, 0],
    [1, 0, 0],
    [0, 0, -1],
    [0, 0, 1],
];

/// One block inside a client-side tilting compound collapse cluster.
#[derive(Debug, Clone)]
pub struct ClusterMemberBlock {
    /// Offset in blocks from the cluster's pivot point.
    pub local_offset: Vec3,
    /// Original world integer block coordinates.
    pub origin_pos: [i32; 3],
    /// Block ID (`minecraft:stone_bricks`, `minecraft:oak_planks`, etc.).
    pub block_id: String,
}

/// A compound rigid cluster of blocks tilting and collapsing together in 3D space.
#[derive(Debug, Clone)]
pub struct CollapsingStructureCluster {
    /// World-space pivot / center of rotation.
    pub pivot_pos: Vec3,
    /// Linear velocity of the pivot ($\text{m/s}$).
    pub vel: Vec3,
    /// Current 3D tilt orientation quaternion.
    pub rotation: Quat,
    /// Angular velocity ($\text{rad/s}$) — drives the cinematic toppling motion!
    pub angular_vel: Vec3,
    /// Member blocks relative to `pivot_pos`.
    pub blocks: Vec<ClusterMemberBlock>,
    /// Elapsed collapse time ($\text{s}$).
    pub age_secs: f32,
    /// Maximum duration before shattering into Voronoi debris shards.
    pub max_duration_secs: f32,
}

/// Server-to-client queued collapse event (for singleplayer / integrated server).
#[derive(Debug, Clone)]
struct QueuedClusterEvent {
    blocks: Vec<([i32; 3], String)>,
    fracture_origin: Vec3,
}

/// Manages structural integrity analysis and active collapsing building clusters.
pub struct StructuralEngine {
    active_clusters: Vec<CollapsingStructureCluster>,
    queued_events: Vec<QueuedClusterEvent>,
    /// Guard flag to prevent recursive `destroy_block` / `block_changed` re-entry while detaching a cluster.
    is_detaching: bool,
    last_time_key: Option<(i64, f32)>,
    total_collapsed_blocks: usize,
}

impl Default for StructuralEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StructuralEngine {
    pub fn new() -> Self {
        Self {
            active_clusters: Vec::new(),
            queued_events: Vec::new(),
            is_detaching: false,
            last_time_key: None,
            total_collapsed_blocks: 0,
        }
    }

    pub fn active_cluster_count(&self) -> usize {
        self.active_clusters.len()
    }

    pub fn total_collapsed_blocks(&self) -> usize {
        self.total_collapsed_blocks
    }

    pub fn clear(&mut self) {
        self.active_clusters.clear();
        self.queued_events.clear();
    }

    /// Analyze neighbors around a broken block at `broken_pos` in the authoritative server world
    /// and trigger a structural collapse if any connected section lost its foundation support.
    pub fn on_server_block_removed(
        &mut self,
        host: &mut dyn Host,
        dimension: &str,
        broken_pos: [i32; 3],
        cfg: &StructuralConfig,
    ) {
        if !cfg.enabled || self.is_detaching {
            return;
        }
        self.inspect_and_collapse_around(host, dimension, &[broken_pos], cfg);
    }

    /// Analyze structural stability around a batch of explosion-destroyed blocks.
    pub fn on_server_explosion_blocks_removed(
        &mut self,
        host: &mut dyn Host,
        dimension: &str,
        destroyed: &[[i32; 3]],
        cfg: &StructuralConfig,
    ) {
        if !cfg.enabled || self.is_detaching || destroyed.is_empty() {
            return;
        }
        // Sample perimeter blocks around the blast crater
        let step = (destroyed.len() / 12).max(1);
        let sampled: Vec<[i32; 3]> = destroyed.iter().copied().step_by(step).collect();
        self.inspect_and_collapse_around(host, dimension, &sampled, cfg);
    }

    fn inspect_and_collapse_around(
        &mut self,
        host: &mut dyn Host,
        dimension: &str,
        origins: &[[i32; 3]],
        cfg: &StructuralConfig,
    ) {
        self.is_detaching = true;
        let mut already_detached: HashSet<[i32; 3]> = HashSet::new();
        for &o in origins {
            already_detached.insert(o);
        }

        let mut total_detached_this_pass = 0usize;
        let max_total = cfg.max_collapse_cluster_size.clamp(4, 128);

        for &origin in origins {
            // Check upper and horizontal neighbors first (most likely to lose support)
            for delta in [
                [0, 1, 0],
                [-1, 0, 0],
                [1, 0, 0],
                [0, 0, -1],
                [0, 0, 1],
            ] {
                if total_detached_this_pass >= max_total {
                    break;
                }
                let seed_pos = [
                    origin[0] + delta[0],
                    origin[1] + delta[1],
                    origin[2] + delta[2],
                ];
                if already_detached.contains(&seed_pos) {
                    continue;
                }
                let seed_id = host.block(seed_pos);
                if is_air_or_fluid(&seed_id) || is_unbreakable_anchor(&seed_id) {
                    continue;
                }
                let seed_mat = classify_material(&seed_id);
                if !seed_mat.is_structural {
                    continue;
                }

                if let Some(unsupported_cluster) =
                    analyze_unsupported_cluster(host, seed_pos, &already_detached, cfg)
                {
                    if unsupported_cluster.len() >= cfg.min_cluster_size.max(1) {
                        for &(p, _) in &unsupported_cluster {
                            already_detached.insert(p);
                        }
                        total_detached_this_pass += unsupported_cluster.len();
                        let fracture_origin =
                            Vec3::new(origin[0] as f32 + 0.5, origin[1] as f32 + 0.5, origin[2] as f32 + 0.5);
                        self.detach_cluster_on_server(
                            host,
                            dimension,
                            unsupported_cluster,
                            fracture_origin,
                            cfg,
                        );
                    }
                }
            }
        }

        self.is_detaching = false;
    }

    /// Detach the unsupported cluster on the server (`set_block` to air + `spawn_falling_block`)
    /// and queue a cinematic tilting cluster for the client renderer.
    fn detach_cluster_on_server(
        &mut self,
        host: &mut dyn Host,
        dimension: &str,
        cluster: Vec<([i32; 3], String)>,
        fracture_origin: Vec3,
        cfg: &StructuralConfig,
    ) {
        let Some(air_state) = host.default_state("minecraft:air") else {
            return;
        };

        // Compute cluster center of mass to push falling blocks outward in a realistic toppling arc
        let mut center = Vec3::ZERO;
        for &(p, _) in &cluster {
            center += Vec3::new(p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5);
        }
        center /= (cluster.len() as f32).max(1.0);

        let topple_away = (center - fracture_origin);
        let topple_horiz = Vec3::new(topple_away.x, 0.0, topple_away.z)
            .try_normalize()
            .unwrap_or(Vec3::new(0.6, 0.0, 0.4));

        // Sort blocks bottom-to-top when removing so upper blocks don't pop prematurely
        let mut sorted = cluster.clone();
        sorted.sort_by_key(|(p, _)| p[1]);

        let mut rng = FastRng::from_pos_and_id(sorted[0].0, cluster.len() as u64);
        for (idx, (pos, block_id)) in sorted.iter().enumerate() {
            let block_state = host.default_state(block_id);
            let _ = host.set_block(dimension, *pos, air_state, 3);

            // Spawn authoritative server `FallingBlock` entities for a subset of the blocks
            // so the collapsed rubble physically lands in the world and can hurt entities below!
            if let Some(state_id) = block_state {
                // Spawn falling blocks for up to 24 blocks per collapse to keep server TPS smooth
                if idx < 24 {
                    let rel_y = ((pos[1] as f32 + 0.5) - fracture_origin.y).max(0.2);
                    // Blocks higher up in a toppling tower get higher horizontal velocity (v = omega x r)
                    let horiz_speed = (0.06 + rel_y * 0.025).clamp(0.05, 0.38);
                    let vx = (topple_horiz.x * horiz_speed + rng.signed_f32() * 0.04) as f64;
                    let vy = (-0.04 + rng.range_f32(-0.05, 0.06)) as f64;
                    let vz = (topple_horiz.z * horiz_speed + rng.signed_f32() * 0.04) as f64;
                    let spawn_pos = [
                        pos[0] as f64 + 0.5,
                        pos[1] as f64,
                        pos[2] as f64 + 0.5,
                    ];
                    let _ = host.spawn_falling_block(
                        dimension,
                        state_id,
                        spawn_pos,
                        [vx, vy, vz],
                        NativeFalling {
                            drop_item: true,
                            hurts: true,
                        },
                    );
                }
            }
        }

        self.total_collapsed_blocks += cluster.len();

        if cfg.cinematic_tilt_clusters && self.queued_events.len() < 16 {
            self.queued_events.push(QueuedClusterEvent {
                blocks: cluster,
                fracture_origin,
            });
        }
    }

    /// Step and render all active client-side tilting structural collapse clusters in `Hooks::world_submit`.
    /// When a tilting cluster impacts the ground or completes its topple arc, its blocks burst into
    /// 3D Voronoi fracture shards in `debris_engine`!
    pub fn step_and_render(
        &mut self,
        host: &dyn Host,
        draw: &mut NativeWorldDraw,
        cfg: &StructuralConfig,
        debris_cfg: &DebrisConfig,
        debris_engine: &mut DebrisEngine,
        gravity: f32,
    ) {
        if !cfg.enabled {
            self.queued_events.clear();
            return;
        }

        // Promote any newly queued server collapse events into 3D tilting rigid clusters
        while let Some(ev) = self.queued_events.pop() {
            if self.active_clusters.len() >= 12 {
                break;
            }
            self.spawn_cinematic_cluster(ev, debris_cfg, debris_engine);
        }

        let time_key = (draw.game_time, draw.partial);
        let dt = if let Some((prev_gt, prev_pt)) = self.last_time_key {
            let ticks = (draw.game_time - prev_gt) as f32 + (draw.partial - prev_pt);
            (ticks * 0.05).clamp(0.0, 0.08)
        } else {
            0.016
        };
        self.last_time_key = Some(time_key);

        if dt > 0.0001 {
            self.step_clusters(host, dt, gravity, debris_cfg, debris_engine);
        }

        self.render_clusters(host, draw);
    }

    fn spawn_cinematic_cluster(
        &mut self,
        ev: QueuedClusterEvent,
        debris_cfg: &DebrisConfig,
        debris_engine: &mut DebrisEngine,
    ) {
        if ev.blocks.is_empty() {
            return;
        }
        // Find lowest block Y to serve as the hinge/pivot of the toppling structure
        let mut min_y = i32::MAX;
        let mut sum_xz = Vec3::ZERO;
        for &(p, _) in &ev.blocks {
            min_y = min_y.min(p[1]);
            sum_xz += Vec3::new(p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5);
        }
        let centroid = sum_xz / (ev.blocks.len() as f32);
        let pivot_pos = Vec3::new(centroid.x, min_y as f32, centroid.z);

        // Compute toppling torque axis perpendicular to `(centroid - fracture_origin)` and `Y`
        let away = (centroid - ev.fracture_origin).with_y(0.0);
        let tilt_dir = away.try_normalize().unwrap_or(Vec3::new(0.8, 0.0, 0.6));
        let torque_axis = Vec3::Y.cross(tilt_dir).normalize_or(Vec3::X);

        let members: Vec<ClusterMemberBlock> = ev
            .blocks
            .into_iter()
            .map(|(p, id)| ClusterMemberBlock {
                local_offset: Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) - pivot_pos,
                origin_pos: p,
                block_id: id,
            })
            .collect();

        // Spawn initial fracture shards right at the shear line
        if let Some(first) = members.first() {
            debris_engine.spawn_block_fracture(
                first.origin_pos,
                &first.block_id,
                Some(ev.fracture_origin),
                tilt_dir * 2.0,
                debris_cfg,
            );
        }

        self.active_clusters.push(CollapsingStructureCluster {
            pivot_pos,
            vel: tilt_dir * 1.2 + Vec3::new(0.0, -0.6, 0.0),
            rotation: Quat::IDENTITY,
            angular_vel: torque_axis * 1.45,
            blocks: members,
            age_secs: 0.0,
            max_duration_secs: 1.35,
        });
    }

    fn step_clusters(
        &mut self,
        host: &dyn Host,
        dt: f32,
        gravity: f32,
        debris_cfg: &DebrisConfig,
        debris_engine: &mut DebrisEngine,
    ) {
        let mut remaining = Vec::with_capacity(self.active_clusters.len());
        for mut cluster in self.active_clusters.drain(..) {
            cluster.age_secs += dt;
            // Accelerating gravitational topple + downward acceleration
            cluster.vel.y -= gravity * 0.48 * dt;
            cluster.angular_vel *= 1.0 + 1.15 * dt;
            cluster.pivot_pos += cluster.vel * dt;
            cluster.rotation = integrate_quat(cluster.rotation, cluster.angular_vel, dt);

            // Check if upper blocks of the tilting cluster have slammed into solid ground
            let mut impacted = cluster.age_secs >= cluster.max_duration_secs;
            if !impacted && cluster.age_secs > 0.30 {
                for member in cluster.blocks.iter().step_by(2) {
                    let world_p =
                        cluster.pivot_pos + cluster.rotation * (member.local_offset + Vec3::splat(0.5));
                    let check_voxel = [
                        world_p.x.floor() as i32,
                        (world_p.y - 0.35).floor() as i32,
                        world_p.z.floor() as i32,
                    ];
                    if is_client_voxel_solid(host, check_voxel) {
                        impacted = true;
                        break;
                    }
                }
            }

            if impacted {
                // Shatter a subset of the cluster's blocks into 3D Voronoi polyhedron shards on impact!
                let stride = (cluster.blocks.len() / 6).max(1);
                for member in cluster.blocks.iter().step_by(stride) {
                    let world_p =
                        cluster.pivot_pos + cluster.rotation * (member.local_offset + Vec3::splat(0.5));
                    let impact_voxel = [
                        world_p.x.floor() as i32,
                        world_p.y.floor() as i32,
                        world_p.z.floor() as i32,
                    ];
                    debris_engine.spawn_block_fracture(
                        impact_voxel,
                        &member.block_id,
                        None,
                        cluster.vel * 0.35,
                        debris_cfg,
                    );
                }
            } else {
                remaining.push(cluster);
            }
        }
        self.active_clusters = remaining;
    }

    fn render_clusters(&self, host: &dyn Host, draw: &mut NativeWorldDraw) {
        for cluster in &self.active_clusters {
            let (axis, deg) = quat_to_axis_angle_deg(cluster.rotation);
            for member in &cluster.blocks {
                draw.push();
                if draw.translate_world([
                    cluster.pivot_pos.x as f64,
                    cluster.pivot_pos.y as f64,
                    cluster.pivot_pos.z as f64,
                ]) {
                    if deg.abs() > 0.05 {
                        draw.rotate_about(axis, deg, [0.0, 0.0, 0.0]);
                    }
                    draw.translate(member.local_offset.to_array());
                    let _ = host.draw_block(draw, &member.block_id, &[]);
                }
                draw.pop();
            }
        }
    }
}

/// Perform a 3D structural BFS from `seed_pos` to determine whether its connected component
/// has a valid load-bearing foundation within its material cantilever (`max_overhang`) and
/// support reach (`support_reach`) limits.
///
/// Returns `Some(cluster_blocks)` if the component is **unsupported** and must collapse,
/// or `None` if the component is safely anchored to a foundation.
pub fn analyze_unsupported_cluster(
    host: &dyn Host,
    seed_pos: [i32; 3],
    already_detached: &HashSet<[i32; 3]>,
    cfg: &StructuralConfig,
) -> Option<Vec<([i32; 3], String)>> {
    let max_search = cfg.max_support_search_nodes.clamp(16, 1024);
    let max_cluster = cfg.max_collapse_cluster_size.clamp(4, 128);
    let strength_mult = cfg.strength_multiplier.clamp(0.25, 4.0);

    let seed_id = host.block(seed_pos);
    let seed_mat = classify_material(&seed_id);
    let max_overhang = ((seed_mat.max_overhang as f32) * strength_mult).round().max(1.0) as i32;
    let max_reach = ((seed_mat.support_reach as f32) * strength_mult).round().max(2.0) as i32;

    let mut visited: HashSet<[i32; 3]> = HashSet::with_capacity(64);
    let mut queue: VecDeque<([i32; 3], i32, i32)> = VecDeque::with_capacity(64);
    let mut cluster: Vec<([i32; 3], String)> = Vec::with_capacity(32);

    visited.insert(seed_pos);
    queue.push_back((seed_pos, 0, 0));

    while let Some((curr, horiz_dist, steps)) = queue.pop_front() {
        let block_id = host.block(curr);
        if is_air_or_fluid(&block_id) {
            continue;
        }

        // 1. Direct unbreakable anchor
        if is_unbreakable_anchor(&block_id) {
            return None;
        }

        // 2. Check if this block rests on a solid vertical ground column (3 consecutive natural foundation blocks downward)
        if has_vertical_ground_pillar(host, curr) {
            return None;
        }

        cluster.push((curr, block_id.clone()));
        if cluster.len() > max_cluster || visited.len() > max_search {
            // Connected to a massive landmass or large grounded building -> stable
            return None;
        }

        let mat = classify_material(&block_id);
        let local_max_overhang = ((mat.max_overhang as f32) * strength_mult)
            .round()
            .max(1.0) as i32
            + max_overhang;

        for d in NEIGHBORS_6 {
            let next = [curr[0] + d[0], curr[1] + d[1], curr[2] + d[2]];
            if already_detached.contains(&next) || visited.contains(&next) {
                continue;
            }
            // Don't climb far above the seed when looking for downward support
            if next[1] > seed_pos[1] + 5 {
                continue;
            }
            let next_horiz = horiz_dist + d[0].abs() + d[2].abs();
            let next_steps = steps + 1;
            if next_horiz > local_max_overhang || next_steps > max_reach {
                // Exceeded material cantilever span without finding a pillar -> shear failure!
                continue;
            }
            let next_id = host.block(next);
            if is_air_or_fluid(&next_id) {
                continue;
            }
            let next_mat = classify_material(&next_id);
            if !next_mat.is_structural && !is_natural_foundation(&next_id) {
                continue;
            }
            visited.insert(next);
            queue.push_back((next, next_horiz, next_steps));
        }
    }

    if cluster.is_empty() {
        None
    } else {
        Some(cluster)
    }
}

/// Returns true if `pos` is directly supported by a continuous vertical column of 4 solid blocks
/// reaching into natural ground terrain below it.
fn has_vertical_ground_pillar(host: &dyn Host, pos: [i32; 3]) -> bool {
    let mut natural_count = 0;
    for dy in 1..=4 {
        let below = [pos[0], pos[1] - dy, pos[2]];
        let id = host.block(below);
        if is_air_or_fluid(&id) {
            return false;
        }
        if is_unbreakable_anchor(&id) {
            return true;
        }
        if is_natural_foundation(&id) {
            natural_count += 1;
        }
    }
    natural_count >= 2
}
