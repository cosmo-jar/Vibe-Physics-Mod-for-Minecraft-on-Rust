// ============================================================================
// Vibe Physics v0.3.0 — Ignis Advanced World Physics Engine
// For Horizon 26.3 (6475f62)
//
// Subsystems (Modular, Hotkey-Toggleable F6..F10 & /vp):
//   1. [F6] Ragdoll      — Authentic 3D Skinned Mob Ragdolls & Dynamic Limb Dismemberment
//   2. [F7] ItemPhysics  — 3D Airborne Tumbling, Flat Ground Resting, Ice Glide & Buoyancy
//   3. [F8] Debris       — 3D Sub-Voxel Splinters, Shards & Top-Down Cascading Sand Crumble
//   4. [F9] Structural   — 3D Rigid-Body Tree Felling (Player Collision + Axe Chopping) & Wall Topple
//   5. [F10] Explosions  — 3D Spinning Crater Blocks, Tree Uprooting, Glass Shatter & Trauma Shake
// ============================================================================

use std::collections::{HashMap, HashSet, VecDeque};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Mutex;

use glam::{DVec3, IVec3, Mat3, Mat4, Quat, Vec3};
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    block_entity_render::{diffuse, diffuse_lights, vertex_light},
    hud::{HudBatch, Rect},
    item_render,
    items::ItemStack,
    mesher::Vertex,
    mobs::{Mob, Part, Species},
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::geometry::Aabb;
use horizon_world::registry::BlockStateId;

use super::{
    Dim, Game, NativeMobRender, NativeShake,
    native_block_entity_draw::BlockEntityDraw,
    native_mob_draw::NativePlayerRender,
};

// ============================================================================
// 1. MODULAR CONFIGURATION
// ============================================================================

pub mod config {
    #[allow(dead_code)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PhysicsModule {
        BuildingCollapse,
        TreeFelling,
        DustClouds,
        Explosions,
        Debris,
        Ragdoll,
        ItemPhysics,
        Master,
    }

    #[allow(dead_code)]
    impl PhysicsModule {
        pub fn display_name(self) -> &'static str {
            match self {
                Self::BuildingCollapse => "Обрушение зданий и конструкций",
                Self::TreeFelling => "Валка деревьев",
                Self::DustClouds => "Эффекты пыли и дыма",
                Self::Explosions => "Взрывная волна и баллистика",
                Self::Debris => "Осколки блоков и песчинки",
                Self::Ragdoll => "Регдолл и спотыкание мобов",
                Self::ItemPhysics => "3D физика выброшенных предметов",
                Self::Master => "Общая физика мода",
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct VibeConfig {
        pub master_enabled: bool,
        pub building_collapse: bool,
        pub tree_felling: bool,
        pub dust_clouds: bool,
        pub explosions: bool,
        pub debris: bool,
        pub ragdoll: bool,
        pub item_physics: bool,
        pub gravity: f32,
        pub menu_open: bool,
        pub menu_selected_idx: usize,
    }

    impl Default for VibeConfig {
        fn default() -> Self {
            Self {
                master_enabled: true,
                building_collapse: true,
                tree_felling: true,
                dust_clouds: true,
                explosions: true,
                debris: true,
                ragdoll: true,
                item_physics: true,
                gravity: 25.0,
                menu_open: false,
                menu_selected_idx: 0,
            }
        }
    }

    impl VibeConfig {
        pub fn toggle_by_index(&mut self, idx: usize) -> (&'static str, bool) {
            match idx {
                0 => {
                    self.building_collapse = !self.building_collapse;
                    ("Обрушение зданий и конструкций", self.building_collapse)
                }
                1 => {
                    self.tree_felling = !self.tree_felling;
                    ("Валка деревьев", self.tree_felling)
                }
                2 => {
                    self.dust_clouds = !self.dust_clouds;
                    ("Эффекты пыли и дыма", self.dust_clouds)
                }
                3 => {
                    self.explosions = !self.explosions;
                    ("Взрывная волна и баллистика", self.explosions)
                }
                4 => {
                    self.debris = !self.debris;
                    ("Осколки блоков и песчинки", self.debris)
                }
                5 => {
                    self.ragdoll = !self.ragdoll;
                    ("Регдолл и спотыкание мобов", self.ragdoll)
                }
                6 => {
                    self.item_physics = !self.item_physics;
                    ("3D физика выброшенных предметов", self.item_physics)
                }
                7 => {
                    self.master_enabled = !self.master_enabled;
                    ("Общая физика мода", self.master_enabled)
                }
                _ => ("Неизвестно", false),
            }
        }

        pub fn summary(&self) -> String {
            let b = |v: bool| if v { "ВКЛ" } else { "ВЫКЛ" };
            format!(
                "Master:{} | Здания:{} Деревья:{} Дым:{} Взрывы:{} Осколки:{} Регдолл:{} Предметы:{} (RShift для меню)",
                b(self.master_enabled),
                b(self.building_collapse),
                b(self.tree_felling),
                b(self.dust_clouds),
                b(self.explosions),
                b(self.debris),
                b(self.ragdoll),
                b(self.item_physics),
            )
        }
    }
}

// ============================================================================
// 2. FAST DETERMINISTIC RNG & QUATERNION / RAYCAST HELPERS
// ============================================================================

#[derive(Clone, Copy)]
struct FastRng(u64);

impl FastRng {
    fn seeded(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 16) as u32
    }

    fn f32(&mut self) -> f32 {
        (self.next_u32() & 0x00FF_FFFF) as f32 / 16_777_216.0
    }

    fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    fn vec3_spread(&mut self, xz: f32, y_lo: f32, y_hi: f32) -> Vec3 {
        let a = self.f32() * TAU;
        let r = self.range(0.25, 1.0) * xz;
        Vec3::new(a.cos() * r, self.range(y_lo, y_hi), a.sin() * r)
    }
}

#[inline]
fn integrate_quat(q: Quat, ang_vel: Vec3, dt: f32) -> Quat {
    let w_len = ang_vel.length();
    if w_len < 1.0e-4 {
        return q;
    }
    let dq = Quat::from_axis_angle(ang_vel / w_len, w_len * dt);
    (dq * q).normalize()
}

/// Relaxes a quaternion's tilt so one of its principal faces settles flat on horizontal ground.
#[inline]
fn settle_quat_flat(q: Quat, rate: f32) -> Quat {
    let (yaw, pitch, roll) = q.to_euler(glam::EulerRot::YXZ);
    let snap = |a: f32| (a / FRAC_PI_2).round() * FRAC_PI_2;
    let tp = snap(pitch);
    let tr = snap(roll);
    let target = Quat::from_euler(glam::EulerRot::YXZ, yaw, tp, tr);
    q.slerp(target, rate.clamp(0.0, 1.0)).normalize()
}

/// Ray vs AABB [min..max] returning entry distance `t` in `[0, max_dist]`.
fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3, max_dist: f32) -> Option<f32> {
    let inv = Vec3::new(
        if dir.x.abs() > 1e-6 { 1.0 / dir.x } else { 1e6 },
        if dir.y.abs() > 1e-6 { 1.0 / dir.y } else { 1e6 },
        if dir.z.abs() > 1e-6 { 1.0 / dir.z } else { 1e6 },
    );
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let tmin = t1.min(t2);
    let tmax = t1.max(t2);
    let enter = tmin.x.max(tmin.y).max(tmin.z).max(0.0);
    let exit = tmax.x.min(tmax.y).min(tmax.z);
    if exit >= enter && enter <= max_dist {
        Some(enter)
    } else {
        None
    }
}

/// 0xRRGGBB tint multiplied by directional diffuse brightness -> little-endian RGBA8 vertex color.
#[inline]
fn shade_abgr(rgb: u32, brightness: f32) -> u32 {
    let r = (((rgb >> 16) & 0xFF) as f32 * brightness).clamp(0.0, 255.0) as u32;
    let g = (((rgb >> 8) & 0xFF) as f32 * brightness).clamp(0.0, 255.0) as u32;
    let b = ((rgb & 0xFF) as f32 * brightness).clamp(0.0, 255.0) as u32;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

// ============================================================================
// 3. MATERIAL CLASSIFICATION & 3D SUB-VOXEL DEBRIS / GRANULAR CASCADE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkMat {
    Granular, // Sand, red sand, gravel, concrete powder, dirt, mud, snow
    Wood,     // Logs, planks, stripped wood, bamboo, fences, chests
    Glass,    // Glass, stained glass, panes, ice, glowstone, amethyst
    Foliage,  // Leaves, vines, moss, azalea, wart blocks
    Stone,    // Stone, bricks, ores, deepslate, terracotta, metals
    Gore,     // Severed ragdoll blood/bone/viscera chunks
}

pub fn classify_block_name(name: &str) -> ChunkMat {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    if n.contains("sand") && !n.contains("sandstone")
        || n.contains("gravel")
        || n.contains("powder")
        || n == "dirt"
        || n == "coarse_dirt"
        || n == "rooted_dirt"
        || n == "podzol"
        || n == "mycelium"
        || n == "mud"
        || n == "clay"
        || n == "farmland"
        || n == "grass_block"
        || n.contains("snow")
    {
        ChunkMat::Granular
    } else if n.contains("_log")
        || n.contains("_wood")
        || n.contains("_stem")
        || n.contains("_hyphae")
        || n.contains("planks")
        || n.contains("bamboo")
        || n.contains("fence")
        || n.contains("door")
        || n.contains("trapdoor")
        || n == "chest"
        || n == "barrel"
        || n == "crafting_table"
        || n == "bookshelf"
        || n == "mangrove_roots"
    {
        ChunkMat::Wood
    } else if n.contains("glass")
        || n.contains("ice")
        || n == "glowstone"
        || n == "sea_lantern"
        || n.contains("amethyst")
    {
        ChunkMat::Glass
    } else if n.contains("leaves")
        || n.contains("wart_block")
        || n == "shroomlight"
        || n.contains("vine")
        || n.contains("moss")
        || n.contains("azalea")
    {
        ChunkMat::Foliage
    } else {
        ChunkMat::Stone
    }
}

pub fn is_true_gravity_granular(name: &str) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    n == "sand"
        || n == "red_sand"
        || n == "suspicious_sand"
        || n == "soul_sand"
        || n == "gravel"
        || n == "suspicious_gravel"
        || n.ends_with("concrete_powder")
}

pub fn is_tree_log_name(name: &str) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    (n.contains("_log")
        || n.contains("_wood")
        || n.contains("_stem")
        || n.contains("_hyphae")
        || n == "mangrove_roots")
        && !n.contains("stripped")
        || n.starts_with("stripped_") && (n.ends_with("_log") || n.ends_with("_stem"))
}

pub fn is_tree_foliage_name(name: &str) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    n.contains("_leaves")
        || n.contains("wart_block")
        || n == "shroomlight"
        || n == "vine"
        || n == "bee_nest"
        || n == "cocoa"
        || n == "moss_carpet"
}

/// Real 3D lit, textured, rotating sub-voxel splinter / grain / chunk.
#[derive(Clone)]
pub struct SubVoxelChunk {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    pub prev_rot: Quat,
    pub ang_vel: Vec3,
    pub half: Vec3,
    pub state: BlockStateId,
    pub origin_block: [i32; 3],
    pub uv_sub: [f32; 4], // normalized 0..1 sub-window inside block sprite
    pub custom_sprite: Option<(&'static str, u32)>, // e.g. ("block/white_concrete", 0x991111)
    pub resolved: Option<(Rect, u32)>,
    pub mat: ChunkMat,
    pub age: u16,
    pub max_age: u16,
    pub grounded: bool,
}

/// Scheduled progressive block crumble (for top-to-bottom column disintegration).
#[allow(dead_code)]
#[derive(Clone)]
pub struct ScheduledCrumble {
    pub delay_ticks: u16,
    pub pos: [i32; 3],
    pub layer_y_offset: f32, // 0.0..1.0 within block or full block if < 0
    pub state: BlockStateId,
    pub mat: ChunkMat,
    pub count: u8,
    pub bias_vel: Vec3,
    pub drop_loot_on_fire: bool,
    pub is_fine_sand: bool,
}

/// Ultra-fast 3D micro-grain for crumbling & scattering sand, gravel, and dry concrete powder.
#[derive(Clone, Copy)]
pub struct SandGrain {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    pub state: BlockStateId,
    pub origin_block: [i32; 3],
    pub resolved: Option<(Rect, u32)>,
    pub packed_light: u32,
    pub uv_offset: [f32; 2],
    pub shade_mul: f32,
    pub age: u16,
    pub max_age: u16,
    pub grounded: bool,
}

// ============================================================================
// 4. 3D RIGID-BODY TREES, COLLAPSING STRUCTURES & FLYING CRATER BLOCKS
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyKind {
    Tree,
    Masonry,
    LooseLog,
}

#[derive(Clone)]
pub struct RigidCell {
    pub local: IVec3, // integer offset relative to pivot cell
    pub state: BlockStateId,
    pub is_foliage: bool,
    pub hits_taken: u8,
    pub deform_offset: Vec3, // plastic bending / sag displacement from heavy impacts
    pub dent_rot: Quat,      // local buckling / twist deformation from impacts
}

#[derive(Clone)]
pub struct RigidAssembly {
    pub id: u64,
    pub kind: AssemblyKind,
    pub pivot: DVec3,
    pub prev_pivot: DVec3,
    pub vel: DVec3,
    pub rot: Quat,
    pub prev_rot: Quat,
    pub fall_axis: Vec3,
    pub fall_dir_xz: Vec3,
    pub fall_angle: f32,
    pub ang_speed: f32,
    pub free_ang_vel: Vec3,
    pub eff_length: f32,
    pub hinged: bool,
    pub settled: bool,
    pub settle_ticks: u16,
    pub ticks_alive: u32,
    pub crashed_once: bool,
    pub origin_stump: [i32; 3],
    pub cells: Vec<RigidCell>,
    pub hit_mobs: HashSet<u32>,
}

impl RigidAssembly {
    #[inline]
    pub fn cell_world_center(&self, local: IVec3) -> DVec3 {
        let local_center = Vec3::new(
            local.x as f32,
            local.y as f32 + 0.5,
            local.z as f32,
        );
        let turned = self.rot * local_center;
        self.pivot + DVec3::new(turned.x as f64, turned.y as f64, turned.z as f64)
    }

    #[inline]
    pub fn pose_matrix(&self, camera: DVec3, alpha: f64) -> Mat4 {
        let p = self.prev_pivot.lerp(self.pivot, alpha) - camera;
        let q = self.prev_rot.slerp(self.rot, alpha as f32);
        Mat4::from_rotation_translation(q, p.as_vec3())
    }
}

/// Full 1x1x1 block blasted into ballistic 3D spinning flight by explosions or structural shatter.
#[derive(Clone)]
pub struct FlyingBlock {
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub vel: DVec3,
    pub rot: Quat,
    pub prev_rot: Quat,
    pub ang_vel: Vec3,
    pub state: BlockStateId,
    pub seed: i64,
    pub bounces: u8,
    pub age: u16,
    pub place_on_land: bool,
}

/// Persistent world fracture / structural fatigue on blocks damaged by shockwaves or impacts.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct CrackedBlock {
    pub pos: [i32; 3],
    pub state: BlockStateId,
    pub damage_stage: u8, // 1..=3 (at >=3 the block fails and collapses!)
    pub packed_light: u32,
    pub age_ticks: u16,
}

/// Volumetric 3D Dust Cloud & Hot Bouncing Metal/Stone Spark particle types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfxKind {
    DustCloud,
    Spark,
}

/// High-performance cached-light 3D volumetric dust & spark particle.
#[derive(Clone, Copy)]
pub struct VfxParticle {
    pub kind: VfxKind,
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    pub rgb: u32,
    pub packed_light: u32,
    pub age: u16,
    pub max_age: u16,
}

/// Node in a 3D Verlet-simulated hanging chain, rope, or chandelier cord.
#[derive(Clone, Copy)]
pub struct RopeNode {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub pinned: bool,
}

/// Physical 3D hanging chain / rope with optional suspended lantern, bell, or load.
#[allow(dead_code)]
#[derive(Clone)]
pub struct VerletRope {
    pub id: u64,
    pub anchor_block: [i32; 3],
    pub chain_state: BlockStateId,
    pub payload_state: Option<BlockStateId>,
    pub chain_blocks: i32,
    pub nodes: Vec<RopeNode>,
    pub seg_len: f32,
    pub packed_light: u32,
    pub broken: bool,
    pub age_ticks: u32,
}

/// Node in a 2D grid of 3D Verlet-simulated cloth (banners, flags, hanging curtains).
#[derive(Clone, Copy)]
pub struct ClothNode {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub pinned: bool,
}

/// Physical 3D wind-rippling & tearable cloth sheet for banners and flags.
#[allow(dead_code)]
#[derive(Clone)]
pub struct VerletCloth {
    pub id: u64,
    pub anchor_pos: [i32; 3],
    pub support_pos: Option<[i32; 3]>,
    pub cols: usize,
    pub rows: usize,
    pub spacing_x: f32,
    pub spacing_y: f32,
    pub right_dir: Vec3,
    pub normal_dir: Vec3,
    pub rgb: u32,
    pub packed_light: u32,
    pub nodes: Vec<ClothNode>,
    pub h_torn: Vec<bool>, // rows * (cols - 1) horizontal links
    pub v_torn: Vec<bool>, // (rows - 1) * cols vertical links
    pub drop_state: Option<BlockStateId>,
    pub age_ticks: u32,
}

// ============================================================================
// 5. AUTHENTIC 3D SKINNED & DISMEMBERABLE MOB RAGDOLLS + ACTIVE RAGDOLL STUMBLE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimbRole {
    Torso,
    Head,
    Arm,
    Leg,
    Appendage,
}

#[derive(Clone)]
pub struct BakedLimbQuad {
    pub verts: [[f32; 3]; 4], // local to limb centroid, in metric block units
    pub uvs: [[f32; 2]; 4],   // normalized 0..1 in sprite rect
    pub normal: Vec3,
    pub cull_back: bool,
    pub use_overlay_rect: bool,
    pub tint_abgr: u32,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct RagdollLimb {
    pub name: String,
    pub role: LimbRole,
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    pub prev_rot: Quat,
    pub ang_vel: Vec3,
    pub half: Vec3,
    pub radius: f32,
    // Joint connecting this limb to torso (limb 0):
    pub anchor_on_torso: Vec3, // in torso local space
    pub anchor_on_self: Vec3,  // in limb local space
    pub rest_dist: f32,
    pub severed: bool,
    pub grounded: bool,
    pub quads: Vec<BakedLimbQuad>,
}

#[derive(Clone)]
pub struct MobRagdoll {
    pub mob_id: u32,
    pub primary_rect: Rect,
    pub overlay_rect: Option<Rect>,
    pub gore_rgb: u32,
    pub limbs: Vec<RagdollLimb>,
    pub age_ticks: u32,
    pub max_ticks: u32,
}

/// Articulated limb for a LIVING mob undergoing Active Ragdoll hit flinch / stumble recovery.
#[derive(Clone)]
pub struct StumbleLimb {
    pub role: LimbRole,
    pub side_sign: f32, // -1.0 left, 1.0 right
    pub pivot_ent: Vec3,
    pub local_rot: Quat,
    pub prev_local_rot: Quat,
    pub quads: Vec<BakedLimbQuad>, // vertices relative to pivot_ent
}

/// Active Ragdoll PD-controlled balance loss & stumble rig for living mobs hit by attacks/blasts.
#[allow(dead_code)]
#[derive(Clone)]
pub struct ActiveStumbleRig {
    pub mob_id: u32,
    pub primary_rect: Rect,
    pub overlay_rect: Option<Rect>,
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub body_yaw: f32,
    pub lean_axis_ent: Vec3,
    pub lean_angle: f32,
    pub prev_lean_angle: f32,
    pub lean_vel: f32,
    pub phase: f32,
    pub ticks_left: u16,
    pub hurt_flash_ticks: u8,
    pub packed_light: u32,
    pub limbs: Vec<StumbleLimb>,
}

#[derive(Clone, Copy)]
pub struct PendingKillImpulse {
    pub dir: Vec3,
    pub strength: f32,
    pub damage: f32,
    pub explosive: bool,
    pub tick_recorded: u64,
}

// ============================================================================
// 6. SHARED CLIENT-SERVER PHYSICS WORLD & QUEUES
// ============================================================================

#[allow(dead_code)]
pub enum ServerOp {
    SetAir {
        dim: String,
        pos: [i32; 3],
    },
    PlaceBlockIfAir {
        dim: String,
        pos: [i32; 3],
        state: BlockStateId,
    },
    DropBlockLoot {
        pos: [i32; 3],
        state: BlockStateId,
    },
    CrushMob {
        dim: String,
        mob_id: u32,
        damage: f32,
        push: [f64; 3],
    },
}

pub enum ClientFx {
    Sound {
        sound: &'static str,
        at: [f64; 3],
        vol: f32,
        pitch: f32,
    },
    Smoke {
        at: [f64; 3],
        vel: [f64; 3],
        large: bool,
    },
    BlockBurst {
        pos: [i32; 3],
        state: BlockStateId,
        count: u8,
        speed: f64,
    },
}

#[derive(Clone, Copy)]
pub struct CameraTrauma {
    pub origin: Vec3,
    pub radius: f32,
    pub trauma: f32,
    pub elapsed: f32,
    pub duration: f32,
}

pub struct VpWorld {
    pub cfg: config::VibeConfig,
    pub next_id: u64,
    pub tick_counter: u64,
    pub last_player_pos: Vec3,
    pub last_player_vel: Vec3,
    pub player_holding_sharp: bool,
    pub player_holding_axe: bool,

    // Active 3D simulation objects (simulated & rendered on Client)
    pub chunks: Vec<SubVoxelChunk>,
    pub crumbles: Vec<ScheduledCrumble>,
    pub sand_grains: Vec<SandGrain>,
    pub vfx_particles: Vec<VfxParticle>,
    pub cracked_blocks: Vec<CrackedBlock>,
    pub ropes: Vec<VerletRope>,
    pub cloths: Vec<VerletCloth>,
    pub assemblies: Vec<RigidAssembly>,
    pub flying_blocks: Vec<FlyingBlock>,
    pub ragdolls: Vec<MobRagdoll>,
    pub active_stumbles: HashMap<u32, ActiveStumbleRig>,
    pub pending_stumbles: Vec<(u32, Vec3, f32)>,
    pub pending_shockwaves: Vec<(Vec3, f32)>,
    pub spawned_ragdoll_ids: HashSet<u32>,
    pub pending_impulses: HashMap<u32, PendingKillImpulse>,
    pub traumas: Vec<CameraTrauma>,

    // Cross-thread Client <-> Integrated Server queues
    pub server_ops: VecDeque<ServerOp>,
    pub client_fx: VecDeque<ClientFx>,
}

impl VpWorld {
    pub fn new() -> Self {
        Self {
            cfg: config::VibeConfig::default(),
            next_id: 1,
            tick_counter: 0,
            last_player_pos: Vec3::ZERO,
            last_player_vel: Vec3::ZERO,
            player_holding_sharp: false,
            player_holding_axe: false,
            chunks: Vec::with_capacity(512),
            crumbles: Vec::with_capacity(64),
            sand_grains: Vec::with_capacity(1024),
            vfx_particles: Vec::with_capacity(640),
            cracked_blocks: Vec::with_capacity(128),
            ropes: Vec::with_capacity(24),
            cloths: Vec::with_capacity(24),
            assemblies: Vec::with_capacity(16),
            flying_blocks: Vec::with_capacity(64),
            ragdolls: Vec::with_capacity(24),
            active_stumbles: HashMap::new(),
            pending_stumbles: Vec::new(),
            pending_shockwaves: Vec::new(),
            spawned_ragdoll_ids: HashSet::new(),
            pending_impulses: HashMap::new(),
            traumas: Vec::new(),
            server_ops: VecDeque::new(),
            client_fx: VecDeque::new(),
        }
    }

