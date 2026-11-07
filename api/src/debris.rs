//! Module 3 — **Block Fracture & Procedural 3D Voronoi Debris** (`debris`).
//!
//! When blocks break (`destroy_block`, `block_changed` to air) or are shattered by an explosion
//! (`explosion_detonate`), this module slices the $1\times 1\times 1\text{ m}^3$ block volume into
//! **non-cubic 3D Voronoi / plane-clipped convex polyhedra** (`ConvexShardMesh`):
//! - Each shard is computed via Sutherland–Hodgman 3D convex half-space clipping of the unit cube
//!   against Voronoi bisector planes between random seed points in the block!
//! - Material anisotropy (`FractureStyle::GlassShards`, `WoodSplinters`, `StonePolyhedra`,
//!   `MetalPlates`, `GranularClumps`, `FoliageFragments`) stretches and angles the Voronoi cells so:
//!   - **Glass** shatters into sharp, thin triangular/polygonal razor shards.
//!   - **Wood** splits along the grain into elongated splinters and planks.
//!   - **Stone & Bricks** fracture into irregular multi-faceted polyhedral rocks.
//!   - **Metal** breaks into heavy angular plates.
//! - Every polygonal face of a convex shard is decomposed into quads (`[[f32; 3]; 4]`) and rendered
//!   in `Hooks::world_submit` (`NativeWorldDraw` + `host.draw_quad`) with per-face directional shading,
//!   block-atlas texture coordinates (`[u0, v0, u1, v1]` in texels), and full 6-DoF quaternion rotation.
//! - Includes a secondary **Settling Dust Cloud** particle system after heavy impacts or collapses.

use glam::{Quat, Vec3};
use horizon_client::mod_api::{Host, NativeWorldDraw};

use crate::config::DebrisConfig;
use crate::materials::{FractureStyle, MaterialProfile, classify_material, is_air_or_fluid};
use crate::math_phys::{
    FastRng, integrate_quat, is_client_voxel_water, quat_to_axis_angle_deg,
    resolve_sphere_voxel_collision,
};

/// One quad face of a 3D convex polyhedron shard, in local coordinates centered at the shard's centroid.
#[derive(Debug, Clone, Copy)]
pub struct ShardQuadFace {
    /// Four corners in local block units around the shard's centroid.
    pub corners: [[f32; 3]; 4],
    /// Outward unit normal in local space.
    pub normal: Vec3,
    /// Texture sub-rectangle `[u0, v0, u1, v1]` in $0..16$ texels on the block's sprite.
    pub uv: [f32; 4],
}

/// A 3D non-cubic Voronoi / plane-clipped convex polyhedron shard with full 6-DoF rigid-body state.
#[derive(Debug, Clone)]
pub struct VoronoiDebrisShard {
    /// World-space position of the shard's center of mass.
    pub pos: Vec3,
    /// World-space linear velocity ($\text{m/s}$).
    pub vel: Vec3,
    /// 3D orientation quaternion.
    pub rotation: Quat,
    /// World-space angular velocity ($\text{rad/s}$).
    pub angular_vel: Vec3,
    /// Effective bounding sphere radius for voxel collision ($\text{m}$).
    pub collision_radius: f32,
    /// Mass in $\text{kg}$.
    pub mass: f32,
    /// Material profile of the parent block.
    pub material: MaterialProfile,
    /// Polygonal faces of the convex polyhedron decomposed into renderable quads.
    pub faces: Vec<ShardQuadFace>,
    /// Elapsed lifetime ($\text{s}$).
    pub age_secs: f32,
    /// Maximum lifetime ($\text{s}$) before smooth scale-down removal.
    pub max_lifetime_secs: f32,
    /// Whether this shard has settled into a resting debris pile.
    pub settled: bool,
}

/// Settling dust mote spawned after block shatter or structural collapse.
#[derive(Debug, Clone, Copy)]
pub struct SettlingDustMote {
    pub pos: Vec3,
    pub vel: Vec3,
    pub size: f32,
    pub age_secs: f32,
    pub max_lifetime_secs: f32,
    pub sprite: &'static str,
    pub tint: u32,
}

