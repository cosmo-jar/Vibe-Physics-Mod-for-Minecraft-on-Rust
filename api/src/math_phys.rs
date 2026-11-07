//! Core 3D Physics & Math Solver (`math_phys`) for **Vibe Physics**.
//!
//! Provides:
//! - Deterministic fast PRNG (`FastRng`) for procedural Voronoi fracture, jitter, and angular impulses.
//! - Quaternion integration from angular velocity ($\dot{q} = \frac{1}{2} \omega q$).
//! - XPBD (Extended Position-Based Dynamics) distance and angular-cone constraint solvers for ragdoll joints.
//! - Sphere/Point-vs-Voxel AABB collision detection and impulse resolution with restitution & Coulomb friction.
//! - 3D DDA (Amanatides & Woo) voxel raycasting for shockwave occlusion & line-of-sight shielding.

use glam::{Quat, Vec3};
use horizon_client::mod_api::Host;

use crate::materials::is_air_or_fluid;

/// Lightweight, deterministic xorshift64* pseudo-random generator (no external `rand` crate needed).
#[derive(Debug, Clone, Copy)]
pub struct FastRng {
    state: u64,
}

impl FastRng {
    pub fn from_seed(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn from_pos_and_id(pos: [i32; 3], id: u64) -> Self {
        let mut h = id.wrapping_mul(0x517C_C1B7_2722_0A95);
        h ^= (pos[0] as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= (pos[1] as i64 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= (pos[2] as i64 as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self::from_seed(h)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        if x == 0 {
            x = 0xCAFE_BABE_DEAD_BEEF;
        }
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform float in $[0.0, 1.0)$.
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as u32 as f32) * (1.0 / (1u32 << 24) as f32)
    }

    /// Uniform float in $[min, max]$.
    pub fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    /// Uniform signed float in $[-1.0, 1.0]$.
    pub fn signed_f32(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }

    /// Approximately uniform unit vector on $S^2$.
    pub fn unit_vec3(&mut self) -> Vec3 {
        let z = self.signed_f32();
        let theta = self.next_f32() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * theta.cos(), z, r * theta.sin())
    }
}

/// Integrate a unit quaternion `q` by world-space angular velocity `omega` ($\text{rad/s}$) over `dt`.
pub fn integrate_quat(q: Quat, omega: Vec3, dt: f32) -> Quat {
    let speed = omega.length();
    if speed < 1e-5 || dt <= 0.0 {
        return q.normalize();
    }
    let half_angle = 0.5 * speed * dt;
    let axis = omega / speed;
    let dq = Quat::from_axis_angle(axis, half_angle * 2.0);
    (dq * q).normalize()
}

/// Convert a unit quaternion `q` into an `(axis, angle_degrees)` pair suitable for
/// `BlockEntityDraw::rotate_about`.
pub fn quat_to_axis_angle_deg(q: Quat) -> ([f32; 3], f32) {
    let qn = q.normalize();
    let (axis, angle_rad) = qn.to_axis_angle();
    let deg = angle_rad.to_degrees();
    if deg.abs() < 0.01 || !axis.is_finite() {
        ([0.0, 1.0, 0.0], 0.0)
    } else {
        (axis.to_array(), deg)
    }
}

/// XPBD distance constraint between two particles with positions `p1`, `p2` and inverse masses `w1`, `w2`.
/// Enforces $|p_2 - p_1| = \text{rest\_length}$ scaled by `stiffness` $\in [0, 1]$.
pub fn solve_distance_constraint(
    p1: &mut Vec3,
    w1: f32,
    p2: &mut Vec3,
    w2: f32,
    rest_length: f32,
    stiffness: f32,
) {
    let w_sum = w1 + w2;
    if w_sum <= 1e-6 {
        return;
    }
    let delta = *p2 - *p1;
    let dist = delta.length();
    if dist <= 1e-6 {
        return;
    }
    let diff = (dist - rest_length) / dist;
    let correction = delta * (diff * stiffness / w_sum);
    *p1 += correction * w1;
    *p2 -= correction * w2;
}

/// XPBD angular cone constraint: keeps unit vector `(child - parent)` within `max_angle_rad`
/// of `reference_dir` while preserving distance `bone_len`.
pub fn solve_cone_constraint(
    parent: Vec3,
    child: &mut Vec3,
    reference_dir: Vec3,
    bone_len: f32,
    max_angle_rad: f32,
    stiffness: f32,
) {
    let offset = *child - parent;
    let len = offset.length();
    if len <= 1e-5 {
        return;
    }
    let dir = offset / len;
    let ref_n = reference_dir.normalize_or_zero();
    if ref_n.length_squared() < 0.5 {
        return;
    }
    let cos_theta = dir.dot(ref_n).clamp(-1.0, 1.0);
    let theta = cos_theta.acos();
    if theta > max_angle_rad {
        let excess = (theta - max_angle_rad) * stiffness;
        let rot_axis = dir.cross(ref_n).normalize_or_zero();
        if rot_axis.length_squared() > 0.5 {
            let clamped_dir = (Quat::from_axis_angle(rot_axis, excess) * dir).normalize();
            *child = parent + clamped_dir * bone_len;
        }
    }
}

/// Returns true if the client world has a solid (collidable) block at integer coordinates `pos`.
pub fn is_client_voxel_solid(host: &dyn Host, pos: [i32; 3]) -> bool {
    let id = host.client_block(pos);
    if is_air_or_fluid(&id) {
        return false;
    }
    let b = crate::materials::bare_id(&id);
    // Passable non-solid flora/decorations
    !matches!(
        b,
        "short_grass"
            | "tall_grass"
            | "fern"
            | "large_fern"
            | "dead_bush"
            | "dandelion"
            | "poppy"
            | "torch"
            | "wall_torch"
            | "soul_torch"
            | "redstone_torch"
            | "redstone_wire"
            | "lever"
            | "tripwire"
            | "rail"
            | "powered_rail"
            | "detector_rail"
            | "activator_rail"
            | "snow"
            | "vine"
            | "glow_lichen"
            | "sugar_cane"
            | "Fire"
            | "fire"
            | "soul_fire"
    )
}

/// Returns true if the client block at `pos` is water.
pub fn is_client_voxel_water(host: &dyn Host, pos: [i32; 3]) -> bool {
    let id = host.client_block(pos);
    let b = crate::materials::bare_id(&id);
    b == "water" || b == "flowing_water" || b == "bubble_column"
}

/// Contact result from resolving a sphere of `radius` against the client voxel grid.
#[derive(Debug, Clone, Copy)]
pub struct VoxelContact {
    pub collided: bool,
    pub normal: Vec3,
    pub impact_speed: f32,
    pub ground_block: [i32; 3],
}

/// Resolve collision of a sphere at `pos` with `vel` and `radius` against solid blocks in the client world.
/// Applies normal restitution and tangential Coulomb friction.
pub fn resolve_sphere_voxel_collision(
    host: &dyn Host,
    pos: &mut Vec3,
    vel: &mut Vec3,
    radius: f32,
    restitution: f32,
    friction: f32,
) -> VoxelContact {
    let mut contact = VoxelContact {
        collided: false,
        normal: Vec3::Y,
        impact_speed: 0.0,
        ground_block: [
            pos.x.floor() as i32,
            (pos.y - radius - 0.05).floor() as i32,
            pos.z.floor() as i32,
        ],
    };

    let min_x = (pos.x - radius).floor() as i32;
    let max_x = (pos.x + radius).floor() as i32;
    let min_y = (pos.y - radius).floor() as i32;
    let max_y = (pos.y + radius).floor() as i32;
    let min_z = (pos.z - radius).floor() as i32;
    let max_z = (pos.z + radius).floor() as i32;

    for by in min_y..=max_y {
        for bx in min_x..=max_x {
            for bz in min_z..=max_z {
                let bpos = [bx, by, bz];
                if !is_client_voxel_solid(host, bpos) {
                    continue;
                }
                let box_min = Vec3::new(bx as f32, by as f32, bz as f32);
                let box_max = box_min + Vec3::ONE;
                let closest = pos.clamp(box_min, box_max);
                let delta = *pos - closest;
                let dist_sq = delta.length_squared();

                if dist_sq < radius * radius {
                    let (normal, penetration) = if dist_sq > 1e-6 {
                        let dist = dist_sq.sqrt();
                        (delta / dist, radius - dist)
                    } else {
                        // Sphere center is inside the unit voxel; push out through nearest face
                        let center = box_min + Vec3::splat(0.5);
                        let local = *pos - center;
                        let ax = 0.5 - local.x.abs();
                        let ay = 0.5 - local.y.abs();
                        let az = 0.5 - local.z.abs();
                        if ay <= ax && ay <= az {
                            let ny = if local.y >= 0.0 { 1.0 } else { -1.0 };
                            (Vec3::new(0.0, ny, 0.0), ay + radius)
                        } else if ax <= az {
                            let nx = if local.x >= 0.0 { 1.0 } else { -1.0 };
                            (Vec3::new(nx, 0.0, 0.0), ax + radius)
                        } else {
                            let nz = if local.z >= 0.0 { 1.0 } else { -1.0 };
                            (Vec3::new(0.0, 0.0, nz), az + radius)
                        }
                    };

                    *pos += normal * penetration;
                    let vn = vel.dot(normal);
                    if vn < 0.0 {
                        let impact = -vn;
                        if impact > contact.impact_speed {
                            contact.impact_speed = impact;
                            contact.normal = normal;
                            contact.ground_block = bpos;
                        }
                        let normal_vel = normal * vn;
                        let tangent_vel = *vel - normal_vel;
                        let bounce = if impact > 0.65 { restitution } else { 0.0 };
                        let fric_retain = (1.0 - friction).clamp(0.0, 0.99);
                        *vel = tangent_vel * fric_retain - normal_vel * bounce;
                    }
                    contact.collided = true;
                }
            }
        }
    }

    contact
}

/// Raycast along the segment `start -> end` in the server world (`host.block`) and compute
/// the blast transmission factor $\in [0.0, 1.0]$ and any fragile blocks intersected along the ray.
pub fn raycast_shockwave_transmission(
    host: &dyn Host,
    start: Vec3,
    end: Vec3,
    collect_fragile: bool,
    fragile_out: &mut Vec<[i32; 3]>,
) -> f32 {
    let dir = end - start;
    let total_dist = dir.length();
    if total_dist <= 0.25 {
        return 1.0;
    }
    let step_dir = dir / total_dist;
    let steps = (total_dist * 2.0).ceil() as i32;
    let mut transmission = 1.0_f32;
    let mut last_voxel = [i32::MIN, i32::MIN, i32::MIN];

    for i in 1..steps {
        let t = (i as f32) * 0.5;
        if t >= total_dist - 0.35 {
            break;
        }
        let sample = start + step_dir * t;
        let voxel = [
            sample.x.floor() as i32,
            sample.y.floor() as i32,
            sample.z.floor() as i32,
        ];
        if voxel == last_voxel {
            continue;
        }
        last_voxel = voxel;

        let block_id = host.block(voxel);
        if is_air_or_fluid(&block_id) {
            continue;
        }
        let mat = crate::materials::classify_material(&block_id);
        if collect_fragile && mat.shockwave_fragile && transmission > 0.15 {
            if !fragile_out.contains(&voxel) {
                fragile_out.push(voxel);
            }
        }
        transmission *= (1.0 - mat.blast_absorption).clamp(0.0, 1.0);
        if transmission < 0.03 {
            return 0.0;
        }
    }

    transmission
}