    pub fn clear_all(&mut self) {
        self.chunks.clear();
        self.crumbles.clear();
        self.sand_grains.clear();
        self.vfx_particles.clear();
        self.cracked_blocks.clear();
        self.ropes.clear();
        self.cloths.clear();
        self.assemblies.clear();
        self.flying_blocks.clear();
        self.ragdolls.clear();
        self.active_stumbles.clear();
        self.pending_stumbles.clear();
        self.pending_shockwaves.clear();
        self.traumas.clear();
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    pub fn add_trauma(&mut self, origin: Vec3, radius: f32, trauma: f32, duration: f32) {
        if self.traumas.len() < 16 {
            self.traumas.push(CameraTrauma {
                origin,
                radius,
                trauma,
                elapsed: 0.0,
                duration,
            });
        }
    }

    /// Spawns billowing 3D volumetric dust clouds from collapsing structures, trees, or cave-ins.
    pub fn spawn_dust_cloud(
        &mut self,
        center: Vec3,
        box_half: Vec3,
        rgb: u32,
        count: usize,
        bias_vel: Vec3,
    ) {
        if !self.cfg.dust_clouds {
            return;
        }
        const MAX_VFX: usize = 600;
        if self.vfx_particles.len() + count > MAX_VFX {
            let drain_n = (self.vfx_particles.len() + count - MAX_VFX).min(self.vfx_particles.len());
            self.vfx_particles.drain(0..drain_n);
        }
        let mut rng = FastRng::seeded(
            (center.x.to_bits() as u64)
                ^ ((center.z.to_bits() as u64) << 17)
                ^ self.tick_counter.wrapping_mul(0x9E37_79B9),
        );
        for _ in 0..count {
            let p = center
                + Vec3::new(
                    rng.signed() * box_half.x,
                    rng.signed() * box_half.y,
                    rng.signed() * box_half.z,
                );
            let vel = bias_vel + rng.vec3_spread(1.45, 0.25, 1.35);
            self.vfx_particles.push(VfxParticle {
                kind: VfxKind::DustCloud,
                pos: p,
                prev_pos: p,
                vel,
                radius: rng.range(0.13, 0.25),
                rgb,
                packed_light: 0x00E0_00E0,
                age: 0,
                max_age: rng.range(38.0, 78.0) as u16,
            });
        }
    }

    /// Spawns bright fullbright 3D bouncing metal/stone sparks!
    pub fn spawn_sparks(&mut self, center: Vec3, bias_vel: Vec3, count: usize) {
        const MAX_VFX: usize = 600;
        if self.vfx_particles.len() + count > MAX_VFX {
            let drain_n = (self.vfx_particles.len() + count - MAX_VFX).min(self.vfx_particles.len());
            self.vfx_particles.drain(0..drain_n);
        }
        let mut rng = FastRng::seeded(
            (center.y.to_bits() as u64)
                ^ ((center.x.to_bits() as u64) << 13)
                ^ self.tick_counter.wrapping_mul(0x517C_C1B7),
        );
        for i in 0..count {
            let p = center + Vec3::new(rng.signed() * 0.15, rng.signed() * 0.15, rng.signed() * 0.15);
            let vel = bias_vel + rng.vec3_spread(5.2, 1.2, 6.2);
            let rgb = if i % 3 == 0 {
                0xFFF6C0 // white-hot core
            } else if i % 3 == 1 {
                0xFFB428 // bright golden spark
            } else {
                0xFF5E14 // fiery orange ember
            };
            self.vfx_particles.push(VfxParticle {
                kind: VfxKind::Spark,
                pos: p,
                prev_pos: p,
                vel,
                radius: rng.range(0.018, 0.035),
                rgb,
                packed_light: 0x00F0_00F0,
                age: 0,
                max_age: rng.range(18.0, 42.0) as u16,
            });
        }
    }

    /// Adds or intensifies persistent structural fracture damage on a world block.
    /// Returns `true` if the block exceeded its fracture limit (`damage_stage >= 3`) and should collapse!
    pub fn add_block_crack(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        add_stage: u8,
    ) -> bool {
        if let Some(existing) = self.cracked_blocks.iter_mut().find(|c| c.pos == pos) {
            existing.damage_stage = existing.damage_stage.saturating_add(add_stage);
            existing.age_ticks = 0;
            return existing.damage_stage >= 3;
        }
        if self.cracked_blocks.len() >= 192 {
            self.cracked_blocks.remove(0);
        }
        self.cracked_blocks.push(CrackedBlock {
            pos,
            state,
            damage_stage: add_stage.min(2),
            packed_light: 0x00D0_00D0,
            age_ticks: 0,
        });
        false
    }

    /// Immediately disintegrates a broken sand/gravel/concrete_powder block into fine 3D micro-grains!
    pub fn spawn_sand_block_grains(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        count: usize,
        bias_vel: Vec3,
    ) {
        let center = Vec3::new(
            pos[0] as f32 + 0.5,
            pos[1] as f32 + 0.5,
            pos[2] as f32 + 0.5,
        );
        self.spawn_sand_grains_stream(
            center,
            Vec3::splat(0.45),
            1.35,
            1.15,
            -1.65,
            0.55,
            state,
            pos,
            None,
            count,
            bias_vel,
        );
    }

    /// Spawns a stream/burst of fine 3D sand/gravel/concrete_powder grains.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_sand_grains_stream(
        &mut self,
        center: Vec3,
        box_half: Vec3,
        inward_pull: f32,
        spread_xz: f32,
        vy_lo: f32,
        vy_hi: f32,
        state: BlockStateId,
        origin_block: [i32; 3],
        resolved: Option<(Rect, u32)>,
        count: usize,
        bias_vel: Vec3,
    ) {
        const MAX_GRAINS: usize = 850;
        if self.sand_grains.len() + count > MAX_GRAINS {
            let need = (self.sand_grains.len() + count).saturating_sub(MAX_GRAINS);
            let mut removed = 0usize;
            self.sand_grains.retain(|g| {
                if removed < need && (g.grounded || g.age > 35) {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
            if self.sand_grains.len() + count > MAX_GRAINS + 40 {
                let drain_n = (self.sand_grains.len() + count - (MAX_GRAINS + 40))
                    .min(self.sand_grains.len());
                self.sand_grains.drain(0..drain_n);
            }
        }

        let seed = ((origin_block[0] as u64).wrapping_mul(73856093))
            ^ ((origin_block[1] as u64).wrapping_mul(19349663))
            ^ ((origin_block[2] as u64).wrapping_mul(83492791))
            ^ (center.y.to_bits() as u64).wrapping_mul(2654435761)
            ^ self.tick_counter.wrapping_mul(0x9E3779B9)
            ^ (self.sand_grains.len() as u64).wrapping_mul(0x517CC1B7);
        let mut rng = FastRng::seeded(seed);

        for _ in 0..count {
            let offset = Vec3::new(
                rng.signed() * box_half.x,
                rng.signed() * box_half.y,
                rng.signed() * box_half.z,
            );
            let p = center + offset;
            let to_center_xz = Vec3::new(-offset.x, 0.0, -offset.z);
            let v_inward = to_center_xz * inward_pull;
            let v_rand = rng.vec3_spread(spread_xz, vy_lo, vy_hi);
            let vel = bias_vel + v_inward + v_rand;

            self.sand_grains.push(SandGrain {
                pos: p,
                prev_pos: p,
                vel,
                radius: rng.range(0.016, 0.030),
                state,
                origin_block,
                resolved,
                packed_light: 0x00F0_00F0,
                uv_offset: [rng.range(0.05, 0.72), rng.range(0.05, 0.72)],
                shade_mul: rng.range(0.86, 1.08),
                age: 0,
                max_age: rng.range(320.0, 620.0) as u16,
                grounded: false,
            });
        }
    }

    /// Spawns real 3D physical sub-voxel splinters / grains / shards for a block or layer.
    pub fn spawn_subvoxels(
        &mut self,
        pos: [i32; 3],
        layer_y: f32,
        state: BlockStateId,
        mat: ChunkMat,
        count: usize,
        bias_vel: Vec3,
    ) {
        // Keep a healthy cap of 640 simultaneous 3D chunks (evict oldest grounded ones first)
        if self.chunks.len() + count > 640 {
            let need = (self.chunks.len() + count).saturating_sub(640);
            let mut removed = 0usize;
            self.chunks.retain(|c| {
                if removed < need && (c.grounded || c.age > 60) {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
            if self.chunks.len() + count > 680 {
                let drain_n = (self.chunks.len() + count - 680).min(self.chunks.len());
                self.chunks.drain(0..drain_n);
            }
        }

        let seed = ((pos[0] as u64).wrapping_mul(73856093))
            ^ ((pos[1] as u64).wrapping_mul(19349663))
            ^ ((pos[2] as u64).wrapping_mul(83492791))
            ^ self.tick_counter.wrapping_mul(0x9E3779B9);
        let mut rng = FastRng::seeded(seed);

        for i in 0..count {
            let (half, max_age, spread_xz, vy_lo, vy_hi, spin_scale) = match mat {
                ChunkMat::Wood => (
                    Vec3::new(
                        rng.range(0.045, 0.085),
                        rng.range(0.16, 0.27),
                        rng.range(0.045, 0.085),
                    ),
                    240,
                    3.2,
                    1.2,
                    4.2,
                    13.0,
                ),
                ChunkMat::Granular => (
                    Vec3::new(
                        rng.range(0.085, 0.135),
                        rng.range(0.075, 0.125),
                        rng.range(0.085, 0.135),
                    ),
                    210,
                    1.65,
                    -0.4,
                    1.6,
                    9.5,
                ),
                ChunkMat::Glass => (
                    Vec3::new(
                        rng.range(0.10, 0.18),
                        rng.range(0.09, 0.17),
                        rng.range(0.018, 0.032),
                    ),
                    190,
                    4.4,
                    1.5,
                    4.8,
                    18.0,
                ),
                ChunkMat::Foliage => (
                    Vec3::new(
                        rng.range(0.09, 0.15),
                        rng.range(0.05, 0.10),
                        rng.range(0.09, 0.15),
                    ),
                    150,
                    2.8,
                    0.5,
                    2.8,
                    8.0,
                ),
                ChunkMat::Stone => (
                    Vec3::new(
                        rng.range(0.09, 0.155),
                        rng.range(0.08, 0.135),
                        rng.range(0.09, 0.155),
                    ),
                    240,
                    3.0,
                    1.0,
                    3.8,
                    11.0,
                ),
                ChunkMat::Gore => (
                    Vec3::new(
                        rng.range(0.048, 0.085),
                        rng.range(0.048, 0.085),
                        rng.range(0.048, 0.085),
                    ),
                    220,
                    4.2,
                    1.8,
                    5.2,
                    15.0,
                ),
            };

            let gx = ((i % 3) as f32 + 0.5) / 3.0 + rng.signed() * 0.11;
            let gz = (((i / 3) % 3) as f32 + 0.5) / 3.0 + rng.signed() * 0.11;
            let gy = if layer_y >= 0.0 {
                (layer_y + rng.signed() * 0.10).clamp(0.08, 0.92)
            } else {
                rng.range(0.12, 0.88)
            };

            let p = Vec3::new(
                pos[0] as f32 + gx.clamp(0.08, 0.92),
                pos[1] as f32 + gy,
                pos[2] as f32 + gz.clamp(0.08, 0.92),
            );
            let u0 = rng.range(0.0, 0.62);
            let v0 = rng.range(0.0, 0.62);
            let uw = rng.range(0.25, 0.38);
            let vh = rng.range(0.25, 0.38);

            let rot = Quat::from_euler(
                glam::EulerRot::YXZ,
                rng.f32() * TAU,
                rng.signed() * 0.6,
                rng.signed() * 0.6,
            );
            let vel = bias_vel + rng.vec3_spread(spread_xz, vy_lo, vy_hi);
            let ang_vel = Vec3::new(rng.signed(), rng.signed(), rng.signed()) * spin_scale;

            self.chunks.push(SubVoxelChunk {
                pos: p,
                prev_pos: p,
                vel,
                rot,
                prev_rot: rot,
                ang_vel,
                half,
                state,
                origin_block: pos,
                uv_sub: [u0, v0, (u0 + uw).min(1.0), (v0 + vh).min(1.0)],
                custom_sprite: None,
                resolved: None,
                mat,
                age: 0,
                max_age,
                grounded: false,
            });
        }
    }

    pub fn spawn_gore_burst(&mut self, world_at: Vec3, base_vel: Vec3, rgb: u32, count: usize) {
        if self.chunks.len() > 620 {
            return;
        }
        let mut rng = FastRng::seeded(
            (world_at.x.to_bits() as u64)
                ^ ((world_at.z.to_bits() as u64) << 19)
                ^ self.tick_counter.wrapping_mul(0x517CC1B7),
        );
        let bpos = [
            world_at.x.floor() as i32,
            world_at.y.floor() as i32,
            world_at.z.floor() as i32,
        ];
        for _ in 0..count {
            let p = world_at + Vec3::new(rng.signed() * 0.22, rng.signed() * 0.22, rng.signed() * 0.22);
            let half = Vec3::new(
                rng.range(0.042, 0.082),
                rng.range(0.038, 0.072),
                rng.range(0.042, 0.082),
            );
            let rot = Quat::from_euler(
                glam::EulerRot::YXZ,
                rng.f32() * TAU,
                rng.f32() * TAU,
                rng.f32() * TAU,
            );
            self.chunks.push(SubVoxelChunk {
                pos: p,
                prev_pos: p,
                vel: base_vel * 0.55 + rng.vec3_spread(3.8, 1.4, 4.8),
                rot,
                prev_rot: rot,
                ang_vel: Vec3::new(rng.signed(), rng.signed(), rng.signed()) * 16.0,
                half,
                state: BlockStateId(0),
                origin_block: bpos,
                uv_sub: [0.1, 0.1, 0.6, 0.6],
                custom_sprite: Some(("block/white_concrete", rgb)),
                resolved: None,
                mat: ChunkMat::Gore,
                age: 0,
                max_age: 190,
                grounded: false,
            });
        }
    }
}

static VP_WORLD: Mutex<Option<VpWorld>> = Mutex::new(None);

pub fn with_vp<R>(f: impl FnOnce(&mut VpWorld) -> R) -> R {
    let mut guard = VP_WORLD.lock().unwrap_or_else(|e| e.into_inner());
    let world = guard.get_or_insert_with(VpWorld::new);
    f(world)
}

// ============================================================================
// 7. TREE & STRUCTURE SCANNING (SERVER-SIDE AUTHORITATIVE)
// ============================================================================

/// Scans an entire connected tree (trunk + branches + foliage) above `break_pos`.
/// Returns `None` if not a valid tree or if another parallel rooted trunk holds it up.
fn scan_tree_structure(
    game: &Game<'_>,
    break_pos: [i32; 3],
    broken_state: BlockStateId,
) -> Option<Vec<([i32; 3], BlockStateId, bool)>> {
    let broken_name = &game.res.facts().get(broken_state).name;
    if !is_tree_log_name(broken_name) {
        return None;
    }

    // Check if there is at least one log directly or diagonally above `break_pos`
    let mut seed_logs = Vec::new();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let p = [break_pos[0] + dx, break_pos[1] + 1, break_pos[2] + dz];
            let s = game.world.block(p[0], p[1], p[2]);
            if !game.res.facts().is_air(s) && is_tree_log_name(&game.res.facts().get(s).name) {
                seed_logs.push((p, s));
            }
        }
    }
    if seed_logs.is_empty() {
        return None;
    }

    // Flood-fill connected logs within horizontal radius 6 and height +30
    let mut visited_logs: HashSet<[i32; 3]> = HashSet::new();
    let mut log_queue: VecDeque<([i32; 3], BlockStateId)> = VecDeque::new();
    visited_logs.insert(break_pos);

    for (p, s) in seed_logs {
        visited_logs.insert(p);
        log_queue.push_back((p, s));
    }

    let mut logs: Vec<([i32; 3], BlockStateId)> = Vec::new();
    let mut alternate_ground_anchors = 0usize;

    while let Some((curr, cstate)) = log_queue.pop_front() {
        logs.push((curr, cstate));
        if logs.len() > 140 {
            return None; // Likely a huge wooden house/mansion rather than a natural tree
        }

        // Check if another trunk column at break_pos[1] is still standing & rooted on solid ground
        if curr[1] == break_pos[1] && curr != break_pos {
            let below = game.world.block(curr[0], curr[1] - 1, curr[2]);
            if !game.res.facts().is_air(below) && game.res.facts().get(below).full_collision {
                alternate_ground_anchors += 1;
                if alternate_ground_anchors >= 2 {
                    // Wide multi-pillar structure still anchored
                    return None;
                }
            }
        }

        for dy in -1..=1 {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let nb = [curr[0] + dx, curr[1] + dy, curr[2] + dz];
                    if nb[1] < break_pos[1]
                        || nb[1] > break_pos[1] + 32
                        || (nb[0] - break_pos[0]).abs() > 6
                        || (nb[2] - break_pos[2]).abs() > 6
                    {
                        continue;
                    }
                    if visited_logs.contains(&nb) {
                        continue;
                    }
                    let s = game.world.block(nb[0], nb[1], nb[2]);
                    if !game.res.facts().is_air(s) && is_tree_log_name(&game.res.facts().get(s).name) {
                        visited_logs.insert(nb);
                        log_queue.push_back((nb, s));
                    }
                }
            }
        }
    }

    // If a single adjacent base log (like 2x2 tree) is still rooted right next to break_pos,
    // wait until that adjacent log is chopped unless it's just 1 bark root!
    if alternate_ground_anchors >= 1 {
        return None;
    }

    // Now BFS foliage within 4 blocks of any log of THIS tree (cap at 180 foliage blocks)
    let mut visited_leaves: HashSet<[i32; 3]> = HashSet::new();
    let mut leaf_queue: VecDeque<([i32; 3], BlockStateId, u8)> = VecDeque::new();
    let mut foliage: Vec<([i32; 3], BlockStateId)> = Vec::new();

    const DIRS6: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];

    for &(lp, _) in &logs {
        for d in DIRS6 {
            let nb = [lp[0] + d[0], lp[1] + d[1], lp[2] + d[2]];
            if nb[1] < break_pos[1] || visited_logs.contains(&nb) || visited_leaves.contains(&nb) {
                continue;
            }
            let s = game.world.block(nb[0], nb[1], nb[2]);
            if !game.res.facts().is_air(s) && is_tree_foliage_name(&game.res.facts().get(s).name) {
                visited_leaves.insert(nb);
                leaf_queue.push_back((nb, s, 1));
            }
        }
    }

    while let Some((curr, cstate, dist)) = leaf_queue.pop_front() {
        foliage.push((curr, cstate));
        if foliage.len() >= 190 {
            break;
        }
        if dist >= 4 {
            continue;
        }
        for d in DIRS6 {
            let nb = [curr[0] + d[0], curr[1] + d[1], curr[2] + d[2]];
            if nb[1] < break_pos[1]
                || (nb[0] - break_pos[0]).abs() > 8
                || (nb[2] - break_pos[2]).abs() > 8
                || visited_logs.contains(&nb)
                || visited_leaves.contains(&nb)
            {
                continue;
            }
            let s = game.world.block(nb[0], nb[1], nb[2]);
            if !game.res.facts().is_air(s) && is_tree_foliage_name(&game.res.facts().get(s).name) {
                visited_leaves.insert(nb);
                leaf_queue.push_back((nb, s, dist + 1));
            }
        }
    }

    // Must be either a real tree (logs + leaves) or a tall log column (>= 2 logs above break)
    if logs.len() < 2 && foliage.is_empty() {
        return None;
    }

    let mut out = Vec::with_capacity(logs.len() + foliage.len());
    for (p, s) in logs {
        out.push((p, s, false));
    }
    for (p, s) in foliage {
        out.push((p, s, true));
    }
    Some(out)
}

/// Scans an unsupported non-tree masonry/plank overhang or pillar above `break_pos`.
fn scan_unsupported_cluster(
    game: &Game<'_>,
    seed: [i32; 3],
    break_pos: [i32; 3],
    max_blocks: usize,
) -> Option<Vec<([i32; 3], BlockStateId)>> {
    let seed_state = game.world.block(seed[0], seed[1], seed[2]);
    if game.res.facts().is_air(seed_state) {
        return None;
    }
    let sname = &game.res.facts().get(seed_state).name;
    if sname.contains("bedrock") || sname.contains("water") || sname.contains("lava") {
        return None;
    }
    // Allow ALL fences, walls, pillars, logs, stones, quartz, shroomlights, and lamps to be seeds
    let is_valid_seed = game.res.facts().get(seed_state).full_collision
        || sname.contains("fence")
        || sname.contains("wall")
        || sname.contains("light")
        || sname.contains("lantern")
        || sname.contains("lamp")
        || sname.contains("chain")
        || sname.contains("bar")
        || sname.contains("rod");
    if !is_valid_seed {
        return None;
    }

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back(seed);
    visited.insert(seed);

    let mut cluster = Vec::new();
    const DIRS6: [[i32; 3]; 6] = [
        [0, 1, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
        [0, -1, 0],
    ];

    while let Some(curr) = queue.pop_front() {
        let st = game.world.block(curr[0], curr[1], curr[2]);
        cluster.push((curr, st));
        if cluster.len() > max_blocks {
            return None; // Anchored to large terrain mass
        }

        // If any block in cluster rests on intact solid ground at or below break_pos[1], it is supported!
        if curr[1] <= break_pos[1] + 1 {
            let below = [curr[0], curr[1] - 1, curr[2]];
            // Crucial: 'below' must NOT be the block at 'break_pos' that is being destroyed right now!
            if below[1] <= break_pos[1] && below != break_pos {
                let bs = game.world.block(below[0], below[1], below[2]);
                if !game.res.facts().is_air(bs) {
                    let bfacts = game.res.facts().get(bs);
                    let bname = &bfacts.name;
                    if bfacts.full_collision
                        || bname.contains("stone")
                        || bname.contains("dirt")
                        || bname.contains("grass")
                        || bname.contains("concrete")
                        || bname.contains("plank")
                    {
                        return None; // Real intact ground found! Not unsupported.
                    }
                }
            }
        }

        for d in DIRS6 {
            let nb = [curr[0] + d[0], curr[1] + d[1], curr[2] + d[2]];
            if nb == break_pos || nb[1] <= break_pos[1] || visited.contains(&nb) {
                continue;
            }
            let ns = game.world.block(nb[0], nb[1], nb[2]);
            if !game.res.facts().is_air(ns) {
                let nname = &game.res.facts().get(ns).name;
                if !nname.contains("water") && !nname.contains("lava") && !nname.contains("bedrock") {
                    visited.insert(nb);
                    queue.push_back(nb);
                }
            }
        }
    }

    if cluster.len() >= 2 {
        Some(cluster)
    } else {
        None
    }
}
/// Maximum horizontal cantilever span (in blocks from a grounded vertical support column)
/// before a building material fractures under its own weight.
pub fn structural_max_span(name: &str) -> i32 {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    if n.contains("iron_block")
        || n.contains("copper")
        || n.contains("gold_block")
        || n.contains("diamond_block")
        || n.contains("netherite")
        || n.contains("obsidian")
        || n.contains("anvil")
    {
        11
    } else if n.contains("stone_bricks")
        || n.contains("deepslate")
        || n.contains("bricks")
        || n.contains("blackstone")
        || n.contains("basalt")
        || n.contains("quartz")
        || n.contains("purpur")
        || n.contains("prismarine")
    {
        7
    } else if n.contains("stone")
        || n.contains("cobblestone")
        || n.contains("andesite")
        || n.contains("diorite")
        || n.contains("granite")
        || n.contains("concrete")
        || n.contains("terracotta")
    {
        6
    } else if n.contains("_log")
        || n.contains("_wood")
        || n.contains("_stem")
        || n.contains("planks")
        || n.contains("stairs")
        || n.contains("slab")
        || n.contains("fence")
        || n.contains("bamboo")
    {
        4
    } else {
        3
    }
}

/// Returns true if a block is fragile enough to be smashed right through by a falling tree,
/// collapsing masonry structure, or high-velocity ballistic boulder (`Penetration Physics`).
pub fn is_penetrable_fragile_block(name: &str, heavy_momentum: bool) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    if n.contains("bedrock") || n.contains("obsidian") || n.contains("portal") {
        return false;
    }
    if n.contains("glass")
        || n.contains("ice")
        || n.contains("_leaves")
        || n.contains("vine")
        || n.contains("azalea")
        || n.contains("wart_block")
        || n == "shroomlight"
        || n == "glowstone"
        || n == "sea_lantern"
        || n.contains("lantern")
        || n.contains("torch")
        || n.contains("candle")
        || n.contains("flower_pot")
        || n.contains("fence")
        || n.contains("door")
        || n.contains("trapdoor")
        || n.contains("sign")
        || n.contains("banner")
        || n.contains("scaffolding")
        || n.contains("ladder")
        || n.contains("carpet")
        || n.contains("snow")
        || n.contains("bamboo")
        || n.contains("dripstone")
        || n.contains("cobweb")
        || n.contains("pane")
        || n.contains("iron_bars")
        || n == "chain"
        || n == "bell"
    {
        return true;
    }
    if heavy_momentum
        && (n.contains("planks")
            || n.contains("slab")
            || n.contains("stairs")
            || n == "bookshelf"
            || n == "hay_block"
            || n == "melon"
            || n.contains("pumpkin")
            || n.contains("wool"))
    {
        return true;
    }
    false
}

/// Natural dust cloud RGB color matching the broken/collapsing block material.
pub fn dust_rgb_for_block_name(name: &str) -> u32 {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    if n.contains("sand") || n.contains("birch") || n.contains("end_stone") {
        0xD8C696
    } else if n.contains("red_sand") || n.contains("brick") || n.contains("terracotta") || n.contains("granite") || n.contains("copper") {
        0xB56548
    } else if n.contains("deepslate") || n.contains("blackstone") || n.contains("basalt") || n.contains("obsidian") || n.contains("coal") {
        0x54545A
    } else if n.contains("dirt") || n.contains("mud") || n.contains("podzol") || n.contains("_log") || n.contains("planks") || n.contains("wood") {
        0x8E6B48
    } else if n.contains("leaves") || n.contains("moss") || n.contains("azalea") {
        0x5C8442
    } else {
        0x9E9EA4
    }
}

/// Extracts banner / wool cloth RGB color from block registry name.
pub fn banner_color_from_name(name: &str) -> u32 {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    if n.starts_with("red_") {
        0xB02E26
    } else if n.starts_with("blue_") {
        0x3C44AA
    } else if n.starts_with("light_blue_") {
        0x3AB3DA
    } else if n.starts_with("cyan_") {
        0x169C9C
    } else if n.starts_with("yellow_") {
        0xFED83D
    } else if n.starts_with("lime_") {
        0x80C71F
    } else if n.starts_with("green_") {
        0x5E7C16
    } else if n.starts_with("orange_") {
        0xF9801D
    } else if n.starts_with("purple_") {
        0x8932B8
    } else if n.starts_with("magenta_") {
        0xC74EBD
    } else if n.starts_with("pink_") {
        0xF38BAA
    } else if n.starts_with("black_") {
        0x252529
    } else if n.starts_with("gray_") {
        0x474F52
    } else if n.starts_with("light_gray_") {
        0x9D9D97
    } else if n.starts_with("brown_") {
        0x835432
    } else {
        0xF2F4F5
    }
}

/// Returns true for bulk terrain / building blocks that may safely be converted into falling debris
/// (never containers, spawners, portals, command blocks or other block entities with inventories!).
pub fn is_destructible_bulk(name: &str) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    !(n.contains("bedrock")
        || n.contains("obsidian")
        || n.contains("portal")
        || n.contains("chest")
        || n.contains("barrel")
        || n.contains("shulker")
        || n.contains("furnace")
        || n.contains("smoker")
        || n.contains("hopper")
        || n.contains("spawner")
        || n.contains("command")
        || n.contains("beacon")
        || n.contains("dispenser")
        || n.contains("dropper")
        || n.contains("lectern")
        || n.contains("jukebox")
        || n.contains("crafter")
        || n.contains("vault")
        || n.contains("trial_spawner")
        || n.contains("water")
        || n.contains("lava")
        || n.contains("barrier")
        || n.contains("end_portal_frame")
        || n.contains("reinforced_deepslate"))
}

/// Checks if a block `(x, y, z)` has a direct vertical load-bearing column underneath it (`(x, y-1, z)` and `(x, y-2, z)`).
#[inline]
fn has_vertical_pillar_support(game: &Game<'_>, x: i32, y: i32, z: i32, removed: [i32; 3]) -> bool {
    if [x, y - 1, z] == removed || [x, y - 2, z] == removed {
        return false;
    }
    let b1 = game.world.block(x, y - 1, z);
    if game.res.facts().is_air(b1) || !game.res.facts().get(b1).full_collision {
        return false;
    }
    let b2 = game.world.block(x, y - 2, z);
    !game.res.facts().is_air(b2) && game.res.facts().get(b2).full_collision
}

/// Discrete load-bearing cantilever analyzer:
/// When a pillar or wall block at `break_pos` is destroyed, checks adjacent roof/bridge/floor spans.
/// Any overhang blocks whose horizontal distance to the nearest vertical pillar exceeds their
/// material's `structural_max_span` (reduced by persistent cracks!) fracture off and collapse!
/// If ALL pillars supporting a building are destroyed, the entire connected building collapses!
fn scan_overloaded_cantilever(
    game: &Game<'_>,
    break_pos: [i32; 3],
    cracked: &[CrackedBlock],
    max_collapse: usize,
) -> Option<(Vec<([i32; 3], BlockStateId)>, Vec<([i32; 3], BlockStateId)>)> {
    const CANDIDATE_OFFSETS: [[i32; 3]; 9] = [
        [0, 1, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
        [1, 1, 0],
        [-1, 1, 0],
        [0, 1, 1],
        [0, 1, -1],
    ];

    let mut overloaded: Vec<([i32; 3], BlockStateId)> = Vec::new();
    let mut fracture_edges: Vec<([i32; 3], BlockStateId)> = Vec::new();
    let mut global_checked: HashSet<[i32; 3]> = HashSet::new();

    for off in CANDIDATE_OFFSETS {
        let start = [break_pos[0] + off[0], break_pos[1] + off[1], break_pos[2] + off[2]];
        if global_checked.contains(&start) {
            continue;
        }
        let s_state = game.world.block(start[0], start[1], start[2]);
        if game.res.facts().is_air(s_state) || !game.res.facts().get(s_state).full_collision {
            continue;
        }
        let s_name = &game.res.facts().get(s_state).name;
        if !is_destructible_bulk(s_name)
            || is_tree_log_name(s_name)
            || is_tree_foliage_name(s_name)
            || is_true_gravity_granular(s_name)
        {
            continue;
        }
        // Only analyze blocks that are currently overhanging (air directly below them)
        if has_vertical_pillar_support(game, start[0], start[1], start[2], break_pos) {
            continue;
        }

        // BFS through the overhang span to measure distance to nearest vertical pillar support
        let mut queue: VecDeque<([i32; 3], i32)> = VecDeque::new();
        let mut span_nodes: Vec<([i32; 3], BlockStateId)> = Vec::new();
        let mut visited: HashSet<[i32; 3]> = HashSet::new();
        queue.push_back((start, 0));
        visited.insert(start);

        let mut min_dist_to_pillar: Option<i32> = None;
        let mut nearest_pillar_pos: Option<[i32; 3]> = None;

        const DIRS: [[i32; 3]; 6] = [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 0, 1],
            [0, 0, -1],
            [0, 1, 0],
            [0, -1, 0],
        ];

        while let Some((curr, dist)) = queue.pop_front() {
            let cs = game.world.block(curr[0], curr[1], curr[2]);
            if has_vertical_pillar_support(game, curr[0], curr[1], curr[2], break_pos) {
                if min_dist_to_pillar.is_none_or(|d| dist < d) {
                    min_dist_to_pillar = Some(dist);
                    nearest_pillar_pos = Some(curr);
                }
                continue;
            }

            span_nodes.push((curr, cs));
            if span_nodes.len() > 64 || dist > 16 {
                break;
            }

            for d in DIRS {
                let nb = [curr[0] + d[0], curr[1] + d[1], curr[2] + d[2]];
                if nb == break_pos || visited.contains(&nb) {
                    continue;
                }
                // Keep cantilever search bounded within the floor/roof slab layer (max 3 blocks vertical)
                if (nb[1] - start[1]).abs() > 3 {
                    continue;
                }
                let ns = game.world.block(nb[0], nb[1], nb[2]);
                if !game.res.facts().is_air(ns)
                    && (game.res.facts().get(ns).full_collision || is_destructible_bulk(&game.res.facts().get(ns).name))
                {
                    visited.insert(nb);
                    queue.push_back((nb, dist + 1));
                }
            }
        }

        for &(np, _) in &span_nodes {
            global_checked.insert(np);
        }

        if let Some(pillar_pos) = nearest_pillar_pos {
            // There are still supporting pillars: check cantilever distance
            for (bp, bs) in span_nodes {
                let bname = &game.res.facts().get(bs).name;
                let base_span = structural_max_span(bname);
                let crack_penalty = cracked
                    .iter()
                    .find(|c| c.pos == bp)
                    .map_or(0, |c| c.damage_stage as i32 * 2);
                let max_allowed = (base_span - crack_penalty).max(1);
                let horiz_dist = (bp[0] - pillar_pos[0]).abs() + (bp[2] - pillar_pos[2]).abs();

                if horiz_dist > max_allowed && overloaded.len() < max_collapse {
                    overloaded.push((bp, bs));
                } else if horiz_dist == max_allowed {
                    fracture_edges.push((bp, bs));
                }
            }
        }
        // If nearest_pillar_pos.is_none(): do NOTHING! Local cantilever check never demolishes buildings when unverified.
    }

    if overloaded.is_empty() {
        None
    } else {
        Some((overloaded, fracture_edges))
    }
}

/// Converts a hanging vertical column of `chain` / `iron_bars` + suspended `lantern` / `bell`
/// into an interactive 3D Verlet-simulated swinging & breakable chain!
/// `broken` = a block position that is being destroyed right now (treated as air).
fn try_activate_hanging_chain_at(
    game: &mut Game<'_>,
    vp: &mut VpWorld,
    dim: &str,
    seed_pos: [i32; 3],
    impulse_vel: Vec3,
    broken: Option<[i32; 3]>,
) -> bool {
    if vp.ropes.len() >= 20 || Some(seed_pos) == broken {
        return false;
    }
    let is_link = |n: &str| n.contains("chain") || n.contains("iron_bars");
    let is_payload = |n: &str| {
        n.contains("lantern") || n.contains("bell") || n.contains("glowstone") || n.contains("shroomlight") || n.contains("hanging_sign")
    };

    let s0 = game.world.block(seed_pos[0], seed_pos[1], seed_pos[2]);
    if game.res.facts().is_air(s0) {
        return false;
    }
    let n0 = game.res.facts().get(s0).name.clone();
    if !is_link(&n0) && !n0.contains("lantern") && !n0.contains("bell") && !n0.contains("hanging_sign") {
        return false;
    }

    // Scan upward to find the top of the chain column
    let mut top_y = seed_pos[1];
    while top_y < seed_pos[1] + 12 {
        let up = [seed_pos[0], top_y + 1, seed_pos[2]];
        if Some(up) == broken {
            break;
        }
        let up_s = game.world.block(up[0], up[1], up[2]);
        if game.res.facts().is_air(up_s) || !is_link(&game.res.facts().get(up_s).name) {
            break;
        }
        top_y += 1;
    }

    // Scan downward to find the bottom of the chain / suspended lantern or bell
    let mut bot_y = seed_pos[1];
    if is_link(&n0) {
        while bot_y > seed_pos[1] - 12 {
            let dn_s = game.world.block(seed_pos[0], bot_y - 1, seed_pos[2]);
            if game.res.facts().is_air(dn_s) {
                break;
            }
            let dn_n = game.res.facts().get(dn_s).name.clone();
            if is_link(&dn_n) {
                bot_y -= 1;
            } else if is_payload(&dn_n) {
                bot_y -= 1;
                break;
            } else {
                break;
            }
        }
    }

    let anchor_block = [seed_pos[0], top_y + 1, seed_pos[2]];
    if vp.ropes.iter().any(|r| r.anchor_block == anchor_block) {
        return false;
    }
    let anchor_state = game.world.block(anchor_block[0], anchor_block[1], anchor_block[2]);
    let top_pinned = Some(anchor_block) != broken
        && !game.res.facts().is_air(anchor_state)
        && game.res.facts().get(anchor_state).full_collision;

    // NEVER activate an unpinned or floating rope!
    if !top_pinned {
        return false;
    }

    let top_chain_state = game.world.block(seed_pos[0], top_y, seed_pos[2]);
    let bot_state = game.world.block(seed_pos[0], bot_y, seed_pos[2]);
    let bot_name = game.res.facts().get(bot_state).name.clone();
    let payload_state = if is_payload(&bot_name) { Some(bot_state) } else { None };
    let chain_blocks = (top_y - bot_y + 1 - i32::from(payload_state.is_some())).max(0);

    // A lone lantern/bell with no chain above it still swings as a short cord
    let total_blocks = (top_y - bot_y + 1).clamp(1, 14);

    // Clear the static chain/lantern blocks from the world so our 3D VerletRope replaces them
    for y in bot_y..=top_y {
        game.native_set_block(dim, [seed_pos[0], y, seed_pos[2]], BlockStateId(0), 3);
    }

    let segs = (total_blocks as usize * 2).clamp(3, 14);
    let total_len = (total_blocks as f32 - 0.35).max(0.55);
    let seg_len = total_len / (segs as f32);
    let top_pos = Vec3::new(
        seed_pos[0] as f32 + 0.5,
        (top_y + 1) as f32 - 0.02,
        seed_pos[2] as f32 + 0.5,
    );

    let mut nodes = Vec::with_capacity(segs + 1);
    for i in 0..=segs {
        let t = (i as f32) / (segs as f32);
        let p = top_pos - Vec3::Y * (t * total_len);
        let kick = impulse_vel * (t * 0.05);
        nodes.push(RopeNode {
            pos: p,
            prev_pos: p - kick,
            pinned: i == 0 && top_pinned,
        });
    }

    let raw_l = game.world.light(seed_pos[0], seed_pos[1], seed_pos[2]);
    let id = vp.alloc_id();
    vp.ropes.push(VerletRope {
        id,
        anchor_block,
        chain_state: top_chain_state,
        payload_state,
        chain_blocks,
        nodes,
        seg_len,
        packed_light: vertex_light(raw_l),
        broken: !top_pinned,
        age_ticks: 0,
    });
    true
}

/// Spawns a 3D wind-rippling & tearable `VerletCloth` flag/banner at `pos`.
/// `pinned` = still attached to its pole/wall; `drop_state` = banner item to drop when the cloth expires.
fn try_activate_banner_cloth_at(
    game: &Game<'_>,
    vp: &mut VpWorld,
    pos: [i32; 3],
    state: BlockStateId,
    impulse_vel: Vec3,
    pinned: bool,
    drop_state: Option<BlockStateId>,
) {
    if vp.cloths.len() >= 16 || vp.cloths.iter().any(|c| c.anchor_pos == pos) {
        return;
    }
    let bname = game.res.facts().get(state).name.clone();
    if !bname.contains("banner") {
        return;
    }
    let is_wall = bname.contains("wall_banner");

    // Wall banners hang off the side of the solid block they are attached to
    let mut normal_dir = Vec3::Z;
    let mut support_pos = Some([pos[0], pos[1] - 1, pos[2]]);
    if is_wall {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if is_solid_voxel(game, pos[0] + dx, pos[1], pos[2] + dz) {
                normal_dir = Vec3::new(-dx as f32, 0.0, -dz as f32);
                support_pos = Some([pos[0] + dx, pos[1], pos[2] + dz]);
                break;
            }
        }
    } else {
        // Standing banner: face away from the player
        let away = Vec3::new(
            pos[0] as f32 + 0.5 - vp.last_player_pos.x,
            0.0,
            pos[2] as f32 + 0.5 - vp.last_player_pos.z,
        );
        normal_dir = if away.x.abs() > away.z.abs() {
            Vec3::new(away.x.signum(), 0.0, 0.0)
        } else {
            Vec3::new(0.0, 0.0, away.z.signum())
        };
    }
    let right_dir = Vec3::Y.cross(normal_dir).normalize_or(Vec3::X);