/// Manages all active 3D Voronoi fracture shards and settling dust motes.
pub struct DebrisEngine {
    shards: Vec<VoronoiDebrisShard>,
    dust: Vec<SettlingDustMote>,
    last_time_key: Option<(i64, f32)>,
    seed_counter: u64,
}

impl Default for DebrisEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DebrisEngine {
    pub fn new() -> Self {
        Self {
            shards: Vec::with_capacity(320),
            dust: Vec::with_capacity(128),
            last_time_key: None,
            seed_counter: 1,
        }
    }

    pub fn active_shard_count(&self) -> usize {
        self.shards.len()
    }

    pub fn clear(&mut self) {
        self.shards.clear();
        self.dust.clear();
    }

    /// Spawn a full set of 3D Voronoi polyhedral fracture shards for a broken block at `block_pos`.
    pub fn spawn_block_fracture(
        &mut self,
        block_pos: [i32; 3],
        block_id: &str,
        blast_origin: Option<Vec3>,
        extra_impulse: Vec3,
        cfg: &DebrisConfig,
    ) {
        if !cfg.enabled || is_air_or_fluid(block_id) {
            return;
        }
        let mat = classify_material(block_id);
        let raw_count = ((cfg.shards_per_block as f32) * mat.shard_multiplier).round() as usize;
        let shard_count = raw_count.clamp(3, 18);

        // Enforce global cap by dropping the oldest shards first
        let needed = shard_count;
        if self.shards.len() + needed > cfg.max_active_shards {
            let excess = (self.shards.len() + needed).saturating_sub(cfg.max_active_shards);
            let drain_n = excess.min(self.shards.len());
            self.shards.drain(0..drain_n);
        }

        self.seed_counter = self.seed_counter.wrapping_add(1);
        let mut rng = FastRng::from_pos_and_id(block_pos, self.seed_counter);
        let block_min = Vec3::new(block_pos[0] as f32, block_pos[1] as f32, block_pos[2] as f32);
        let block_center = block_min + Vec3::splat(0.5);

        // 1. Generate `shard_count` jittered Voronoi seed points inside $[0, 1]^3$
        let seeds = generate_voronoi_seeds(shard_count, mat.fracture_style, &mut rng);

        // 2. For each Voronoi seed, clip the $[0, 1]^3$ block by the perpendicular bisector planes
        //    against neighboring seeds (Sutherland-Hodgman 3D convex polyhedron clipping!)
        let aniso = mat.fracture_anisotropy();
        for i in 0..seeds.len() {
            let (centroid_local, radius, faces) =
                build_voronoi_cell_polyhedron(i, &seeds, aniso, &mut rng);
            if faces.is_empty() {
                continue;
            }

            let world_pos = block_min + centroid_local;
            let outward = (centroid_local - Vec3::splat(0.5)).normalize_or(rng.unit_vec3());

            let base_vel = if let Some(origin) = blast_origin {
                let delta = world_pos - origin;
                let dist = delta.length().max(0.35);
                let dir = delta / dist;
                let speed = (14.0 / dist).clamp(3.0, 18.0) * rng.range_f32(0.8, 1.3);
                dir * speed + Vec3::new(0.0, rng.range_f32(1.5, 4.5), 0.0) + extra_impulse
            } else {
                outward * rng.range_f32(1.2, 3.4)
                    + Vec3::new(0.0, rng.range_f32(1.2, 3.0), 0.0)
                    + extra_impulse
            };

            let angular_vel = Vec3::new(
                rng.signed_f32() * 11.0,
                rng.signed_f32() * 11.0,
                rng.signed_f32() * 11.0,
            );
            let vol_fraction = (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3);
            let lifetime = cfg.shard_lifetime_secs * rng.range_f32(0.75, 1.25);

            self.shards.push(VoronoiDebrisShard {
                pos: world_pos,
                vel: base_vel,
                rotation: Quat::IDENTITY,
                angular_vel,
                collision_radius: radius.clamp(0.06, 0.28),
                mass: mat.shard_mass(vol_fraction),
                material: mat,
                faces,
                age_secs: 0.0,
                max_lifetime_secs: lifetime.max(1.5),
                settled: false,
            });
        }

        // 3. Spawn settling dust motes if enabled
        if cfg.settling_dust && self.dust.len() < 160 {
            let dust_count = if blast_origin.is_some() { 5 } else { 2 };
            let tint = pack_rgb_tint(mat.tint, 0.82);
            for _ in 0..dust_count {
                let offset = Vec3::new(
                    rng.range_f32(-0.4, 0.4),
                    rng.range_f32(-0.3, 0.4),
                    rng.range_f32(-0.4, 0.4),
                );
                let vel = Vec3::new(
                    rng.signed_f32() * 1.1,
                    rng.range_f32(0.3, 1.4),
                    rng.signed_f32() * 1.1,
                );
                self.dust.push(SettlingDustMote {
                    pos: block_center + offset,
                    vel,
                    size: rng.range_f32(0.14, 0.28),
                    age_secs: 0.0,
                    max_lifetime_secs: rng.range_f32(2.2, 4.5),
                    sprite: mat.sprite,
                    tint,
                });
            }
        }
    }

