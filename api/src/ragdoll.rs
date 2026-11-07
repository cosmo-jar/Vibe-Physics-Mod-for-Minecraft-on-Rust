//! Module 1 — **Ragdoll & Creature Physics** (`ragdoll`).
//!
//! Replaces the rigid vanilla tipping-over death animation and static hurt flash with:
//! 1. **Full Skeletal XPBD Ragdoll** on death (`mob.death_time > 0` or `living_death`):
//!    - Articulated multi-bone skeletons (Pelvis, Torso, Head, Left/Right Arms or Front/Hind Legs).
//!    - XPBD distance & angular cone constraints between connected bones.
//!    - Per-bone world voxel sphere collision (`resolve_sphere_voxel_collision`), restitution,
//!      and ground sliding friction.
//!    - Directional impulse transfer from weapon strikes (`living_knockback_v2`) and shockwaves.
//!    - Renders the creature's actual baked `ModelPart` tree (`host.bake_layer` + `host.draw_model_part`)
//!      with each limb driven by the physical skeleton's simulated joint angles, plus procedural
//!      articulated fallback cubes if a custom entity layer is absent.
//! 2. **Hybrid Active-Ragdoll Hit Reactions** (`mob.hurt_time > 0` while alive):
//!    - Smooth physical upper-body / head flinch and limb recoil blended over vanilla locomotion.

use std::collections::HashMap;

use glam::{Quat, Vec3};
use horizon_client::mod_api::{
    BlockEntityDraw, CubeFace, Flow, Host, ModelPart, NativeMobRender, NativePlayerRender,
};

use crate::config::RagdollConfig;
use crate::materials::bare_id;
use crate::math_phys::{
    FastRng, resolve_sphere_voxel_collision, solve_cone_constraint, solve_distance_constraint,
};

/// Archetype of a creature's skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkeletonArchetype {
    /// Humanoid biped: Player, Zombie, Skeleton, Piglin, Villager, Illager, Enderman, Iron Golem.
    Biped,
    /// Four-legged animal: Cow, Pig, Sheep, Wolf, Horse, Polar Bear, Fox, Goat, Camel.
    Quadruped,
    /// Creeper (tall torso, head, 4 short lower legs).
    Creeper,
    /// Arachnid: Spider, Cave Spider (low wide thorax/abdomen, head, 8 splayed legs).
    Arachnid,
}

/// Bone indices inside [`ArticulatedRagdoll::bones`]:
/// - `0`: Pelvis / Root (`root`)
/// - `1`: Upper Torso / Chest (`body`)
/// - `2`: Head (`head`)
/// - `3`: Left Arm / Left Front Leg (`left_arm` / `left_front_leg`)
/// - `4`: Right Arm / Right Front Leg (`right_arm` / `right_front_leg`)
/// - `5`: Left Leg / Left Hind Leg (`left_leg` / `left_hind_leg`)
/// - `6`: Right Leg / Right Hind Leg (`right_leg` / `right_hind_leg`)
pub const BONE_PELVIS: usize = 0;
pub const BONE_TORSO: usize = 1;
pub const BONE_HEAD: usize = 2;
pub const BONE_LEFT_UPPER: usize = 3;
pub const BONE_RIGHT_UPPER: usize = 4;
pub const BONE_LEFT_LOWER: usize = 5;
pub const BONE_RIGHT_LOWER: usize = 6;
pub const BONE_COUNT: usize = 7;

/// One physical bone node in the XPBD skeleton.
#[derive(Debug, Clone, Copy)]
pub struct RagdollBone {
    /// World-space position of this bone's extremity/center.
    pub pos: Vec3,
    /// Previous step world-space position.
    pub prev_pos: Vec3,
    /// World-space linear velocity ($\text{m/s}$).
    pub vel: Vec3,
    /// Inverse mass ($1/m$).
    pub inv_mass: f32,
    /// Collision sphere radius ($\text{m}$).
    pub radius: f32,
}

/// Distance constraint linking two bones in the ragdoll.
#[derive(Debug, Clone, Copy)]
pub struct DistanceLink {
    pub a: usize,
    pub b: usize,
    pub rest_len: f32,
    pub stiffness: f32,
}

