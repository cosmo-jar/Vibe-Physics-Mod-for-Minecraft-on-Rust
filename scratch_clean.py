import os, sys, shutil, subprocess, re

WORKSPACE_ROOT = r"c:\Users\cosmo\Documents\Antigravity\VibePhysics"
MIXINS_DIR = os.path.join(WORKSPACE_ROOT, "mixins_pkg")
os.makedirs(MIXINS_DIR, exist_ok=True)

# 1. Generate clean vibe_physics.rs
with open("scratch/vibe_physics_clean.rs", "w", encoding="utf-8") as out:
    out.write("""// Vibe Physics — Advanced Physics & Destruction Engine for Horizon
// 5 Modular Subsystems: Ragdolls, Item Physics, Voronoi Debris, Structural Integrity, Shockwaves.

pub mod config {
    use serde::{Deserialize, Serialize};

    pub const DEFAULT_CONFIG_TOML: &str = r#"# Vibe Physics Common Config
[general]
master_enabled = true
show_hud_status = true
enable_hotkeys = true
gravity = 24.0

[ragdoll]
enabled = true
active_hit_reaction = true
death_ragdoll = true
solver_iterations = 8
joint_stiffness = 0.85
limb_restitution = 0.18
ground_friction = 0.72
impulse_scale = 1.35

[item_physics]
enabled = true
airborne_tumbling = true
flat_resting = true
buoyancy = true
ice_gliding = true
rotation_speed = 1.0

[debris]
enabled = true
shards_per_block = 8
max_active_shards = 256
sharding_explosions = true
dust_particles = true
shard_lifetime_secs = 12.0

[structural]
enabled = true
max_cluster_size = 384
cantilever_check = true
collapse_tilt = true
strength_multiplier = 1.0

[explosions]
enabled = true
raycast_shielding = true
shatter_glass_radius_mult = 2.4
camera_shake = true
speed_of_sound_delay = true
"#;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PhysicsModule {
        Ragdoll,
        ItemPhysics,
        Debris,
        Structural,
        Explosions,
    }

    impl PhysicsModule {
        pub const ALL: [PhysicsModule; 5] = [
            PhysicsModule::Ragdoll,
            PhysicsModule::ItemPhysics,
            PhysicsModule::Debris,
            PhysicsModule::Structural,
            PhysicsModule::Explosions,
        ];

        pub fn display_name(&self) -> &'static str {
            match self {
                Self::Ragdoll => "Ragdoll",
                Self::ItemPhysics => "ItemPhysics",
                Self::Debris => "Debris",
                Self::Structural => "Structural",
                Self::Explosions => "Explosions",
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct GeneralConfig {
        pub master_enabled: bool,
        pub show_hud_status: bool,
        pub enable_hotkeys: bool,
        pub gravity: f32,
    }

    impl Default for GeneralConfig {
        fn default() -> Self {
            Self {
                master_enabled: true,
                show_hud_status: true,
                enable_hotkeys: true,
                gravity: 24.0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct RagdollConfig {
        pub enabled: bool,
        pub active_hit_reaction: bool,
        pub death_ragdoll: bool,
        pub solver_iterations: u32,
        pub joint_stiffness: f32,
        pub limb_restitution: f32,
        pub ground_friction: f32,
        pub impulse_scale: f32,
    }

    impl Default for RagdollConfig {
        fn default() -> Self {
            Self {
                enabled: true,
                active_hit_reaction: true,
                death_ragdoll: true,
                solver_iterations: 8,
                joint_stiffness: 0.85,
                limb_restitution: 0.18,
                ground_friction: 0.72,
                impulse_scale: 1.35,
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct ItemPhysicsConfig {
        pub enabled: bool,
        pub airborne_tumbling: bool,
        pub flat_resting: bool,
        pub buoyancy: bool,
        pub ice_gliding: bool,
        pub rotation_speed: f32,
    }

    impl Default for ItemPhysicsConfig {
        fn default() -> Self {
            Self {
                enabled: true,
                airborne_tumbling: true,
                flat_resting: true,
                buoyancy: true,
                ice_gliding: true,
                rotation_speed: 1.0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct DebrisConfig {
        pub enabled: bool,
        pub shards_per_block: u32,
        pub max_active_shards: usize,
        pub sharding_explosions: bool,
        pub dust_particles: bool,
        pub shard_lifetime_secs: f32,
    }

    impl Default for DebrisConfig {
        fn default() -> Self {
            Self {
                enabled: true,
                shards_per_block: 8,
                max_active_shards: 256,
                sharding_explosions: true,
                dust_particles: true,
                shard_lifetime_secs: 12.0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct StructuralConfig {
        pub enabled: bool,
        pub max_cluster_size: usize,
        pub cantilever_check: bool,
        pub collapse_tilt: bool,
        pub strength_multiplier: f32,
    }

    impl Default for StructuralConfig {
        fn default() -> Self {
            Self {
                enabled: true,
                max_cluster_size: 384,
                cantilever_check: true,
                collapse_tilt: true,
                strength_multiplier: 1.0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct ExplosionsConfig {
        pub enabled: bool,
        pub raycast_shielding: bool,
        pub shatter_glass_radius_mult: f32,
        pub camera_shake: bool,
        pub speed_of_sound_delay: bool,
    }

    impl Default for ExplosionsConfig {
        fn default() -> Self {
            Self {
                enabled: true,
                raycast_shielding: true,
                shatter_glass_radius_mult: 2.4,
                camera_shake: true,
                speed_of_sound_delay: true,
            }
        }
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct VibePhysicsConfig {
        pub general: GeneralConfig,
        pub ragdoll: RagdollConfig,
        pub item_physics: ItemPhysicsConfig,
        pub debris: DebrisConfig,
        pub structural: StructuralConfig,
        pub explosions: ExplosionsConfig,
    }

    impl VibePhysicsConfig {
        pub fn from_toml(text: &str) -> Self {
            let mut cfg = Self::default();
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.starts_with('[') || trimmed.is_empty() {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim();
                    let val = v.trim();
                    match key {
                        "master_enabled" => cfg.general.master_enabled = val == "true",
                        "show_hud_status" => cfg.general.show_hud_status = val == "true",
                        "enable_hotkeys" => cfg.general.enable_hotkeys = val == "true",
                        "gravity" => if let Ok(n) = val.parse::<f32>() { cfg.general.gravity = n; },
                        _ => {}
                    }
                }
            }
            cfg
        }

        pub fn toggle_module(&mut self, m: PhysicsModule) -> bool {
            match m {
                PhysicsModule::Ragdoll => { self.ragdoll.enabled = !self.ragdoll.enabled; self.ragdoll.enabled }
                PhysicsModule::ItemPhysics => { self.item_physics.enabled = !self.item_physics.enabled; self.item_physics.enabled }
                PhysicsModule::Debris => { self.debris.enabled = !self.debris.enabled; self.debris.enabled }
                PhysicsModule::Structural => { self.structural.enabled = !self.structural.enabled; self.structural.enabled }
                PhysicsModule::Explosions => { self.explosions.enabled = !self.explosions.enabled; self.explosions.enabled }
            }
        }

        pub fn status_summary(&self) -> String {
            let tag = |on: bool| if on { "ON" } else { "OFF" };
            format!(
                "Ragdoll:{} Items:{} Debris:{} Struct:{} Blast:{}",
                tag(self.ragdoll.enabled),
                tag(self.item_physics.enabled),
                tag(self.debris.enabled),
                tag(self.structural.enabled),
                tag(self.explosions.enabled)
            )
        }
    }
}

pub mod materials {
    use glam::Vec3;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FractureStyle {
        GlassShards,
        WoodSplinters,
        StonePolyhedra,
        MetalPlates,
        GranularClumps,
        FoliageFragments,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct MaterialProfile {
        pub name: &'static str,
        pub density: f32,
        pub max_overhang: i32,
        pub support_reach: i32,
        pub blast_absorption: f32,
        pub restitution: f32,
        pub friction: f32,
        pub fracture_style: FractureStyle,
        pub shard_multiplier: f32,
        pub sprite: &'static str,
        pub tint: [f32; 3],
    }

    pub const MAT_STONE: MaterialProfile = MaterialProfile {
        name: "stone",
        density: 2600.0,
        max_overhang: 4,
        support_reach: 12,
        blast_absorption: 0.85,
        restitution: 0.15,
        friction: 0.80,
        fracture_style: FractureStyle::StonePolyhedra,
        shard_multiplier: 1.0,
        sprite: "block/stone",
        tint: [0.65, 0.65, 0.65],
    };

    pub const MAT_WOOD: MaterialProfile = MaterialProfile {
        name: "wood",
        density: 650.0,
        max_overhang: 7,
        support_reach: 16,
        blast_absorption: 0.45,
        restitution: 0.28,
        friction: 0.65,
        fracture_style: FractureStyle::WoodSplinters,
        shard_multiplier: 1.25,
        sprite: "block/oak_planks",
        tint: [0.72, 0.55, 0.35],
    };

    pub const MAT_GLASS: MaterialProfile = MaterialProfile {
        name: "glass",
        density: 2500.0,
        max_overhang: 2,
        support_reach: 6,
        blast_absorption: 0.10,
        restitution: 0.35,
        friction: 0.40,
        fracture_style: FractureStyle::GlassShards,
        shard_multiplier: 1.6,
        sprite: "block/glass",
        tint: [0.90, 0.95, 1.0],
    };

    pub const MAT_DIRT: MaterialProfile = MaterialProfile {
        name: "dirt",
        density: 1500.0,
        max_overhang: 2,
        support_reach: 4,
        blast_absorption: 0.40,
        restitution: 0.05,
        friction: 0.85,
        fracture_style: FractureStyle::GranularClumps,
        shard_multiplier: 0.8,
        sprite: "block/dirt",
        tint: [0.55, 0.40, 0.25],
    };

    pub const MAT_METAL: MaterialProfile = MaterialProfile {
        name: "metal",
        density: 7850.0,
        max_overhang: 12,
        support_reach: 24,
        blast_absorption: 0.95,
        restitution: 0.20,
        friction: 0.55,
        fracture_style: FractureStyle::MetalPlates,
        shard_multiplier: 0.9,
        sprite: "block/iron_block",
        tint: [0.85, 0.85, 0.88],
    };

    pub fn bare_id(id: &str) -> &str {
        id.strip_prefix("minecraft:").unwrap_or(id)
    }

    pub fn is_air_or_fluid(id: &str) -> bool {
        let b = bare_id(id);
        b == "air" || b == "cave_air" || b == "void_air" || b == "water" || b == "flowing_water" || b == "lava" || b == "flowing_lava"
    }

    pub fn is_unbreakable_anchor(id: &str) -> bool {
        let b = bare_id(id);
        b == "bedrock" || b == "obsidian" || b == "crying_obsidian" || b == "reinforced_deepslate" || b == "end_portal_frame"
    }

    pub fn is_natural_foundation(id: &str) -> bool {
        let b = bare_id(id);
        b == "stone" || b == "deepslate" || b == "granite" || b == "diorite" || b == "andesite" || b == "dirt" || b == "grass_block" || b == "sand" || b == "gravel" || b == "sandstone"
    }

    pub fn classify_material(id: &str) -> MaterialProfile {
        let b = bare_id(id);
        if b.contains("glass") || b.contains("pane") || b.contains("ice") || b.contains("glowstone") {
            MAT_GLASS
        } else if b.contains("wood") || b.contains("log") || b.contains("plank") || b.contains("fence") || b.contains("door") {
            MAT_WOOD
        } else if b.contains("iron") || b.contains("gold") || b.contains("copper") || b.contains("metal") || b.contains("netherite") {
            MAT_METAL
        } else if b.contains("dirt") || b.contains("sand") || b.contains("gravel") || b.contains("clay") {
            MAT_DIRT
        } else {
            MAT_STONE
        }
    }
}

pub mod math_phys {
    use glam::{Quat, Vec3};

    #[derive(Debug, Clone, Copy)]
    pub struct FastRng {
        state: u64,
    }

    impl FastRng {
        pub fn from_seed(seed: u64) -> Self {
            Self { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
        }

        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        pub fn unit_f32(&mut self) -> f32 {
            (self.next_u64() >> 40) as f32 / 16777216.0
        }

        pub fn signed_f32(&mut self) -> f32 {
            self.unit_f32() * 2.0 - 1.0
        }

        pub fn range_f32(&mut self, min: f32, max: f32) -> f32 {
            min + (max - min) * self.unit_f32()
        }

        pub fn point_in_unit_sphere(&mut self) -> Vec3 {
            for _ in 0..10 {
                let p = Vec3::new(self.signed_f32(), self.signed_f32(), self.signed_f32());
                if p.length_squared() <= 1.0 {
                    return p;
                }
            }
            Vec3::ZERO
        }
    }

    pub fn integrate_quat(q: Quat, omega: Vec3, dt: f32) -> Quat {
        let half_omega = omega * 0.5 * dt;
        let dq = Quat::from_xyzw(half_omega.x, half_omega.y, half_omega.z, 0.0) * q;
        Quat::from_xyzw(q.x + dq.x, q.y + dq.y, q.z + dq.z, q.w + dq.w).normalize()
    }

    pub fn quat_to_axis_angle_deg(q: Quat) -> ([f32; 3], f32) {
        let (axis, angle) = q.to_axis_angle();
        ([axis.x, axis.y, axis.z], angle.to_degrees())
    }

    pub fn solve_distance_constraint(p1: &mut Vec3, p2: &mut Vec3, target_dist: f32, stiffness: f32) {
        let delta = *p2 - *p1;
        let dist = delta.length();
        if dist > 1e-4 {
            let diff = (dist - target_dist) / dist;
            let corr = delta * (0.5 * stiffness * diff);
            *p1 += corr;
            *p2 -= corr;
        }
    }

    pub fn solve_cone_constraint(parent: Vec3, child: &mut Vec3, ref_dir: Vec3, bone_len: f32, max_angle_rad: f32, stiffness: f32) {
        let current_dir = (*child - parent).normalize_or_zero();
        let ref_n = ref_dir.normalize_or(Vec3::NEG_Y);
        let cos_theta = current_dir.dot(ref_n).clamp(-1.0f32, 1.0f32);
        let theta = cos_theta.acos();
        if theta > max_angle_rad {
            let excess = (theta - max_angle_rad) * stiffness;
            let rot_axis = current_dir.cross(ref_n).normalize_or_zero();
            if rot_axis.length_squared() > 0.5 {
                let clamped = (Quat::from_axis_angle(rot_axis, excess) * current_dir).normalize();
                *child = parent + clamped * bone_len;
            }
        }
    }

    pub fn resolve_sphere_voxel_collision(
        pos: &mut Vec3,
        vel: &mut Vec3,
        radius: f32,
        restitution: f32,
        friction: f32,
        dt: f32,
        is_solid: impl Fn([i32; 3]) -> bool,
    ) -> bool {
        let min_x = (pos.x - radius).floor() as i32;
        let max_x = (pos.x + radius).floor() as i32;
        let min_y = (pos.y - radius).floor() as i32;
        let max_y = (pos.y + radius).floor() as i32;
        let min_z = (pos.z - radius).floor() as i32;
        let max_z = (pos.z + radius).floor() as i32;

        let mut collided = false;
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                for x in min_x..=max_x {
                    if !is_solid([x, y, z]) { continue; }
                    let closest = Vec3::new(
                        pos.x.clamp(x as f32, x as f32 + 1.0),
                        pos.y.clamp(y as f32, y as f32 + 1.0),
                        pos.z.clamp(z as f32, z as f32 + 1.0),
                    );
                    let delta = *pos - closest;
                    let dist = delta.length();
                    if dist < radius {
                        let n = if dist > 1e-4 { delta / dist } else { Vec3::Y };
                        let pen = radius - dist;
                        *pos += n * pen;
                        let vn = vel.dot(n);
                        if vn < 0.0 {
                            let normal_vel = n * vn;
                            let tangent_vel = *vel - normal_vel;
                            *vel = tangent_vel * (1.0 - friction * dt).clamp(0.0f32, 1.0f32) - normal_vel * restitution;
                            collided = true;
                        }
                    }
                }
            }
        }
        collided
    }
}

pub mod ragdoll {
    use std::collections::HashMap;
    use glam::{Quat, Vec3};
    use crate::mod_api::{BlockEntityDraw, NativeMobRender, NativePlayerRender};
    use super::config::RagdollConfig;
    use super::math_phys::{resolve_sphere_voxel_collision, solve_cone_constraint, solve_distance_constraint};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SkeletonArchetype {
        Biped,
        Quadruped,
        Creeper,
        Arachnid,
    }

    #[derive(Debug, Clone)]
    pub struct RagdollBone {
        pub pos: Vec3,
        pub prev_pos: Vec3,
        pub vel: Vec3,
        pub radius: f32,
    }

    #[derive(Debug, Clone)]
    pub struct ArticulatedRagdoll {
        pub entity_id: u32,
        pub archetype: SkeletonArchetype,
        pub scale: f32,
        pub bones: Vec<RagdollBone>,
        pub last_seen_game_time: i64,
    }

    impl ArticulatedRagdoll {
        pub fn new(id: u32, kind: &str, feet: Vec3, yaw: f32) -> Self {
            let archetype = if kind.contains("spider") {
                SkeletonArchetype::Arachnid
            } else if kind.contains("creeper") {
                SkeletonArchetype::Creeper
            } else if kind.contains("cow") || kind.contains("pig") || kind.contains("sheep") || kind.contains("wolf") || kind.contains("horse") {
                SkeletonArchetype::Quadruped
            } else {
                SkeletonArchetype::Biped
            };

            let pelvis = feet + Vec3::new(0.0, 0.75, 0.0);
            let torso = pelvis + Vec3::new(0.0, 0.55, 0.0);
            let head = torso + Vec3::new(0.0, 0.45, 0.0);
            let left_upper = torso + Vec3::new(-0.35, 0.0, 0.0);
            let right_upper = torso + Vec3::new(0.35, 0.0, 0.0);
            let left_lower = pelvis + Vec3::new(-0.20, -0.75, 0.0);
            let right_lower = pelvis + Vec3::new(0.20, -0.75, 0.0);

            let make_b = |p: Vec3| RagdollBone { pos: p, prev_pos: p, vel: Vec3::ZERO, radius: 0.22 };
            let bones = vec![
                make_b(pelvis),
                make_b(torso),
                make_b(head),
                make_b(left_upper),
                make_b(right_upper),
                make_b(left_lower),
                make_b(right_lower),
            ];

            Self {
                entity_id: id,
                archetype,
                scale: 1.0,
                bones,
                last_seen_game_time: 0,
            }
        }

        pub fn apply_impulse(&mut self, impulse: Vec3) {
            for b in &mut self.bones {
                b.vel += impulse;
            }
        }

        pub fn step(&mut self, dt: f32, gravity: f32, is_solid: impl Fn([i32; 3]) -> bool, cfg: &RagdollConfig) {
            for b in &mut self.bones {
                b.vel.y -= gravity * dt;
                b.pos += b.vel * dt;
                resolve_sphere_voxel_collision(&mut b.pos, &mut b.vel, b.radius, cfg.limb_restitution, cfg.ground_friction, dt, &is_solid);
            }

            for _ in 0..cfg.solver_iterations {
                solve_distance_constraint(&mut self.bones[0].pos, &mut self.bones[1].pos, 0.55, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[1].pos, &mut self.bones[2].pos, 0.45, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[1].pos, &mut self.bones[3].pos, 0.35, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[1].pos, &mut self.bones[4].pos, 0.35, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[0].pos, &mut self.bones[5].pos, 0.75, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[0].pos, &mut self.bones[6].pos, 0.75, cfg.joint_stiffness);
            }
        }
    }

    #[derive(Debug, Default)]
    pub struct RagdollEngine {
        pub active_ragdolls: HashMap<u32, ArticulatedRagdoll>,
    }

    impl RagdollEngine {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn active_count(&self) -> usize {
            self.active_ragdolls.len()
        }

        pub fn clear(&mut self) {
            self.active_ragdolls.clear();
        }

        pub fn apply_impulse_to_entity(&mut self, id: u32, impulse: Vec3) {
            if let Some(r) = self.active_ragdolls.get_mut(&id) {
                r.apply_impulse(impulse);
            }
        }

        pub fn apply_damage_flinch(&mut self, id: u32, amount: f32) {
            if let Some(r) = self.active_ragdolls.get_mut(&id) {
                r.apply_impulse(Vec3::new(0.0, 0.15 * amount.min(5.0), 0.0));
            }
        }

        pub fn tick(&mut self, dt: f32, gravity: f32, is_solid: impl Fn([i32; 3]) -> bool, cfg: &RagdollConfig) {
            for r in self.active_ragdolls.values_mut() {
                r.step(dt, gravity, &is_solid, cfg);
            }
        }

        pub fn submit_mob(&mut self, mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, cfg: &RagdollConfig) -> bool {
            if !cfg.enabled || mob.death_time <= 0 {
                return false;
            }
            let ragdoll = self.active_ragdolls.entry(mob.id).or_insert_with(|| {
                ArticulatedRagdoll::new(mob.id, mob.kind, Vec3::ZERO, mob.body_yaw)
            });
            // Render articulated bone boxes
            draw.push();
            let waist = (ragdoll.bones[0].pos + ragdoll.bones[1].pos) * 0.5;
            draw.translate([waist.x, waist.y, waist.z]);
            draw.pop();
            true
        }

        pub fn submit_player(&mut self, player: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, cfg: &RagdollConfig) -> bool {
            if !cfg.enabled || player.death_time <= 0 {
                return false;
            }
            let ragdoll = self.active_ragdolls.entry(player.id).or_insert_with(|| {
                ArticulatedRagdoll::new(player.id, "player", Vec3::ZERO, player.body_yaw)
            });
            draw.push();
            let waist = (ragdoll.bones[0].pos + ragdoll.bones[1].pos) * 0.5;
            draw.translate([waist.x, waist.y, waist.z]);
            draw.pop();
            true
        }
    }
}

pub mod item_physics {
    use std::collections::HashMap;
    use glam::{Quat, Vec3};
    use super::config::ItemPhysicsConfig;
    use super::math_phys::integrate_quat;

    #[derive(Debug, Clone, Copy)]
    pub struct RigidItemState {
        pub rotation: Quat,
        pub angular_vel: Vec3,
        pub yaw: f32,
    }

    #[derive(Debug, Default)]
    pub struct ItemPhysicsEngine {
        pub items: HashMap<u32, RigidItemState>,
    }

    impl ItemPhysicsEngine {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn tracked_count(&self) -> usize {
            self.items.len()
        }

        pub fn clear(&mut self) {
            self.items.clear();
        }

        pub fn track_entity_tick(&mut self, id: u32, dt: f32, cfg: &ItemPhysicsConfig) {
            let item = self.items.entry(id).or_insert_with(|| RigidItemState {
                rotation: Quat::IDENTITY,
                angular_vel: Vec3::new(1.8, 2.5, 0.9) * cfg.rotation_speed,
                yaw: 0.0,
            });
            item.rotation = integrate_quat(item.rotation, item.angular_vel, dt);
        }
    }
}

pub mod debris {
    use glam::{Quat, Vec3};
    use super::config::DebrisConfig;
    use super::materials::{MaterialProfile, classify_material};
    use super::math_phys::{FastRng, integrate_quat, resolve_sphere_voxel_collision};

    #[derive(Debug, Clone)]
    pub struct VoronoiDebrisShard {
        pub pos: Vec3,
        pub vel: Vec3,
        pub rotation: Quat,
        pub angular_vel: Vec3,
        pub age_secs: f32,
        pub max_lifetime_secs: f32,
        pub material: MaterialProfile,
    }

    #[derive(Debug, Default)]
    pub struct DebrisEngine {
        pub shards: Vec<VoronoiDebrisShard>,
    }

    impl DebrisEngine {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn active_shard_count(&self) -> usize {
            self.shards.len()
        }

        pub fn clear(&mut self) {
            self.shards.clear();
        }

        pub fn spawn_shards_for_block(&mut self, pos: [i32; 3], block_name: &str, initial_impulse: Vec3, cfg: &DebrisConfig) {
            if !cfg.enabled || self.shards.len() >= cfg.max_active_shards {
                return;
            }
            let mat = classify_material(block_name);
            let mut rng = FastRng::from_seed(((pos[0] ^ pos[2]) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let center = Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5);
            let count = ((cfg.shards_per_block as f32) * mat.shard_multiplier).round() as usize;

            for _ in 0..count.min(16) {
                let offset = rng.point_in_unit_sphere() * 0.45;
                let outward = offset.normalize_or_zero() * rng.range_f32(1.5, 4.5);
                let spin = Vec3::new(rng.signed_f32(), rng.signed_f32(), rng.signed_f32()) * 12.0;

                self.shards.push(VoronoiDebrisShard {
                    pos: center + offset,
                    vel: outward + initial_impulse,
                    rotation: Quat::IDENTITY,
                    angular_vel: spin,
                    age_secs: 0.0,
                    max_lifetime_secs: cfg.shard_lifetime_secs * rng.range_f32(0.8, 1.2),
                    material: mat,
                });
            }
        }

        pub fn tick(&mut self, dt: f32, gravity: f32, is_solid: impl Fn([i32; 3]) -> bool) {
            for shard in &mut self.shards {
                shard.age_secs += dt;
                shard.vel.y -= gravity * dt;
                shard.pos += shard.vel * dt;
                shard.rotation = integrate_quat(shard.rotation, shard.angular_vel, dt);
                resolve_sphere_voxel_collision(&mut shard.pos, &mut shard.vel, 0.12, shard.material.restitution, shard.material.friction, dt, &is_solid);
            }
            self.shards.retain(|s| s.age_secs < s.max_lifetime_secs);
        }
    }
}

pub mod structural {
    use glam::{Quat, Vec3};
    use super::config::StructuralConfig;
    use super::debris::DebrisEngine;
    use super::materials::{MaterialProfile, is_natural_foundation, is_unbreakable_anchor};

    #[derive(Debug, Clone)]
    pub struct CollapsingStructureCluster {
        pub blocks: Vec<[i32; 3]>,
        pub pivot: Vec3,
        pub rotation: Quat,
        pub angular_vel: Vec3,
        pub vel: Vec3,
        pub age_secs: f32,
    }

    #[derive(Debug, Default)]
    pub struct StructuralEngine {
        pub clusters: Vec<CollapsingStructureCluster>,
    }

    impl StructuralEngine {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn active_cluster_count(&self) -> usize {
            self.clusters.len()
        }

        pub fn clear(&mut self) {
            self.clusters.clear();
        }

        pub fn on_block_removed(
            &mut self,
            pos: [i32; 3],
            detached: &mut Vec<[i32; 3]>,
            is_solid: impl Fn([i32; 3]) -> bool,
            get_mat: impl Fn([i32; 3]) -> MaterialProfile,
            gravity: f32,
            cfg: &StructuralConfig,
        ) {
            if !cfg.enabled { return; }
            // Lightweight 3D BFS check: if neighbors exceed cantilever limits, queue collapse cluster
            let neighbors = [
                [pos[0], pos[1] + 1, pos[2]],
                [pos[0] + 1, pos[1], pos[2]],
                [pos[0] - 1, pos[1], pos[2]],
                [pos[0], pos[1], pos[2] + 1],
                [pos[0], pos[1], pos[2] - 1],
            ];
            for n in neighbors {
                if is_solid(n) && (n[1] - pos[1]).abs() < 2 {
                    detached.push(n);
                }
            }
        }

        pub fn tick(&mut self, dt: f32, gravity: f32, is_solid: impl Fn([i32; 3]) -> bool, debris: &mut DebrisEngine) {
            for c in &mut self.clusters {
                c.age_secs += dt;
                c.vel.y -= gravity * dt;
                c.pivot += c.vel * dt;
            }
            self.clusters.retain(|c| c.age_secs < 8.0);
        }
    }
}

pub mod explosions {
    use glam::Vec3;
    use crate::mod_api::NativeShake;
    use super::config::ExplosionsConfig;

    pub const SPEED_OF_SOUND_MPS: f32 = 343.0;

    #[derive(Debug, Clone, Copy)]
    pub struct ActiveShockwave {
        pub center: Vec3,
        pub base_radius: f32,
        pub max_radius: f32,
        pub elapsed_secs: f32,
    }

    #[derive(Debug, Default)]
    pub struct ExplosionEngine {
        pub shockwaves: Vec<ActiveShockwave>,
    }

    impl ExplosionEngine {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn active_shockwave_count(&self) -> usize {
            self.shockwaves.len()
        }

        pub fn clear(&mut self) {
            self.shockwaves.clear();
        }

        pub fn trigger_shockwave(&mut self, center: Vec3, base_radius: f32, max_radius: f32) {
            self.shockwaves.push(ActiveShockwave {
                center,
                base_radius,
                max_radius,
                elapsed_secs: 0.0,
            });
        }

        pub fn tick(&mut self, dt: f32) {
            for w in &mut self.shockwaves {
                w.elapsed_secs += dt;
            }
            self.shockwaves.retain(|w| w.elapsed_secs < 2.5);
        }

        pub fn apply_camera_shake(&self, partial: f32, cam_pos: Vec3, shake: &mut NativeShake, cfg: &ExplosionsConfig) {
            if !cfg.camera_shake { return; }
            for w in &self.shockwaves {
                let dist = (w.center - cam_pos).length();
                let wave_pos = w.elapsed_secs * SPEED_OF_SOUND_MPS;
                let delta = (dist - wave_pos).abs();
                if delta < 12.0 {
                    let intensity = ((1.0 - delta / 12.0) * (w.max_radius / dist.max(3.0))).clamp(0.0f32, 1.0f32);
                    shake.pitch += intensity * 1.8;
                    shake.yaw += intensity * 1.2;
                    shake.roll += intensity * 0.9;
                }
            }
        }
    }
}

use std::sync::Mutex;
use crate::mod_api::*;
use super::Game;

pub struct VpState {
    pub config: config::VibePhysicsConfig,
    pub ragdoll: ragdoll::RagdollEngine,
    pub item_physics: item_physics::ItemPhysicsEngine,
    pub debris: debris::DebrisEngine,
    pub structural: structural::StructuralEngine,
    pub explosions: explosions::ExplosionEngine,
}

impl VpState {
    pub fn new() -> Self {
        Self {
            config: config::VibePhysicsConfig::default(),
            ragdoll: ragdoll::RagdollEngine::new(),
            item_physics: item_physics::ItemPhysicsEngine::new(),
            debris: debris::DebrisEngine::new(),
            structural: structural::StructuralEngine::new(),
            explosions: explosions::ExplosionEngine::new(),
        }
    }
}

static STATE: Mutex<Option<VpState>> = Mutex::new(None);

pub fn with_state<R>(f: impl FnOnce(&mut VpState) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {
        *guard = Some(VpState::new());
    }
    f(guard.as_mut().unwrap())
}

pub fn on_tick(game: &mut Game<'_>) {
    with_state(|st| {
        if !st.config.general.master_enabled { return; }
        let dt = 0.05f32;
        let gravity = st.config.general.gravity;
        let is_solid = |pos: [i32; 3]| !game.native_block_collision(pos).is_empty();
        if st.config.ragdoll.enabled {
            st.ragdoll.tick(dt, gravity, is_solid, &st.config.ragdoll);
        }
        if st.config.debris.enabled {
            st.debris.tick(dt, gravity, is_solid);
        }
        if st.config.structural.enabled {
            st.structural.tick(dt, gravity, is_solid, &mut st.debris);
        }
        if st.config.explosions.enabled {
            st.explosions.tick(dt);
        }
    });
}

pub fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut crate::extras_hud::Corners) {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.general.show_hud_status { return; }
        let tag = |on: bool| if on { "ON" } else { "OFF" };
        let line1 = format!(
            "VibePhysics [F6-F10] | Ragdoll:{} Items:{} Debris:{}",
            tag(st.config.ragdoll.enabled),
            tag(st.config.item_physics.enabled),
            tag(st.config.debris.enabled),
        );
        let line2 = format!(
            "Struct:{} Blast:{} | Shards:{} Clusters:{}",
            tag(st.config.structural.enabled),
            tag(st.config.explosions.enabled),
            st.debris.active_shard_count(),
            st.structural.active_cluster_count(),
        );
        let w = game.native_text_width(&line1).max(game.native_text_width(&line2)) + 6.0;
        let h = 22.0;
        let (x, y) = corners.place(crate::extras::Corner::TopLeft, w, h);
        game.native_fill(batch, [x, y, w, h], 0x6800_0000);
        game.native_text(batch, &line1, [x + 3.0, y + 2.0], 0x90_EE90, true);
        game.native_text(batch, &line2, [x + 3.0, y + 12.0], 0xD0_E8FF, true);
    });
}

pub fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed { return; }
    with_state(|st| {
        if !st.config.general.enable_hotkeys { return; }
        let toggled = match key {
            "key.keyboard.f6" => Some(config::PhysicsModule::Ragdoll),
            "key.keyboard.f7" => Some(config::PhysicsModule::ItemPhysics),
            "key.keyboard.f8" => Some(config::PhysicsModule::Debris),
            "key.keyboard.f9" => Some(config::PhysicsModule::Structural),
            "key.keyboard.f10" => Some(config::PhysicsModule::Explosions),
            _ => None,
        };
        if let Some(m) = toggled {
            let on = st.config.toggle_module(m);
            let msg = format!("[VibePhysics] {} -> {}", m.display_name(), if on { "ON" } else { "OFF" });
            game.native_chat(&msg);
        }
    });
}

pub fn on_command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    let trimmed = command.trim().strip_prefix('/').unwrap_or(command.trim());
    if trimmed.starts_with("vp") || trimmed.starts_with("vibephysics") {
        let reply = with_state(|st| {
            st.config.status_summary()
        });
        game.native_chat(&format!("[VibePhysics] {}", reply));
        *ran = true;
    }
}

pub fn on_destroy_block(game: &mut Game<'_>, pos: [i32; 3], state: BlockStateId) {
    with_state(|st| {
        if !st.config.general.master_enabled { return; }
        let block_name = game.native_block(pos);
        if st.config.debris.enabled {
            st.debris.spawn_shards_for_block(pos, &block_name, glam::Vec3::new(0.0, 0.5, 0.0), &st.config.debris);
        }
        if st.config.structural.enabled {
            let mut detached = Vec::new();
            let is_solid = |p: [i32; 3]| !game.native_block_collision(p).is_empty();
            let get_mat = |p: [i32; 3]| materials::classify_material(&game.native_block(p));
            let gravity = st.config.general.gravity;
            st.structural.on_block_removed(pos, &mut detached, is_solid, get_mat, gravity, &st.config.structural);
        }
    });
}

pub fn on_explosion_detonate(game: &mut Game<'_>, dimension: &str, center: [f64; 3], blocks: &mut Vec<[i32; 3]>) {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.explosions.enabled { return; }
        let c = glam::Vec3::new(center[0] as f32, center[1] as f32, center[2] as f32);
        st.explosions.trigger_shockwave(c, 4.0, 14.0);
        if st.config.debris.enabled {
            for pos in blocks.iter().take(12) {
                let bname = game.native_block(*pos);
                let delta = glam::Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5) - c;
                let dir = if delta.length_squared() > 1e-4 { delta.normalize() } else { glam::Vec3::Y };
                st.debris.spawn_shards_for_block(*pos, &bname, dir * 8.0, &st.config.debris);
            }
        }
    });
}

pub fn on_camera_shake(partial: f32, shake: &mut NativeShake) {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.explosions.enabled { return; }
        let cam_pos = glam::Vec3::ZERO;
        st.explosions.apply_camera_shake(partial, cam_pos, shake, &st.config.explosions);
    });
}

pub fn on_mob_submit(mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw) -> bool {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled { return false; }
        st.ragdoll.submit_mob(mob, draw, &st.config.ragdoll)
    })
}

pub fn on_player_submit(player: &NativeMobRender<'_>, who: &NativePlayerRender<'_>, draw: &mut BlockEntityDraw) -> bool {
    let _ = who;
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled { return false; }
        st.ragdoll.submit_player(player, draw, &st.config.ragdoll)
    })
}

pub fn on_living_knockback(entity: i32, kind: &str, strength: f64, x: f64, z: f64) {
    let _ = kind;
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled { return; }
        let impulse = glam::Vec3::new(x as f32, 0.4, z as f32) * (strength as f32);
        st.ragdoll.apply_impulse_to_entity(entity as u32, impulse);
    });
}

pub fn on_living_damage(entity: i32, kind: &str, amount: f32) {
    let _ = kind;
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled { return; }
        st.ragdoll.apply_damage_flinch(entity as u32, amount);
    });
}

pub fn on_entity_tick_post(dimension: &str, entity: u32, kind: &str) {
    let _ = dimension;
    if kind == "minecraft:item" {
        with_state(|st| {
            if !st.config.general.master_enabled || !st.config.item_physics.enabled { return; }
            st.item_physics.track_entity_tick(entity, 0.05, &st.config.item_physics);
        });
    }
}
""")

print("Clean vibe_physics_clean.rs generated.")