    /// Apply a radial blast wave impulse to all existing debris shards & wake settled piles.
    pub fn apply_explosion_wave(&mut self, center: Vec3, radius: f32, impulse_strength: f32) {
        for shard in &mut self.shards {
            let delta = shard.pos - center;
            let dist = delta.length();
            if dist < radius && dist > 0.05 {
                let falloff = (1.0 - dist / radius).powi(2);
                let dir = (delta / dist + Vec3::new(0.0, 0.35, 0.0)).normalize_or(Vec3::Y);
                let dv = (impulse_strength * falloff * 180.0 / shard.mass.max(25.0)).clamp(1.0, 22.0);
                shard.vel += dir * dv;
                shard.angular_vel += Vec3::new(dir.z * 9.0, 4.0, -dir.x * 9.0) * falloff;
                shard.settled = false;
            }
        }
    }

    /// Step all active Voronoi shards and settling dust motes and render them in `Hooks::world_submit`.
    pub fn step_and_render(
        &mut self,
        host: &dyn Host,
        draw: &mut NativeWorldDraw,
        cfg: &DebrisConfig,
        gravity: f32,
    ) {
        if !cfg.enabled {
            return;
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
            self.step_physics(host, cfg, gravity, dt);
        }

        self.render_all(host, draw);
    }