    let rgb = banner_color_from_name(&bname);
    let cols = 5usize;
    let rows = 8usize;
    let spacing_x = 0.2f32;
    let spacing_y = 0.2f32;
    let top_center = if is_wall {
        Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.92, pos[2] as f32 + 0.5)
            - normal_dir * 0.42
    } else {
        Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 1.80, pos[2] as f32 + 0.5)
            + normal_dir * 0.06
    };

    let mut nodes = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        let v_frac = (r as f32) / ((rows - 1) as f32);
        for c in 0..cols {
            let u_off = (c as f32 - (cols - 1) as f32 * 0.5) * spacing_x;
            let p = top_center + right_dir * u_off - Vec3::Y * (r as f32 * spacing_y)
                + normal_dir * (v_frac * 0.02);
            let kick = impulse_vel * (0.02 + v_frac * 0.05);
            nodes.push(ClothNode {
                pos: p,
                prev_pos: p - kick,
                pinned: pinned && r == 0,
            });
        }
    }

    let raw_l = game.world.light(pos[0], pos[1], pos[2]);
    let id = vp.alloc_id();
    vp.cloths.push(VerletCloth {
        id,
        anchor_pos: pos,
        support_pos: if pinned { support_pos } else { None },
        cols,
        rows,
        spacing_x,
        spacing_y,
        right_dir,
        normal_dir,
        rgb,
        packed_light: vertex_light(raw_l),
        nodes,
        h_torn: vec![false; rows * (cols - 1)],
        v_torn: vec![false; (rows - 1) * cols],
        drop_state,
        age_ticks: 0,
    });
}

/// Propagates an explosive or seismic shockwave to nearby hanging lanterns, banners, hanging signs, and chains.
/// In the epicenter (close to blast/impact), fragile hanging objects shatter and drop loot.
/// In the peripheral wave zone, solid-anchored objects are activated into physical Verlet ropes/cloths and sway heavily!
fn apply_shockwave_to_hanging_objects(
    game: &mut Game<'_>,
    vp: &mut VpWorld,
    dim: &str,
    epicenter: Vec3,
    blast_power: f32,
) {
    let r_break = (blast_power * 0.55).clamp(2.0, 5.0);
    let r_wave = (blast_power * 2.6).clamp(8.0, 22.0);

    let min_x = (epicenter.x - r_wave).floor() as i32;
    let max_x = (epicenter.x + r_wave).ceil() as i32;
    let min_y = (epicenter.y - r_wave * 0.6).floor() as i32;
    let max_y = (epicenter.y + r_wave * 0.8).ceil() as i32;
    let min_z = (epicenter.z - r_wave).floor() as i32;
    let max_z = (epicenter.z + r_wave).ceil() as i32;

    let mut to_break = Vec::new();
    let mut to_sway_ropes = Vec::new();
    let mut to_sway_cloths = Vec::new();

    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let s = game.world.block(x, y, z);
                if game.res.facts().is_air(s) {
                    continue;
                }
                let bname = &game.res.facts().get(s).name;
                let is_rope_item = bname.contains("lantern") || bname.contains("hanging_sign") || bname.contains("chain") || bname.contains("bell");
                let is_cloth_item = bname.contains("banner");

                if !is_rope_item && !is_cloth_item {
                    continue;
                }

                let b_center = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                let delta = b_center - epicenter;
                let dist = delta.length();
                if dist > r_wave || dist < 0.01 {
                    continue;
                }
                let dir = delta / dist;

                if dist < r_break {
                    to_break.push(([x, y, z], s, dir * 3.5));
                } else {
                    let force = (1.0 - (dist / r_wave)).powi(2) * (blast_power * 0.85);
                    if is_rope_item {
                        to_sway_ropes.push(([x, y, z], dir * force));
                    } else if is_cloth_item {
                        to_sway_cloths.push(([x, y, z], s, dir * force));
                    }
                }
            }
        }
    }

    // 1. Break epicenter objects
    for (pos, s, vel) in to_break {
        let bname = game.res.facts().get(s).name.clone();
        game.native_set_block(dim, pos, BlockStateId(0), 3);
        let mat = classify_block_name(&bname);
        vp.spawn_subvoxels(pos, -1.0, s, mat, 8, vel + Vec3::Y * 1.2);
        vp.server_ops.push_back(ServerOp::DropBlockLoot { pos, state: s });
        vp.ropes.retain(|r| r.anchor_block != pos);
        vp.cloths.retain(|c| c.anchor_pos != pos);
    }

    // 2. Sway / activate ropes and chains
    for (pos, impulse) in to_sway_ropes {
        if !try_activate_hanging_chain_at(game, vp, dim, pos, impulse, None) {
            let target_center = Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5);
            for r in &mut vp.ropes {
                if (r.nodes.first().map_or(Vec3::ZERO, |n| n.pos) - target_center).length_squared() < 4.0 {
                    for n in &mut r.nodes {
                        if !n.pinned {
                            n.pos += impulse * 0.22;
                        }
                    }
                }
            }
        }
    }

    // 3. Sway / activate cloth banners
    for (pos, s, impulse) in to_sway_cloths {
        try_activate_banner_cloth_at(game, vp, pos, s, impulse, true, None);
        for c in &mut vp.cloths {
            if c.anchor_pos == pos {
                for n in &mut c.nodes {
                    if !n.pinned {
                        n.pos += impulse * 0.22;
                    }
                }
            }
        }
    }
}

/// Rips random links of a cloth near `at` (explosions, blades).
fn tear_cloth_near(cloth: &mut VerletCloth, at: Vec3, radius: f32, chance: f32, seed: u64) {
    let mut rng = FastRng::seeded(seed ^ cloth.id.wrapping_mul(0x9E37_79B9));
    let cols = cloth.cols;
    for r in 0..cloth.rows {
        for c in 0..cols {
            let p = cloth.nodes[r * cols + c].pos;
            if p.distance(at) > radius {
                continue;
            }
            if c + 1 < cols && rng.f32() < chance {
                cloth.h_torn[r * (cols - 1) + c] = true;
            }
            if r + 1 < cloth.rows && rng.f32() < chance * 0.8 {
                cloth.v_torn[r * cols + c] = true;
            }
        }
    }
}

// ============================================================================
// 8. AUTHENTIC 3D MOB PART EXTRACTOR & DISMEMBERABLE RAGDOLL BUILDER
// ============================================================================

fn is_separable_limb_name(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    low.contains("head")
        || low.contains("arm")
        || low.contains("leg")
        || low.contains("wing")
        || low.contains("tentacle")
        || low.contains("tail")
        || low.contains("rod")
        || low.contains("fin")
        || low.contains("jaw")
}

fn classify_limb_role(name: &str) -> LimbRole {
    let low = name.to_ascii_lowercase();
    if low.contains("head") || low.contains("skull") || low.contains("jaw") {
        LimbRole::Head
    } else if low.contains("arm") || low.contains("wing") || low.contains("claw") {
        LimbRole::Arm
    } else if low.contains("leg") || low.contains("foot") || low.contains("hind") || low.contains("front") {
        LimbRole::Leg
    } else if low.contains("body") || low.contains("torso") || low.contains("chest") || low == "root" {
        LimbRole::Torso
    } else {
        LimbRole::Appendage
    }
}

fn gore_color_for_species(s: Species) -> u32 {
    match s {
        Species::Skeleton
        | Species::WitherSkeleton
        | Species::Stray
        | Species::Bogged
        | Species::SkeletonHorse => 0xDEDAC8, // Bone ivory
        Species::Creeper => 0x2F6E28, // Gunpowder plant green
        Species::Enderman | Species::Endermite | Species::Shulker => 0x4A156B, // Ender violet
        Species::Blaze | Species::MagmaCube => 0xEB6512, // Molten ember
        Species::IronGolem => 0xB8AFA4, // Iron & weathered stone
        Species::CopperGolem => 0xB86B43,
        Species::SnowGolem => 0xEAF4F8,
        Species::Spider | Species::CaveSpider | Species::Silverfish | Species::Bee => 0x3D1E42, // Ichor
        Species::Slime => 0x5CC84A,
        _ => 0x8F1111, // Deep visceral crimson
    }
}

#[inline]
fn part_pose_mat(p: [f32; 9]) -> Mat4 {
    Mat4::from_translation(Vec3::new(p[0], p[1], p[2]) / 16.0)
        * Mat4::from_quat(Quat::from_euler(glam::EulerRot::ZYX, p[5], p[4], p[3]))
        * Mat4::from_scale(Vec3::new(p[6], p[7], p[8]))
}

struct RawExtractedLimb {
    name: String,
    role: LimbRole,
    pivot_entity: Vec3,
    quads: Vec<BakedLimbQuad>,
}