/// Full articulated ragdoll instance for a single entity.
#[derive(Debug, Clone)]
pub struct ArticulatedRagdoll {
    pub entity_id: u32,
    pub kind: String,
    pub archetype: SkeletonArchetype,
    pub scale: f32,
    /// World-space anchor where the ragdoll started (or latest entity feet position).
    pub origin_feet: Vec3,
    /// Initial yaw in radians when the ragdoll activated.
    pub initial_yaw_rad: f32,
    pub bones: [RagdollBone; BONE_COUNT],
    pub links: Vec<DistanceLink>,
    /// Last frame's total age (`mob.age`) to compute exact frame `dt`.
    pub last_age_ticks: f32,
    /// Last time this ragdoll was rendered or updated (for cleanup).
    pub last_seen_game_time: i64,
    /// Accumulated settling time on ground.
    pub settled_time: f32,
}

impl ArticulatedRagdoll {
    /// Construct a new articulated ragdoll posed at `feet_world` facing `body_yaw_deg`.
    pub fn new(
        entity_id: u32,
        kind: &str,
        baby: bool,
        feet_world: Vec3,
        body_yaw_deg: f32,
        age_ticks: f32,
        game_time: i64,
        initial_impulse: Vec3,
    ) -> Self {
        let archetype = classify_archetype(kind);
        let scale = if baby { 0.5 } else { 1.0 };
        // In Horizon `mob_pose`, yaw=0 faces +Z, yaw=90 faces -X.
        let yaw_rad = (-body_yaw_deg).to_radians();
        let rot_y = Quat::from_rotation_y(yaw_rad);
        let right = rot_y * Vec3::X;
        let up = Vec3::Y;
        let forward = rot_y * Vec3::Z;

        let (local_offsets, radii, masses) = match archetype {
            SkeletonArchetype::Biped => (
                [
                    Vec3::new(0.0, 0.72, 0.0),   // 0: Pelvis
                    Vec3::new(0.0, 1.35, 0.0),   // 1: Torso/Shoulders
                    Vec3::new(0.0, 1.72, 0.04),  // 2: Head
                    Vec3::new(0.38, 0.78, 0.02), // 3: Left Hand/Arm
                    Vec3::new(-0.38, 0.78, 0.02),// 4: Right Hand/Arm
                    Vec3::new(0.15, 0.14, 0.0),  // 5: Left Foot/Leg
                    Vec3::new(-0.15, 0.14, 0.0), // 6: Right Foot/Leg
                ],
                [0.22, 0.24, 0.24, 0.14, 0.14, 0.14, 0.14],
                [18.0, 24.0, 7.0, 4.5, 4.5, 7.5, 7.5],
            ),
            SkeletonArchetype::Quadruped => (
                [
                    Vec3::new(0.0, 0.68, -0.38),  // 0: Rear Pelvis
                    Vec3::new(0.0, 0.72, 0.38),   // 1: Front Chest
                    Vec3::new(0.0, 1.02, 0.68),   // 2: Head
                    Vec3::new(0.22, 0.14, 0.38),  // 3: Front Left Leg
                    Vec3::new(-0.22, 0.14, 0.38), // 4: Front Right Leg
                    Vec3::new(0.22, 0.14, -0.38), // 5: Hind Left Leg
                    Vec3::new(-0.22, 0.14, -0.38),// 6: Hind Right Leg
                ],
                [0.24, 0.26, 0.22, 0.13, 0.13, 0.13, 0.13],
                [22.0, 26.0, 8.0, 5.0, 5.0, 6.0, 6.0],
            ),
            SkeletonArchetype::Creeper => (
                [
                    Vec3::new(0.0, 0.45, 0.0),    // 0: Lower Body
                    Vec3::new(0.0, 1.10, 0.0),    // 1: Upper Body
                    Vec3::new(0.0, 1.50, 0.0),    // 2: Head
                    Vec3::new(0.20, 0.14, 0.20),  // 3: Front-Left Foot
                    Vec3::new(-0.20, 0.14, 0.20), // 4: Front-Right Foot
                    Vec3::new(0.20, 0.14, -0.20), // 5: Back-Left Foot
                    Vec3::new(-0.20, 0.14, -0.20),// 6: Back-Right Foot
                ],
                [0.22, 0.22, 0.24, 0.13, 0.13, 0.13, 0.13],
                [18.0, 20.0, 7.0, 4.0, 4.0, 4.0, 4.0],
            ),
            SkeletonArchetype::Arachnid => (
                [
                    Vec3::new(0.0, 0.42, -0.35),  // 0: Abdomen
                    Vec3::new(0.0, 0.40, 0.18),   // 1: Cephalothorax
                    Vec3::new(0.0, 0.44, 0.48),   // 2: Head/Fangs
                    Vec3::new(0.58, 0.14, 0.28),  // 3: Left Front Leg Bank
                    Vec3::new(-0.58, 0.14, 0.28), // 4: Right Front Leg Bank
                    Vec3::new(0.58, 0.14, -0.28), // 5: Left Rear Leg Bank
                    Vec3::new(-0.58, 0.14, -0.28),// 6: Right Rear Leg Bank
                ],
                [0.26, 0.24, 0.20, 0.14, 0.14, 0.14, 0.14],
                [16.0, 18.0, 6.0, 3.5, 3.5, 3.5, 3.5],
            ),
        };

        let mut rng = FastRng::from_seed((entity_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        // Give a natural asymmetric crumple/topple velocity if no strong impulse was registered
        let fallback_dir = if initial_impulse.length_squared() > 0.25 {
            initial_impulse
        } else {
            let side = rng.signed_f32() * 2.2;
            let back = -rng.range_f32(1.4, 3.2);
            right * side + forward * back + up * 1.2
        };

        let mut bones = [RagdollBone {
            pos: Vec3::ZERO,
            prev_pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            inv_mass: 1.0,
            radius: 0.15,
        }; BONE_COUNT];

        for i in 0..BONE_COUNT {
            let local = local_offsets[i] * scale;
            let world_p = feet_world + right * local.x + up * local.y + forward * local.z;
            // Upper bones receive slightly stronger toppling velocity for natural angular momentum
            let height_factor = (local_offsets[i].y / 1.5).clamp(0.25, 1.35);
            let jitter = rng.unit_vec3() * 0.65;
            let bone_vel = fallback_dir * height_factor + jitter;
            bones[i] = RagdollBone {
                pos: world_p,
                prev_pos: world_p,
                vel: bone_vel,
                inv_mass: 1.0 / (masses[i] * scale.max(0.35)),
                radius: (radii[i] * scale).max(0.08),
            };
        }

        // Build structural & cross-bracing XPBD links
        let mut links = Vec::with_capacity(14);
        let mut add_link = |a: usize, b: usize, stiffness: f32| {
            let dist = (bones[a].pos - bones[b].pos).length().max(0.05);
            links.push(DistanceLink {
                a,
                b,
                rest_len: dist,
                stiffness,
            });
        };

        // Primary anatomical bones
        add_link(BONE_PELVIS, BONE_TORSO, 0.98);
        add_link(BONE_TORSO, BONE_HEAD, 0.95);
        add_link(BONE_TORSO, BONE_LEFT_UPPER, 0.90);
        add_link(BONE_TORSO, BONE_RIGHT_UPPER, 0.90);
        add_link(BONE_PELVIS, BONE_LEFT_LOWER, 0.92);
        add_link(BONE_PELVIS, BONE_RIGHT_LOWER, 0.92);

        // Soft anatomical limiters (prevent limbs from collapsing through the torso or crossing unnatural angles)
        add_link(BONE_PELVIS, BONE_HEAD, 0.45);
        add_link(BONE_LEFT_UPPER, BONE_RIGHT_UPPER, 0.25);
        add_link(BONE_LEFT_LOWER, BONE_RIGHT_LOWER, 0.30);
        add_link(BONE_TORSO, BONE_LEFT_LOWER, 0.22);
        add_link(BONE_TORSO, BONE_RIGHT_LOWER, 0.22);

        Self {
            entity_id,
            kind: kind.to_owned(),
            archetype,
            scale,
            origin_feet: feet_world,
            initial_yaw_rad: yaw_rad,
            bones,
            links,
            last_age_ticks: age_ticks,
            last_seen_game_time: game_time,
            settled_time: 0.0,
        }
    }

    /// Apply an external world-space impulse (e.g., from an explosion or sword strike).
    pub fn apply_impulse(&mut self, impulse: Vec3, hit_height_fraction: f32) {
        for (i, bone) in self.bones.iter_mut().enumerate() {
            let weight = match i {
                BONE_HEAD | BONE_TORSO if hit_height_fraction > 0.5 => 1.35,
                BONE_LEFT_UPPER | BONE_RIGHT_UPPER => 1.15,
                _ => 0.85,
            };
            bone.vel += impulse * weight;
        }
        self.settled_time = 0.0;
    }

    /// Advance the XPBD ragdoll simulation by `dt` seconds against client world voxels.
    pub fn step(&mut self, host: &dyn Host, cfg: &RagdollConfig, gravity: f32, dt: f32) {
        let dt = dt.clamp(0.001, 0.05);
        let substeps = 2;
        let sub_dt = dt / (substeps as f32);
        let iters = cfg.solver_iterations.clamp(2, 16);
        let stiffness = cfg.joint_stiffness.clamp(0.2, 1.0);

        for _ in 0..substeps {
            // 1. Symplectic Euler / Verlet prediction with gravity & air damping
            for bone in &mut self.bones {
                bone.prev_pos = bone.pos;
                bone.vel.y -= gravity * sub_dt;
                bone.vel *= (1.0 - 0.35 * sub_dt).clamp(0.85, 0.999);
                bone.pos += bone.vel * sub_dt;
            }

            // 2. XPBD Distance & Cone Constraint Projection
            for _ in 0..iters {
                for link in &self.links {
                    let mut pa = self.bones[link.a].pos;
                    let wa = self.bones[link.a].inv_mass;
                    let mut pb = self.bones[link.b].pos;
                    let wb = self.bones[link.b].inv_mass;
                    solve_distance_constraint(
                        &mut pa,
                        wa,
                        &mut pb,
                        wb,
                        link.rest_len,
                        link.stiffness * stiffness,
                    );
                    self.bones[link.a].pos = pa;
                    self.bones[link.b].pos = pb;
                }

                // Neck cone constraint: keep head within 55 deg of spine axis (pelvis -> torso)
                let spine_dir = (self.bones[BONE_TORSO].pos - self.bones[BONE_PELVIS].pos)
                    .normalize_or(Vec3::Y);
                let neck_len = 0.38 * self.scale;
                let torso_pos = self.bones[BONE_TORSO].pos;
                solve_cone_constraint(
                    torso_pos,
                    &mut self.bones[BONE_HEAD].pos,
                    spine_dir,
                    neck_len,
                    0.95,
                    0.65 * stiffness,
                );
            }

            // 3. Reconstruct velocities from projected positions & resolve voxel collisions
            for bone in &mut self.bones {
                bone.vel = (bone.pos - bone.prev_pos) / sub_dt;
                resolve_sphere_voxel_collision(
                    host,
                    &mut bone.pos,
                    &mut bone.vel,
                    bone.radius,
                    cfg.limb_restitution,
                    cfg.ground_friction,
                );
            }
        }
    }
}

/// Pending directional impulse recorded from server damage/knockback/explosion hooks.
#[derive(Debug, Clone, Copy)]
struct PendingImpulse {
    vel: Vec3,
    game_time: i64,
}

/// Cached baked vanilla `ModelPart` trees by model layer key (e.g. `minecraft:zombie#main`).
pub struct RagdollEngine {
    active_ragdolls: HashMap<u32, ArticulatedRagdoll>,
    pending_impulses: HashMap<u32, PendingImpulse>,
    baked_models: HashMap<String, Option<ModelPart>>,
    last_cleanup_time: i64,
}

impl Default for RagdollEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RagdollEngine {
    pub fn new() -> Self {
        Self {
            active_ragdolls: HashMap::new(),
            pending_impulses: HashMap::new(),
            baked_models: HashMap::new(),
            last_cleanup_time: 0,
        }
    }

    pub fn active_count(&self) -> usize {
        self.active_ragdolls.len()
    }

    pub fn clear(&mut self) {
        self.active_ragdolls.clear();
        self.pending_impulses.clear();
    }

    /// Record a directional knockback or blast impulse for `entity_id`.
    pub fn record_impulse(&mut self, entity_id: u32, impulse: Vec3, game_time: i64) {
        if let Some(ragdoll) = self.active_ragdolls.get_mut(&entity_id) {
            ragdoll.apply_impulse(impulse, 0.7);
        } else {
            let entry = self.pending_impulses.entry(entity_id).or_insert(PendingImpulse {
                vel: Vec3::ZERO,
                game_time,
            });
            entry.vel = (entry.vel + impulse).clamp(Vec3::splat(-28.0), Vec3::splat(28.0));
            entry.game_time = game_time;
        }
    }

    /// Apply a radial blast wave to all active ragdolls within `radius`.
    pub fn apply_explosion_wave(&mut self, center: Vec3, radius: f32, strength: f32) {
        for ragdoll in self.active_ragdolls.values_mut() {
            let delta = ragdoll.bones[BONE_TORSO].pos - center;
            let dist = delta.length();
            if dist < radius && dist > 0.05 {
                let falloff = (1.0 - dist / radius).powi(2);
                let dir = (delta / dist + Vec3::new(0.0, 0.45, 0.0)).normalize_or(Vec3::Y);
                ragdoll.apply_impulse(dir * (strength * falloff), 0.8);
            }
        }
    }

    /// Handle `Hooks::mob_submit` or `Hooks::player_submit`.
    /// Returns `Flow::Cancel` when Vibe Physics takes over rendering the creature's model.
    pub fn submit_creature(
        &mut self,
        host: &dyn Host,
        mob: &NativeMobRender<'_>,
        player: Option<&NativePlayerRender<'_>>,
        draw: &mut BlockEntityDraw,
        cfg: &RagdollConfig,
        gravity: f32,
    ) -> Flow {
        if !cfg.enabled {
            return Flow::Continue;
        }

        let game_time = draw.game_time;
        if game_time - self.last_cleanup_time > 40 {
            self.last_cleanup_time = game_time;
            self.active_ragdolls
                .retain(|_, r| (game_time - r.last_seen_game_time).abs() <= 40);
            self.pending_impulses
                .retain(|_, p| (game_time - p.game_time).abs() <= 60);
        }

        // Case A: Death Ragdoll (`death_time > 0`)
        if mob.death_time > 0 && cfg.death_ragdoll {
            let feet_world = host
                .client_entity_position(mob.id)
                .map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
                .unwrap_or(Vec3::ZERO);

            let pending = self
                .pending_impulses
                .remove(&mob.id)
                .map(|p| p.vel * cfg.impulse_scale)
                .unwrap_or(Vec3::ZERO);

            let ragdoll = self.active_ragdolls.entry(mob.id).or_insert_with(|| {
                ArticulatedRagdoll::new(
                    mob.id,
                    mob.kind,
                    mob.baby,
                    feet_world,
                    mob.body_yaw,
                    mob.age,
                    game_time,
                    pending,
                )
            });

            if pending.length_squared() > 0.01 {
                ragdoll.apply_impulse(pending, 0.75);
            }

            let dt_ticks = (mob.age - ragdoll.last_age_ticks).clamp(0.0, 2.5);
            ragdoll.last_age_ticks = mob.age;
            ragdoll.last_seen_game_time = game_time;

            if dt_ticks > 0.0001 {
                ragdoll.step(host, cfg, gravity, dt_ticks * 0.05);
            }

            let ragdoll_snapshot = ragdoll.clone();
            if self.render_articulated_ragdoll(host, mob, player, &ragdoll_snapshot, feet_world, draw)
            {
                return Flow::Cancel;
            }
            return Flow::Continue;
        }

        // Clean up if mob is alive
        self.active_ragdolls.remove(&mob.id);

        // Case B: Hybrid Active-Ragdoll Hit Reaction (`hurt_time > 0` while alive)
        if mob.hurt_time > 0 && cfg.active_hit_reaction && player.is_none() {
            if self.render_active_hit_flinch(host, mob, draw, cfg) {
                return Flow::Cancel;
            }
        }

        Flow::Continue
    }

    /// Render a living mob during `hurt_time > 0` with procedural physical upper-body flinch & stagger
    /// while preserving lower-body leg walk animation!
    fn render_active_hit_flinch(
        &mut self,
        host: &dyn Host,
        mob: &NativeMobRender<'_>,
        draw: &mut BlockEntityDraw,
        cfg: &RagdollConfig,
    ) -> bool {
        let Some((layer_key, sprite)) = resolve_mob_layer_and_sprite(mob.kind, mob.baby) else {
            return false;
        };
        let Some(mut root_part) = self.get_or_bake_model(host, &layer_key) else {
            return false;
        };

        root_part.reset_all_poses();
        let hurt_phase = ((mob.hurt_time as f32 - mob.partial).max(0.0)) / 10.0;
        let recoil = (hurt_phase * std::f32::consts::PI).sin() * 0.55 * cfg.impulse_scale;
        let side_twist = ((mob.id as f32 * 1.7).sin()) * recoil * 0.65;

        // Standard walk swing for legs (hybrid animation + physics!)
        let walk_pos = mob.walk_position;
        let walk_spd = mob.walk_speed;
        let leg_swing = (walk_pos * 0.6662).cos() * 1.4 * walk_spd;

        // Apply hybrid physical flinch to head, body, and arms while legs keep walking
        if let Some(head) = root_part.get_child_mut("head") {
            head.x_rot += mob.pitch.to_radians() - recoil * 0.85;
            head.y_rot += mob.head_yaw.to_radians() + side_twist;
            head.z_rot += side_twist * 0.5;
        }
        if let Some(body) = root_part.get_child_mut("body") {
            body.x_rot -= recoil * 0.45;
            body.z_rot += side_twist * 0.35;
        }
        if let Some(la) = root_part.get_child_mut("left_arm") {
            la.x_rot -= recoil * 1.1;
            la.z_rot -= recoil * 0.6;
        }
        if let Some(ra) = root_part.get_child_mut("right_arm") {
            ra.x_rot -= recoil * 1.1;
            ra.z_rot += recoil * 0.6;
        }
        if let Some(ll) = root_part.get_child_mut("left_leg") {
            ll.x_rot = -leg_swing;
        }
        if let Some(rl) = root_part.get_child_mut("right_leg") {
            rl.x_rot = leg_swing;
        }

        draw.push();
        // Slight physical stagger displacement at the hips
        draw.rotate_about([1.0, 0.0, 0.0], -recoil.to_degrees() * 0.45, [0.0, 0.6, 0.0]);
        draw.rotate_about([0.0, 0.0, 1.0], side_twist.to_degrees() * 0.35, [0.0, 0.6, 0.0]);
        // Standard LivingEntityRenderer model-frame transform (+y down, 24 texels = 1.501 blocks)
        draw.scale([1.0, -1.0, -1.0]);
        draw.translate([0.0, -1.501, 0.0]);

        // Warm red-tinted hurt overlay tint
        let tint = 0xFF_B8B8;
        let ok = host.draw_model_part(draw, &root_part, sprite, tint);
        draw.pop();
        ok
    }

    /// Render the full XPBD articulated ragdoll in `mob_submit` / `player_submit`.
    fn render_articulated_ragdoll(
        &mut self,
        host: &dyn Host,
        mob: &NativeMobRender<'_>,
        _player: Option<&NativePlayerRender<'_>>,
        ragdoll: &ArticulatedRagdoll,
        current_feet_world: Vec3,
        draw: &mut BlockEntityDraw,
    ) -> bool {
        // Compute world offset from `current_feet_world` (where `draw` is currently anchored)
        // to the ragdoll's simulated pelvis/torso center.
        let pelvis_world = ragdoll.bones[BONE_PELVIS].pos;
        let torso_world = ragdoll.bones[BONE_TORSO].pos;
        let head_world = ragdoll.bones[BONE_HEAD].pos;
        let lu_world = ragdoll.bones[BONE_LEFT_UPPER].pos;
        let ru_world = ragdoll.bones[BONE_RIGHT_UPPER].pos;
        let ll_world = ragdoll.bones[BONE_LEFT_LOWER].pos;
        let rl_world = ragdoll.bones[BONE_RIGHT_LOWER].pos;

        // `draw` starts posed with `mob_pose(feet - camera, mob.body_yaw)`, which rotates by `-mob.body_yaw`.
        // Let's undo `-mob.body_yaw` first so our coordinate frame is pure world-aligned (+X east, +Y up, +Z south)
        // centered at `current_feet_world`!
        draw.push();
        draw.rotate_about([0.0, 1.0, 0.0], mob.body_yaw, [0.0, 0.0, 0.0]);

        // Compute spine orientation from simulated Pelvis -> Torso and shoulder roll from LeftUpper -> RightUpper
        let spine_vec = (torso_world - pelvis_world).normalize_or(Vec3::Y);
        let across_vec = (lu_world - ru_world).normalize_or(Vec3::X);

        // Try baking the creature's native ModelPart tree first
        if let Some((layer_key, sprite)) = resolve_mob_layer_and_sprite(mob.kind, mob.baby)
            && let Some(mut root_part) = self.get_or_bake_model(host, &layer_key)
        {
            root_part.reset_all_poses();

            // Build an orthonormal body frame from the simulated ragdoll bones:
            // Up = spine_vec (for bipeds/creepers) or cross product (for quadrupeds)
            let (body_up, body_forward) = match ragdoll.archetype {
                SkeletonArchetype::Biped | SkeletonArchetype::Creeper => {
                    let up = spine_vec;
                    let right = across_vec.reject_from(up).normalize_or(Vec3::X);
                    let fwd = right.cross(up).normalize_or(Vec3::Z);
                    (up, fwd)
                }
                SkeletonArchetype::Quadruped | SkeletonArchetype::Arachnid => {
                    let fwd = spine_vec;
                    let right = across_vec.reject_from(fwd).normalize_or(Vec3::X);
                    let up = fwd.cross(right).normalize_or(Vec3::Y);
                    (up, fwd)
                }
            };
            let body_right = body_up.cross(body_forward).normalize_or(Vec3::X);
            let body_rot = glam::Mat3::from_cols(body_right, body_up, body_forward);
            let body_quat = Quat::from_mat3(&body_rot).normalize();
            let inv_body_quat = body_quat.inverse();

            // Convert world limb vectors into the body's local frame to set exact `ModelPart` joint angles!
            let dir_to_angles = |world_dir: Vec3| -> (f32, f32) {
                let local = (inv_body_quat * world_dir).normalize_or(Vec3::NEG_Y);
                // In ModelPart frame, limbs hang along -Y (down in world = +Y in ModelPart)
                let pitch = (-local.z).atan2((-local.y).max(0.05));
                let roll = (local.x).atan2((-local.y).max(0.05));
                (pitch.clamp(-2.4, 2.4), roll.clamp(-1.8, 1.8))
            };

            let (head_p, head_r) = dir_to_angles(head_world - torso_world);
            let (lu_p, lu_r) = dir_to_angles(lu_world - torso_world);
            let (ru_p, ru_r) = dir_to_angles(ru_world - torso_world);
            let (ll_p, ll_r) = dir_to_angles(ll_world - pelvis_world);
            let (rl_p, rl_r) = dir_to_angles(rl_world - pelvis_world);

            if let Some(head) = root_part.get_child_mut("head") {
                head.x_rot = -head_p * 0.85;
                head.z_rot = head_r * 0.85;
            }
            for (name, (p, r)) in [
                ("left_arm", (lu_p, lu_r)),
                ("right_arm", (ru_p, ru_r)),
                ("left_front_leg", (lu_p, lu_r)),
                ("right_front_leg", (ru_p, ru_r)),
                ("left_leg", (ll_p, ll_r)),
                ("right_leg", (rl_p, rl_r)),
                ("left_hind_leg", (ll_p, ll_r)),
                ("right_hind_leg", (rl_p, rl_r)),
            ] {
                if let Some(part) = root_part.get_child_mut(name) {
                    part.x_rot = p;
                    part.z_rot = r;
                }
            }

            // Position the model at the simulated pelvis/waist in world space
            let waist_offset = (pelvis_world + torso_world) * 0.5 - current_feet_world;
            draw.translate([waist_offset.x, waist_offset.y, waist_offset.z]);

            let (axis, angle_deg) = crate::math_phys::quat_to_axis_angle_deg(body_quat);
            if angle_deg.abs() > 0.05 {
                draw.rotate_about(axis, angle_deg, [0.0, 0.0, 0.0]);
            }

            // Pivot around the creature's waist (0.75 blocks above feet)
            draw.translate([0.0, -0.75 * ragdoll.scale, 0.0]);
            draw.scale([1.0, -1.0, -1.0]);
            draw.translate([0.0, -1.501, 0.0]);

            let tint = if mob.hurt_time > 0 {
                0xFF_B0B0
            } else {
                0xFF_FFFF
            };
            if host.draw_model_part(draw, &root_part, sprite, tint) {
                draw.pop();
                return true;
            }
        }

        // Procedural multi-bone articulated 3D fallback renderer (draws each physical bone segment
        // directly in world space if the entity's layer is custom/unavailable).
        self.render_procedural_bones(host, ragdoll, current_feet_world, draw);
        draw.pop();
        true
    }

    /// Fallback renderer that draws each physical bone segment of `ragdoll` as an oriented 3D box.
    fn render_procedural_bones(
        &self,
        host: &dyn Host,
        ragdoll: &ArticulatedRagdoll,
        current_feet_world: Vec3,
        draw: &mut BlockEntityDraw,
    ) {
        let face = Some(CubeFace {
            sprite: "block/white_concrete",
            uv: None,
        });
        let faces = [face; 6];
        let segments = [
            (BONE_PELVIS, BONE_TORSO, 3.5, 0xD8_C8B8),
            (BONE_TORSO, BONE_HEAD, 3.8, 0xE8_D8C8),
            (BONE_TORSO, BONE_LEFT_UPPER, 2.2, 0xC0_B0A0),
            (BONE_TORSO, BONE_RIGHT_UPPER, 2.2, 0xC0_B0A0),
            (BONE_PELVIS, BONE_LEFT_LOWER, 2.2, 0xA8_9888),
            (BONE_PELVIS, BONE_RIGHT_LOWER, 2.2, 0xA8_9888),
        ];

        for (a, b, thickness, tint) in segments {
            let pa = ragdoll.bones[a].pos - current_feet_world;
            let pb = ragdoll.bones[b].pos - current_feet_world;
            let mid = (pa + pb) * 0.5;
            let delta = pb - pa;
            let len = delta.length().max(0.1);
            let dir = delta / len;
            let q = Quat::from_rotation_arc(Vec3::Y, dir);
            let (axis, deg) = crate::math_phys::quat_to_axis_angle_deg(q);

            draw.push();
            draw.translate(mid.to_array());
            if deg.abs() > 0.05 {
                draw.rotate_about(axis, deg, [0.0, 0.0, 0.0]);
            }
            let half_h = (len * 8.0).clamp(1.5, 12.0);
            let w = thickness * ragdoll.scale;
            let _ = host.draw_cube(draw, [-w, -half_h, -w], [w, half_h, w], faces, tint);
            draw.pop();
        }
    }

    fn get_or_bake_model(&mut self, host: &dyn Host, layer_key: &str) -> Option<ModelPart> {
        if !self.baked_models.contains_key(layer_key) {
            let baked = host.bake_layer(layer_key);
            self.baked_models.insert(layer_key.to_owned(), baked);
        }
        self.baked_models.get(layer_key).and_then(|m| m.clone())
    }
}

/// Classify a mob's entity ID into a physical [`SkeletonArchetype`].
pub fn classify_archetype(kind: &str) -> SkeletonArchetype {
    let b = bare_id(kind);
    if b.contains("spider") || b == "silverfish" || b == "endermite" {
        SkeletonArchetype::Arachnid
    } else if b == "creeper" {
        SkeletonArchetype::Creeper
    } else if matches!(
        b,
        "cow"
            | "mooshroom"
            | "pig"
            | "sheep"
            | "wolf"
            | "cat"
            | "ocelot"
            | "fox"
            | "goat"
            | "polar_bear"
            | "panda"
            | "horse"
            | "donkey"
            | "mule"
            | "skeleton_horse"
            | "zombie_horse"
            | "llama"
            | "trader_llama"
            | "camel"
            | "hoglin"
            | "zoglin"
            | "ravager"
            | "sniffer"
            | "armadillo"
            | "turtle"
            | "rabbit"
            | "AXOLOTL"
            | "axolotl"
            | "frog"
    ) {
        SkeletonArchetype::Quadruped
    } else {
        SkeletonArchetype::Biped
    }
}

/// Map a mob entity ID (`minecraft:zombie`, `minecraft:skeleton`, `minecraft:cow`, etc.)
/// to its `(model_layer_key, atlas_sprite_name)` for `host.bake_layer` and `host.draw_model_part`.
pub fn resolve_mob_layer_and_sprite(kind: &str, baby: bool) -> Option<(String, &'static str)> {
    let b = bare_id(kind);
    let (base_layer, sprite) = match b {
        "zombie" => ("zombie", "entity/zombie/zombie"),
        "husk" => ("husk", "entity/zombie/husk"),
        "drowned" => ("drowned", "entity/zombie/drowned"),
        "skeleton" => ("skeleton", "entity/skeleton/skeleton"),
        "stray" => ("stray", "entity/skeleton/stray"),
        "bogged" => ("bogged", "entity/skeleton/bogged"),
        "wither_skeleton" => ("wither_skeleton", "entity/skeleton/wither_skeleton"),
        "creeper" => ("creeper", "entity/creeper/creeper"),
        "spider" => ("spider", "entity/spider/spider"),
        "cave_spider" => ("cave_spider", "entity/spider/cave_spider"),
        "enderman" => ("enderman", "entity/enderman/enderman"),
        "piglin" => ("piglin", "entity/piglin/piglin"),
        "piglin_brute" => ("piglin_brute", "entity/piglin/piglin_brute"),
        "zombified_piglin" => ("zombified_piglin", "entity/piglin/zombified_piglin"),
        "villager" => ("villager", "entity/villager/villager"),
        "pillager" => ("pillager", "entity/illager/pillager"),
        "vindicator" => ("vindicator", "entity/illager/vindicator"),
        "evoker" => ("evoker", "entity/illager/evoker"),
        "witch" => ("witch", "entity/witch"),
        "iron_golem" => ("iron_golem", "entity/iron_golem/iron_golem"),
        "snow_golem" => ("snow_golem", "entity/snow_golem"),
        "cow" => ("cow", "entity/cow/cow_temperate"),
        "mooshroom" => ("mooshroom", "entity/cow/mooshroom_red"),
        "pig" => ("pig", "entity/pig/pig_temperate"),
        "sheep" => ("sheep", "entity/sheep/sheep"),
        "wolf" => ("wolf", "entity/wolf/wolf"),
        "goat" => ("goat", "entity/goat/goat"),
        "polar_bear" => ("polar_bear", "entity/bear/polarbear"),
        "fox" => ("fox", "entity/fox/fox"),
        "chicken" => ("chicken", "entity/chicken/chicken_temperate"),
        "blaze" => ("blaze", "entity/blaze"),
        "warden" => ("warden", "entity/warden/warden"),
        _ => return None,
    };

    let layer = if baby {
        format!("minecraft:{base_layer}_baby#main")
    } else {
        format!("minecraft:{base_layer}#main")
    };
    Some((layer, sprite))
}