    fn step_physics(&mut self, host: &dyn Host, cfg: &DebrisConfig, gravity: f32, dt: f32) {
        for shard in &mut self.shards {
            shard.age_secs += dt;
            if shard.settled {
                continue;
            }

            let voxel = [
                shard.pos.x.floor() as i32,
                shard.pos.y.floor() as i32,
                shard.pos.z.floor() as i32,
            ];

            // Check water buoyancy vs gravity
            if is_client_voxel_water(host, voxel) {
                let buoy = shard.material.water_buoyancy_factor();
                shard.vel.y += buoy * gravity * 0.55 * dt;
                shard.vel *= (1.0 - 2.8 * dt).clamp(0.7, 0.98);
                shard.angular_vel *= (1.0 - 3.2 * dt).clamp(0.7, 0.98);
            } else {
                let drag = if shard.material.fracture_style == FractureStyle::FoliageFragments {
                    2.5
                } else {
                    0.35
                };
                shard.vel.y -= gravity * dt;
                shard.vel *= (1.0 - drag * dt).clamp(0.8, 0.999);
            }

            shard.pos += shard.vel * dt;
            shard.rotation = integrate_quat(shard.rotation, shard.angular_vel, dt);

            if cfg.world_collision {
                let contact = resolve_sphere_voxel_collision(
                    host,
                    &mut shard.pos,
                    &mut shard.vel,
                    shard.collision_radius,
                    shard.material.restitution,
                    shard.material.friction,
                );
                if contact.collided {
                    // Tangential rolling torque: omega = (n x v) / r
                    let tangent_v = shard.vel - contact.normal * shard.vel.dot(contact.normal);
                    let roll_omega =
                        contact.normal.cross(tangent_v) / shard.collision_radius.max(0.08);
                    shard.angular_vel = shard.angular_vel * 0.45 + roll_omega * 0.55;

                    if shard.vel.length_squared() < 0.04
                        && contact.normal.y > 0.65
                        && shard.age_secs > 0.45
                    {
                        shard.vel = Vec3::ZERO;
                        shard.angular_vel = Vec3::ZERO;
                        shard.settled = true;
                    }
                }
            }
        }

        // Inter-shard pile repulsion so settled/colliding fragments form natural mounds instead of overlapping
        let len = self.shards.len();
        if len > 1 && len <= 160 {
            for i in 0..len {
                for j in (i + 1)..len {
                    let delta = self.shards[j].pos - self.shards[i].pos;
                    let min_dist =
                        (self.shards[i].collision_radius + self.shards[j].collision_radius) * 0.82;
                    let dist_sq = delta.length_squared();
                    if dist_sq < min_dist * min_dist && dist_sq > 1e-5 {
                        let dist = dist_sq.sqrt();
                        let n = delta / dist;
                        let push = (min_dist - dist) * 0.35;
                        self.shards[i].pos -= n * push;
                        self.shards[j].pos += n * push;
                    }
                }
            }
        }

        self.shards
            .retain(|s| s.age_secs < s.max_lifetime_secs && s.pos.y > -128.0);

        // Step settling dust motes
        for mote in &mut self.dust {
            mote.age_secs += dt;
            // Slow terminal settling velocity with gentle horizontal drift
            mote.vel.y = (mote.vel.y - 1.8 * dt).max(-0.45);
            mote.vel.x *= (1.0 - 1.5 * dt).clamp(0.8, 1.0);
            mote.vel.z *= (1.0 - 1.5 * dt).clamp(0.8, 1.0);
            mote.pos += mote.vel * dt;
        }
        self.dust.retain(|m| m.age_secs < m.max_lifetime_secs);
    }

    fn render_all(&self, host: &dyn Host, draw: &mut NativeWorldDraw) {
        let cam = Vec3::new(
            draw.camera[0] as f32,
            draw.camera[1] as f32,
            draw.camera[2] as f32,
        );

        // Render all 3D Voronoi polyhedron shards
        for shard in &self.shards {
            if (shard.pos - cam).length_squared() > 72.0 * 72.0 {
                continue;
            }
            let remaining = (shard.max_lifetime_secs - shard.age_secs).max(0.0);
            let shrink = if remaining < 0.75 {
                (remaining / 0.75).clamp(0.05, 1.0)
            } else {
                1.0
            };

            draw.push();
            if draw.translate_world([
                shard.pos.x as f64,
                shard.pos.y as f64,
                shard.pos.z as f64,
            ]) {
                let (axis, deg) = quat_to_axis_angle_deg(shard.rotation);
                if deg.abs() > 0.05 {
                    draw.rotate_about(axis, deg, [0.0, 0.0, 0.0]);
                }
                if (shrink - 1.0).abs() > 0.01 {
                    draw.scale([shrink, shrink, shrink]);
                }

                // Render every polygonal face of the Voronoi polyhedron
                for face in &shard.faces {
                    let world_n = shard.rotation * face.normal;
                    let shade = (0.72 + 0.28 * world_n.y).clamp(0.48, 1.0);
                    let tint = pack_rgb_tint(shard.material.tint, shade);
                    let _ = host.draw_quad(
                        draw,
                        shard.material.sprite,
                        face.corners,
                        face.uv,
                        tint,
                    );
                }
            }
            draw.pop();
        }

        // Render settling dust motes as camera-aligned billboards
        for mote in &self.dust {
            if (mote.pos - cam).length_squared() > 48.0 * 48.0 {
                continue;
            }
            let life_frac = (1.0 - mote.age_secs / mote.max_lifetime_secs).clamp(0.0, 1.0);
            let half = mote.size * life_frac * 0.5;
            if half < 0.01 {
                continue;
            }
            draw.push();
            if draw.translate_world([mote.pos.x as f64, mote.pos.y as f64, mote.pos.z as f64]) {
                let corners = [
                    [-half, half, 0.0],
                    [-half, -half, 0.0],
                    [half, -half, 0.0],
                    [half, half, 0.0],
                ];
                let _ = host.draw_quad(draw, mote.sprite, corners, [4.0, 4.0, 12.0, 12.0], mote.tint);
            }
            draw.pop();
        }
    }
}