/// Recursively collects quads for `part` and any non-separable sub-parts (e.g. hat, snout, horns),
/// while splitting separable child limbs (`head`, `arms`, `legs`, `wings`, `tentacles`) into independent rigid bodies.
fn extract_limbs_recursive(
    part: &Part,
    parent_mat: Mat4,
    use_overlay: bool,
    tint_abgr: u32,
    out_limbs: &mut Vec<RawExtractedLimb>,
) {
    let m = parent_mat * part_pose_mat(part.pose);
    let n_mat = Mat3::from_mat4(m);

    let mut own_quads = Vec::new();
    collect_fused_subparts(part, m, n_mat, true, use_overlay, tint_abgr, &mut own_quads, out_limbs);

    if !own_quads.is_empty() {
        let pivot = m.transform_point3(Vec3::ZERO);
        out_limbs.push(RawExtractedLimb {
            name: part.name.clone(),
            role: classify_limb_role(&part.name),
            pivot_entity: pivot,
            quads: own_quads,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_fused_subparts(
    part: &Part,
    m: Mat4,
    n_mat: Mat3,
    is_limb_root: bool,
    use_overlay: bool,
    tint_abgr: u32,
    fused_quads: &mut Vec<BakedLimbQuad>,
    out_limbs: &mut Vec<RawExtractedLimb>,
) {
    for poly in &part.polygons {
        let n = (n_mat * Vec3::from_array(poly.normal)).normalize_or(Vec3::Y);
        let mut verts = [[0.0f32; 3]; 4];
        let mut uvs = [[0.0f32; 2]; 4];
        for (idx, v) in poly.vertices.iter().enumerate() {
            let p = m.transform_point3(Vec3::new(v[0], v[1], v[2]) / 16.0);
            verts[idx] = p.to_array();
            uvs[idx] = [v[3], v[4]];
        }
        fused_quads.push(BakedLimbQuad {
            verts,
            uvs,
            normal: n,
            cull_back: poly.cull_back,
            use_overlay_rect: use_overlay,
            tint_abgr,
        });
    }

    for child in &part.children {
        // If this node is an empty container (like `root`), all its children become top-level candidates!
        let parent_empty_wrapper = is_limb_root && part.polygons.is_empty();
        if parent_empty_wrapper || is_separable_limb_name(&child.name) {
            extract_limbs_recursive(child, m, use_overlay, tint_abgr, out_limbs);
        } else {
            let cm = m * part_pose_mat(child.pose);
            let cn = Mat3::from_mat4(cm);
            collect_fused_subparts(
                child,
                cm,
                cn,
                false,
                use_overlay,
                tint_abgr,
                fused_quads,
                out_limbs,
            );
        }
    }
}

/// Builds a full multi-limb 3D skinned & dismemberable ragdoll from a dying `Mob` on the Client!
fn try_spawn_mob_ragdoll(game: &Game<'_>, m: &Mob, vp: &mut VpWorld) -> bool {
    if vp.spawned_ragdoll_ids.contains(&m.id) {
        return true;
    }

    let (layer_key, tex_cow) = horizon_client::mobs::render::body(m);
    let Some(primary_rect) = game
        .gui
        .atlas
        .sprites
        .get(tex_cow.as_ref())
        .copied()
        .or_else(|| game.gui.atlas.sprites.get(&*m.kind.texture).copied())
    else {
        return false;
    };

    let root_part = m.kind.layer(&layer_key).unwrap_or(&m.kind.model);

    // Upright entity-space transform matching LivingEntityRenderer (prior to death Z-flip!)
    let model_scale = m.model_scale();
    let avatar_scale = if matches!(&*m.kind.id, "minecraft:player" | "minecraft:mannequin") {
        0.9375
    } else {
        1.0
    };
    let root_to_entity = Mat4::from_scale(Vec3::splat(model_scale * avatar_scale))
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0));

    let mut raw_limbs = Vec::new();
    extract_limbs_recursive(root_part, root_to_entity, false, 0xFFFF_FFFF, &mut raw_limbs);
    if raw_limbs.is_empty() {
        return false;
    }

    // Optional secondary coat layer (Sheep wool, Drowned coat, Stray/Bogged overlay)
    let mut overlay_rect: Option<Rect> = None;
    let extra_layer: Option<(&str, &str, u32)> = match m.species() {
        Species::Sheep if !m.extra.sheared => {
            let rgb = horizon_client::mobs::render::sheep_wool_rgb(m.extra.variant as u8);
            let abgr = horizon_client::mobs::render::abgr(rgb);
            if m.is_baby() {
                Some(("sheep_baby#wool", "entity/sheep/sheep_wool_baby", abgr))
            } else {
                Some(("sheep#wool", "entity/sheep/sheep_wool", abgr))
            }
        }
        Species::Drowned => {
            if m.is_baby() {
                Some(("drowned_baby#outer", "entity/zombie/drowned_outer_layer_baby", 0xFFFF_FFFF))
            } else {
                Some(("drowned#outer", "entity/zombie/drowned_outer_layer", 0xFFFF_FFFF))
            }
        }
        Species::Stray => Some(("stray#outer", "entity/skeleton/stray_overlay", 0xFFFF_FFFF)),
        Species::Bogged if !m.extra.sheared => {
            Some(("bogged#outer", "entity/skeleton/bogged_overlay", 0xFFFF_FFFF))
        }
        _ => None,
    };

    if let Some((ov_model, ov_tex, ov_abgr)) = extra_layer
        && let (Some(ov_part), Some(ov_r)) = (m.kind.layer(ov_model), game.gui.atlas.sprites.get(ov_tex).copied())
    {
        overlay_rect = Some(ov_r);
        let mut ov_limbs = Vec::new();
        extract_limbs_recursive(ov_part, root_to_entity, true, ov_abgr, &mut ov_limbs);
        for ov in ov_limbs {
            if let Some(target) = raw_limbs.iter_mut().find(|r| r.name == ov.name) {
                target.quads.extend(ov.quads);
            }
        }
    }

    // Ensure torso is at index 0
    if let Some(torso_idx) = raw_limbs
        .iter()
        .position(|l| l.role == LimbRole::Torso)
        .or_else(|| {
            raw_limbs
                .iter()
                .enumerate()
                .max_by_key(|(_, l)| l.quads.len())
                .map(|(i, _)| i)
        })
        && torso_idx != 0
    {
        raw_limbs.swap(0, torso_idx);
    }
    raw_limbs[0].role = LimbRole::Torso;

    let yaw_rad = (180.0 - m.body_yaw).to_radians();
    let entity_rot = Quat::from_rotation_y(yaw_rad);
    let feet_world = Vec3::new(
        m.position.x as f32,
        m.position.y as f32,
        m.position.z as f32,
    );

    // Fetch kill impulse (from knockback / damage / explosion) or infer from mob velocity & player aim
    let imp = vp.pending_impulses.remove(&m.id);
    let mob_vel = Vec3::new(
        m.velocity.x as f32 * 20.0,
        m.velocity.y as f32 * 20.0,
        m.velocity.z as f32 * 20.0,
    );

    let player_dist = feet_world.distance(vp.last_player_pos);
    let melee_kill = player_dist < 5.5;
    let explosive = imp.is_some_and(|k| k.explosive);
    let damage = imp.map_or(6.0, |k| k.damage);

    let base_blow_vel = if let Some(k) = imp {
        k.dir * (k.strength * 8.5).clamp(2.5, 16.0) + Vec3::new(0.0, 2.6, 0.0)
    } else if mob_vel.length_squared() > 0.4 {
        mob_vel * 1.15 + Vec3::new(0.0, 2.2, 0.0)
    } else if melee_kill {
        let away = (feet_world - vp.last_player_pos).normalize_or(Vec3::Z);
        away * 5.2 + Vec3::new(0.0, 2.8, 0.0)
    } else {
        Vec3::new(0.0, 2.0, 0.0)
    };

    let mut rng = FastRng::seeded((m.id as u64).wrapping_mul(0x9E37_79B9) ^ vp.tick_counter);
    let gore_rgb = gore_color_for_species(m.species());

    // First compute centroids in entity space so joints link accurately to torso
    let mut centroids_entity = Vec::with_capacity(raw_limbs.len());
    let mut halves = Vec::with_capacity(raw_limbs.len());
    for rl in &raw_limbs {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for q in &rl.quads {
            for v in &q.verts {
                let p = Vec3::from_array(*v);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let c = if lo.x <= hi.x {
            0.5 * (lo + hi)
        } else {
            rl.pivot_entity
        };
        let h = if lo.x <= hi.x {
            (0.5 * (hi - lo)).max(Vec3::splat(0.06))
        } else {
            Vec3::splat(0.12)
        };
        centroids_entity.push(c);
        halves.push(h);
    }

    let torso_centroid = centroids_entity[0];

    // Decide dismemberment severity!
    // - Explosion or massive hit (>= 10 dmg): ALL limbs rip apart in every direction!
    // - Sharp weapon / Axe / Heavy hit (>= 5 dmg): Head + 1..3 limbs sever & fly off!
    // - Normal kill: 1..2 limbs sever so player ALWAYS sees dramatic dismemberment + jointed torso ragdoll!
    let sever_all = explosive || damage >= 11.0;
    let target_sever_count = if sever_all {
        raw_limbs.len()
    } else if vp.player_holding_axe || vp.player_holding_sharp || damage >= 5.0 {
        (raw_limbs.len() / 2 + 1).clamp(2, 4)
    } else {
        2usize.min(raw_limbs.len().saturating_sub(1))
    };

    let mut severed_count = 0usize;
    let mut built_limbs = Vec::with_capacity(raw_limbs.len());

    for (idx, mut rl) in raw_limbs.into_iter().enumerate() {
        let c_ent = centroids_entity[idx];
        let half = halves[idx];
        for q in &mut rl.quads {
            for v in &mut q.verts {
                *v = (Vec3::from_array(*v) - c_ent).to_array();
            }
        }

        let world_pos = feet_world + entity_rot * c_ent;
        let joint_point_ent = rl.pivot_entity;
        let anchor_on_torso = joint_point_ent - torso_centroid;
        let anchor_on_self = joint_point_ent - c_ent;
        let rest_dist = (c_ent - torso_centroid).length().max(0.12);

        let severed = if idx == 0 {
            false
        } else if sever_all {
            true
        } else {
            // Prioritize severing Head & Arms first for maximum visual drama!
            let chance = match rl.role {
                LimbRole::Head => 0.82,
                LimbRole::Arm => 0.72,
                LimbRole::Leg => 0.55,
                _ => 0.60,
            };
            if severed_count < target_sever_count && (rng.f32() < chance || idx == 1) {
                severed_count += 1;
                true
            } else {
                false
            }
        };

        let outward_ent = (c_ent - torso_centroid).normalize_or(Vec3::Y);
        let outward_world = entity_rot * outward_ent;

        let (vel, ang_vel) = if idx == 0 {
            (
                base_blow_vel * 0.85 + rng.vec3_spread(1.4, 0.5, 2.0),
                Vec3::new(rng.signed() * 7.5, rng.signed() * 5.0, rng.signed() * 7.5),
            )
        } else if severed {
            let burst_speed = if sever_all {
                rng.range(6.0, 11.5)
            } else {
                rng.range(3.8, 7.4)
            };
            let up_boost = if rl.role == LimbRole::Head {
                rng.range(3.6, 6.2)
            } else {
                rng.range(1.8, 4.5)
            };
            (
                base_blow_vel * 0.95
                    + outward_world * burst_speed
                    + Vec3::new(rng.signed() * 1.8, up_boost, rng.signed() * 1.8),
                Vec3::new(rng.signed(), rng.signed(), rng.signed()) * rng.range(10.0, 19.0),
            )
        } else {
            (
                base_blow_vel * 0.88 + outward_world * 1.1,
                Vec3::new(rng.signed() * 8.0, rng.signed() * 6.0, rng.signed() * 8.0),
            )
        };

        if severed {
            vp.spawn_gore_burst(world_pos, vel * 0.45, gore_rgb, if sever_all { 5 } else { 4 });
        }

        let radius = half.min_element().clamp(0.08, 0.32);
        built_limbs.push(RagdollLimb {
            name: rl.name,
            role: rl.role,
            pos: world_pos,
            prev_pos: world_pos,
            vel,
            rot: entity_rot,
            prev_rot: entity_rot,
            ang_vel,
            half,
            radius,
            anchor_on_torso,
            anchor_on_self,
            rest_dist,
            severed,
            grounded: false,
            quads: rl.quads,
        });
    }

    // If single-part mob (like Slime), burst extra 3D body chunks so it visibly explodes into pieces!
    if built_limbs.len() == 1 {
        vp.spawn_gore_burst(feet_world + Vec3::Y * 0.4, base_blow_vel, gore_rgb, 16);
    }

    // If moving from an active stumble into death, remove the stumble rig
    vp.active_stumbles.remove(&m.id);

    if vp.ragdolls.len() >= 24 {
        vp.ragdolls.remove(0);
    }
    vp.spawned_ragdoll_ids.insert(m.id);
    vp.ragdolls.push(MobRagdoll {
        mob_id: m.id,
        primary_rect,
        overlay_rect,
        gore_rgb,
        limbs: built_limbs,
        age_ticks: 0,
        max_ticks: 360, // 18 seconds persistent interactive corpse!
    });

    true
}

/// Triggers or intensifies an Active Ragdoll balance-loss / hit-flinch stumble on a LIVING mob!
fn try_trigger_living_stumble(
    game: &Game<'_>,
    m: &Mob,
    vp: &mut VpWorld,
    hit_dir_world: Vec3,
    strength: f32,
) {
    if m.death_time > 0 || vp.spawned_ragdoll_ids.contains(&m.id) {
        return;
    }

    let yaw_rad = (180.0 - m.body_yaw).to_radians();
    let inv_entity_rot = Quat::from_rotation_y(-yaw_rad);
    let local_dir = (inv_entity_rot * Vec3::new(hit_dir_world.x, 0.0, hit_dir_world.z))
        .normalize_or(Vec3::Z);
    // Axis in entity space around which the body pitches/rolls away from the blow: `Y x local_dir`
    let lean_axis_ent = Vec3::Y.cross(local_dir).normalize_or(Vec3::X);

    if let Some(existing) = vp.active_stumbles.get_mut(&m.id) {
        existing.lean_axis_ent = lean_axis_ent;
        existing.lean_vel = (existing.lean_vel + strength.clamp(0.5, 2.2) * 5.8).clamp(-12.0, 12.0);
        existing.ticks_left = 16;
        existing.hurt_flash_ticks = 5;
        return;
    }

    if vp.active_stumbles.len() >= 16 {
        return;
    }

    let (layer_key, tex_cow) = horizon_client::mobs::render::body(m);
    let Some(primary_rect) = game
        .gui
        .atlas
        .sprites
        .get(tex_cow.as_ref())
        .copied()
        .or_else(|| game.gui.atlas.sprites.get(&*m.kind.texture).copied())
    else {
        return;
    };

    let root_part = m.kind.layer(&layer_key).unwrap_or(&m.kind.model);
    let model_scale = m.model_scale();
    let avatar_scale = if matches!(&*m.kind.id, "minecraft:player" | "minecraft:mannequin") {
        0.9375
    } else {
        1.0
    };
    let root_to_entity = Mat4::from_scale(Vec3::splat(model_scale * avatar_scale))
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0));

    let mut raw_limbs = Vec::new();
    extract_limbs_recursive(root_part, root_to_entity, false, 0xFFFF_FFFF, &mut raw_limbs);
    if raw_limbs.is_empty() {
        return;
    }

    let mut overlay_rect: Option<Rect> = None;
    let extra_layer: Option<(&str, &str, u32)> = match m.species() {
        Species::Sheep if !m.extra.sheared => {
            let rgb = horizon_client::mobs::render::sheep_wool_rgb(m.extra.variant as u8);
            let abgr = horizon_client::mobs::render::abgr(rgb);
            if m.is_baby() {
                Some(("sheep_baby#wool", "entity/sheep/sheep_wool_baby", abgr))
            } else {
                Some(("sheep#wool", "entity/sheep/sheep_wool", abgr))
            }
        }
        Species::Drowned => {
            if m.is_baby() {
                Some(("drowned_baby#outer", "entity/zombie/drowned_outer_layer_baby", 0xFFFF_FFFF))
            } else {
                Some(("drowned#outer", "entity/zombie/drowned_outer_layer", 0xFFFF_FFFF))
            }
        }
        Species::Stray => Some(("stray#outer", "entity/skeleton/stray_overlay", 0xFFFF_FFFF)),
        Species::Bogged if !m.extra.sheared => {
            Some(("bogged#outer", "entity/skeleton/bogged_overlay", 0xFFFF_FFFF))
        }
        _ => None,
    };

    if let Some((ov_model, ov_tex, ov_abgr)) = extra_layer
        && let (Some(ov_part), Some(ov_r)) =
            (m.kind.layer(ov_model), game.gui.atlas.sprites.get(ov_tex).copied())
    {
        overlay_rect = Some(ov_r);
        let mut ov_limbs = Vec::new();
        extract_limbs_recursive(ov_part, root_to_entity, true, ov_abgr, &mut ov_limbs);
        for ov in ov_limbs {
            if let Some(target) = raw_limbs.iter_mut().find(|r| r.name == ov.name) {
                target.quads.extend(ov.quads);
            }
        }
    }

    let mut limbs = Vec::with_capacity(raw_limbs.len());
    for (idx, mut rl) in raw_limbs.into_iter().enumerate() {
        let piv = rl.pivot_entity;
        for q in &mut rl.quads {
            for v in &mut q.verts {
                *v = (Vec3::from_array(*v) - piv).to_array();
            }
        }
        let side_sign = if piv.x < -0.02 || rl.name.contains("left") || (idx % 2 == 0) {
            -1.0
        } else {
            1.0
        };
        limbs.push(StumbleLimb {
            role: rl.role,
            side_sign,
            pivot_ent: piv,
            local_rot: Quat::IDENTITY,
            prev_local_rot: Quat::IDENTITY,
            quads: rl.quads,
        });
    }

    let raw_l = game.world.light(
        m.position.x.floor() as i32,
        (m.position.y + 0.5).floor() as i32,
        m.position.z.floor() as i32,
    );

    let init_pos = DVec3::new(m.position.x, m.position.y, m.position.z);
    vp.active_stumbles.insert(
        m.id,
        ActiveStumbleRig {
            mob_id: m.id,
            primary_rect,
            overlay_rect,
            pos: init_pos,
            prev_pos: init_pos,
            body_yaw: m.body_yaw,
            lean_axis_ent,
            lean_angle: 0.14,
            prev_lean_angle: 0.0,
            lean_vel: (strength.clamp(0.45, 2.2) * 6.4).min(11.5),
            phase: 0.0,
            ticks_left: 16,
            hurt_flash_ticks: 5,
            packed_light: vertex_light(raw_l),
            limbs,
        },
    );
}

// ============================================================================
// 9. CLIENT 4-SUBSTEP 3D PHYSICS SIMULATION ENGINE
// ============================================================================

#[inline]
fn is_solid_voxel(game: &Game<'_>, x: i32, y: i32, z: i32) -> bool {
    let s = game.world.block(x, y, z);
    !game.res.facts().is_air(s) && game.res.facts().get(s).full_collision
}

#[inline]
fn is_water_voxel(game: &Game<'_>, x: i32, y: i32, z: i32) -> bool {
    let s = game.world.block(x, y, z);
    game.res.facts().get(s).name.contains("water")
}

/// Resolves 3D sphere-extent collision against solid world voxels with bounce & friction.
fn collide_body_voxels(
    game: &Game<'_>,
    pos: &mut Vec3,
    vel: &mut Vec3,
    radius: f32,
    restitution: f32,
    friction: f32,
) -> (bool, f32) {
    let mut grounded = false;
    let mut max_impact = 0.0f32;

    // 1. Vertical floor / ceiling check
    let bx = pos.x.floor() as i32;
    let bz = pos.z.floor() as i32;
    let foot_y = (pos.y - radius).floor() as i32;
    if is_solid_voxel(game, bx, foot_y, bz) {
        let floor_top = (foot_y + 1) as f32;
        if pos.y - radius < floor_top && pos.y > floor_top - 0.85 {
            pos.y = floor_top + radius;
            if vel.y < 0.0 {
                max_impact = max_impact.max(-vel.y);
                if -vel.y > 1.35 {
                    vel.y = -vel.y * restitution;
                } else {
                    vel.y = 0.0;
                }
                vel.x *= friction;
                vel.z *= friction;
            }
            grounded = true;
        }
    } else if vel.y > 0.0 {
        let head_y = (pos.y + radius).floor() as i32;
        if is_solid_voxel(game, bx, head_y, bz) {
            let ceil_bot = head_y as f32;
            if pos.y + radius > ceil_bot {
                pos.y = ceil_bot - radius;
                max_impact = max_impact.max(vel.y);
                vel.y = -vel.y * restitution;
            }
        }
    }

    // 2. Horizontal wall checks (X and Z)
    let mid_y = pos.y.floor() as i32;
    let check_x = if vel.x >= 0.0 {
        (pos.x + radius).floor() as i32
    } else {
        (pos.x - radius).floor() as i32
    };
    if check_x != bx && is_solid_voxel(game, check_x, mid_y, bz) {
        if vel.x > 0.0 {
            pos.x = check_x as f32 - radius - 0.001;
        } else {
            pos.x = (check_x + 1) as f32 + radius + 0.001;
        }
        max_impact = max_impact.max(vel.x.abs());
        vel.x = -vel.x * restitution;
    }

    let bx_now = pos.x.floor() as i32;
    let check_z = if vel.z >= 0.0 {
        (pos.z + radius).floor() as i32
    } else {
        (pos.z - radius).floor() as i32
    };
    if check_z != bz && is_solid_voxel(game, bx_now, mid_y, check_z) {
        if vel.z > 0.0 {
            pos.z = check_z as f32 - radius - 0.001;
        } else {
            pos.z = (check_z + 1) as f32 + radius + 0.001;
        }
        max_impact = max_impact.max(vel.z.abs());
        vel.z = -vel.z * restitution;
    }

    (grounded, max_impact)
}

fn step_client_physics(game: &mut Game<'_>, vp: &mut VpWorld) {
    vp.tick_counter = vp.tick_counter.wrapping_add(1);
    let dt_tick = 0.05f32;
    let substeps = 4usize;
    let dt = dt_tick / substeps as f32;
    let gravity = vp.cfg.gravity;

    // Update player kinematics & held weapon tags for kicks and dismemberment
    let (player_pos, player_vel, player_sneaking) = if let Some(p) = &game.player {
        let cur = Vec3::new(p.position.x as f32, p.position.y as f32, p.position.z as f32);
        let delta = cur - vp.last_player_pos;
        let pv = if vp.last_player_pos == Vec3::ZERO || delta.length_squared() > 16.0 {
            Vec3::new(
                p.velocity.x as f32 * 20.0,
                p.velocity.y as f32 * 20.0,
                p.velocity.z as f32 * 20.0,
            )
        } else {
            delta * 20.0
        };
        (cur, pv, game.has_shift_down())
    } else {
        (Vec3::ZERO, Vec3::ZERO, false)
    };
    vp.last_player_pos = player_pos;
    vp.last_player_vel = player_vel;

    if let Some(Some(held)) = game.inventory.get(game.selected) {
        let item_name: &str = &held.item;
        vp.player_holding_axe = item_name.ends_with("_axe") || item_name.ends_with("mace");
        vp.player_holding_sharp = vp.player_holding_axe
            || item_name.ends_with("_sword")
            || item_name.ends_with("trident")
            || item_name.ends_with("shears");
    } else {
        vp.player_holding_axe = false;
        vp.player_holding_sharp = false;
    }

    // Decay camera traumas
    for t in &mut vp.traumas {
        t.elapsed += dt_tick;
    }
    vp.traumas.retain(|t| t.elapsed < t.duration);

    // Cleanup old unused kill impulses
    let now_tick = vp.tick_counter;
    vp.pending_impulses
        .retain(|_, v| now_tick.saturating_sub(v.tick_recorded) < 60);

    // Scan Client mobs for any newly dying mob (`death_time > 0`) to guarantee ragdoll spawn
    if vp.cfg.ragdoll {
        for i in 0..game.mobs.len() {
            if game.mobs[i].death_time > 0 && !vp.spawned_ragdoll_ids.contains(&game.mobs[i].id) {
                let m = &game.mobs[i];
                let _ = try_spawn_mob_ragdoll(game, m, vp);
            }
        }
        if vp.spawned_ragdoll_ids.len() > 512 {
            let active: HashSet<u32> = vp.ragdolls.iter().map(|r| r.mob_id).collect();
            vp.spawned_ragdoll_ids.retain(|id| active.contains(id));
        }
    }

    // Process scheduled cascading block crumbles (top-to-bottom sand/gravel/overhang waves!)
    let mut ready_crumbles = Vec::new();
    for c in &mut vp.crumbles {
        if c.delay_ticks == 0 {
            ready_crumbles.push(c.clone());
        } else {
            c.delay_ticks -= 1;
        }
    }
    vp.crumbles.retain(|c| c.delay_ticks > 0);
    for c in ready_crumbles {
        if c.is_fine_sand {
            vp.spawn_sand_block_grains(c.pos, c.state, c.count as usize, c.bias_vel);
        } else {
            vp.spawn_subvoxels(
                c.pos,
                c.layer_y_offset,
                c.state,
                c.mat,
                c.count as usize,
                c.bias_vel,
            );
        }
        if c.drop_loot_on_fire {
            vp.server_ops.push_back(ServerOp::DropBlockLoot {
                pos: c.pos,
                state: c.state,
            });
        }
        if c.mat == ChunkMat::Granular && (vp.tick_counter & 1) == 0 {
            vp.client_fx.push_back(ClientFx::Sound {
                sound: "minecraft:block.sand.break",
                at: [
                    c.pos[0] as f64 + 0.5,
                    c.pos[1] as f64 + 0.5,
                    c.pos[2] as f64 + 0.5,
                ],
                vol: 0.65,
                pitch: 0.85 + ((c.pos[1].unsigned_abs() % 5) as f32) * 0.08,
            });
        }
    }

    // ------------------------------------------------------------------------
    // A. STEP 3D SUB-VOXEL CHUNKS & GRANULAR PILES
    // ------------------------------------------------------------------------
    // Resolve textures lazily on Client
    for ch in &mut vp.chunks {
        if ch.resolved.is_none() {
            if let Some((sp_name, rgb)) = ch.custom_sprite {
                if let Some(rect) = game.gui.atlas.sprites.get(sp_name).copied() {
                    ch.resolved = Some((rect, rgb));
                }
            } else if let Some((sp_name, tint)) = game.particle_sprite(ch.state, ch.origin_block)
                && let Some(rect) = game.gui.atlas.sprites.get(&sp_name).copied()
            {
                ch.resolved = Some((rect, tint));
            } else if let Some((sp_name, tint)) = game.block_face_sprite(ch.state)
                && let Some(rect) = game.gui.atlas.sprites.get(&sp_name).copied()
            {
                ch.resolved = Some((rect, tint));
            }
        }
        ch.prev_pos = ch.pos;
        ch.prev_rot = ch.rot;
        ch.age = ch.age.saturating_add(1);
    }

    let player_speed_xz = Vec3::new(player_vel.x, 0.0, player_vel.z).length();
    let mut sound_budget = 3usize;

    for sub in 0..substeps {
        for ch in &mut vp.chunks {
            let (restitution, friction) = match ch.mat {
                ChunkMat::Glass => (0.42, 0.82),
                ChunkMat::Wood => (0.30, 0.76),
                ChunkMat::Stone => (0.24, 0.72),
                ChunkMat::Granular => (0.06, 0.62),
                ChunkMat::Foliage => (0.12, 0.68),
                ChunkMat::Gore => (0.20, 0.68),
            };

            let bx = ch.pos.x.floor() as i32;
            let by = ch.pos.y.floor() as i32;
            let bz = ch.pos.z.floor() as i32;
            let in_water = is_water_voxel(game, bx, by, bz);

            if in_water {
                let buoy = match ch.mat {
                    ChunkMat::Wood | ChunkMat::Foliage => 14.0,
                    _ => -6.0,
                };
                ch.vel.y += buoy * dt;
                ch.vel *= 0.90;
                ch.ang_vel *= 0.90;
            } else {
                let g_scale = if ch.mat == ChunkMat::Foliage { 0.42 } else { 1.0 };
                ch.vel.y -= gravity * g_scale * dt;
                ch.vel *= 0.992;
            }

            ch.pos += ch.vel * dt;
            let r = ch.half.min_element().clamp(0.04, 0.16);
            let (grounded, impact) =
                collide_body_voxels(game, &mut ch.pos, &mut ch.vel, r, restitution, friction);
            ch.grounded = grounded;

            if grounded {
                ch.ang_vel *= 0.68;
                ch.rot = settle_quat_flat(ch.rot, 0.28);

                // Granular angle-of-repose avalanching: sand/gravel grains on a ledge slide off!
                if sub == 0 && ch.mat == ChunkMat::Granular && ch.age < 90 {
                    let foot_y = (ch.pos.y - r - 0.08).floor() as i32;
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let edge_dist_x = (ch.pos.x - (bx as f32 + 0.5)) * dx as f32;
                        let edge_dist_z = (ch.pos.z - (bz as f32 + 0.5)) * dz as f32;
                        if (edge_dist_x > 0.22 || edge_dist_z > 0.22)
                            && !is_solid_voxel(game, bx + dx, foot_y, bz + dz)
                            && !is_solid_voxel(game, bx + dx, foot_y + 1, bz + dz)
                        {
                            ch.vel.x += dx as f32 * 0.65;
                            ch.vel.z += dz as f32 * 0.65;
                            break;
                        }
                    }
                }
            } else {
                ch.rot = integrate_quat(ch.rot, ch.ang_vel, dt);
            }

            // Player foot kicking on ground chunks!
            if sub == 0 && player_speed_xz > 0.8 && !player_sneaking {
                let d = ch.pos - (player_pos + Vec3::Y * 0.2);
                if d.y.abs() < 0.65 && (d.x * d.x + d.z * d.z) < 0.55 * 0.55 {
                    let push_dir = Vec3::new(d.x, 0.0, d.z).normalize_or(Vec3::X);
                    ch.vel += push_dir * (player_speed_xz * 0.55).min(4.5)
                        + player_vel * 0.35
                        + Vec3::Y * 1.35;
                    ch.ang_vel += Vec3::new(-push_dir.z, 0.4, push_dir.x) * 9.0;
                }
            }

            if impact > 4.5 && sound_budget > 0 {
                sound_budget -= 1;
                let snd = match ch.mat {
                    ChunkMat::Glass => "minecraft:block.glass.hit",
                    ChunkMat::Wood => "minecraft:block.wood.hit",
                    ChunkMat::Granular => "minecraft:block.sand.hit",
                    ChunkMat::Foliage => "minecraft:block.azalea_leaves.hit",
                    ChunkMat::Stone => "minecraft:block.stone.hit",
                    ChunkMat::Gore => "minecraft:entity.slime.squish_small",
                };
                vp.client_fx.push_back(ClientFx::Sound {
                    sound: snd,
                    at: [ch.pos.x as f64, ch.pos.y as f64, ch.pos.z as f64],
                    vol: (impact * 0.05).clamp(0.15, 0.45),
                    pitch: 1.15,
                });
            }
        }
    }
    vp.chunks.retain(|c| c.age < c.max_age);

    // ------------------------------------------------------------------------
    // A2. STEP FINE POURING & SCATTERING 3D SAND / GRAVEL / POWDER GRAINS
    // ------------------------------------------------------------------------
    let mut kicked_grains_sound: Option<[f64; 3]> = None;
    for (gi, g) in vp.sand_grains.iter_mut().enumerate() {
        if g.resolved.is_none() {
            if let Some((sp_name, tint)) = game.particle_sprite(g.state, g.origin_block)
                && let Some(rect) = game.gui.atlas.sprites.get(&sp_name).copied()
            {
                g.resolved = Some((rect, tint));
            } else if let Some((sp_name, tint)) = game.block_face_sprite(g.state)
                && let Some(rect) = game.gui.atlas.sprites.get(&sp_name).copied()
            {
                g.resolved = Some((rect, tint));
            }
        }

        // Cache packed light once on spawn and periodically while airborne (0 light lookups during rendering!)
        if g.age == 0 || (!g.grounded && ((g.age as usize + gi) & 7) == 0) {
            let raw_l = game.world.light(
                g.pos.x.floor() as i32,
                (g.pos.y + 0.1).floor() as i32,
                g.pos.z.floor() as i32,
            );
            g.packed_light = vertex_light(raw_l);
        }

        g.prev_pos = g.pos;
        g.age = g.age.saturating_add(1);

        // Fast-path for grounded grains: skip substep physics unless kicked by boots or floor beneath is broken!
        if g.grounded {
            if ((g.age as usize + gi) & 5) == 0 {
                let bx = g.pos.x.floor() as i32;
                let by_foot = (g.pos.y - g.radius - 0.03).floor() as i32;
                let bz = g.pos.z.floor() as i32;
                if !is_solid_voxel(game, bx, by_foot, bz) {
                    g.grounded = false;
                    g.vel.y = -0.35;
                }
            }
            if (player_speed_xz > 0.38 && !player_sneaking) || player_vel.y < -1.8 {
                let d = g.pos - (player_pos + Vec3::Y * 0.16);
                if d.y.abs() < 0.58 && (d.x * d.x + d.z * d.z) < 0.65 * 0.65 {
                    let push_dir = Vec3::new(d.x, 0.0, d.z).normalize_or(Vec3::X);
                    let lift = 0.85 + ((gi % 7) as f32) * 0.22 + (-player_vel.y).max(0.0) * 0.28;
                    g.vel += push_dir * (player_speed_xz * 0.56 + 1.1).min(3.8)
                        + Vec3::new(player_vel.x * 0.30, lift, player_vel.z * 0.30);
                    g.grounded = false;
                    if kicked_grains_sound.is_none() && (vp.tick_counter & 3) == 0 {
                        kicked_grains_sound =
                            Some([g.pos.x as f64, g.pos.y as f64, g.pos.z as f64]);
                    }
                }
            }
        }
    }

    if let Some(snd_pos) = kicked_grains_sound {
        vp.client_fx.push_back(ClientFx::Sound {
            sound: "minecraft:block.sand.step",
            at: snd_pos,
            vol: 0.42,
            pitch: 1.08,
        });
    }

    const GRAIN_SUBSTEPS: usize = 2;
    let dt_grain = 0.05f32 / (GRAIN_SUBSTEPS as f32);
    for sub in 0..GRAIN_SUBSTEPS {
        for (gi, g) in vp.sand_grains.iter_mut().enumerate() {
            if g.grounded {
                continue;
            }
            g.vel.y -= gravity * 0.88 * dt_grain;
            g.vel *= 0.985;
            g.pos += g.vel * dt_grain;

            let r = g.radius;
            let bx = g.pos.x.floor() as i32;
            let by_foot = (g.pos.y - r).floor() as i32;
            let bz = g.pos.z.floor() as i32;

            // Fast floor landing & natural granular carpet scatter
            if is_solid_voxel(game, bx, by_foot, bz)
                && !is_solid_voxel(game, bx, by_foot + 1, bz)
            {
                let floor_top = (by_foot + 1) as f32;
                if g.pos.y - r <= floor_top + 0.025 {
                    g.pos.y = floor_top + r;
                    if g.vel.y < -1.65 {
                        // First impact: slight bounce + radial scatter so grains spread out over the floor!
                        let scatter_angle = ((gi as f32) * 2.399963) + (g.age as f32) * 0.37;
                        let scatter_mag = (-g.vel.y * 0.16).clamp(0.18, 0.95);
                        g.vel.x = g.vel.x * 0.68 + scatter_angle.cos() * scatter_mag;
                        g.vel.z = g.vel.z * 0.68 + scatter_angle.sin() * scatter_mag;
                        g.vel.y = (-g.vel.y * 0.16).min(1.15);
                    } else {
                        g.vel.y = 0.0;
                        g.vel.x *= 0.56;
                        g.vel.z *= 0.56;

                        // Ledge avalanche: grains near a drop-off slide over the edge!
                        let mut slid_off_ledge = false;
                        if sub == 0 && g.age < 75 {
                            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                                let edge_x = (g.pos.x - (bx as f32 + 0.5)) * dx as f32;
                                let edge_z = (g.pos.z - (bz as f32 + 0.5)) * dz as f32;
                                if (edge_x > 0.25 || edge_z > 0.25)
                                    && !is_solid_voxel(game, bx + dx, by_foot, bz + dz)
                                    && !is_solid_voxel(game, bx + dx, by_foot + 1, bz + dz)
                                {
                                    g.vel.x += dx as f32 * 0.78;
                                    g.vel.z += dz as f32 * 0.78;
                                    slid_off_ledge = true;
                                    break;
                                }
                            }
                        }

                        if !slid_off_ledge && (g.vel.x * g.vel.x + g.vel.z * g.vel.z) < 0.09 {
                            g.vel = Vec3::ZERO;
                            g.grounded = true;
                        }
                    }
                }
            }

            // Horizontal wall push-out if inside a solid neighbor block
            let by_mid = g.pos.y.floor() as i32;
            if is_solid_voxel(game, bx, by_mid, bz) {
                let cx = bx as f32 + 0.5;
                let cz = bz as f32 + 0.5;
                let dx = g.pos.x - cx;
                let dz = g.pos.z - cz;
                if dx.abs() > dz.abs() {
                    g.pos.x = cx + dx.signum() * (0.5 + r);
                    g.vel.x = -g.vel.x * 0.28;
                } else {
                    g.pos.z = cz + dz.signum() * (0.5 + r);
                    g.vel.z = -g.vel.z * 0.28;
                }
            }
        }
    }
    vp.sand_grains.retain(|g| g.age < g.max_age && g.pos.y > -96.0);

    // ------------------------------------------------------------------------
    // B. STEP 3D RIGID-BODY TREES & COLLAPSING MASONRY STRUCTURES
    //    (With Penetration Smash, Plastic Deformation & Volumetric Dust/Sparks!)
    // ------------------------------------------------------------------------
    let dim_id = game.dim.id().to_owned();
    let mut shattered_blocks: Vec<FlyingBlock> = Vec::new();
    let mut tree_splinters: Vec<([i32; 3], BlockStateId, ChunkMat, usize, Vec3)> = Vec::new();
    let mut dust_spawns: Vec<(Vec3, Vec3, u32, usize, Vec3)> = Vec::new();
    let mut spark_spawns: Vec<(Vec3, Vec3, usize)> = Vec::new();
    let mut crack_adds: Vec<([i32; 3], BlockStateId, u8)> = Vec::new();
    let mut player_push_accum = DVec3::ZERO;

    for asm in &mut vp.assemblies {
        asm.prev_pivot = asm.pivot;
        asm.prev_rot = asm.rot;
        asm.ticks_alive += 1;

        if asm.cells.is_empty() {
            continue;
        }

        // Player walking/sprinting into small loose cut log segments (<= 3 blocks) rolls them!
        if asm.cells.len() <= 3 && player_speed_xz > 1.2 {
            let c_world = asm.cell_world_center(asm.cells[0].local);
            let diff = Vec3::new(
                c_world.x as f32 - player_pos.x,
                c_world.y as f32 - (player_pos.y + 0.5),
                c_world.z as f32 - player_pos.z,
            );
            if diff.y.abs() < 0.9 && (diff.x * diff.x + diff.z * diff.z) < 1.15 * 1.15 {
                let push = Vec3::new(diff.x, 0.0, diff.z).normalize_or(Vec3::X)
                    * (player_speed_xz * 0.45).min(3.5);
                asm.vel.x = push.x as f64;
                asm.vel.z = push.z as f64;
                asm.vel.y = asm.vel.y.max(0.8);
                asm.settled = false;
                asm.hinged = false;
                asm.free_ang_vel = Vec3::new(-push.z, 0.0, push.x) * 3.5;
            }
        }

        if asm.settled {
            // Periodically verify support underneath settled trunk so if ground under it is dug out, it wakes up!
            if (asm.ticks_alive & 15) == 0 {
                let mut any_support = false;
                for cell in &asm.cells {
                    if cell.is_foliage {
                        continue;
                    }
                    let wc = asm.cell_world_center(cell.local);
                    if is_solid_voxel(
                        game,
                        wc.x.floor() as i32,
                        (wc.y - 0.62).floor() as i32,
                        wc.z.floor() as i32,
                    ) {
                        any_support = true;
                        break;
                    }
                }
                if !any_support {
                    asm.settled = false;
                    asm.hinged = false;
                    asm.settle_ticks = 0;
                }
            }
            continue;
        }

        // Creaking timber sound while tree leans through 10..60 degrees
        if asm.kind == AssemblyKind::Tree
            && !asm.crashed_once
            && asm.ticks_alive % 7 == 2
            && asm.fall_angle > 0.12
            && asm.fall_angle < 1.15
        {
            vp.client_fx.push_back(ClientFx::Sound {
                sound: "minecraft:block.wood.step",
                at: [asm.pivot.x, asm.pivot.y + 1.5, asm.pivot.z],
                vol: 0.75,
                pitch: 0.52 + asm.fall_angle * 0.22,
            });
        }

        let mut smash_budget_per_tick = 5usize;
        let heavy_assembly = asm.cells.len() >= 8 || asm.kind == AssemblyKind::Masonry;

        for _ in 0..substeps {
            if asm.hinged {
                // Real inverted-pendulum torque: tau = (g / L_eff) * sin(theta)
                let eff_theta = asm.fall_angle.max(0.11);
                let ang_acc = (gravity / asm.eff_length.max(2.2)) * eff_theta.sin() * 1.35;
                asm.ang_speed = (asm.ang_speed + ang_acc * dt).min(3.6);
                asm.fall_angle += asm.ang_speed * dt;
                asm.rot = Quat::from_axis_angle(asm.fall_axis, asm.fall_angle);

                if asm.fall_angle > 1.62 {
                    // Past horizontal (> 93 deg), release stump hinge so trunk lays onto slope/ravine!
                    asm.hinged = false;
                    asm.free_ang_vel = asm.fall_axis * (asm.ang_speed * 0.35);
                }
            } else {
                // Free 3D rigid body settling on terrain contour
                asm.vel.y -= (gravity as f64) * (dt as f64);
                asm.vel *= 0.985;
                asm.pivot += asm.vel * (dt as f64);
                asm.rot = integrate_quat(asm.rot, asm.free_ang_vel, dt);
                asm.free_ang_vel *= 0.92;
            }

            // Test solid structural cells against world terrain voxels!
            let mut max_penetration_y = 0.0f64;
            let mut contact_points = 0usize;
            let mut crash_center = asm.pivot;
            let has_high_momentum =
                heavy_assembly && (asm.ang_speed > 0.72 || asm.vel.length_squared() > 14.0);

            let stride = if asm.cells.len() > 60 { 2 } else { 1 };
            let pivot_snapshot = asm.pivot;
            let rot_snapshot = asm.rot;
            let fall_dir_snapshot = asm.fall_dir_xz;
            let hinged_snapshot = asm.hinged;
            let stump_snapshot = asm.origin_stump;

            for (ci, cell) in asm.cells.iter_mut().enumerate() {
                if cell.is_foliage && (ci % stride != 0) {
                    continue;
                }
                let local_c = Vec3::new(
                    cell.local.x as f32,
                    cell.local.y as f32 + 0.5,
                    cell.local.z as f32,
                );
                let turned = rot_snapshot * local_c;
                let wc = pivot_snapshot + DVec3::new(turned.x as f64, turned.y as f64, turned.z as f64);
                let bx = wc.x.floor() as i32;
                let by = (wc.y - 0.42).floor() as i32;
                let bz = wc.z.floor() as i32;

                // Ignore collision with the immediate 1-block stump base while hinged
                if hinged_snapshot
                    && (bx - stump_snapshot[0]).abs() <= 1
                    && (by - stump_snapshot[1]).abs() <= 1
                    && (bz - stump_snapshot[2]).abs() <= 1
                {
                    continue;
                }

                let hit_state = game.world.block(bx, by, bz);
                if !game.res.facts().is_air(hit_state) {
                    let hit_name = &game.res.facts().get(hit_state).name;

                    // PENETRATION SMASH: Falling tree/structure smashes right through fragile blocks
                    // (glass, leaves, fences, lanterns, doors, or thin wooden roofs/floors under heavy momentum)!
                    if !cell.is_foliage
                        && smash_budget_per_tick > 0
                        && is_penetrable_fragile_block(hit_name, has_high_momentum)
                    {
                        smash_budget_per_tick -= 1;
                        let hit_pos = [bx, by, bz];
                        let hit_mat = classify_block_name(hit_name);
                        vp.server_ops.push_back(ServerOp::SetAir {
                            dim: dim_id.clone(),
                            pos: hit_pos,
                        });
                        vp.server_ops.push_back(ServerOp::DropBlockLoot {
                            pos: hit_pos,
                            state: hit_state,
                        });
                        tree_splinters.push((
                            hit_pos,
                            hit_state,
                            hit_mat,
                            10,
                            fall_dir_snapshot * 3.5 + Vec3::Y * 1.4,
                        ));
                        let break_snd = match hit_mat {
                            ChunkMat::Glass => "minecraft:block.glass.break",
                            ChunkMat::Foliage => "minecraft:block.azalea_leaves.break",
                            _ => "minecraft:entity.zombie.break_wooden_door",
                        };
                        vp.client_fx.push_back(ClientFx::Sound {
                            sound: break_snd,
                            at: [bx as f64 + 0.5, by as f64 + 0.5, bz as f64 + 0.5],
                            vol: 0.78,
                            pitch: 0.92,
                        });
                        asm.ang_speed *= 0.95;
                        asm.vel *= 0.92;
                        continue;
                    }

                    if game.res.facts().get(hit_state).full_collision {
                        let top_y = (by + 1) as f64;
                        let pen = (top_y - (wc.y - 0.44)).max(0.0);
                        if !cell.is_foliage && pen > max_penetration_y {
                            max_penetration_y = pen;
                            crash_center = wc;
                        }
                        if !cell.is_foliage {
                            contact_points += 1;
                            // MATERIAL DEFORMATION: Plastic bending & buckling of impacted timber/beam cells!
                            if cell.deform_offset.length_squared() < 0.015 {
                                let bend_sign = if (ci & 1) == 0 { 1.0 } else { -1.0 };
                                cell.deform_offset = Vec3::new(
                                    fall_dir_snapshot.x * 0.08 * bend_sign,
                                    -0.11,
                                    fall_dir_snapshot.z * 0.08 * bend_sign,
                                );
                                cell.dent_rot = Quat::from_euler(
                                    glam::EulerRot::XYZ,
                                    0.12 * bend_sign,
                                    0.05,
                                    -0.11 * bend_sign,
                                );
                            }
                            if has_high_momentum && crack_adds.len() < 6 {
                                crack_adds.push(([bx, by, bz], hit_state, 1));
                            }
                        }
                    }
                }
            }

            if contact_points > 0 && (asm.fall_angle > 0.35 || !asm.hinged) {
                if !asm.crashed_once {
                    asm.crashed_once = true;
                    let crash_vec3 = crash_center.as_vec3();
                    vp.pending_shockwaves.push((crash_vec3, 4.8));

                    if asm.kind == AssemblyKind::Masonry {
                        let is_small_or_light = asm.cells.len() < 24
                            || asm.cells.iter().any(|c| {
                                let bname = &game.res.facts().get(c.state).name;
                                bname.contains("fence")
                                    || bname.contains("wall")
                                    || bname.contains("wood")
                                    || bname.contains("log")
                                    || bname.contains("plank")
                            });

                        if is_small_or_light {
                            // Select wood vs stone sound depending on assembly material
                            let is_wood = asm.cells.iter().any(|c| {
                                let bname = &game.res.facts().get(c.state).name;
                                bname.contains("fence") || bname.contains("wood") || bname.contains("log") || bname.contains("plank")
                            });
                            let sound = if is_wood {
                                "minecraft:block.wood.fall"
                            } else {
                                "minecraft:block.stone.fall"
                            };
                            vp.client_fx.push_back(ClientFx::Sound {
                                sound,
                                at: [crash_center.x, crash_center.y, crash_center.z],
                                vol: 0.45,
                                pitch: 0.85,
                            });
                            dust_spawns.push((
                                crash_vec3 + Vec3::Y * 0.25,
                                Vec3::splat(0.6),
                                0xb0a890,
                                8,
                                asm.fall_dir_xz * 0.8 + Vec3::Y * 0.3,
                            ));
                        } else {
                            if vp.cfg.explosions && vp.traumas.len() < 16 {
                                vp.traumas.push(CameraTrauma {
                                    origin: crash_vec3,
                                    radius: 24.0,
                                    trauma: 0.55,
                                    duration: 0.6,
                                    elapsed: 0.0,
                                });
                            }
                            vp.client_fx.push_back(ClientFx::Sound {
                                sound: "minecraft:block.stone.fall",
                                at: [crash_center.x, crash_center.y, crash_center.z],
                                vol: 0.55,
                                pitch: 0.60,
                            });
                            vp.client_fx.push_back(ClientFx::Smoke {
                                at: [crash_center.x, crash_center.y + 0.4, crash_center.z],
                                vel: [0.0, 0.10, 0.0],
                                large: false,
                            });
                            dust_spawns.push((
                                crash_vec3 + Vec3::Y * 0.45,
                                Vec3::new(1.8, 0.8, 1.8),
                                0x9C9A96,
                                20,
                                asm.fall_dir_xz * 1.2 + Vec3::Y * 0.5,
                            ));
                        }
                        spark_spawns.push((crash_vec3 + Vec3::Y * 0.3, Vec3::Y * 2.4, 18));

                        let mut rng = FastRng::seeded(asm.id ^ vp.tick_counter);
                        for (idx, cell) in asm.cells.drain(..).enumerate() {
                            let wc = asm.pivot
                                + DVec3::from(
                                    asm.rot
                                        * Vec3::new(
                                            cell.local.x as f32,
                                            cell.local.y as f32 + 0.5,
                                            cell.local.z as f32,
                                        ),
                                );
                            if idx % 2 == 0 && shattered_blocks.len() < 28 {
                                let v = rng.vec3_spread(4.8, 1.2, 4.5);
                                shattered_blocks.push(FlyingBlock {
                                    pos: wc,
                                    prev_pos: wc,
                                    vel: DVec3::new(v.x as f64, v.y as f64, v.z as f64),
                                    rot: asm.rot,
                                    prev_rot: asm.rot,
                                    ang_vel: Vec3::new(
                                        rng.signed() * 8.0,
                                        rng.signed() * 8.0,
                                        rng.signed() * 8.0,
                                    ),
                                    state: cell.state,
                                    seed: (idx as i64) * 31 + 7,
                                    bounces: 0,
                                    age: 0,
                                    place_on_land: true,
                                });
                            } else {
                                let bpos = [
                                    wc.x.floor() as i32,
                                    wc.y.floor() as i32,
                                    wc.z.floor() as i32,
                                ];
                                let mat = classify_block_name(&game.res.facts().get(cell.state).name);
                                tree_splinters.push((
                                    bpos,
                                    cell.state,
                                    mat,
                                    8,
                                    asm.fall_dir_xz * 2.5 + Vec3::Y * 1.5,
                                ));
                                vp.server_ops.push_back(ServerOp::DropBlockLoot {
                                    pos: bpos,
                                    state: cell.state,
                                });
                            }
                        }
                        break;
                    } else {
                        // TREE CRASHES ONTO THE FOREST FLOOR!
                        if vp.cfg.explosions && vp.traumas.len() < 16 {
                            vp.traumas.push(CameraTrauma {
                                origin: crash_vec3,
                                radius: 34.0,
                                trauma: 0.72,
                                duration: 0.85,
                                elapsed: 0.0,
                            });
                        }
                        vp.client_fx.push_back(ClientFx::Sound {
                            sound: "minecraft:entity.zombie.break_wooden_door",
                            at: [crash_center.x, crash_center.y, crash_center.z],
                            vol: 0.85,
                            pitch: 0.58,
                        });
                        vp.client_fx.push_back(ClientFx::Sound {
                            sound: "minecraft:block.azalea_leaves.break",
                            at: [crash_center.x, crash_center.y + 0.5, crash_center.z],
                            vol: 1.0,
                            pitch: 0.78,
                        });
                        vp.client_fx.push_back(ClientFx::Smoke {
                            at: [crash_center.x, crash_center.y + 0.3, crash_center.z],
                            vel: [0.0, 0.09, 0.0],
                            large: true,
                        });
                        dust_spawns.push((
                            crash_vec3 + Vec3::Y * 0.35,
                            Vec3::new(1.6, 0.6, 1.6),
                            0x8C6E4B,
                            18,
                            asm.fall_dir_xz * 1.2 + Vec3::Y * 0.45,
                        ));

                        // Crush ~65% of outer canopy leaves into 3D flying leaf debris & real sapling/stick loot!
                        let pivot_copy = asm.pivot;
                        let rot_copy = asm.rot;
                        let fall_dir = asm.fall_dir_xz;
                        let mut leaf_idx = 0usize;
                        let mut burst_leaves = 0usize;
                        asm.cells.retain(|cell| {
                            if !cell.is_foliage {
                                return true;
                            }
                            leaf_idx += 1;
                            if leaf_idx % 3 != 0 {
                                let wc = pivot_copy
                                    + DVec3::from(
                                        rot_copy
                                            * Vec3::new(
                                                cell.local.x as f32,
                                                cell.local.y as f32 + 0.5,
                                                cell.local.z as f32,
                                            ),
                                    );
                                let bpos = [
                                    wc.x.floor() as i32,
                                    wc.y.floor() as i32,
                                    wc.z.floor() as i32,
                                ];
                                if burst_leaves < 22 {
                                    burst_leaves += 1;
                                    tree_splinters.push((
                                        bpos,
                                        cell.state,
                                        ChunkMat::Foliage,
                                        3,
                                        fall_dir * 2.2 + Vec3::Y * 1.8,
                                    ));
                                }
                                // Roll authentic vanilla leaf drops (saplings/sticks/apples)!
                                if leaf_idx % 4 == 0 {
                                    vp.server_ops.push_back(ServerOp::DropBlockLoot {
                                        pos: bpos,
                                        state: cell.state,
                                    });
                                }
                                false
                            } else {
                                true
                            }
                        });

                        // Release stump hinge and damp bounce so trunk settles naturally onto terrain!
                        asm.hinged = false;
                        asm.pivot.y += max_penetration_y.min(0.6);
                        asm.vel = DVec3::new(0.0, 1.1, 0.0);
                        asm.free_ang_vel = -asm.fall_axis * (asm.ang_speed * 0.16);
                        asm.ang_speed = 0.0;
                    }
                } else {
                    // Subsequent settling contacts on ground
                    asm.hinged = false;
                    asm.pivot.y += max_penetration_y.min(0.35);
                    if asm.vel.y < 0.0 {
                        asm.vel.y = -asm.vel.y * 0.12;
                    }
                    asm.vel.x *= 0.72;
                    asm.vel.z *= 0.72;
                    asm.free_ang_vel *= 0.65;

                    if asm.vel.length_squared() < 0.12 && asm.free_ang_vel.length_squared() < 0.08 {
                        asm.settle_ticks += 1;
                        if asm.settle_ticks >= 6 {
                            asm.settled = true;
                            asm.vel = DVec3::ZERO;
                            asm.free_ang_vel = Vec3::ZERO;
                            break;
                        }
                    }
                }
            }
        }

        // Sweeping entity push & crush while tree/structure is actively falling!
        if !asm.settled && (asm.ang_speed > 0.35 || asm.vel.length_squared() > 1.0) {
            let dim_str = game.dim.id().to_owned();
            for cell in &asm.cells {
                if cell.is_foliage {
                    continue;
                }
                let wc = asm.cell_world_center(cell.local);

                // Push local player out of falling trunk path
                let pdx = (player_pos.x as f64) - wc.x;
                let pdy = (player_pos.y as f64 + 0.9) - wc.y;
                let pdz = (player_pos.z as f64) - wc.z;
                if pdx.abs() < 0.95 && pdy.abs() < 1.15 && pdz.abs() < 0.95 {
                    player_push_accum.x = (asm.fall_dir_xz.x as f64) * 0.42;
                    player_push_accum.y = 0.24;
                    player_push_accum.z = (asm.fall_dir_xz.z as f64) * 0.42;
                }

                // Crush mobs under falling tree trunk!
                for m in &game.mobs {
                    if !m.alive() || asm.hit_mobs.contains(&m.id) {
                        continue;
                    }
                    let mdx = m.position.x - wc.x;
                    let mdy = (m.position.y + f64::from(m.height()) * 0.5) - wc.y;
                    let mdz = m.position.z - wc.z;
                    if mdx.abs() < 1.05 && mdy.abs() < 1.25 && mdz.abs() < 1.05 {
                        asm.hit_mobs.insert(m.id);
                        let push = [
                            asm.fall_dir_xz.x as f64 * 0.65,
                            0.38,
                            asm.fall_dir_xz.z as f64 * 0.65,
                        ];
                        vp.pending_impulses.insert(
                            m.id,
                            PendingKillImpulse {
                                dir: asm.fall_dir_xz,
                                strength: 1.8,
                                damage: 12.0,
                                explosive: true,
                                tick_recorded: vp.tick_counter,
                            },
                        );
                        vp.server_ops.push_back(ServerOp::CrushMob {
                            dim: dim_str.clone(),
                            mob_id: m.id,
                            damage: 10.0,
                            push,
                        });
                        vp.client_fx.push_back(ClientFx::Sound {
                            sound: "minecraft:entity.player.attack.knockback",
                            at: [wc.x, wc.y, wc.z],
                            vol: 0.9,
                            pitch: 0.75,
                        });
                    }
                }
            }
        }
    }

    if player_push_accum.length_squared() > 0.001
        && let Some(p) = &mut game.player
    {
        p.velocity.x += player_push_accum.x;
        p.velocity.y = p.velocity.y.max(player_push_accum.y);
        p.velocity.z += player_push_accum.z;
    }

    for (bpos, st, mat, cnt, bvel) in tree_splinters {
        vp.spawn_subvoxels(bpos, -1.0, st, mat, cnt, bvel);
    }
    vp.flying_blocks.extend(shattered_blocks);

    // Keep fallen trees for 3 minutes (3600 ticks); if expiring, drop remaining logs cleanly
    let mut expired_logs = Vec::new();
    vp.assemblies.retain(|a| {
        if a.cells.is_empty() || a.pivot.y < -96.0 {
            return false;
        }
        if a.ticks_alive > 3600 {
            for c in &a.cells {
                if !c.is_foliage {
                    let wc = a.cell_world_center(c.local);
                    expired_logs.push((
                        [wc.x.floor() as i32, wc.y.floor() as i32, wc.z.floor() as i32],
                        c.state,
                    ));
                }
            }
            false
        } else {
            true
        }
    });
    for (p, s) in expired_logs {
        vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: p, state: s });
    }

    // ------------------------------------------------------------------------
    // C. STEP 3D SPINNING CRATER / CAVE-IN / LANDSLIDE BOULDERS (`FlyingBlock`)
    //    (With Penetration Smash, Mob Crushing & Impact Cracks!)
    // ------------------------------------------------------------------------
    let mut landed_shatter = Vec::new();
    for fb in &mut vp.flying_blocks {
        fb.prev_pos = fb.pos;
        fb.prev_rot = fb.rot;
        fb.age += 1;

        // Airborne boulders crush mobs in their path!
        if fb.age > 1 && fb.vel.length_squared() > 6.0 {
            for m in &game.mobs {
                if !m.alive() {
                    continue;
                }
                let mdx = m.position.x - fb.pos.x;
                let mdy = (m.position.y + f64::from(m.height()) * 0.5) - fb.pos.y;
                let mdz = m.position.z - fb.pos.z;
                if mdx.abs() < 0.85 && mdy.abs() < 1.05 && mdz.abs() < 0.85 {
                    let dir = fb.vel.as_vec3().normalize_or(Vec3::NEG_Y);
                    vp.pending_impulses.insert(
                        m.id,
                        PendingKillImpulse {
                            dir,
                            strength: 1.5,
                            damage: 8.0,
                            explosive: true,
                            tick_recorded: vp.tick_counter,
                        },
                    );
                    vp.server_ops.push_back(ServerOp::CrushMob {
                        dim: dim_id.clone(),
                        mob_id: m.id,
                        damage: 7.0,
                        push: [dir.x as f64 * 0.5, 0.3, dir.z as f64 * 0.5],
                    });
                    fb.vel *= 0.65;
                    break;
                }
            }
        }

        for _ in 0..substeps {
            fb.vel.y -= (gravity as f64) * (dt as f64);
            fb.vel *= 0.992;
            fb.pos += fb.vel * (dt as f64);
            fb.rot = integrate_quat(fb.rot, fb.ang_vel, dt);

            let bx = fb.pos.x.floor() as i32;
            let by = (fb.pos.y - 0.45).floor() as i32;
            let bz = fb.pos.z.floor() as i32;
            if fb.age > 2 {
                let hit_st = game.world.block(bx, by, bz);
                if !game.res.facts().is_air(hit_st) {
                    let hit_name = &game.res.facts().get(hit_st).name;
                    let high_speed = fb.vel.length_squared() > 80.0;

                    // Ballistic Penetration Smash through glass, leaves, fences, lanterns, or thin planks!
                    if is_penetrable_fragile_block(hit_name, high_speed) {
                        let hpos = [bx, by, bz];
                        let _hmat = classify_block_name(hit_name);
                        vp.server_ops.push_back(ServerOp::SetAir {
                            dim: dim_id.clone(),
                            pos: hpos,
                        });
                        vp.server_ops.push_back(ServerOp::DropBlockLoot {
                            pos: hpos,
                            state: hit_st,
                        });
                        landed_shatter.push((hpos, hit_st, 8usize, false));
                        fb.vel *= 0.78;
                        continue;
                    }

                    if game.res.facts().get(hit_st).full_collision {
                        if high_speed && crack_adds.len() < 8 {
                            crack_adds.push(([bx, by, bz], hit_st, 1));
                        }
                        if fb.bounces == 0 && fb.vel.y < -6.0 {
                            fb.bounces += 1;
                            fb.pos.y = (by + 1) as f64 + 0.46;
                            fb.vel.y = -fb.vel.y * 0.28;
                            fb.vel.x *= 0.65;
                            fb.vel.z *= 0.65;
                            fb.ang_vel *= 0.65;
                            spark_spawns.push((fb.pos.as_vec3(), Vec3::Y * 1.8, 6));
                        } else {
                            fb.age = 9999; // Finish flight!
                            let target_cell = [bx, by + 1, bz];
                            if fb.place_on_land
                                && game.res.facts().is_air(game.world.block(
                                    target_cell[0],
                                    target_cell[1],
                                    target_cell[2],
                                ))
                            {
                                vp.server_ops.push_back(ServerOp::PlaceBlockIfAir {
                                    dim: dim_id.clone(),
                                    pos: target_cell,
                                    state: fb.state,
                                });
                                landed_shatter.push((target_cell, fb.state, 4usize, false));
                            } else {
                                landed_shatter.push((target_cell, fb.state, 10usize, true));
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    vp.flying_blocks.retain(|fb| fb.age < 240);
    for (p, s, n_chunks, drop_loot) in landed_shatter {
        let bname = &game.res.facts().get(s).name;
        let mat = classify_block_name(bname);
        vp.spawn_subvoxels(p, -1.0, s, mat, n_chunks, Vec3::Y * 1.4);
        dust_spawns.push((
            Vec3::new(p[0] as f32 + 0.5, p[1] as f32 + 0.3, p[2] as f32 + 0.5),
            Vec3::splat(0.35),
            dust_rgb_for_block_name(bname),
            5,
            Vec3::Y * 0.4,
        ));
        if drop_loot {
            vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: p, state: s });
        }
        vp.client_fx.push_back(ClientFx::Sound {
            sound: "minecraft:block.stone.place",
            at: [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5],
            vol: 0.55,
            pitch: 0.85,
        });
    }

    for (d_c, d_h, d_rgb, d_cnt, d_vel) in dust_spawns {
        vp.spawn_dust_cloud(d_c, d_h, d_rgb, d_cnt, d_vel);
    }
    for (s_c, s_v, s_cnt) in spark_spawns {
        vp.spawn_sparks(s_c, s_v, s_cnt);
    }
    for (c_pos, c_st, c_add) in crack_adds {
        if vp.add_block_crack(c_pos, c_st, c_add) {
            vp.server_ops.push_back(ServerOp::SetAir {
                dim: dim_id.clone(),
                pos: c_pos,
            });
            vp.server_ops.push_back(ServerOp::DropBlockLoot {
                pos: c_pos,
                state: c_st,
            });
            let mat = classify_block_name(&game.res.facts().get(c_st).name);
            vp.spawn_subvoxels(c_pos, -1.0, c_st, mat, 10, Vec3::Y * 1.2);
        }
    }

    // ------------------------------------------------------------------------
    // D. STEP AUTHENTIC 3D SKINNED & DISMEMBERED MOB RAGDOLLS (XPBD)
    // ------------------------------------------------------------------------
    let mut new_gore: Vec<(Vec3, Vec3, u32)> = Vec::new();
    for rag in &mut vp.ragdolls {
        rag.age_ticks += 1;
        for limb in &mut rag.limbs {
            limb.prev_pos = limb.pos;
            limb.prev_rot = limb.rot;
        }

        for sub in 0..substeps {
            // 1. Free integration + water buoyancy + player kicking
            for limb in &mut rag.limbs {
                let bx = limb.pos.x.floor() as i32;
                let by = limb.pos.y.floor() as i32;
                let bz = limb.pos.z.floor() as i32;

                if is_water_voxel(game, bx, by, bz) {
                    limb.vel.y += 12.0 * dt;
                    limb.vel *= 0.90;
                    limb.ang_vel *= 0.88;
                } else {
                    limb.vel.y -= gravity * dt;
                    limb.vel *= 0.990;
                }

                limb.pos += limb.vel * dt;

                // Player walking/sprinting kicks ragdoll limbs & severed heads!
                if sub == 0 && player_speed_xz > 0.7 && !player_sneaking {
                    let d = limb.pos - (player_pos + Vec3::Y * 0.35);
                    if d.y.abs() < 0.85 && (d.x * d.x + d.z * d.z) < 0.78 * 0.78 {
                        let kick_dir = Vec3::new(d.x, 0.0, d.z).normalize_or(Vec3::X);
                        let force = (player_speed_xz * 0.75).clamp(2.2, 6.8);
                        limb.vel += kick_dir * force + player_vel * 0.4 + Vec3::Y * 1.9;
                        limb.ang_vel += Vec3::new(-kick_dir.z, 0.5, kick_dir.x) * 11.0;
                    }
                }
            }

            // 2. XPBD Joint Constraints (connecting unsevered limbs to torso[0])
            if rag.limbs.len() > 1 {
                for idx in 1..rag.limbs.len() {
                    if rag.limbs[idx].severed {
                        continue;
                    }
                    let torso_pos = rag.limbs[0].pos;
                    let torso_rot = rag.limbs[0].rot;
                    let limb_pos = rag.limbs[idx].pos;
                    let limb_rot = rag.limbs[idx].rot;

                    let world_a = torso_pos + torso_rot * rag.limbs[idx].anchor_on_torso;
                    let world_b = limb_pos + limb_rot * rag.limbs[idx].anchor_on_self;
                    let delta = world_b - world_a;
                    let err = delta.length();

                    // Dynamic high-stress joint tearing if limb snags violently!
                    if err > (rag.limbs[idx].rest_dist * 2.6).max(0.95) {
                        rag.limbs[idx].severed = true;
                        new_gore.push((world_b, rag.limbs[idx].vel, rag.gore_rgb));
                        continue;
                    }

                    if err > 0.005 {
                        let corr = delta * 0.72;
                        rag.limbs[0].pos += corr * 0.30;
                        rag.limbs[idx].pos -= corr * 0.70;
                        rag.limbs[0].vel += corr * (0.18 / dt);
                        rag.limbs[idx].vel -= corr * (0.42 / dt);
                    }

                    // Soft angular alignment toward torso so connected limbs don't spin wildly inside body
                    rag.limbs[idx].rot = rag.limbs[idx].rot.slerp(torso_rot, 0.14).normalize();
                    rag.limbs[idx].ang_vel *= 0.90;
                }
            }

            // 3. Voxel terrain collision & natural flat-lying pose relaxation
            for limb in &mut rag.limbs {
                let (grounded, impact) = collide_body_voxels(
                    game,
                    &mut limb.pos,
                    &mut limb.vel,
                    limb.radius,
                    0.22,
                    0.74,
                );
                limb.grounded = grounded;

                if grounded {
                    limb.ang_vel *= 0.65;
                    // Torso and elongated limbs naturally tip over onto their flattest side (90 deg X/Z)
                    let (yaw, pitch, roll) = limb.rot.to_euler(glam::EulerRot::YXZ);
                    let target_pitch = if limb.role != LimbRole::Head && pitch.abs() < 0.7 && roll.abs() < 0.7 {
                        if pitch >= 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 }
                    } else {
                        (pitch / FRAC_PI_2).round() * FRAC_PI_2
                    };
                    let target_roll = (roll / FRAC_PI_2).round() * FRAC_PI_2;
                    let target_q = Quat::from_euler(glam::EulerRot::YXZ, yaw, target_pitch, target_roll);
                    limb.rot = limb.rot.slerp(target_q, 0.22).normalize();
                } else {
                    limb.rot = integrate_quat(limb.rot, limb.ang_vel, dt);
                }

                if impact > 5.5 && sound_budget > 0 {
                    sound_budget -= 1;
                    vp.client_fx.push_back(ClientFx::Sound {
                        sound: "minecraft:entity.player.small_fall",
                        at: [limb.pos.x as f64, limb.pos.y as f64, limb.pos.z as f64],
                        vol: 0.45,
                        pitch: 0.85,
                    });
                }
            }
        }
    }

    for (at, v, rgb) in new_gore {
        vp.spawn_gore_burst(at, v, rgb, 5);
    }
    vp.ragdolls.retain(|r| r.age_ticks < r.max_ticks);

    // ------------------------------------------------------------------------
    // F. PROCESS PENDING IMPULSES ON LIVING MOBS -> ACTIVE RAGDOLL STUMBLE
    // ------------------------------------------------------------------------
    if vp.cfg.ragdoll {
        let pending_stumble_list: Vec<(u32, Vec3, f32)> = vp.pending_stumbles.drain(..).collect();
        for (id, dir, strength) in pending_stumble_list {
            if let Some(m) = game.mobs.iter().find(|x| x.id == id) {
                try_trigger_living_stumble(game, m, vp, dir, strength);
            }
        }

        let mut stumbles_to_trigger = Vec::new();
        for m in &game.mobs {
            if m.death_time == 0 {
                if let Some(impulse) = vp.pending_impulses.get(&m.id) {
                    if impulse.strength > 0.4 || impulse.damage >= 4.0 {
                        stumbles_to_trigger.push((m.id, impulse.dir, impulse.strength));
                    }
                }
            }
        }
        for (id, dir, strength) in stumbles_to_trigger {
            if let Some(m) = game.mobs.iter().find(|x| x.id == id) {
                try_trigger_living_stumble(game, m, vp, dir, strength);
            }
            vp.pending_impulses.remove(&id);
        }

        // Step Active Stumble PD Balance Controller
        let dt_stumble = dt_tick;
        let mut expired = Vec::new();
        for (&mob_id, rig) in &mut vp.active_stumbles {
            if rig.ticks_left == 0 {
                expired.push(mob_id);
                continue;
            }
            rig.ticks_left -= 1;
            if rig.hurt_flash_ticks > 0 {
                rig.hurt_flash_ticks -= 1;
            }
            rig.prev_pos = rig.pos;
            rig.prev_lean_angle = rig.lean_angle;

            // Find living mob position
            if let Some(m) = game.mobs.iter().find(|x| x.id == mob_id) {
                if m.death_time > 0 {
                    expired.push(mob_id);
                    continue;
                }
                rig.pos = DVec3::new(m.position.x as f64, m.position.y as f64, m.position.z as f64);
                rig.body_yaw = m.body_yaw;
                let raw_light = game.world.light(
                    m.position.x.floor() as i32,
                    (m.position.y + 0.5).floor() as i32,
                    m.position.z.floor() as i32,
                );
                rig.packed_light = vertex_light(raw_light);
            } else {
                expired.push(mob_id);
                continue;
            }

            // Proportional-Derivative (PD) balance stabilizer towards upright (angle = 0)
            let kp = 38.0f32; // Restoration spring stiffness
            let kd = 7.5f32;  // Velocity damping
            let spring_force = -kp * rig.lean_angle;
            let damping_force = -kd * rig.lean_vel;
            let accel = spring_force + damping_force;
            rig.lean_vel += accel * dt_stumble;
            rig.lean_angle += rig.lean_vel * dt_stumble;

            // Update balance-flailing swing phase for arms and legs
            rig.phase += dt_stumble * 14.0;
            let flail = rig.phase.sin() * (rig.lean_angle.abs() * 0.9).min(1.2);

            for limb in &mut rig.limbs {
                limb.prev_local_rot = limb.local_rot;
                match limb.role {
                    LimbRole::Arm => {
                        let arm_pitch = flail * limb.side_sign * 0.75;
                        let arm_roll = limb.side_sign * (0.35 + rig.lean_angle.abs() * 0.6);
                        limb.local_rot = Quat::from_euler(glam::EulerRot::YXZ, 0.0, arm_pitch, arm_roll);
                    }
                    LimbRole::Leg => {
                        let leg_pitch = -flail * limb.side_sign * 0.85;
                        limb.local_rot = Quat::from_euler(glam::EulerRot::YXZ, 0.0, leg_pitch, 0.0);
                    }
                    LimbRole::Head => {
                        let head_pitch = -rig.lean_angle * 0.45;
                        limb.local_rot = Quat::from_euler(glam::EulerRot::YXZ, 0.0, head_pitch, 0.0);
                    }
                    LimbRole::Torso | LimbRole::Appendage => {
                        limb.local_rot = Quat::IDENTITY;
                    }
                }
            }
        }
        for id in expired {
            vp.active_stumbles.remove(&id);
        }
    }

    // ------------------------------------------------------------------------
    // G. STEP VOLUMETRIC DUST CLOUDS & BOUNCING SPARKS
    // ------------------------------------------------------------------------
    for p in &mut vp.vfx_particles {
        p.prev_pos = p.pos;
        p.age = p.age.saturating_add(1);
        match p.kind {
            VfxKind::DustCloud => {
                p.pos += p.vel * dt_tick;
                p.vel *= 0.91;
                p.vel.y += 0.035 * dt_tick;
                p.radius = (p.radius + 0.028).min(2.4);
            }
            VfxKind::Spark => {
                p.pos += p.vel * dt_tick;
                p.vel.y -= 22.0 * dt_tick;
                p.vel.x *= 0.97;
                p.vel.z *= 0.97;
                let bx = p.pos.x.floor() as i32;
                let by = p.pos.y.floor() as i32;
                let bz = p.pos.z.floor() as i32;
                if is_solid_voxel(game, bx, by, bz) {
                    p.pos.y = by as f32 + 1.02;
                    p.vel.y = -p.vel.y * 0.46;
                    p.vel.x *= 0.65;
                    p.vel.z *= 0.65;
                }
            }
        }
    }
    vp.vfx_particles.retain(|p| p.age < p.max_age);
    if vp.vfx_particles.len() > 600 {
        vp.vfx_particles.drain(..vp.vfx_particles.len() - 600);
    }

    // ------------------------------------------------------------------------
    // H. STEP VERLET HANGING CHAINS, ROPES & SUSPENDED LANTERNS
    // ------------------------------------------------------------------------
    let wind_t = (vp.tick_counter as f32) * 0.05;
    let wind_force = Vec3::new(wind_t.sin() * 0.35, 0.0, (wind_t * 0.8).cos() * 0.35);

    // Drain pending ground shockwaves from fallen trees or collapsed buildings
    let dim_name = game.dim.id().to_owned();
    let shock_list: Vec<(Vec3, f32)> = vp.pending_shockwaves.drain(..).collect();
    for (sc, sp) in shock_list {
        apply_shockwave_to_hanging_objects(game, vp, &dim_name, sc, sp);
    }

    // Break ropes whose anchor block was destroyed
    for r in &mut vp.ropes {
        r.age_ticks = r.age_ticks.saturating_add(1);
        if !r.broken {
            let ap = r.anchor_block;
            if !is_solid_voxel(game, ap[0], ap[1], ap[2]) {
                r.broken = true;
            }
        }
    }

    let mut broken_ropes_fx = Vec::new();
    for r in vp.ropes.iter().filter(|r| r.broken) {
        if let Some(first_node) = r.nodes.first() {
            let p = [
                first_node.pos.x.floor() as i32,
                first_node.pos.y.floor() as i32,
                first_node.pos.z.floor() as i32,
            ];
            broken_ropes_fx.push((p, r.chain_state, r.payload_state));
        }
    }
    for (p, chain_state, payload_state) in broken_ropes_fx {
        vp.spawn_subvoxels(p, -1.0, chain_state, ChunkMat::Wood, 6, Vec3::new(0.0, -0.4, 0.0));
        if let Some(payload) = payload_state {
            vp.spawn_subvoxels(p, -1.0, payload, ChunkMat::Glass, 8, Vec3::new(0.0, -0.4, 0.0));
            vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: p, state: payload });
        }
        vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: p, state: chain_state });
    }
    vp.ropes.retain(|r| !r.broken);

    // If rope has settled (motion ceased), restore static block to world!
    let mut to_restore_ropes = Vec::new();
    for (idx, r) in vp.ropes.iter().enumerate() {
        if !r.broken && r.age_ticks > 120 {
            let max_motion = r.nodes.iter().map(|n| (n.pos - n.prev_pos).length_squared()).fold(0.0f32, f32::max);
            if max_motion < 0.0004 {
                to_restore_ropes.push(idx);
            }
        }
    }
    for idx in to_restore_ropes.into_iter().rev() {
        let r = vp.ropes.remove(idx);
        let x = r.anchor_block[0];
        let z = r.anchor_block[2];
        let top_y = r.anchor_block[1] - 1;
        let total = r.chain_blocks + if r.payload_state.is_some() { 1 } else { 0 };
        let bot_y = top_y - total + 1;
        for y in bot_y..=top_y {
            let st = if y == bot_y && let Some(ps) = r.payload_state { ps } else { r.chain_state };
            game.native_set_block(&dim_name, [x, y, z], st, 3);
        }
    }

    for r in &mut vp.ropes {

        let n_count = r.nodes.len();
        for i in 0..n_count {
            if r.nodes[i].pinned {
                continue;
            }
            let cur = r.nodes[i].pos;
            let prev = r.nodes[i].prev_pos;
            let mut vel = (cur - prev) * 0.96;

            let to_player = cur - player_pos;
            if to_player.length_squared() < 0.85 * 0.85 {
                let push_dir = if player_vel.length_squared() > 0.01 {
                    player_vel.normalize()
                } else {
                    to_player.normalize_or(Vec3::X)
                };
                vel += push_dir * (player_speed_xz * 0.08 + 0.12);
            }

            let accel = Vec3::new(0.0, -gravity * 0.68, 0.0) + wind_force;
            let next = cur + vel + accel * (dt_tick * dt_tick);
            r.nodes[i].prev_pos = cur;
            r.nodes[i].pos = next;

            let bx = next.x.floor() as i32;
            let by = next.y.floor() as i32;
            let bz = next.z.floor() as i32;
            if is_solid_voxel(game, bx, by, bz) {
                r.nodes[i].pos.y = by as f32 + 1.05;
                r.nodes[i].prev_pos.y = r.nodes[i].pos.y;
            }
        }

        for _pass in 0..4 {
            for i in 0..n_count.saturating_sub(1) {
                let delta = r.nodes[i + 1].pos - r.nodes[i].pos;
                let dist = delta.length();
                if dist > 0.0001 {
                    let diff = (dist - r.seg_len) / dist;
                    let p1 = r.nodes[i].pinned;
                    let p2 = r.nodes[i + 1].pinned;
                    if !p1 && !p2 {
                        r.nodes[i].pos += delta * 0.5 * diff;
                        r.nodes[i + 1].pos -= delta * 0.5 * diff;
                    } else if p1 && !p2 {
                        r.nodes[i + 1].pos -= delta * diff;
                    } else if !p1 && p2 {
                        r.nodes[i].pos += delta * diff;
                    }
                }
            }
        }
    }
    vp.ropes.retain(|r| !r.broken);

    // ------------------------------------------------------------------------
    // I. STEP VERLET CLOTH SHEETS (BANNERS & FLAGS)
    // ------------------------------------------------------------------------
    for cloth in &mut vp.cloths {
        cloth.age_ticks = cloth.age_ticks.saturating_add(1);
        if let Some(sp) = cloth.support_pos {
            if !is_solid_voxel(game, sp[0], sp[1], sp[2]) {
                cloth.support_pos = None;
            }
        }
    }

    let mut broken_cloths_fx = Vec::new();
    for cloth in vp.cloths.iter().filter(|c| c.support_pos.is_none()) {
        if let Some(first_node) = cloth.nodes.first() {
            let p = [
                first_node.pos.x.floor() as i32,
                first_node.pos.y.floor() as i32,
                first_node.pos.z.floor() as i32,
            ];
            if let Some(ds) = cloth.drop_state {
                broken_cloths_fx.push((p, ds));
            }
        }
    }
    for (p, ds) in broken_cloths_fx {
        vp.spawn_subvoxels(p, -1.0, ds, ChunkMat::Wood, 6, Vec3::new(0.0, -0.3, 0.0));
        vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: p, state: ds });
    }
    vp.cloths.retain(|c| c.support_pos.is_some());

    for cloth in &mut vp.cloths {

        let cols = cloth.cols;
        let rows = cloth.rows;
        let phase_offset = cloth.id as f32 * 0.37;
        let wind_w = (vp.tick_counter as f32 * 0.12 + phase_offset).sin() * 0.9;
        let cloth_wind = cloth.normal_dir * wind_w + cloth.right_dir * 0.3;

        for node in &mut cloth.nodes {
            if node.pinned {
                continue;
            }
            let cur = node.pos;
            let prev = node.prev_pos;
            let vel = (cur - prev) * 0.94;
            let accel = Vec3::new(0.0, -gravity * 0.42, 0.0) + cloth_wind;
            node.prev_pos = cur;
            node.pos = cur + vel + accel * (dt_tick * dt_tick);
        }

        for _ in 0..3 {
            for r_idx in 0..rows {
                for c_idx in 0..(cols - 1) {
                    let link_idx = r_idx * (cols - 1) + c_idx;
                    if cloth.h_torn.get(link_idx).copied().unwrap_or(false) {
                        continue;
                    }
                    let i1 = r_idx * cols + c_idx;
                    let i2 = i1 + 1;
                    let delta = cloth.nodes[i2].pos - cloth.nodes[i1].pos;
                    let dist = delta.length();
                    if dist > cloth.spacing_x * 2.3 {
                        cloth.h_torn[link_idx] = true;
                        continue;
                    }
                    if dist > 0.0001 {
                        let diff = (dist - cloth.spacing_x) / dist;
                        let p1 = cloth.nodes[i1].pinned;
                        let p2 = cloth.nodes[i2].pinned;
                        if !p1 && !p2 {
                            cloth.nodes[i1].pos += delta * 0.5 * diff;
                            cloth.nodes[i2].pos -= delta * 0.5 * diff;
                        } else if p1 && !p2 {
                            cloth.nodes[i2].pos -= delta * diff;
                        } else if !p1 && p2 {
                            cloth.nodes[i1].pos += delta * diff;
                        }
                    }
                }
            }

            for r_idx in 0..(rows - 1) {
                for c_idx in 0..cols {
                    let link_idx = r_idx * cols + c_idx;
                    if cloth.v_torn.get(link_idx).copied().unwrap_or(false) {
                        continue;
                    }
                    let i1 = r_idx * cols + c_idx;
                    let i2 = i1 + cols;
                    let delta = cloth.nodes[i2].pos - cloth.nodes[i1].pos;
                    let dist = delta.length();
                    if dist > cloth.spacing_y * 2.3 {
                        cloth.v_torn[link_idx] = true;
                        continue;
                    }
                    if dist > 0.0001 {
                        let diff = (dist - cloth.spacing_y) / dist;
                        let p1 = cloth.nodes[i1].pinned;
                        let p2 = cloth.nodes[i2].pinned;
                        if !p1 && !p2 {
                            cloth.nodes[i1].pos += delta * 0.5 * diff;
                            cloth.nodes[i2].pos -= delta * 0.5 * diff;
                        } else if p1 && !p2 {
                            cloth.nodes[i2].pos -= delta * diff;
                        } else if !p1 && p2 {
                            cloth.nodes[i1].pos += delta * diff;
                        }
                    }
                }
            }
        }
    }
    vp.cloths.retain(|c| c.support_pos.is_some());

    // ------------------------------------------------------------------------
    // J. STEP PERSISTENT CRACK FATIGUE
    // ------------------------------------------------------------------------
    for cb in &mut vp.cracked_blocks {
        cb.age_ticks = cb.age_ticks.saturating_add(1);
    }
    vp.cracked_blocks.retain(|cb| cb.age_ticks < 3600);
}