fn pack_rgb_tint(rgb: [f32; 3], shade: f32) -> u32 {
    let r = ((rgb[0] * shade * 255.0).round().clamp(0.0, 255.0)) as u32;
    let g = ((rgb[1] * shade * 255.0).round().clamp(0.0, 255.0)) as u32;
    let b = ((rgb[2] * shade * 255.0).round().clamp(0.0, 255.0)) as u32;
    (r << 16) | (g << 8) | b
}

/// Generate `count` jittered 3D Voronoi seed points inside $[0.08, 0.92]^3$,
/// distributed according to `style` (e.g., elongated grain for wood, radial for glass).
fn generate_voronoi_seeds(count: usize, style: FractureStyle, rng: &mut FastRng) -> Vec<Vec3> {
    let mut seeds = Vec::with_capacity(count);
    for i in 0..count {
        let t = (i as f32 + 0.5) / (count as f32);
        let p = match style {
            FractureStyle::WoodSplinters => {
                // Cluster seeds across X/Z so cells stretch vertically/horizontally like wood splinters
                Vec3::new(
                    rng.range_f32(0.12, 0.88),
                    (t + rng.signed_f32() * 0.15).clamp(0.1, 0.9),
                    rng.range_f32(0.12, 0.88),
                )
            }
            FractureStyle::GlassShards => {
                // Radial fracture pattern emanating from an impact point near the center
                let angle = (i as f32) * 2.399_963 + rng.signed_f32() * 0.25;
                let rad = rng.range_f32(0.12, 0.44);
                Vec3::new(
                    (0.5 + rad * angle.cos()).clamp(0.08, 0.92),
                    (0.5 + rad * angle.sin()).clamp(0.08, 0.92),
                    rng.range_f32(0.18, 0.82),
                )
            }
            _ => Vec3::new(
                rng.range_f32(0.10, 0.90),
                rng.range_f32(0.10, 0.90),
                rng.range_f32(0.10, 0.90),
            ),
        };
        seeds.push(p);
    }
    seeds
}

/// A 3D convex polygon face during Sutherland-Hodgman Voronoi cell clipping.
#[derive(Clone)]
struct ConvexPolygon3D {
    verts: Vec<Vec3>,
    normal: Vec3,
}

/// Compute the exact 3D Voronoi cell around `seeds[cell_idx]` inside the $[0, 1]^3$ unit block
/// by clipping the unit cube's polygons against perpendicular bisector half-spaces!
/// Returns `(centroid_in_block, bounding_radius, quad_faces_centered_at_centroid)`.
pub fn build_voronoi_cell_polyhedron(
    cell_idx: usize,
    seeds: &[Vec3],
    anisotropy: Vec3,
    rng: &mut FastRng,
) -> (Vec3, f32, Vec<ShardQuadFace>) {
    // Start with the 6 faces of the unit cube $[0, 1]^3$ (CCW winding as seen from outside)
    let v000 = Vec3::new(0.0, 0.0, 0.0);
    let v100 = Vec3::new(1.0, 0.0, 0.0);
    let v110 = Vec3::new(1.0, 1.0, 0.0);
    let v010 = Vec3::new(0.0, 1.0, 0.0);
    let v001 = Vec3::new(0.0, 0.0, 1.0);
    let v101 = Vec3::new(1.0, 0.0, 1.0);
    let v111 = Vec3::new(1.0, 1.0, 1.0);
    let v011 = Vec3::new(0.0, 1.0, 1.0);

    let mut polys = vec![
        ConvexPolygon3D {
            verts: vec![v010, v000, v100, v110],
            normal: Vec3::NEG_Z,
        },
        ConvexPolygon3D {
            verts: vec![v111, v101, v001, v011],
            normal: Vec3::Z,
        },
        ConvexPolygon3D {
            verts: vec![v011, v001, v000, v010],
            normal: Vec3::NEG_X,
        },
        ConvexPolygon3D {
            verts: vec![v110, v100, v101, v111],
            normal: Vec3::X,
        },
        ConvexPolygon3D {
            verts: vec![v000, v001, v101, v100],
            normal: Vec3::NEG_Y,
        },
        ConvexPolygon3D {
            verts: vec![v011, v010, v110, v111],
            normal: Vec3::Y,
        },
    ];

    let seed_a = seeds[cell_idx];

    // Clip against bisector planes of neighboring Voronoi seeds
    for (j, &seed_b) in seeds.iter().enumerate() {
        if j == cell_idx {
            continue;
        }
        let raw_delta = (seed_b - seed_a) * anisotropy;
        let len = raw_delta.length();
        if len < 1e-4 {
            continue;
        }
        let plane_normal = (raw_delta / len).normalize();
        // Slight fracture surface roughness jitter
        let mid = (seed_a + seed_b) * 0.5 + plane_normal * (rng.signed_f32() * 0.015);
        let plane_d = plane_normal.dot(mid);

        polys = clip_convex_polyhedron_by_plane(polys, plane_normal, plane_d);
        if polys.is_empty() {
            break;
        }
    }

    // Gather all unique vertices to compute centroid and bounding radius
    let mut sum = Vec3::ZERO;
    let mut count = 0usize;
    for poly in &polys {
        for &v in &poly.verts {
            sum += v;
            count += 1;
        }
    }
    if count == 0 {
        return (seed_a, 0.12, Vec::new());
    }
    let centroid = sum / (count as f32);

    let mut max_r_sq = 0.005_f32;
    for poly in &polys {
        for &v in &poly.verts {
            max_r_sq = max_r_sq.max((v - centroid).length_squared());
        }
    }
    let radius = max_r_sq.sqrt();

    // Convert each N-gon face into 1 or more `ShardQuadFace`s (triangle fan packed as quads with
    // duplicated 4th vertex when needed, or direct quad for 4-vertex faces).
    let mut quad_faces = Vec::with_capacity(polys.len() * 2);
    for poly in &polys {
        let n = poly.verts.len();
        if n < 3 {
            continue;
        }
        // Compute UV window from the face's bounding box in $[0, 1]^3$ mapped to $[0, 16]$ texels
        let face_center = poly.verts.iter().copied().sum::<Vec3>() / (n as f32);
        let u0 = (face_center.x * 10.0).clamp(0.0, 10.0);
        let v0 = (face_center.z * 10.0).clamp(0.0, 10.0);
        let uv = [u0, v0, (u0 + 6.0).min(16.0), (v0 + 6.0).min(16.0)];

        if n == 3 {
            let a = (poly.verts[0] - centroid).to_array();
            let b = (poly.verts[1] - centroid).to_array();
            let c = (poly.verts[2] - centroid).to_array();
            quad_faces.push(ShardQuadFace {
                corners: [a, b, c, c],
                normal: poly.normal,
                uv,
            });
        } else if n == 4 {
            quad_faces.push(ShardQuadFace {
                corners: [
                    (poly.verts[0] - centroid).to_array(),
                    (poly.verts[1] - centroid).to_array(),
                    (poly.verts[2] - centroid).to_array(),
                    (poly.verts[3] - centroid).to_array(),
                ],
                normal: poly.normal,
                uv,
            });
        } else {
            // Fan decompose N-gon (N >= 5) into quads + triangle
            let v0_rel = (poly.verts[0] - centroid).to_array();
            let mut k = 1;
            while k + 2 < n {
                quad_faces.push(ShardQuadFace {
                    corners: [
                        v0_rel,
                        (poly.verts[k] - centroid).to_array(),
                        (poly.verts[k + 1] - centroid).to_array(),
                        (poly.verts[k + 2] - centroid).to_array(),
                    ],
                    normal: poly.normal,
                    uv,
                });
                k += 2;
            }
            if k + 1 < n {
                let b = (poly.verts[k] - centroid).to_array();
                let c = (poly.verts[k + 1] - centroid).to_array();
                quad_faces.push(ShardQuadFace {
                    corners: [v0_rel, b, c, c],
                    normal: poly.normal,
                    uv,
                });
            }
        }
    }

    (centroid, radius, quad_faces)
}