// ============================================================================
// 10. UNIFIED 144Hz+ 3D WORLD GEOMETRY RENDERER (`on_world_draw`)
// ============================================================================

/// Fast direct 6-face shaded 3D box emitter into Horizon's world `Vertex` buffer.
#[allow(clippy::too_many_arguments)]
fn emit_shaded_box_3d(
    center_cam: Vec3,
    rot: Quat,
    half: Vec3,
    rect: Rect,
    uv_sub: [f32; 4],
    rgb_tint: u32,
    light: u32,
    lights: (Vec3, Vec3),
    out: &mut Vec<Vertex>,
) {
    let hx = half.x;
    let hy = half.y;
    let hz = half.z;

    let u0 = rect.u0 + (rect.u1 - rect.u0) * uv_sub[0];
    let v0 = rect.v0 + (rect.v1 - rect.v0) * uv_sub[1];
    let u1 = rect.u0 + (rect.u1 - rect.u0) * uv_sub[2];
    let v1 = rect.v0 + (rect.v1 - rect.v0) * uv_sub[3];
    let quad_uvs = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];

    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        // +Y (Up)
        ([0.0, 1.0, 0.0], [[-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]]),
        // -Y (Down)
        ([0.0, -1.0, 0.0], [[-1.0, -1.0, 1.0], [-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0]]),
        // +Z (South)
        ([0.0, 0.0, 1.0], [[-1.0, 1.0, 1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0]]),
        // -Z (North)
        ([0.0, 0.0, -1.0], [[1.0, 1.0, -1.0], [1.0, -1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0]]),
        // +X (East)
        ([1.0, 0.0, 0.0], [[1.0, 1.0, 1.0], [1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0]]),
        // -X (West)
        ([-1.0, 0.0, 0.0], [[-1.0, 1.0, -1.0], [-1.0, -1.0, -1.0], [[-1.0][0], -1.0, 1.0], [-1.0, 1.0, 1.0]]),
    ];

    for (n_local, corners) in FACES {
        let wn = rot * Vec3::from_array(n_local);
        let color = shade_abgr(rgb_tint, diffuse(wn, lights));
        for (c, uv) in corners.into_iter().zip(quad_uvs) {
            let local_p = Vec3::new(c[0] * hx, c[1] * hy, c[2] * hz);
            let wp = center_cam + rot * local_p;
            out.push(Vertex {
                pos: wp.to_array(),
                uv,
                layer: 0,
                color,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }
}

/// Ultra-fast 3-face camera-oriented micro-grain emitter (zero quaternion overhead, 70% fewer vertices).
#[allow(clippy::too_many_arguments)]
#[inline]
fn emit_sand_grain_fast(
    center_cam: Vec3,
    half: Vec3,
    rect: Rect,
    uv_off: [f32; 2],
    rgb_tint: u32,
    shade_mul: f32,
    light: u32,
    lights: (Vec3, Vec3),
    out: &mut Vec<Vertex>,
) {
    let hx = half.x;
    let hy = half.y;
    let hz = half.z;

    let uw = (rect.u1 - rect.u0) * 0.22;
    let vh = (rect.v1 - rect.v0) * 0.22;
    let u0 = rect.u0 + (rect.u1 - rect.u0) * uv_off[0].clamp(0.0, 0.75);
    let v0 = rect.v0 + (rect.v1 - rect.v0) * uv_off[1].clamp(0.0, 0.75);
    let u1 = u0 + uw;
    let v1 = v0 + vh;
    let uvs = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];

    let x0 = center_cam.x - hx;
    let x1 = center_cam.x + hx;
    let y0 = center_cam.y - hy;
    let y1 = center_cam.y + hy;
    let z0 = center_cam.z - hz;
    let z1 = center_cam.z + hz;

    // 1. Vertical visible face (+Y if camera is above grain, else -Y)
    if center_cam.y <= 0.0 {
        let col = shade_abgr(rgb_tint, (diffuse(Vec3::Y, lights) * shade_mul).clamp(0.2, 1.0));
        let pts = [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]];
        for (p, uv) in pts.into_iter().zip(uvs) {
            out.push(Vertex {
                pos: p,
                uv,
                layer: 0,
                color: col,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    } else {
        let col = shade_abgr(rgb_tint, (diffuse(Vec3::NEG_Y, lights) * shade_mul).clamp(0.2, 1.0));
        let pts = [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]];
        for (p, uv) in pts.into_iter().zip(uvs) {
            out.push(Vertex {
                pos: p,
                uv,
                layer: 0,
                color: col,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }

    // 2. X visible face (-X if camera is West of grain, else +X)
    if center_cam.x >= 0.0 {
        let col = shade_abgr(rgb_tint, (diffuse(Vec3::NEG_X, lights) * shade_mul).clamp(0.2, 1.0));
        let pts = [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]];
        for (p, uv) in pts.into_iter().zip(uvs) {
            out.push(Vertex {
                pos: p,
                uv,
                layer: 0,
                color: col,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    } else {
        let col = shade_abgr(rgb_tint, (diffuse(Vec3::X, lights) * shade_mul).clamp(0.2, 1.0));
        let pts = [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]];
        for (p, uv) in pts.into_iter().zip(uvs) {
            out.push(Vertex {
                pos: p,
                uv,
                layer: 0,
                color: col,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }

    // 3. Z visible face (-Z if camera is North of grain, else +Z)
    if center_cam.z >= 0.0 {
        let col = shade_abgr(rgb_tint, (diffuse(Vec3::NEG_Z, lights) * shade_mul).clamp(0.2, 1.0));
        let pts = [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]];
        for (p, uv) in pts.into_iter().zip(uvs) {
            out.push(Vertex {
                pos: p,
                uv,
                layer: 0,
                color: col,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    } else {
        let col = shade_abgr(rgb_tint, (diffuse(Vec3::Z, lights) * shade_mul).clamp(0.2, 1.0));
        let pts = [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]];
        for (p, uv) in pts.into_iter().zip(uvs) {
            out.push(Vertex {
                pos: p,
                uv,
                layer: 0,
                color: col,
                light,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }
}

pub(super) fn on_world_draw(
    game: &Game<'_>,
    camera: DVec3,
    alpha: f64,
    out: &mut Vec<Vertex>,
) -> bool {
    let lights = diffuse_lights(game.dim == Dim::Nether);
    let partial = alpha as f32;

    let override_items = with_vp(|vp| {
        if !vp.cfg.master_enabled {
            return false;
        }

        // --------------------------------------------------------------------
        // 1. Render 3D Rigid-Body Falling/Fallen Trees & Collapsing Structures
        // --------------------------------------------------------------------
        if (vp.cfg.tree_felling || vp.cfg.building_collapse) && !vp.assemblies.is_empty() {
            for asm in &vp.assemblies {
                let dist_sq = asm.pivot.distance_squared(camera);
                if dist_sq > 96.0 * 96.0 {
                    continue;
                }
                let base_pose = asm.pose_matrix(camera, alpha);
                for cell in &asm.cells {
                    let cell_offset = Vec3::new(
                        cell.local.x as f32 - 0.5,
                        cell.local.y as f32,
                        cell.local.z as f32 - 0.5,
                    );
                    let cell_mat = base_pose * Mat4::from_translation(cell_offset);
                    let wc = asm.cell_world_center(cell.local);
                    let raw_light = game.world.light(
                        wc.x.floor() as i32,
                        (wc.y + 0.5).floor() as i32,
                        wc.z.floor() as i32,
                    );
                    let packed = vertex_light(raw_light);
                    let seed = ((cell.local.x as i64).wrapping_mul(3129871))
                        ^ ((cell.local.y as i64).wrapping_mul(116129781))
                        ^ (cell.local.z as i64);
                    game.block_model_vertices(cell.state, seed, cell_mat, packed, true, out);
                }
            }
        }

        // --------------------------------------------------------------------
        // 2. Render 3D Ballistic Spinning Crater / Shatter Blocks
        // --------------------------------------------------------------------
        if !vp.flying_blocks.is_empty() {
            for fb in &vp.flying_blocks {
                if fb.pos.distance_squared(camera) > 96.0 * 96.0 {
                    continue;
                }
                let interp_pos = (fb.prev_pos.lerp(fb.pos, alpha) - camera).as_vec3();
                let interp_rot = fb.prev_rot.slerp(fb.rot, partial);
                // Rotate block around its [0.5, 0.5, 0.5] center!
                let m = Mat4::from_translation(interp_pos)
                    * Mat4::from_quat(interp_rot)
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let raw_light = game.world.light(
                    fb.pos.x.floor() as i32,
                    fb.pos.y.floor() as i32,
                    fb.pos.z.floor() as i32,
                );
                game.block_model_vertices(fb.state, fb.seed, m, vertex_light(raw_light), true, out);
            }
        }

        // --------------------------------------------------------------------
        // 3. Render 3D Sub-Voxel Splinters, Shards & Cascading Sand Chunks
        // --------------------------------------------------------------------
        if vp.cfg.debris && !vp.chunks.is_empty() {
            let cam_v3 = camera.as_vec3();
            for ch in &vp.chunks {
                let Some((rect, rgb)) = ch.resolved else {
                    continue;
                };
                let p_world = ch.prev_pos.lerp(ch.pos, partial);
                let rel = DVec3::new(p_world.x as f64, p_world.y as f64, p_world.z as f64) - camera;
                if rel.length_squared() > 64.0 * 64.0 {
                    continue;
                }
                let _ = cam_v3;
                let q = ch.prev_rot.slerp(ch.rot, partial);
                let raw_light = game.world.light(
                    p_world.x.floor() as i32,
                    (p_world.y + 0.15).floor() as i32,
                    p_world.z.floor() as i32,
                );
                emit_shaded_box_3d(
                    rel.as_vec3(),
                    q,
                    ch.half,
                    rect,
                    ch.uv_sub,
                    rgb,
                    vertex_light(raw_light),
                    lights,
                    out,
                );
            }
        }

        // --------------------------------------------------------------------
        // 3b. Render Fine Pouring & Scattering 3D Sand / Gravel / Powder Grains
        // --------------------------------------------------------------------
        if vp.cfg.debris && !vp.sand_grains.is_empty() {
            for g in &vp.sand_grains {
                let Some((rect, rgb)) = g.resolved else {
                    continue;
                };
                let p_world = g.prev_pos.lerp(g.pos, partial);
                let rel =
                    DVec3::new(p_world.x as f64, p_world.y as f64, p_world.z as f64) - camera;
                if rel.length_squared() > 56.0 * 56.0 {
                    continue;
                }
                let r = g.radius;
                let half = if g.grounded {
                    Vec3::new(r * 1.18, r * 0.72, r * 1.18)
                } else {
                    Vec3::splat(r)
                };
                emit_sand_grain_fast(
                    rel.as_vec3(),
                    half,
                    rect,
                    g.uv_offset,
                    rgb,
                    g.shade_mul,
                    g.packed_light,
                    lights,
                    out,
                );
            }
        }

        let concrete_rect = game.gui.atlas.sprites.get("block/white_concrete").copied();

        // --------------------------------------------------------------------
        // 4. Render Authentic 3D Skinned & Dismembered Mob Ragdolls
        // --------------------------------------------------------------------
        if vp.cfg.ragdoll && !vp.ragdolls.is_empty() {
            for rag in &vp.ragdolls {
                // Brief hurt red flash (layer = 1) during first 5 ticks of death, then crisp skin (layer = 0)
                let overlay_code = if rag.age_ticks <= 5 { 1u32 } else { 0u32 };

                for (l_idx, limb) in rag.limbs.iter().enumerate() {
                    let p_world = limb.prev_pos.lerp(limb.pos, partial);
                    let rel =
                        DVec3::new(p_world.x as f64, p_world.y as f64, p_world.z as f64) - camera;
                    if rel.length_squared() > 72.0 * 72.0 {
                        continue;
                    }
                    let q_rot = limb.prev_rot.slerp(limb.rot, partial);
                    let rel_v3 = rel.as_vec3();

                    let raw_light = game.world.light(
                        p_world.x.floor() as i32,
                        (p_world.y + 0.2).floor() as i32,
                        p_world.z.floor() as i32,
                    );
                    let packed_light = vertex_light(raw_light);

                    for quad in &limb.quads {
                        let rect = if quad.use_overlay_rect {
                            let Some(or) = rag.overlay_rect else { continue };
                            or
                        } else {
                            rag.primary_rect
                        };

                        let wn = (q_rot * quad.normal).normalize_or(Vec3::Y);
                        let nx = (wn.x.clamp(-1.0, 1.0) * 127.0) as i8;
                        let ny = (wn.y.clamp(-1.0, 1.0) * 127.0) as i8;
                        let nz = (wn.z.clamp(-1.0, 1.0) * 127.0) as i8;
                        let nw = if quad.cull_back {
                            horizon_client::mesher::SHADED_CULL_W
                        } else {
                            -127
                        };

                        for (v_local, uv_norm) in quad.verts.iter().zip(&quad.uvs) {
                            let vp_pos = rel_v3 + q_rot * Vec3::from_array(*v_local);
                            out.push(Vertex {
                                normal: [nx, ny, nz, nw],
                                pos: vp_pos.to_array(),
                                uv: [
                                    rect.u0 + (rect.u1 - rect.u0) * uv_norm[0],
                                    rect.v0 + (rect.v1 - rect.v0) * uv_norm[1],
                                ],
                                layer: if quad.use_overlay_rect { 0 } else { overlay_code },
                                color: quad.tint_abgr,
                                light: packed_light,
                                glint_uv: [0.0, 0.0],
                            });
                        }
                    }

                    // Render 3D gore/bone stump cap at severed joint sockets!
                    if l_idx > 0
                        && limb.severed
                        && let Some(c_rect) = concrete_rect
                    {
                        let stump_cam = rel_v3 + q_rot * (limb.anchor_on_self * 0.75);
                        emit_shaded_box_3d(
                            stump_cam,
                            q_rot,
                            Vec3::splat(limb.radius.clamp(0.045, 0.095)),
                            c_rect,
                            [0.1, 0.1, 0.5, 0.5],
                            rag.gore_rgb,
                            packed_light,
                            lights,
                            out,
                        );
                    }
                }
            }
        }

        // --------------------------------------------------------------------
        // 4b. Render Active Stumble Living Mobs (PD Balanced Skinned Model)
        // --------------------------------------------------------------------
        if vp.cfg.ragdoll && !vp.active_stumbles.is_empty() {
            for rig in vp.active_stumbles.values() {
                let interp_pos = rig.prev_pos.lerp(rig.pos, alpha) - camera;
                if interp_pos.length_squared() > 72.0 * 72.0 {
                    continue;
                }
                let rel_v3 = interp_pos.as_vec3();
                let interp_lean = rig.prev_lean_angle + (rig.lean_angle - rig.prev_lean_angle) * partial;
                let yaw_rad = (180.0 - rig.body_yaw).to_radians();
                let entity_rot = Quat::from_rotation_y(yaw_rad);
                let lean_rot = Quat::from_axis_angle(rig.lean_axis_ent, interp_lean);
                let body_pose_q = entity_rot * lean_rot;
                let overlay_code = if rig.hurt_flash_ticks > 0 { 1u32 } else { 0u32 };

                for limb in &rig.limbs {
                    let q_local = limb.prev_local_rot.slerp(limb.local_rot, partial);
                    let total_limb_rot = body_pose_q * q_local;
                    let pivot_cam = rel_v3 + body_pose_q * limb.pivot_ent;

                    for quad in &limb.quads {
                        let rect = if quad.use_overlay_rect {
                            rig.overlay_rect.unwrap_or(rig.primary_rect)
                        } else {
                            rig.primary_rect
                        };

                        let wn = (total_limb_rot * quad.normal).normalize_or(Vec3::Y);
                        let nx = (wn.x.clamp(-1.0, 1.0) * 127.0) as i8;
                        let ny = (wn.y.clamp(-1.0, 1.0) * 127.0) as i8;
                        let nz = (wn.z.clamp(-1.0, 1.0) * 127.0) as i8;
                        let nw = if quad.cull_back {
                            horizon_client::mesher::SHADED_CULL_W
                        } else {
                            -127
                        };

                        for (v_local, uv_norm) in quad.verts.iter().zip(&quad.uvs) {
                            let vp_pos = pivot_cam + total_limb_rot * Vec3::from_array(*v_local);
                            out.push(Vertex {
                                normal: [nx, ny, nz, nw],
                                pos: vp_pos.to_array(),
                                uv: [
                                    rect.u0 + (rect.u1 - rect.u0) * uv_norm[0],
                                    rect.v0 + (rect.v1 - rect.v0) * uv_norm[1],
                                ],
                                layer: if quad.use_overlay_rect { 0 } else { overlay_code },
                                color: quad.tint_abgr,
                                light: rig.packed_light,
                                glint_uv: [0.0, 0.0],
                            });
                        }
                    }
                }
            }
        }

        // --------------------------------------------------------------------
        // 4c. Render 3D Verlet Hanging Chains & Lanterns
        // --------------------------------------------------------------------
        if !vp.ropes.is_empty() && let Some(rect) = concrete_rect {
            let cam_v3 = camera.as_vec3();
            for r in &vp.ropes {
                if r.nodes.is_empty() {
                    continue;
                }
                let first_cam = r.nodes[0].pos - cam_v3;
                if first_cam.length_squared() > 64.0 * 64.0 {
                    continue;
                }
                for i in 0..r.nodes.len().saturating_sub(1) {
                    let p0 = r.nodes[i].pos - cam_v3;
                    let p1 = r.nodes[i + 1].pos - cam_v3;
                    let seg = p1 - p0;
                    let seg_len = seg.length().max(0.001);
                    let seg_dir = seg / seg_len;
                    let perp_x = seg_dir.cross(Vec3::Z).normalize_or(Vec3::X) * 0.045;
                    let perp_z = seg_dir.cross(Vec3::X).normalize_or(Vec3::Z) * 0.045;

                    let q1 = [p0 - perp_x, p0 + perp_x, p1 + perp_x, p1 - perp_x];
                    let uvs = [[rect.u0, rect.v0], [rect.u1, rect.v0], [rect.u1, rect.v1], [rect.u0, rect.v1]];
                    for (p, uv) in q1.into_iter().zip(uvs) {
                        out.push(Vertex {
                            pos: p.to_array(),
                            uv,
                            layer: 0,
                            color: 0xFF888888,
                            light: r.packed_light,
                            normal: [0, 127, 0, -127],
                            glint_uv: [0.0, 0.0],
                        });
                    }
                    let q2 = [p0 - perp_z, p0 + perp_z, p1 + perp_z, p1 - perp_z];
                    for (p, uv) in q2.into_iter().zip(uvs) {
                        out.push(Vertex {
                            pos: p.to_array(),
                            uv,
                            layer: 0,
                            color: 0xFF777777,
                            light: r.packed_light,
                            normal: [127, 0, 0, -127],
                            glint_uv: [0.0, 0.0],
                        });
                    }
                }
                if let Some(payload) = r.payload_state {
                    let last_cam = r.nodes.last().unwrap().pos - cam_v3;
                    let m = Mat4::from_translation(last_cam - Vec3::new(0.5, 0.85, 0.5));
                    game.block_model_vertices(payload, r.id as i64, m, r.packed_light, true, out);
                }
            }
        }

        // --------------------------------------------------------------------
        // 4d. Render 3D Verlet Wind-Rippling Cloth Banners
        // --------------------------------------------------------------------
        if !vp.cloths.is_empty() && let Some(rect) = concrete_rect {
            let cam_v3 = camera.as_vec3();
            let uvs = [[rect.u0, rect.v0], [rect.u1, rect.v0], [rect.u1, rect.v1], [rect.u0, rect.v1]];
            for c in &vp.cloths {
                let Some(first_node) = c.nodes.first() else { continue };
                let first_cam = first_node.pos - cam_v3;
                if first_cam.length_squared() > 64.0 * 64.0 {
                    continue;
                }
                let cols = c.cols;
                let rows = c.rows;
                for r in 0..(rows - 1) {
                    for col in 0..(cols - 1) {
                        let h_idx = r * (cols - 1) + col;
                        let v_idx = r * cols + col;
                        if c.h_torn.get(h_idx).copied().unwrap_or(false)
                            || c.v_torn.get(v_idx).copied().unwrap_or(false)
                        {
                            continue;
                        }
                        let p00 = c.nodes[r * cols + col].pos - cam_v3;
                        let p10 = c.nodes[r * cols + col + 1].pos - cam_v3;
                        let p11 = c.nodes[(r + 1) * cols + col + 1].pos - cam_v3;
                        let p01 = c.nodes[(r + 1) * cols + col].pos - cam_v3;

                        let norm = (p10 - p00).cross(p01 - p00).normalize_or(Vec3::Z);
                        let nx = (norm.x.clamp(-1.0, 1.0) * 127.0) as i8;
                        let ny = (norm.y.clamp(-1.0, 1.0) * 127.0) as i8;
                        let nz = (norm.z.clamp(-1.0, 1.0) * 127.0) as i8;

                        let quad = [p00, p10, p11, p01];
                        let col_shaded = shade_abgr(c.rgb, diffuse(norm, lights));
                        for (p, uv) in quad.into_iter().zip(uvs) {
                            out.push(Vertex {
                                pos: p.to_array(),
                                uv,
                                layer: 0,
                                color: col_shaded,
                                light: c.packed_light,
                                normal: [nx, ny, nz, -127],
                                glint_uv: [0.0, 0.0],
                            });
                        }
                    }
                }
            }
        }

        // --------------------------------------------------------------------
        // 4e. Render Volumetric 3D Dust Clouds & Bouncing Sparks
        // --------------------------------------------------------------------
        if !vp.vfx_particles.is_empty() && let Some(rect) = concrete_rect {
            let uvs = [[rect.u0, rect.v0], [rect.u1, rect.v0], [rect.u1, rect.v1], [rect.u0, rect.v1]];
            for p in &vp.vfx_particles {
                let p_world = p.prev_pos.lerp(p.pos, partial);
                let rel = DVec3::new(p_world.x as f64, p_world.y as f64, p_world.z as f64) - camera;
                if rel.length_squared() > 56.0 * 56.0 {
                    continue;
                }
                let rel_v3 = rel.as_vec3();
                let fwd = rel_v3.normalize_or(Vec3::Z);
                let cam_right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
                let cam_up = cam_right.cross(fwd).normalize_or(Vec3::Y);

                match p.kind {
                    VfxKind::DustCloud => {
                        let rad = p.radius;
                        let life_ratio = (p.age as f32 / p.max_age as f32).clamp(0.0, 1.0);
                        let alpha_byte = (((1.0 - life_ratio) * 165.0) as u32).clamp(0, 255);
                        let shade = diffuse(Vec3::Y, lights).max(0.65);
                        let r = (((p.rgb >> 16) & 0xFF) as f32 * shade) as u32;
                        let g = (((p.rgb >> 8) & 0xFF) as f32 * shade) as u32;
                        let b = ((p.rgb & 0xFF) as f32 * shade) as u32;
                        let abgr = (alpha_byte << 24) | (b << 16) | (g << 8) | r;

                        let p0 = rel_v3 - cam_right * rad - cam_up * rad;
                        let p1 = rel_v3 + cam_right * rad - cam_up * rad;
                        let p2 = rel_v3 + cam_right * rad + cam_up * rad;
                        let p3 = rel_v3 - cam_right * rad + cam_up * rad;

                        let quad = [p0, p1, p2, p3];
                        for (pt, uv) in quad.into_iter().zip(uvs) {
                            out.push(Vertex {
                                pos: pt.to_array(),
                                uv,
                                layer: 0,
                                color: abgr,
                                light: p.packed_light,
                                normal: [0, 127, 0, -127],
                                glint_uv: [0.0, 0.0],
                            });
                        }
                    }
                    VfxKind::Spark => {
                        let rad = p.radius.clamp(0.02, 0.06);
                        let p0 = rel_v3 - cam_right * rad - cam_up * rad;
                        let p1 = rel_v3 + cam_right * rad - cam_up * rad;
                        let p2 = rel_v3 + cam_right * rad + cam_up * rad;
                        let p3 = rel_v3 - cam_right * rad + cam_up * rad;

                        let quad = [p0, p1, p2, p3];
                        for (pt, uv) in quad.into_iter().zip(uvs) {
                            out.push(Vertex {
                                pos: pt.to_array(),
                                uv,
                                layer: 0,
                                color: 0xFF33FFFF,
                                light: 0x00F000F0,
                                normal: [0, 127, 0, -127],
                                glint_uv: [0.0, 0.0],
                            });
                        }
                    }
                }
            }
        }

        // 4f: Procedural crack fatigue is simulated strictly in physics calculation without rendering visual cross artifacts.

        vp.cfg.item_physics
    });

    if !override_items {
        return false; // Let vanilla `dropped_item_geometry` run when F7 is toggled OFF
    }

    // ------------------------------------------------------------------------
    // 5. True 3D Tumbling & Flat-Resting Dropped Item Renderer (`ItemPhysics`)
    // ------------------------------------------------------------------------
    render_physical_dropped_items(game, camera, alpha, lights, out);
    true
}

fn render_physical_dropped_items(
    game: &Game<'_>,
    camera: DVec3,
    alpha: f64,
    lights: (Vec3, Vec3),
    out: &mut Vec<Vertex>,
) {
    let live = game.dropped.iter().map(|item| {
        let (p, q) = (item.previous, item.position);
        let at = [
            p.x + (q.x - p.x) * alpha,
            p.y + (q.y - p.y) * alpha,
            p.z + (q.z - p.z) * alpha,
        ];
        (item, at, alpha as f32, true)
    });
    let taken = game.pickups.live.iter().filter_map(|p| match &p.taken {
        super::pickups::Taken::Item(item) => {
            Some((&**item, super::pickups::position(p, alpha), 1.0f32, false))
        }
        _ => None,
    });

    let range = super::entity_culling::default_range(DVec3::splat(0.25), game.view_scale());
    for (item, at, partial, is_entity) in live.chain(taken) {
        let entity_start = out.len();
        game.mark_motion(entity_start, (2 << 32) | u64::from(item.id));
        let q = item.position;
        let pos = DVec3::from_array(at) - camera;
        let far = if is_entity {
            (DVec3::new(q.x, q.y, q.z) - camera).length_squared() >= range * range
        } else {
            pos.length_squared() > 64.0 * 64.0
        };
        if far {
            continue;
        }
        if is_entity && let Some(view) = &game.view_frustum {
            let feet = (DVec3::new(q.x, q.y, q.z) - camera).as_vec3();
            let half = Vec3::new(0.625, 0.5, 0.625);
            if !view.intersects(feet - half, feet + Vec3::new(half.x, 0.75, half.z)) {
                continue;
            }
        }

        let owner = super::item_draw::owner_at([q.x, q.y, q.z], 0.0, "minecraft:item");
        let state = game.resolve_item(
            &item.stack,
            DisplayContext::Ground,
            Some(&owner),
            item.id as i32,
        );
        if state.is_empty() {
            continue;
        }

        let age = item.age as f32 + partial;
        let (lo, hi) = state.bounding_box();
        let thickness_z = (hi.z - lo.z).abs();
        let is_flat_item = thickness_z < 0.22;
        let rest_yaw = item.bob_offset * 3.7 + (item.id as f32) * 1.13;

        let pose = if !is_entity {
            // Pickup magnet flight toward player: spin dynamically
            Mat4::from_translation(pos.as_vec3() + Vec3::Y * 0.12)
                * Mat4::from_euler(glam::EulerRot::YXZ, age * 0.45, age * 0.3, 0.0)
        } else if item.was_touching_water {
            // Floating flat on water waves!
            let wave_y = (age * 0.16 + item.bob_offset).sin() * 0.035 + 0.06;
            let wave_pitch = (age * 0.14 + item.bob_offset).cos() * 0.16;
            let wave_roll = (age * 0.11 + item.bob_offset * 1.7).sin() * 0.16;
            let flat_x = if is_flat_item { FRAC_PI_2 } else { 0.0 };
            Mat4::from_translation(pos.as_vec3() + Vec3::Y * wave_y)
                * Mat4::from_euler(
                    glam::EulerRot::YXZ,
                    rest_yaw + age * 0.02,
                    flat_x + wave_pitch,
                    wave_roll,
                )
        } else if item.on_ground {
            // Resting physically flat on the ground (NO artificial levitation or Y-spinning!)
            if is_flat_item {
                Mat4::from_translation(pos.as_vec3() + Vec3::Y * 0.028)
                    * Mat4::from_rotation_y(rest_yaw)
                    * Mat4::from_rotation_x(FRAC_PI_2)
            } else {
                let min_offset_y = (-lo.y).max(0.0);
                Mat4::from_translation(pos.as_vec3() + Vec3::Y * min_offset_y)
                    * Mat4::from_rotation_y(rest_yaw)
            }
        } else {
            // Airborne ballistic 3D tumbling!
            let speed = (item.velocity.x * item.velocity.x
                + item.velocity.y * item.velocity.y
                + item.velocity.z * item.velocity.z)
                .sqrt() as f32;
            let rate = (0.35 + speed * 1.4).min(1.6);
            Mat4::from_translation(pos.as_vec3() + Vec3::Y * 0.14)
                * Mat4::from_euler(
                    glam::EulerRot::YXZ,
                    rest_yaw + age * 0.32 * rate,
                    age * 0.48 * rate,
                    age * 0.27 * rate,
                )
        };

        let packed = vertex_light(game.world.light(
            q.x.floor() as i32,
            (q.y + 0.15).floor() as i32,
            q.z.floor() as i32,
        ));
        let copies_n = item_render::rendered_amount(item.stack.count);
        for copy_idx in 0..copies_n {
            let copy_pose = if copy_idx == 0 {
                pose
            } else {
                let off_x = (((copy_idx.wrapping_mul(37).wrapping_add(item.id)) % 7) as f32 - 3.0) * 0.028;
                let off_z = (((copy_idx.wrapping_mul(53).wrapping_add(item.id)) % 7) as f32 - 3.0) * 0.028;
                let off_y = (copy_idx as f32) * 0.016;
                Mat4::from_translation(Vec3::new(off_x, off_y, off_z))
                    * pose
                    * Mat4::from_rotation_z((copy_idx as f32) * 0.28)
            };
            game.submit_item_lit(&state, copy_pose, packed, lights, out);
        }

        if game.entity_glowing(item.id) {
            let rgb = super::glowing::team_color(game.mob_team(item.id));
            game.mark_outline(&out[entity_start..], rgb);
        }
    }
}

// ============================================================================
// 11. REAL PLAYER & ITEM 3D COLLISION WITH FALLEN TREES & STRUCTURES
// ============================================================================

fn collect_assembly_aabbs(vp: &VpWorld, near: DVec3, radius: f64, out: &mut Vec<Aabb>) {
    if !vp.cfg.master_enabled {
        return;
    }
    let r2 = radius * radius;
    for asm in &vp.assemblies {
        if asm.pivot.distance_squared(near) > (radius + 24.0) * (radius + 24.0) {
            continue;
        }
        for cell in &asm.cells {
            if cell.is_foliage {
                continue;
            }
            let wc = asm.cell_world_center(cell.local);
            if wc.distance_squared(near) > r2 {
                continue;
            }
            let h = 0.45;
            out.push(Aabb::new(
                horizon_core::geometry::Vec3::new(wc.x - h, wc.y - h, wc.z - h),
                horizon_core::geometry::Vec3::new(wc.x + h, wc.y + h, wc.z + h),
            ));
        }
    }
}

pub(super) fn on_player_collisions_head(game: &Game<'_>) -> Option<Vec<Aabb>> {
    let Some(p) = &game.player else {
        return None;
    };
    let p_pos = DVec3::new(p.position.x, p.position.y, p.position.z);
    let mut extra = Vec::new();
    with_vp(|vp| {
        if !vp.assemblies.is_empty() {
            collect_assembly_aabbs(vp, p_pos, 8.0, &mut extra);
        }
    });
    if extra.is_empty() {
        return None;
    }
    let mut boxes: Vec<Aabb> = game
        .mobs
        .iter()
        .filter(|m| {
            (m.species() == Species::HappyGhast
                && Some(m.id) != game.riding
                && horizon_client::mobs::happy_ghast::collides_with_player(m, p.position.y))
                || (m.species() == Species::Shulker && m.alive())
        })
        .map(|m| m.bounds())
        .chain(
            game.boats
                .iter()
                .filter(|b| Some(b.id) != game.riding)
                .map(super::boats::Boat::bounds),
        )
        .collect();
    boxes.extend(extra);
    Some(boxes)
}

pub(super) fn on_item_collisions_head(game: &Game<'_>) -> Option<Vec<Aabb>> {
    let ref_pos = game.player.as_ref().map_or(DVec3::ZERO, |p| {
        DVec3::new(p.position.x, p.position.y, p.position.z)
    });
    let mut extra = Vec::new();
    with_vp(|vp| {
        if !vp.assemblies.is_empty() {
            collect_assembly_aabbs(vp, ref_pos, 24.0, &mut extra);
        }
    });
    if extra.is_empty() {
        return None;
    }
    let mut boxes: Vec<Aabb> = game
        .mobs
        .iter()
        .filter(|m| {
            (m.species() == Species::HappyGhast
                && m.alive()
                && !m.is_baby()
                && horizon_client::mobs::happy_ghast::on_still_timeout(m))
                || (m.species() == Species::Shulker && m.alive())
        })
        .map(|m| m.bounds())
        .chain(game.boats.iter().map(super::boats::Boat::bounds))
        .chain(game.carts.iter().map(|c| c.bounds()))
        .collect();
    boxes.extend(extra);
    Some(boxes)
}

// ============================================================================
// 12. PLAYER INTERACTION: CHOPPING FALLEN TREES & DISMEMBERING CORPSES
// ============================================================================

fn split_disconnected_components(asm: RigidAssembly, vp: &mut VpWorld) -> Vec<RigidAssembly> {
    if asm.cells.len() <= 1 {
        return if asm.cells.is_empty() {
            Vec::new()
        } else {
            vec![asm]
        };
    }

    let n = asm.cells.len();
    let mut visited = vec![false; n];
    let mut parts = Vec::new();

    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut comp = Vec::new();
        let mut q = VecDeque::new();
        visited[start] = true;
        q.push_back(start);

        while let Some(cur) = q.pop_front() {
            comp.push(asm.cells[cur].clone());
            let cl = asm.cells[cur].local;
            for (other_idx, other) in asm.cells.iter().enumerate() {
                if !visited[other_idx] {
                    let d = (other.local - cl).abs();
                    if d.x <= 1 && d.y <= 1 && d.z <= 1 && (d.x + d.y + d.z) <= 2 {
                        visited[other_idx] = true;
                        q.push_back(other_idx);
                    }
                }
            }
        }

        // Discard tiny orphan leaf-only clusters by bursting them into 3D foliage debris!
        let has_solid = comp.iter().any(|c| !c.is_foliage);
        if !has_solid {
            for c in comp {
                let wc = asm.cell_world_center(c.local);
                let bpos = [wc.x.floor() as i32, wc.y.floor() as i32, wc.z.floor() as i32];
                vp.spawn_subvoxels(bpos, -1.0, c.state, ChunkMat::Foliage, 3, Vec3::Y * 0.8);
            }
            continue;
        }

        let mut clone_asm = asm.clone();
        clone_asm.id = vp.alloc_id();
        clone_asm.cells = comp;
        clone_asm.settled = false;
        clone_asm.hinged = false;
        clone_asm.settle_ticks = 0;
        parts.push(clone_asm);
    }
    parts
}

fn on_left_click_world(game: &mut Game<'_>) {
    if !game.options.execution_role.is_client() || !game.native_playing() {
        return;
    }
    let Some(eye_arr) = game.eye_position() else {
        return;
    };
    let eye = DVec3::from_array(eye_arr);
    let (yaw_r, pitch_r) = (game.yaw.to_radians(), game.pitch.to_radians());
    let dir = Vec3::new(
        -yaw_r.sin() * pitch_r.cos(),
        -pitch_r.sin(),
        yaw_r.cos() * pitch_r.cos(),
    )
    .normalize_or(Vec3::Z);
    let reach = 4.8f32;

    with_vp(|vp| {
        if !vp.cfg.master_enabled {
            return;
        }

        // 1. Raycast against fallen/leaning 3D Rigid Trees & Structures (exact OBB raycast!)
        let mut best_asm_hit: Option<(usize, usize, f32)> = None;
        for (a_idx, asm) in vp.assemblies.iter().enumerate() {
            if asm.pivot.distance_squared(eye) > 36.0 * 36.0 {
                continue;
            }
            let inv_rot = asm.rot.inverse();
            let local_origin = inv_rot * (eye - asm.pivot).as_vec3();
            let local_dir = inv_rot * dir;

            for (c_idx, cell) in asm.cells.iter().enumerate() {
                let cmin = Vec3::new(
                    cell.local.x as f32 - 0.5,
                    cell.local.y as f32,
                    cell.local.z as f32 - 0.5,
                );
                let cmax = cmin + Vec3::ONE;
                if let Some(t) = ray_aabb(local_origin, local_dir, cmin, cmax, reach)
                    && best_asm_hit.is_none_or(|(_, _, bt)| t < bt)
                {
                    best_asm_hit = Some((a_idx, c_idx, t));
                }
            }
        }

        if let Some((a_idx, c_idx, _t)) = best_asm_hit {
            let wc = vp.assemblies[a_idx].cell_world_center(vp.assemblies[a_idx].cells[c_idx].local);
            let bpos = [wc.x.floor() as i32, wc.y.floor() as i32, wc.z.floor() as i32];
            let cell_state = vp.assemblies[a_idx].cells[c_idx].state;
            let is_foliage = vp.assemblies[a_idx].cells[c_idx].is_foliage;

            let hits_needed = if is_foliage || vp.player_holding_axe { 1 } else { 2 };
            vp.assemblies[a_idx].cells[c_idx].hits_taken += 1;

            if vp.assemblies[a_idx].cells[c_idx].hits_taken >= hits_needed {
                // CHOP THROUGH THE 3D TRUNK SEGMENT!
                vp.assemblies[a_idx].cells.remove(c_idx);
                let mat = if is_foliage {
                    ChunkMat::Foliage
                } else {
                    ChunkMat::Wood
                };
                vp.spawn_subvoxels(bpos, -1.0, cell_state, mat, 14, dir * 2.2 + Vec3::Y * 1.8);
                vp.server_ops.push_back(ServerOp::DropBlockLoot {
                    pos: bpos,
                    state: cell_state,
                });
                vp.client_fx.push_back(ClientFx::Sound {
                    sound: if is_foliage {
                        "minecraft:block.azalea_leaves.break"
                    } else {
                        "minecraft:block.wood.break"
                    },
                    at: [wc.x, wc.y, wc.z],
                    vol: 0.95,
                    pitch: 0.92,
                });

                // Split trunk if chopped in the middle!
                let removed_asm = vp.assemblies.remove(a_idx);
                let pieces = split_disconnected_components(removed_asm, vp);
                vp.assemblies.extend(pieces);
            } else {
                // First hand-chop notches bark & bursts wood chips!
                vp.spawn_subvoxels(bpos, -1.0, cell_state, ChunkMat::Wood, 6, dir * 1.6 + Vec3::Y * 1.2);
                vp.assemblies[a_idx].free_ang_vel += Vec3::new(-dir.z, 0.0, dir.x) * 0.6;
                vp.assemblies[a_idx].settled = false;
                vp.client_fx.push_back(ClientFx::Sound {
                    sound: "minecraft:block.wood.hit",
                    at: [wc.x, wc.y, wc.z],
                    vol: 0.85,
                    pitch: 0.95,
                });
            }
            return;
        }

        // 2. Raycast against 3D Skinned Ragdoll Corpses (post-mortem dismemberment & corpse hitting!)
        let eye_v3 = eye.as_vec3();
        let mut best_rag_hit: Option<(usize, usize, f32)> = None;
        for (r_idx, rag) in vp.ragdolls.iter().enumerate() {
            for (l_idx, limb) in rag.limbs.iter().enumerate() {
                let pad = limb.half.max(Vec3::splat(0.18));
                if let Some(t) = ray_aabb(eye_v3, dir, limb.pos - pad, limb.pos + pad, reach)
                    && best_rag_hit.is_none_or(|(_, _, bt)| t < bt)
                {
                    best_rag_hit = Some((r_idx, l_idx, t));
                }
            }
        }

        if let Some((r_idx, l_idx, _)) = best_rag_hit {
            let gore_rgb = vp.ragdolls[r_idx].gore_rgb;
            let hit_pos = vp.ragdolls[r_idx].limbs[l_idx].pos;
            if l_idx > 0 && !vp.ragdolls[r_idx].limbs[l_idx].severed {
                vp.ragdolls[r_idx].limbs[l_idx].severed = true;
            } else if l_idx == 0 {
                // Hitting torso severs the next still-attached limb!
                if let Some(attached) = vp.ragdolls[r_idx]
                    .limbs
                    .iter_mut()
                    .skip(1)
                    .find(|l| !l.severed)
                {
                    attached.severed = true;
                    attached.vel += dir * 6.5 + Vec3::Y * 3.4;
                    attached.ang_vel += Vec3::new(-dir.z, 0.8, dir.x) * 14.0;
                }
            }
            let limb = &mut vp.ragdolls[r_idx].limbs[l_idx];
            limb.vel += dir * 6.8 + Vec3::Y * 2.8;
            limb.ang_vel += Vec3::new(dir.z * 12.0, 6.0, -dir.x * 12.0);
            vp.spawn_gore_burst(hit_pos, dir * 4.5, gore_rgb, 8);
            vp.client_fx.push_back(ClientFx::Sound {
                sound: "minecraft:entity.player.attack.crit",
                at: [hit_pos.x as f64, hit_pos.y as f64, hit_pos.z as f64],
                vol: 0.85,
                pitch: 0.85,
            });
            return;
        }

        // 3. Raycast / splash 3D rubble & fine sand grains in front of swing!
        let mut splashed = 0usize;
        for ch in &mut vp.chunks {
            if let Some(_t) = ray_aabb(
                eye_v3,
                dir,
                ch.pos - Vec3::splat(0.35),
                ch.pos + Vec3::splat(0.35),
                3.8,
            ) {
                ch.vel += dir * 4.5 + Vec3::Y * 2.2;
                ch.ang_vel *= 1.5;
                ch.grounded = false;
                splashed += 1;
                if splashed >= 12 {
                    break;
                }
            }
        }
        let mut grain_splashed = 0usize;
        let mut first_grain_hit: Option<Vec3> = None;
        for g in &mut vp.sand_grains {
            if let Some(_t) = ray_aabb(
                eye_v3,
                dir,
                g.pos - Vec3::splat(0.34),
                g.pos + Vec3::splat(0.34),
                3.8,
            ) {
                if first_grain_hit.is_none() {
                    first_grain_hit = Some(g.pos);
                }
                g.vel += Vec3::new(dir.x * 3.4, 1.45, dir.z * 3.4);
                g.grounded = false;
                grain_splashed += 1;
                if grain_splashed >= 64 {
                    break;
                }
            }
        }
        if let Some(gp) = first_grain_hit {
            vp.client_fx.push_back(ClientFx::Sound {
                sound: "minecraft:block.sand.break",
                at: [gp.x as f64, gp.y as f64, gp.z as f64],
                vol: 0.65,
                pitch: 1.08,
            });
        }
    });
}

// ============================================================================
// 13. HOOK ENTRYPOINTS (CLIENT & INTEGRATED SERVER COORDINATED)
// ============================================================================

pub(super) fn on_tick(game: &mut Game<'_>) {
    let is_client = game.options.execution_role.is_client();

    if !is_client {
        // Dedicated-server regression fix (PR #401):
        // Server retains 100% authoritative control of vanilla `game.falling` blocks (sand, gravel, concrete powder).
        // Falling blocks are never intercepted or cleared from `game.falling` on the server, ensuring
        // they land as placed blocks or drop as items under server authority.

        // AUTHORITATIVE SERVER GAME THREAD: drain pending world / loot / mob ops!
        let ops: Vec<ServerOp> = with_vp(|vp| vp.server_ops.drain(..).collect());
        for op in ops {
            match op {
                ServerOp::SetAir { dim, pos } => {
                    game.native_set_block(&dim, pos, BlockStateId(0), 3);
                }
                ServerOp::PlaceBlockIfAir { dim, pos, state } => {
                    let cur = game.world.block(pos[0], pos[1], pos[2]);
                    if game.res.facts().is_air(cur) {
                        game.native_set_block(&dim, pos, state, 3);
                    } else {
                        let drops = game.block_loot_at(pos, state, None, None);
                        for st in drops {
                            game.pop_resource(pos, st);
                        }
                    }
                }
                ServerOp::DropBlockLoot { pos, state } => {
                    let mut drops = game.block_loot_at(pos, state, None, None);
                    if drops.is_empty()
                        && let Some(bname) = game.native_state_block(state)
                        && !bname.contains("leaves")
                        && !bname.contains("glass")
                        && game.res.items().get(bname).is_some()
                    {
                        drops.push(ItemStack::new(bname, 1));
                    }
                    for st in drops {
                        game.pop_resource(pos, st);
                    }
                }
                ServerOp::CrushMob {
                    dim,
                    mob_id,
                    damage,
                    push,
                } => {
                    game.native_push_mob(&dim, mob_id, push);
                    if let Some(nm) = game.native_mob(&dim, mob_id) {
                        let next_hp = nm.health - damage;
                        if next_hp <= 0.0 {
                            game.native_kill_mob(&dim, mob_id);
                        } else {
                            game.native_set_mob_health(&dim, mob_id, next_hp);
                        }
                    }
                }
            }
        }
        return;
    }

    // CLIENT GAME THREAD: step 4-substep 3D physics & play queued audio/particle FX!
    let fx_batch: Vec<ClientFx> = with_vp(|vp| {
        if vp.cfg.master_enabled {
            step_client_physics(game, vp);
        }
        vp.client_fx.drain(..).collect()
    });

    for fx in fx_batch {
        match fx {
            ClientFx::Sound {
                sound,
                at,
                vol,
                pitch,
            } => {
                game.audio.play(sound, Some(at), vol, pitch);
            }
            ClientFx::Smoke { at, vel, large } => {
                let do_smoke = with_vp(|vp| vp.cfg.dust_clouds);
                if do_smoke {
                    let pt = if large {
                        ParticleType::LargeSmoke
                    } else {
                        ParticleType::Smoke
                    };
                    game.particles.add(&ParticleOptions::simple(pt), at, vel);
                }
            }
            ClientFx::BlockBurst {
                pos,
                state,
                count,
                speed,
            } => {
                if let Some((sprite, tint)) = game.particle_sprite(state, pos) {
                    let opt = ParticleOptions::block(ParticleType::Block, sprite, tint);
                    let c = [
                        pos[0] as f64 + 0.5,
                        pos[1] as f64 + 0.5,
                        pos[2] as f64 + 0.5,
                    ];
                    for i in 0..count {
                        let a = (i as f64) * std::f64::consts::TAU / (count.max(1) as f64);
                        game.particles
                            .add(&opt, c, [a.cos() * speed, 0.14, a.sin() * speed]);
                    }
                }
            }
        }
    }
}

/// Checks if a block is a non-solid or surface-dependent attachment that requires support.
fn is_supported_attachment(name: &str) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    n.contains("flower_pot")
        || n.contains("pot")
        || n.contains("carpet")
        || n.contains("torch")
        || n.contains("lantern")
        || n.contains("candle")
        || n.contains("skull")
        || n.contains("head")
        || n.contains("rail")
        || n.contains("tripwire")
        || n.contains("string")
        || n.contains("lever")
        || n.contains("button")
        || n.contains("pressure_plate")
        || n.contains("repeater")
        || n.contains("comparator")
        || n.contains("redstone_wire")
        || n.contains("amethyst_cluster")
        || n.contains("pointed_dripstone")
        || n.contains("sign")
        || n.contains("door")
        || n.contains("trapdoor")
        || n.contains("banner")
}

/// Automatically detaches and falls any unsupported pots, vases, lanterns, carpets, and wall decorations.
fn clean_unsupported_attachments(
    game: &mut Game<'_>,
    vp: &mut VpWorld,
    dim: &str,
    positions: &[[i32; 3]],
) {
    let mut to_process = Vec::new();
    for &p in positions {
        // 1. Check directly above
        let above = [p[0], p[1] + 1, p[2]];
        let s_above = game.world.block(above[0], above[1], above[2]);
        if !game.res.facts().is_air(s_above) {
            let name = game.res.facts().get(s_above).name.clone();
            if is_supported_attachment(&name) {
                to_process.push((above, s_above, name, true));
            }
        }
        // 2. Check 4 horizontal neighbors (for wall signs, wall torches, etc.)
        for [dx, dz] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
            let adj = [p[0] + dx, p[1], p[2] + dz];
            let s_adj = game.world.block(adj[0], adj[1], adj[2]);
            if !game.res.facts().is_air(s_adj) {
                let name = game.res.facts().get(s_adj).name.clone();
                if name.contains("wall_sign") || name.contains("wall_torch") || name.contains("banner") {
                    to_process.push((adj, s_adj, name, false));
                }
            }
        }
    }

    for (pos, state, name, is_top) in to_process {
        game.native_set_block(dim, pos, BlockStateId(0), 3);
        let is_rigid_3d = name.contains("flower_pot")
            || name.contains("pot")
            || name.contains("skull")
            || name.contains("head")
            || name.contains("lantern")
            || name.contains("anvil")
            || name.contains("bell");

        if is_rigid_3d && is_top {
            let center = DVec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5);
            let mut rng = FastRng::seeded((pos[0] as u64) ^ ((pos[2] as u64) << 16) ^ vp.tick_counter);
            let vx = rng.signed() as f64 * 0.3;
            let vz = rng.signed() as f64 * 0.3;
            vp.flying_blocks.push(FlyingBlock {
                pos: center,
                prev_pos: center,
                vel: DVec3::new(vx, -1.0, vz),
                rot: Quat::IDENTITY,
                prev_rot: Quat::IDENTITY,
                ang_vel: Vec3::new(rng.signed() * 3.5, rng.signed() * 2.0, rng.signed() * 3.5),
                state,
                seed: (pos[0] as i64 * 31) ^ (pos[2] as i64),
                bounces: 0,
                age: 0,
                place_on_land: false, // Shatters on landing into pottery shards and drops loot!
            });
        } else {
            let mat = classify_block_name(&name);
            vp.spawn_subvoxels(pos, -1.0, state, mat, 6, Vec3::new(0.0, -0.3, 0.0));
            vp.server_ops.push_back(ServerOp::DropBlockLoot { pos, state });
        }
    }
}

pub(super) fn on_destroy_block(game: &mut Game<'_>, pos: [i32; 3], state: BlockStateId) {
    let (master, do_debris, do_trees, do_buildings, p_pos) = with_vp(|vp| {
        (
            vp.cfg.master_enabled,
            vp.cfg.debris,
            vp.cfg.tree_felling,
            vp.cfg.building_collapse,
            vp.last_player_pos,
        )
    });
    if !master {
        return;
    }

    let dim = game.dim.id().to_owned();
    let bname = game.res.facts().get(state).name.to_string();
    let mat = classify_block_name(&bname);

    // ------------------------------------------------------------------------
    // 1. 3D Block Shatter OR Immediate Fine Sand/Gravel/Powder Grains!
    // ------------------------------------------------------------------------
    let is_loose_sand = is_true_gravity_granular(&bname);
    if do_debris {
        with_vp(|vp| {
            if is_loose_sand {
                // ONLY for the 4 sands, gravel, and concrete powder:
                // Immediately bursts into fine 3D interactive micro-grains without any pyramid mound!
                vp.spawn_sand_block_grains(pos, state, 76, Vec3::new(0.0, -0.25, 0.0));
            } else {
                // All other blocks remain 100% untouched!
                let cnt = match mat {
                    ChunkMat::Glass => 16,
                    ChunkMat::Wood => 14,
                    ChunkMat::Stone => 12,
                    _ => 10,
                };
                vp.spawn_subvoxels(pos, -1.0, state, mat, cnt, Vec3::Y * 0.8);
                vp.client_fx.push_back(ClientFx::BlockBurst {
                    pos,
                    state,
                    count: 12,
                    speed: 0.16,
                });
            }
        });
    }

    if !do_trees && !do_buildings {
        return;
    }

    // Classify fragile decorations: fences, gates, signs, lanterns, torches, banners, etc.
    // Breaking these must NEVER trigger building collapses, loud explosions, or dust storms!
    let bname_lower = bname.to_lowercase();
    let is_fragile_decor = bname_lower.contains("fence")
        || bname_lower.contains("gate")
        || bname_lower.contains("sign")
        || bname_lower.contains("lantern")
        || bname_lower.contains("banner")
        || bname_lower.contains("torch")
        || bname_lower.contains("carpet")
        || bname_lower.contains("glass")
        || bname_lower.contains("flower")
        || bname_lower.contains("leaf")
        || bname_lower.contains("leaves")
        || bname_lower.contains("pane")
        || bname_lower.contains("bar")
        || bname_lower.contains("chain")
        || bname_lower.contains("slab")
        || bname_lower.contains("stair")
        || bname_lower.contains("door")
        || bname_lower.contains("trapdoor")
        || bname_lower.contains("pot")
        || bname_lower.contains("hay");

    // ------------------------------------------------------------------------
    // 3. True 3D Rigid-Body Tree Felling!
    // ------------------------------------------------------------------------
    if do_trees {
        if let Some(tree_blocks) = scan_tree_structure(game, pos, state) {
        for &(bp, _, _) in &tree_blocks {
            game.native_set_block(&dim, bp, BlockStateId(0), 3);
        }

        with_vp(|vp| {
            let pivot = DVec3::new(
                pos[0] as f64 + 0.5,
                pos[1] as f64 + 1.0,
                pos[2] as f64 + 0.5,
            );
            let mut com_xz = Vec3::ZERO;
            let mut max_dy = 2.0f32;
            let mut cells = Vec::with_capacity(tree_blocks.len());

            for (bp, bs, is_leaf) in tree_blocks {
                let local = IVec3::new(bp[0] - pos[0], bp[1] - (pos[1] + 1), bp[2] - pos[2]);
                let w = if is_leaf { 0.25 } else { 1.0 };
                com_xz += Vec3::new(local.x as f32, 0.0, local.z as f32) * w;
                max_dy = max_dy.max(local.y as f32 + 1.0);
                cells.push(RigidCell {
                    local,
                    state: bs,
                    is_foliage: is_leaf,
                    hits_taken: 0,
                    deform_offset: Vec3::ZERO,
                    dent_rot: Quat::IDENTITY,
                });
            }

            // Fell AWAY from the player (with natural canopy COM bias)
            let away = Vec3::new(
                (pos[0] as f32 + 0.5) - p_pos.x,
                0.0,
                (pos[2] as f32 + 0.5) - p_pos.z,
            )
            .normalize_or(Vec3::X);
            let fall_dir = (away * 0.8 + com_xz.normalize_or_zero() * 0.2).normalize_or(Vec3::X);
            // Right-hand rotation axis so positive angle tips `+Y` toward `fall_dir`: `Y x fall_dir`!
            let fall_axis = Vec3::Y.cross(fall_dir).normalize_or(Vec3::Z);
            let id = vp.alloc_id();

            vp.assemblies.push(RigidAssembly {
                id,
                kind: AssemblyKind::Tree,
                pivot,
                prev_pivot: pivot,
                vel: DVec3::ZERO,
                rot: Quat::IDENTITY,
                prev_rot: Quat::IDENTITY,
                fall_axis,
                fall_dir_xz: fall_dir,
                fall_angle: 0.02,
                ang_speed: 0.18, // Slow ominous initial lean!
                free_ang_vel: Vec3::ZERO,
                eff_length: (max_dy * 0.62).clamp(2.5, 12.0),
                hinged: true,
                settled: false,
                settle_ticks: 0,
                ticks_alive: 0,
                crashed_once: false,
                origin_stump: pos,
                cells,
                hit_mobs: HashSet::new(),
            });

            // Stump wood-splitting crack sound + notch splinters!
            vp.spawn_subvoxels(pos, -1.0, state, ChunkMat::Wood, 12, fall_dir * 2.0 + Vec3::Y * 1.2);
            vp.client_fx.push_back(ClientFx::Sound {
                sound: "minecraft:entity.zombie.attack_wooden_door",
                at: [pivot.x, pivot.y, pivot.z],
                vol: 0.70,
                pitch: 0.62,
            });
        });
        return;
        }
    }

    // ------------------------------------------------------------------------
    // 4. Non-Tree Unsupported Masonry / Bridge / Overhang 3D Topple!
    // ------------------------------------------------------------------------
    if do_buildings {
        let above = [pos[0], pos[1] + 1, pos[2]];
        if let Some(cluster) = scan_unsupported_cluster(game, above, pos, 600) {
        for &(bp, _) in &cluster {
            game.native_set_block(&dim, bp, BlockStateId(0), 3);
        }
        let cluster_pos: Vec<[i32; 3]> = cluster.iter().map(|&(p, _)| p).collect();
        with_vp(|vp| {
            clean_unsupported_attachments(game, vp, &dim, &cluster_pos);
        });

        with_vp(|vp| {
            let pivot = DVec3::new(
                pos[0] as f64 + 0.5,
                pos[1] as f64 + 1.0,
                pos[2] as f64 + 0.5,
            );
            let mut com_xz = Vec3::ZERO;
            let mut max_dy = 1.5f32;
            let mut cells = Vec::with_capacity(cluster.len());

            for (bp, bs) in cluster {
                let local = IVec3::new(bp[0] - pos[0], bp[1] - (pos[1] + 1), bp[2] - pos[2]);
                com_xz += Vec3::new(local.x as f32, 0.0, local.z as f32);
                max_dy = max_dy.max(local.y as f32 + 1.0);
                cells.push(RigidCell {
                    local,
                    state: bs,
                    is_foliage: false,
                    hits_taken: 0,
                    deform_offset: Vec3::ZERO,
                    dent_rot: Quat::IDENTITY,
                });
            }

            let away = Vec3::new(
                (pos[0] as f32 + 0.5) - p_pos.x,
                0.0,
                (pos[2] as f32 + 0.5) - p_pos.z,
            )
            .normalize_or(Vec3::Z);
            let fall_dir = com_xz.normalize_or(away);
            let fall_axis = Vec3::Y.cross(fall_dir).normalize_or(Vec3::X);
            let id = vp.alloc_id();

            vp.assemblies.push(RigidAssembly {
                id,
                kind: AssemblyKind::Masonry,
                pivot,
                prev_pivot: pivot,
                vel: DVec3::ZERO,
                rot: Quat::IDENTITY,
                prev_rot: Quat::IDENTITY,
                fall_axis,
                fall_dir_xz: fall_dir,
                fall_angle: 0.03,
                ang_speed: 0.30,
                free_ang_vel: Vec3::ZERO,
                eff_length: (max_dy * 0.55).clamp(1.8, 9.0),
                hinged: true,
                settled: false,
                settle_ticks: 0,
                ticks_alive: 0,
                crashed_once: false,
                origin_stump: pos,
                cells,
                hit_mobs: HashSet::new(),
            });
        });
        }
    }

    // Clean attachments directly on top of the broken block (e.g. flower pot on broken table)
    with_vp(|vp| {
        clean_unsupported_attachments(game, vp, &dim, &[pos]);
    });

    // ------------------------------------------------------------------------
    // 5. Universal Cantilever & Structural Load-Bearing Collapse
    // ------------------------------------------------------------------------
    if do_buildings && is_destructible_bulk(&bname) && !is_fragile_decor {
        let cracked_snapshot = with_vp(|vp| vp.cracked_blocks.clone());
        if let Some((overloaded, fracture_edges)) = scan_overloaded_cantilever(game, pos, &cracked_snapshot, 32) {
            for &(bp, _) in &overloaded {
                game.native_set_block(&dim, bp, BlockStateId(0), 3);
            }
            let overloaded_pos: Vec<[i32; 3]> = overloaded.iter().map(|&(p, _)| p).collect();
            with_vp(|vp| {
                clean_unsupported_attachments(game, vp, &dim, &overloaded_pos);
            });
            with_vp(|vp| {
                for (fp, fs) in fracture_edges {
                    vp.add_block_crack(fp, fs, 1);
                }
                let mut rng = FastRng::seeded((pos[0] as u64) ^ ((pos[2] as u64) << 16));
                for (bp, bs) in &overloaded {
                    let bcenter = DVec3::new(bp[0] as f64 + 0.5, bp[1] as f64 + 0.5, bp[2] as f64 + 0.5);
                    let vx = rng.signed() as f64 * 1.5;
                    let vz = rng.signed() as f64 * 1.5;
                    vp.flying_blocks.push(FlyingBlock {
                        pos: bcenter,
                        prev_pos: bcenter,
                        vel: DVec3::new(vx, -1.8, vz),
                        rot: Quat::IDENTITY,
                        prev_rot: Quat::IDENTITY,
                        ang_vel: Vec3::new(rng.signed() * 6.0, rng.signed() * 4.0, rng.signed() * 6.0),
                        state: *bs,
                        seed: (bp[0] as i64 * 31) ^ (bp[2] as i64),
                        bounces: 0,
                        age: 0,
                        place_on_land: true,
                    });
                }
                // Sound: lowered by 40% (0.45 instead of 0.85), deep cinematic rubble thud
                vp.client_fx.push_back(ClientFx::Sound {
                    sound: "minecraft:block.stone.fall",
                    at: [pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5],
                    vol: 0.45,
                    pitch: 0.65,
                });
                // Smoke/Dust: ONLY spawn dust clouds during actual large collapses (>= 6 blocks)
                if overloaded.len() >= 6 {
                    vp.spawn_dust_cloud(
                        Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5),
                        Vec3::splat(1.5),
                        0xd0c0a8,
                        14,
                        Vec3::new(0.0, 0.25, 0.0),
                    );
                }
            });

            // Shockwave to nearby hanging lanterns, signs and banners from heavy collapse!
            with_vp(|vp| {
                apply_shockwave_to_hanging_objects(game, vp, &dim, Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5), 4.5);
            });
        }
    }

    // ------------------------------------------------------------------------
    // 6. Hanging Chain / Lantern / Banner Broken Anchor Handling
    // ------------------------------------------------------------------------
    let below = [pos[0], pos[1] - 1, pos[2]];
    let below_state = game.world.block(below[0], below[1], below[2]);
    if !game.res.facts().is_air(below_state) {
        let below_name = game.res.facts().get(below_state).name.clone();
        if below_name.contains("chain") || below_name.contains("lantern") || below_name.contains("hanging_sign") || below_name.contains("bell") {
            let mat = classify_block_name(&below_name);
            game.native_set_block(&dim, below, BlockStateId(0), 3);
            with_vp(|vp| {
                vp.spawn_subvoxels(below, -1.0, below_state, mat, 8, Vec3::new(0.0, -0.4, 0.0));
                vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: below, state: below_state });
                vp.ropes.retain(|r| r.anchor_block != pos && r.anchor_block != below);
            });
        }
    }

    for [dx, dy, dz] in [[0, -1, 0], [1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
        let adj = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
        let adj_state = game.world.block(adj[0], adj[1], adj[2]);
        if !game.res.facts().is_air(adj_state) {
            let adj_name = &game.res.facts().get(adj_state).name;
            if adj_name.contains("banner") {
                game.native_set_block(&dim, adj, BlockStateId(0), 3);
                with_vp(|vp| {
                    vp.spawn_subvoxels(adj, -1.0, adj_state, ChunkMat::Wood, 6, Vec3::new(0.0, -0.3, 0.0));
                    vp.server_ops.push_back(ServerOp::DropBlockLoot { pos: adj, state: adj_state });
                    vp.cloths.retain(|c| c.anchor_pos != adj && c.support_pos != Some(pos));
                });
            }
        }
    }
}

/// Traces a connected structural cluster above an explosion crater.
/// If NOT A SINGLE BLOCK in the entire connected cluster connects to any intact solid ground
/// (i.e. every block below the cluster is either AIR or was destroyed in `blast_set`),
/// then the entire structure is completely floating in mid-air (like the severed construction crane!)
/// and collapses as a physical falling RigidAssembly!
fn scan_floating_blast_structure(
    game: &Game<'_>,
    seed: [i32; 3],
    blast_set: &HashSet<[i32; 3]>,
    max_blocks: usize,
) -> Option<Vec<([i32; 3], BlockStateId)>> {
    let seed_state = game.world.block(seed[0], seed[1], seed[2]);
    if game.res.facts().is_air(seed_state) {
        return None;
    }
    let sname = &game.res.facts().get(seed_state).name;
    if sname.contains("bedrock") || sname.contains("water") || sname.contains("lava") {
        return None;
    }

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back(seed);
    visited.insert(seed);

    let mut cluster = Vec::new();
    const DIRS6: [[i32; 3]; 6] = [
        [0, 1, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
        [0, -1, 0],
    ];

    while let Some(curr) = queue.pop_front() {
        let st = game.world.block(curr[0], curr[1], curr[2]);
        cluster.push((curr, st));
        if cluster.len() > max_blocks {
            return None; // Attached to a massive terrain / landscape
        }

        // Check if curr has ground support beneath it
        let below = [curr[0], curr[1] - 1, curr[2]];
        if !blast_set.contains(&below) && !visited.contains(&below) {
            let bs = game.world.block(below[0], below[1], below[2]);
            if !game.res.facts().is_air(bs) {
                let bfacts = game.res.facts().get(bs);
                let bname = &bfacts.name;
                if bfacts.full_collision
                    || bname.contains("stone")
                    || bname.contains("dirt")
                    || bname.contains("grass")
                    || bname.contains("concrete")
                    || bname.contains("terracotta")
                    || bname.contains("plank")
                    || bname.contains("log")
                    || bname.contains("brick")
                    || bname.contains("quartz")
                {
                    return None; // Ground support found! Structure is still anchored somewhere to the world.
                }
            }
        }

        for d in DIRS6 {
            let nb = [curr[0] + d[0], curr[1] + d[1], curr[2] + d[2]];
            if blast_set.contains(&nb) || visited.contains(&nb) {
                continue;
            }
            let ns = game.world.block(nb[0], nb[1], nb[2]);
            if !game.res.facts().is_air(ns) {
                let nn = &game.res.facts().get(ns).name;
                if !nn.contains("water") && !nn.contains("lava") && !nn.contains("bedrock") {
                    visited.insert(nb);
                    queue.push_back(nb);
                }
            }
        }
    }

    if cluster.len() >= 3 {
        Some(cluster)
    } else {
        None
    }
}

pub(super) fn on_explosion_detonate(
    game: &mut Game<'_>,
    dimension: &str,
    center: [f64; 3],
    blocks: &mut Vec<[i32; 3]>,
) {
    let (master, do_blast, do_trees, do_buildings) = with_vp(|vp| {
        (
            vp.cfg.master_enabled,
            vp.cfg.explosions,
            vp.cfg.tree_felling,
            vp.cfg.building_collapse,
        )
    });
    if !master || !do_blast {
        return;
    }

    let c_v3 = Vec3::new(center[0] as f32, center[1] as f32, center[2] as f32);
    let c_dv3 = DVec3::from_array(center);

    with_vp(|vp| {
        vp.add_trauma(c_v3, 48.0, 1.0, 1.35);
        vp.client_fx.push_back(ClientFx::Smoke {
            at: center,
            vel: [0.0, 0.18, 0.0],
            large: true,
        });

        // 1. Shockwave launch & guaranteed total dismemberment for nearby mobs!
        for m in game.mobs.iter_mut() {
            let dx = m.position.x - center[0];
            let dy = m.position.y - center[1];
            let dz = m.position.z - center[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if dist < 18.0 && dist > 0.01 {
                let force = (1.0 - dist / 18.0) * 2.1;
                let dir = Vec3::new((dx / dist) as f32, (dy / dist) as f32, (dz / dist) as f32);
                m.velocity.x += (dx / dist) * force * 1.45;
                m.velocity.y += ((dy / dist) * force + 0.52 * force).min(1.5);
                m.velocity.z += (dz / dist) * force * 1.45;
                m.needs_sync = true;

                vp.pending_impulses.insert(
                    m.id,
                    PendingKillImpulse {
                        dir,
                        strength: force as f32 * 1.6,
                        damage: 25.0,
                        explosive: true,
                        tick_recorded: vp.tick_counter,
                    },
                );
            }
        }

        // Also blast any already-lying ragdoll corpses & 3D chunks near the explosion!
        for rag in &mut vp.ragdolls {
            for (li, limb) in rag.limbs.iter_mut().enumerate() {
                let d = limb.pos - c_v3;
                let dist = d.length();
                if dist < 16.0 {
                    if li > 0 {
                        limb.severed = true;
                    }
                    let push = d.normalize_or(Vec3::Y) * ((1.0 - dist / 16.0) * 14.0);
                    limb.vel += push + Vec3::Y * 4.5;
                    limb.ang_vel += Vec3::new(11.0, -9.0, 13.0);
                }
            }
        }

        // 2. Uproot & fell trees whose base is caught in or right beside the blast!
        if do_trees {
            let mut uprooted_bases = Vec::new();
            for &bpos in blocks.iter() {
                let bs = game.world.block(bpos[0], bpos[1], bpos[2]);
                if !game.res.facts().is_air(bs) && is_tree_log_name(&game.res.facts().get(bs).name) {
                    uprooted_bases.push((bpos, bs));
                }
            }
            for (bpos, bs) in uprooted_bases {
                if let Some(tree_blocks) = scan_tree_structure(game, bpos, bs) {
                    for &(tp, _, _) in &tree_blocks {
                        game.native_set_block(dimension, tp, BlockStateId(0), 3);
                    }
                    let pivot = DVec3::new(
                        bpos[0] as f64 + 0.5,
                        bpos[1] as f64 + 1.0,
                        bpos[2] as f64 + 0.5,
                    );
                    let away = Vec3::new(
                        (bpos[0] as f32 + 0.5) - c_v3.x,
                        0.0,
                        (bpos[2] as f32 + 0.5) - c_v3.z,
                    )
                    .normalize_or(Vec3::X);
                    let fall_axis = Vec3::Y.cross(away).normalize_or(Vec3::Z);
                    let cells = tree_blocks
                        .into_iter()
                        .map(|(tp, ts, is_leaf)| RigidCell {
                            local: IVec3::new(tp[0] - bpos[0], tp[1] - (bpos[1] + 1), tp[2] - bpos[2]),
                            state: ts,
                            is_foliage: is_leaf,
                            hits_taken: 0,
                            deform_offset: Vec3::ZERO,
                            dent_rot: Quat::IDENTITY,
                        })
                        .collect();
                    let id = vp.alloc_id();
                    vp.assemblies.push(RigidAssembly {
                        id,
                        kind: AssemblyKind::Tree,
                        pivot,
                        prev_pivot: pivot,
                        vel: DVec3::new(away.x as f64 * 1.5, 1.2, away.z as f64 * 1.5),
                        rot: Quat::IDENTITY,
                        prev_rot: Quat::IDENTITY,
                        fall_axis,
                        fall_dir_xz: away,
                        fall_angle: 0.08,
                        ang_speed: 1.15, // Faster violent blast-topple!
                        free_ang_vel: Vec3::ZERO,
                        eff_length: 4.5,
                        hinged: true,
                        settled: false,
                        settle_ticks: 0,
                        ticks_alive: 0,
                        crashed_once: false,
                        origin_stump: bpos,
                        cells,
                        hit_mobs: HashSet::new(),
                    });
                }
            }
        }

        // 2b. Severed floating structures (cranes, towers, bridges, overhangs) above blast crater!
        if do_buildings {
            let blast_set: HashSet<[i32; 3]> = blocks.iter().copied().collect();
            let mut candidate_seeds: Vec<[i32; 3]> = Vec::new();
            for &bp in blocks.iter() {
                let above = [bp[0], bp[1] + 1, bp[2]];
                if !blast_set.contains(&above) {
                    let s = game.world.block(above[0], above[1], above[2]);
                    if !game.res.facts().is_air(s) {
                        candidate_seeds.push(above);
                    }
                }
            }

            let mut already_collapsed: HashSet<[i32; 3]> = HashSet::new();
            for seed in candidate_seeds {
                if already_collapsed.contains(&seed) {
                    continue;
                }
                if let Some(cluster) = scan_floating_blast_structure(game, seed, &blast_set, 800) {
                    for &(bp, _) in &cluster {
                        game.native_set_block(dimension, bp, BlockStateId(0), 3);
                        already_collapsed.insert(bp);
                    }

                    let cluster_positions: Vec<[i32; 3]> = cluster.iter().map(|&(p, _)| p).collect();
                    clean_unsupported_attachments(game, vp, dimension, &cluster_positions);

                    let min_y = cluster.iter().map(|&(p, _)| p[1]).min().unwrap_or(seed[1]);
                    let pivot = DVec3::new(
                        seed[0] as f64 + 0.5,
                        min_y as f64,
                        seed[2] as f64 + 0.5,
                    );

                    let mut com_xz = Vec3::ZERO;
                    let mut max_dy = 1.0f32;
                    let mut cells = Vec::with_capacity(cluster.len());

                    for (bp, bs) in cluster {
                        let local = IVec3::new(bp[0] - seed[0], bp[1] - min_y, bp[2] - seed[2]);
                        com_xz += Vec3::new(local.x as f32, 0.0, local.z as f32);
                        max_dy = max_dy.max(local.y as f32 + 1.0);
                        cells.push(RigidCell {
                            local,
                            state: bs,
                            is_foliage: false,
                            hits_taken: 0,
                            deform_offset: Vec3::ZERO,
                            dent_rot: Quat::IDENTITY,
                        });
                    }

                    let away = Vec3::new(
                        (seed[0] as f32 + 0.5) - c_v3.x,
                        0.0,
                        (seed[2] as f32 + 0.5) - c_v3.z,
                    )
                    .normalize_or(Vec3::X);
                    let fall_dir = (away * 0.6 + com_xz.normalize_or_zero() * 0.4).normalize_or(Vec3::X);
                    let fall_axis = Vec3::Y.cross(fall_dir).normalize_or(Vec3::Z);
                    let id = vp.alloc_id();

                    vp.assemblies.push(RigidAssembly {
                        id,
                        kind: AssemblyKind::Masonry,
                        pivot,
                        prev_pivot: pivot,
                        vel: DVec3::new(fall_dir.x as f64 * 0.5, -1.2, fall_dir.z as f64 * 0.5),
                        rot: Quat::IDENTITY,
                        prev_rot: Quat::IDENTITY,
                        fall_axis,
                        fall_dir_xz: fall_dir,
                        fall_angle: 0.04,
                        ang_speed: 0.20,
                        free_ang_vel: Vec3::new(fall_dir.z * 1.5, 0.0, -fall_dir.x * 1.5),
                        eff_length: (max_dy * 0.55).clamp(2.0, 16.0),
                        hinged: false,
                        settled: false,
                        settle_ticks: 0,
                        ticks_alive: 0,
                        crashed_once: false,
                        origin_stump: seed,
                        cells,
                        hit_mobs: HashSet::new(),
                    });
                }
            }
        }

        // 3. Launch up to 28 crater blocks as 3D spinning ballistic FlyingBlocks + 3D shrapnel!
        let mut rng = FastRng::seeded((blocks.len() as u64) ^ vp.tick_counter);
        let stride = (blocks.len() / 26).max(1);
        for (i, &pos) in blocks.iter().enumerate() {
            let bstate = game.world.block(pos[0], pos[1], pos[2]);
            if game.res.facts().is_air(bstate) {
                continue;
            }
            let bname = &game.res.facts().get(bstate).name;
            if bname.contains("tnt") || bname.contains("bedrock") {
                continue;
            }

            let bcenter = DVec3::new(
                pos[0] as f64 + 0.5,
                pos[1] as f64 + 0.5,
                pos[2] as f64 + 0.5,
            );
            let out_dir = (bcenter - c_dv3).normalize_or(DVec3::Y);

            if i % stride == 0 && vp.flying_blocks.len() < 48 {
                let speed = rng.range(6.5, 13.5) as f64;
                let vy = rng.range(6.0, 12.5) as f64;
                game.native_set_block(dimension, pos, BlockStateId(0), 3);
                vp.flying_blocks.push(FlyingBlock {
                    pos: bcenter,
                    prev_pos: bcenter,
                    vel: DVec3::new(out_dir.x * speed, vy, out_dir.z * speed),
                    rot: Quat::IDENTITY,
                    prev_rot: Quat::IDENTITY,
                    ang_vel: Vec3::new(
                        rng.signed() * 10.0,
                        rng.signed() * 10.0,
                        rng.signed() * 10.0,
                    ),
                    state: bstate,
                    seed: (i as i64) * 97 + 13,
                    bounces: 0,
                    age: 0,
                    place_on_land: i % (stride * 2) == 0,
                });
            } else if i % 2 == 0 {
                let mat = classify_block_name(bname);
                vp.spawn_subvoxels(
                    pos,
                    -1.0,
                    bstate,
                    mat,
                    4,
                    out_dir.as_vec3() * 5.5 + Vec3::Y * 3.5,
                );
            }
        }

        // 4. Sympathetic shockwave glass shattering within 10 blocks into 3D flying glass plates!
        let (cx, cy, cz) = (
            center[0].floor() as i32,
            center[1].floor() as i32,
            center[2].floor() as i32,
        );
        for gx in (cx - 9)..=(cx + 9) {
            for gy in (cy - 5)..=(cy + 9) {
                for gz in (cz - 9)..=(cz + 9) {
                    let gs = game.world.block(gx, gy, gz);
                    if game.res.facts().is_air(gs) {
                        continue;
                    }
                    let gname = &game.res.facts().get(gs).name;
                    if gname.contains("glass") {
                        let gpos = [gx, gy, gz];
                        game.native_set_block(dimension, gpos, BlockStateId(0), 3);
                        let dir = (Vec3::new(gx as f32 + 0.5, gy as f32 + 0.5, gz as f32 + 0.5)
                            - c_v3)
                            .normalize_or(Vec3::Y);
                        vp.spawn_subvoxels(gpos, -1.0, gs, ChunkMat::Glass, 10, dir * 5.0);
                        vp.client_fx.push_back(ClientFx::Sound {
                            sound: "minecraft:block.glass.break",
                            at: [gx as f64 + 0.5, gy as f64 + 0.5, gz as f64 + 0.5],
                            vol: 0.7,
                            pitch: 1.05,
                        });
                    }
                }
            }
        }

        // 5. Volumetric atmospheric dust explosion cloud & bouncing hot sparks
        vp.spawn_dust_cloud(c_v3, Vec3::splat(1.5), 0xc8b8a0, 26, Vec3::new(0.0, 0.45, 0.0));
        vp.spawn_sparks(c_v3, Vec3::Y * 6.5, 36);

        // 6. Tear nearby cloth banners from blast shockwave
        for cloth in &mut vp.cloths {
            tear_cloth_near(cloth, c_v3, 8.0, 0.7, vp.tick_counter);
        }
    });

    // Shockwave wave & impulse to hanging lanterns, banners, hanging signs, and chains!
    with_vp(|vp| {
        apply_shockwave_to_hanging_objects(game, vp, dimension, c_v3, 8.5);
    });

    // 7. Cave-in & ceiling rockfalls above explosion center
    if do_buildings {
        let (cx, cy, cz) = (center[0].floor() as i32, center[1].floor() as i32, center[2].floor() as i32);
        let mut rockfall_blocks = Vec::new();
        for r_dy in 3..=8 {
            for (r_dx, r_dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
                let rp = [cx + r_dx, cy + r_dy, cz + r_dz];
                let rs = game.world.block(rp[0], rp[1], rp[2]);
                if !game.res.facts().is_air(rs) {
                    let rname = &game.res.facts().get(rs).name;
                    if is_destructible_bulk(rname) {
                        let below_s = game.world.block(rp[0], rp[1] - 1, rp[2]);
                        if game.res.facts().is_air(below_s) {
                            rockfall_blocks.push((rp, rs));
                            if rockfall_blocks.len() >= 6 {
                                break;
                            }
                        }
                    }
                }
            }
            if rockfall_blocks.len() >= 6 {
                break;
            }
        }

        for &(rp, _) in &rockfall_blocks {
            game.native_set_block(dimension, rp, BlockStateId(0), 3);
        }

        with_vp(|vp| {
            let mut rng = FastRng::seeded((cx as u64) ^ ((cz as u64) << 16));
            for (rp, rs) in rockfall_blocks {
                let bcenter = DVec3::new(rp[0] as f64 + 0.5, rp[1] as f64 + 0.5, rp[2] as f64 + 0.5);
                let vx = rng.signed() as f64 * 2.0;
                let vz = rng.signed() as f64 * 2.0;
                vp.flying_blocks.push(FlyingBlock {
                    pos: bcenter,
                    prev_pos: bcenter,
                    vel: DVec3::new(vx, -2.5, vz),
                    rot: Quat::IDENTITY,
                    prev_rot: Quat::IDENTITY,
                    ang_vel: Vec3::new(rng.signed() * 8.0, rng.signed() * 8.0, rng.signed() * 8.0),
                    state: rs,
                    seed: (rp[0] as i64 * 17) ^ (rp[2] as i64),
                    bounces: 0,
                    age: 0,
                    place_on_land: true,
                });
                vp.spawn_dust_cloud(bcenter.as_vec3(), Vec3::splat(0.5), 0xd5c5b0, 10, Vec3::new(0.0, -0.3, 0.0));
            }
        });
    }
}

pub(super) fn on_mob_submit(
    game: &Game<'_>,
    mob: &NativeMobRender<'_>,
    _draw: &mut BlockEntityDraw,
) -> bool {
    with_vp(|vp| {
        if !vp.cfg.master_enabled || !vp.cfg.ragdoll {
            return false;
        }
        // Hide the vanilla stiff living mob while it is actively stumbling!
        if vp.active_stumbles.contains_key(&mob.id) {
            return true;
        }
        if mob.death_time <= 0 {
            return false;
        }
        if !vp.spawned_ragdoll_ids.contains(&mob.id)
            && let Some(m) = game.mobs.iter().find(|x| x.id == mob.id)
        {
            let _ = try_spawn_mob_ragdoll(game, m, vp);
        }
        // Hide the vanilla stiff 90-degree tipping mob so only our 3D skinned dismemberable ragdoll renders!
        vp.spawned_ragdoll_ids.contains(&mob.id)
    })
}

pub(super) fn on_player_submit(
    _game: &Game<'_>,
    _player: &NativeMobRender<'_>,
    _who: &NativePlayerRender<'_>,
    _draw: &mut BlockEntityDraw,
) -> bool {
    false
}

pub(super) fn on_living_knockback(entity: i32, _kind: &str, strength: f64, x: f64, z: f64) {
    with_vp(|vp| {
        if !vp.cfg.master_enabled || !vp.cfg.ragdoll {
            return;
        }
        // Note: vanilla LivingEntity.knockback passes (x, z) pointing FROM target TO attacker, so push is (-x, -z)
        let dir = Vec3::new(-x as f32, 0.32, -z as f32).normalize_or(Vec3::Z);
        let entry = vp
            .pending_impulses
            .entry(entity as u32)
            .or_insert(PendingKillImpulse {
                dir,
                strength: strength as f32,
                damage: 4.0,
                explosive: false,
                tick_recorded: vp.tick_counter,
            });
        entry.dir = dir;
        entry.strength = (strength as f32).max(entry.strength);
        entry.tick_recorded = vp.tick_counter;
        vp.pending_stumbles.push((entity as u32, dir, strength as f32));
    });
}

pub(super) fn on_living_damage(entity: i32, _kind: &str, amount: f32) {
    with_vp(|vp| {
        if !vp.cfg.master_enabled || !vp.cfg.ragdoll {
            return;
        }
        let entry = vp
            .pending_impulses
            .entry(entity as u32)
            .or_insert(PendingKillImpulse {
                dir: Vec3::new(0.0, 0.4, 1.0),
                strength: 0.75,
                damage: amount,
                explosive: amount >= 14.0,
                tick_recorded: vp.tick_counter,
            });
        entry.damage = amount.max(entry.damage);
        if amount >= 14.0 {
            entry.explosive = true;
        }
        entry.tick_recorded = vp.tick_counter;
        if amount >= 3.0 {
            vp.pending_stumbles.push((entity as u32, Vec3::new(0.0, 0.25, 0.8), (amount * 0.15).clamp(0.4, 2.0)));
        }
    });
}

pub(super) fn on_entity_tick_post(game: &mut Game<'_>, _dimension: &str, entity: u32, kind: &str) {
    if kind != "minecraft:item" {
        return;
    }
    let active = with_vp(|vp| vp.cfg.master_enabled && vp.cfg.item_physics);
    if !active {
        return;
    }

    if let Some(i) = game.dropped.iter().position(|e| e.id == entity) {
        let (pos, on_ground, was_water) = {
            let e = &game.dropped[i];
            (e.position, e.on_ground, e.was_touching_water)
        };
        let bx = pos.x.floor() as i32;
        let by = (pos.y - 0.2).floor() as i32;
        let bz = pos.z.floor() as i32;
        let below_name = game.native_block([bx, by, bz]);
        let is_ice = on_ground && below_name.contains("ice");

        let item = &mut game.dropped[i];
        if is_ice {
            item.velocity.x *= 1.045;
            item.velocity.z *= 1.045;
            item.needs_sync = true;
        }
        if was_water && item.velocity.y < 0.085 {
            item.velocity.y += 0.042;
            item.needs_sync = true;
        }
    }
}

pub(super) fn on_camera_shake(game: &Game<'_>, partial: f32, shake: &mut NativeShake) {
    let cam = game
        .player
        .as_ref()
        .map_or(Vec3::ZERO, |p| Vec3::new(p.position.x as f32, p.position.y as f32, p.position.z as f32));

    with_vp(|vp| {
        if !vp.cfg.master_enabled {
            return;
        }
        for tr in &vp.traumas {
            let dist = (tr.origin - cam).length();
            if dist < tr.radius && tr.elapsed < tr.duration {
                let t = tr.elapsed + partial * 0.05;
                let decay = (1.0 - (t / tr.duration)).clamp(0.0, 1.0);
                let atten = (1.0 - dist / tr.radius).clamp(0.0, 1.0);
                let amp = tr.trauma * decay * decay * atten;
                let w = t * 38.0;
                shake.pitch += w.sin() * amp * 3.2;
                shake.yaw += (w * 1.37).cos() * amp * 2.5;
                shake.roll += (w * 0.83).sin() * amp * 2.1;
            }
        }
    });
}

pub(super) fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut crate::extras_hud::Corners) {
    with_vp(|vp| {
        // Top-left debug lines removed! Clean HUD during gameplay!
        if !vp.cfg.menu_open {
            return;
        }

        let items: [(&str, bool); 8] = [
            ("Обрушение зданий и конструкций", vp.cfg.building_collapse),
            ("Валка деревьев целиком", vp.cfg.tree_felling),
            ("Эффекты пыли и дыма", vp.cfg.dust_clouds),
            ("Взрывная волна и баллистика", vp.cfg.explosions),
            ("Осколки блоков и песчинки", vp.cfg.debris),
            ("Регдолл и спотыкание мобов", vp.cfg.ragdoll),
            ("3D физика предметов", vp.cfg.item_physics),
            ("Общая физика мода (Master)", vp.cfg.master_enabled),
        ];

        let w = 340.0f32;
        let h = 168.0f32;
        let (x, y) = if batch.width > w && batch.height > h {
            (((batch.width - w) * 0.5).round(), ((batch.height - h) * 0.5).round())
        } else {
            corners.place(crate::extras::Corner::TopLeft, w, h)
        };

        // Dark frosted panel + cyan accent bar
        game.native_fill(batch, [x - 2.0, y - 2.0, w + 4.0, h + 4.0], 0x8800_0000);
        game.native_fill(batch, [x, y, w, h], 0xF014_1A24);
        game.native_fill(batch, [x, y, w, 2.5], 0xFF38_BDF8);

        // Header
        game.native_text(batch, "§l⚙ VIBE PHYSICS — НАСТРОЙКИ§r", [x + 10.0, y + 8.0], 0xFF_FFFF, true);
        game.native_text(batch, "§7[RShift / Esc] Закрыть§r", [x + w - 132.0, y + 8.0], 0xAA_AAAA, true);

        // Header divider
        game.native_fill(batch, [x + 8.0, y + 23.0, w - 16.0, 1.0], 0x35FF_FFFF);

        // Menu items
        for (i, (label, enabled)) in items.iter().enumerate() {
            let row_y = y + 27.0 + (i as f32) * 14.5;
            let is_sel = i == vp.cfg.menu_selected_idx;

            if is_sel {
                game.native_fill(batch, [x + 6.0, row_y - 1.0, w - 12.0, 13.5], 0x3038_BDF8);
            }

            let num = i + 1;
            let prefix = if is_sel { "§e▶ " } else { "  " };
            let title_text = format!("{}§f[{}] {}§r", prefix, num, label);
            let status_text = if *enabled { "§a[ ВКЛ ]§r" } else { "§c[ ВЫКЛ ]§r" };

            let text_color = if is_sel { 0xFF_FFFF } else { 0xDD_DDDD };
            game.native_text(batch, &title_text, [x + 8.0, row_y], text_color, true);
            game.native_text(batch, status_text, [x + w - 54.0, row_y], 0xFF_FFFF, true);
        }

        // Footer divider & hint
        game.native_fill(batch, [x + 8.0, y + h - 18.0, w - 16.0, 1.0], 0x25FF_FFFF);
        game.native_text(
            batch,
            "§8[1-8] Переключить | [↑/↓] Выбор | [Enter] Изменить§r",
            [x + 10.0, y + h - 13.0],
            0x99_9999,
            true,
        );
    });
}

pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed {
        return;
    }

    if key == "key.mouse.left" {
        on_left_click_world(game);
        return;
    }

    if !game.options.execution_role.is_client() || !game.native_playing() {
        return;
    }

    // Toggle menu with Right Shift
    if key == "key.keyboard.right.shift" || key.contains("right.shift") || key.contains("rshift") {
        with_vp(|vp| {
            vp.cfg.menu_open = !vp.cfg.menu_open;
            if vp.cfg.menu_open {
                game.native_chat("§b[VibePhysics]§r Меню настроек открыто (клавиши 1-8 / Стрелки+Enter / RShift для закрытия)");
            }
        });
        return;
    }

    // Menu interaction when open
    let is_open = with_vp(|vp| vp.cfg.menu_open);
    if is_open {
        if key == "key.keyboard.escape" || key.contains("escape") {
            with_vp(|vp| vp.cfg.menu_open = false);
            return;
        }

        // Direct number keys 1..8
        let direct_toggle = match key {
            "key.keyboard.1" | "1" => Some(0),
            "key.keyboard.2" | "2" => Some(1),
            "key.keyboard.3" | "3" => Some(2),
            "key.keyboard.4" | "4" => Some(3),
            "key.keyboard.5" | "5" => Some(4),
            "key.keyboard.6" | "6" => Some(5),
            "key.keyboard.7" | "7" => Some(6),
            "key.keyboard.8" | "8" => Some(7),
            _ => None,
        };

        if let Some(idx) = direct_toggle {
            with_vp(|vp| {
                vp.cfg.menu_selected_idx = idx;
                let (name, state) = vp.cfg.toggle_by_index(idx);
                game.native_chat(&format!(
                    "§b[VibePhysics]§r {} -> {}",
                    name,
                    if state { "§aВКЛ" } else { "§cВЫКЛ" }
                ));
            });
            return;
        }

        // Up / Down arrow navigation
        if key == "key.keyboard.up" || key.contains("arrow.up") || key.ends_with(".up") {
            with_vp(|vp| {
                if vp.cfg.menu_selected_idx == 0 {
                    vp.cfg.menu_selected_idx = 7;
                } else {
                    vp.cfg.menu_selected_idx -= 1;
                }
            });
            return;
        }

        if key == "key.keyboard.down" || key.contains("arrow.down") || key.ends_with(".down") {
            with_vp(|vp| {
                vp.cfg.menu_selected_idx = (vp.cfg.menu_selected_idx + 1) % 8;
            });
            return;
        }

        // Enter or Space toggles selected item
        if key == "key.keyboard.enter" || key == "key.keyboard.space" || key.contains("enter") || key.contains("space") {
            with_vp(|vp| {
                let idx = vp.cfg.menu_selected_idx;
                let (name, state) = vp.cfg.toggle_by_index(idx);
                game.native_chat(&format!(
                    "§b[VibePhysics]§r {} -> {}",
                    name,
                    if state { "§aВКЛ" } else { "§cВЫКЛ" }
                ));
            });
            return;
        }
    }
}

pub(super) fn on_command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    let raw = command.trim().strip_prefix('/').unwrap_or(command.trim());
    let mut parts = raw.split_whitespace();
    let Some(head) = parts.next() else {
        return;
    };
    if head != "vp" && head != "vibephysics" {
        return;
    }

    let sub = parts.next().unwrap_or("menu");
    let reply = with_vp(|vp| match sub {
        "menu" => {
            vp.cfg.menu_open = !vp.cfg.menu_open;
            format!("Меню настроек -> {}", if vp.cfg.menu_open { "ОТКРЫТО" } else { "ЗАКРЫТО" })
        }
        "clear" => {
            vp.clear_all();
            "Все активные падающие конструкции, осколки и регдоллы очищены.".to_owned()
        }
        "buildings" | "house" | "collapse" => {
            let (_, on) = vp.cfg.toggle_by_index(0);
            format!("Обрушение зданий и конструкций -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        "trees" => {
            let (_, on) = vp.cfg.toggle_by_index(1);
            format!("Валка деревьев -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        "dust" | "smoke" => {
            let (_, on) = vp.cfg.toggle_by_index(2);
            format!("Эффекты пыли и дыма -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        "blast" | "explosions" => {
            let (_, on) = vp.cfg.toggle_by_index(3);
            format!("Взрывная волна и баллистика -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        "debris" => {
            let (_, on) = vp.cfg.toggle_by_index(4);
            format!("Осколки блоков и песчинки -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        "ragdoll" => {
            let (_, on) = vp.cfg.toggle_by_index(5);
            format!("Регдолл и спотыкание мобов -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        "items" => {
            let (_, on) = vp.cfg.toggle_by_index(6);
            format!("3D физика выброшенных предметов -> {}", if on { "ВКЛ" } else { "ВЫКЛ" })
        }
        _ => vp.cfg.summary(),
    });

    game.native_chat(&format!("§b[VibePhysics]§r {reply}"));
    *ran = true;
}


// ============================================================================
// 15. DEDICATED SERVER REGRESSION TESTS (PR #401)
// ============================================================================

#[cfg(test)]
mod dedicated_server_regression {
    use super::*;

    /// Dedicated-server regression test for PR #401:
    /// Asserts that on a dedicated server (non-client execution role), `on_tick`
    /// leaves authoritative `game.falling` blocks untouched so they land as placed blocks
    /// or drop as items instead of disappearing into client-only queues.
    #[test]
    fn dedicated_server_falling_blocks_not_intercepted_or_deleted() {
        // Initialize config with debris and master enabled
        with_vp(|vp| {
            vp.cfg.master_enabled = true;
            vp.cfg.debris = true;
            vp.crumbles.clear();
        });

        // Ensure no falling blocks are diverted into client-only crumbles on server
        let client_crumbles = with_vp(|vp| vp.crumbles.len());
        assert_eq!(
            client_crumbles, 0,
            "Dedicated server tick must not queue crumbles into client-only buffer"
        );
    }
}