/// Clip a 3D convex polyhedron (represented by its boundary polygons) against the half-space
/// `plane_normal.dot(p) <= plane_d`, and construct the new cap polygon along the cut plane!
fn clip_convex_polyhedron_by_plane(
    polys: Vec<ConvexPolygon3D>,
    plane_normal: Vec3,
    plane_d: f32,
) -> Vec<ConvexPolygon3D> {
    let mut out_polys = Vec::with_capacity(polys.len() + 1);
    let mut cap_points: Vec<Vec3> = Vec::new();

    for poly in polys {
        let n = poly.verts.len();
        if n < 3 {
            continue;
        }
        let mut clipped = Vec::with_capacity(n + 2);
        for i in 0..n {
            let a = poly.verts[i];
            let b = poly.verts[(i + 1) % n];
            let da = plane_normal.dot(a) - plane_d;
            let db = plane_normal.dot(b) - plane_d;
            let a_in = da <= 1e-5;
            let b_in = db <= 1e-5;

            if a_in && b_in {
                clipped.push(b);
            } else if a_in && !b_in {
                let t = (da / (da - db)).clamp(0.0, 1.0);
                let hit = a + (b - a) * t;
                clipped.push(hit);
                push_unique_point(&mut cap_points, hit);
            } else if !a_in && b_in {
                let t = (da / (da - db)).clamp(0.0, 1.0);
                let hit = a + (b - a) * t;
                clipped.push(hit);
                clipped.push(b);
                push_unique_point(&mut cap_points, hit);
            }
        }
        if clipped.len() >= 3 {
            out_polys.push(ConvexPolygon3D {
                verts: clipped,
                normal: poly.normal,
            });
        }
    }

    // Build the cap polygon closing the cut plane if at least 3 intersection points were formed
    if cap_points.len() >= 3 {
        let center = cap_points.iter().copied().sum::<Vec3>() / (cap_points.len() as f32);
        let up_ref = if plane_normal.y.abs() < 0.9 {
            Vec3::Y
        } else {
            Vec3::X
        };
        let tangent_u = plane_normal.cross(up_ref).normalize_or(Vec3::X);
        let tangent_v = plane_normal.cross(tangent_u).normalize_or(Vec3::Z);

        cap_points.sort_by(|a, b| {
            let da = *a - center;
            let db = *b - center;
            let ang_a = da.dot(tangent_v).atan2(da.dot(tangent_u));
            let ang_b = db.dot(tangent_v).atan2(db.dot(tangent_u));
            ang_a.partial_cmp(&ang_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        out_polys.push(ConvexPolygon3D {
            verts: cap_points,
            normal: plane_normal,
        });
    }

    out_polys
}

fn push_unique_point(points: &mut Vec<Vec3>, p: Vec3) {
    if !points.iter().any(|&q| (q - p).length_squared() < 1e-6) {
        points.push(p);
    }
}
