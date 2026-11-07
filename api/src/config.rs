//! Modular runtime configuration for **Vibe Physics**.
//!
//! Every subsystem (`ragdoll`, `item_physics`, `debris`, `structural`, `explosions`)
//! has its own `enabled: bool` switch and fine-grained parameters. Changes can be made:
//! 1. By editing `config/vibe_physics-common.toml` (auto-reloaded at runtime by Horizon).
//! 2. In-game via `/vibephysics` or `/vp` commands (e.g., `/vp toggle ragdoll`,
//!    `/vp only item_physics`, `/vp preset cinematic`).
//! 3. Via quick-toggle hotkeys (`F6`..`F10`).

use serde::{Deserialize, Serialize};

/// Default TOML configuration written to `config/vibe_physics-common.toml` on first launch.
pub const DEFAULT_CONFIG_TOML: &str = r#"# =============================================================================
# Vibe Physics — Advanced Physics & Destruction Engine
# Modular Configuration (Live-reloaded on save!)
# =============================================================================

[general]
# Master switch for the entire Vibe Physics engine.
master_enabled = true
# Show a compact status indicator on the HUD when toggling modules.
show_hud_status = true
# Enable F6-F10 hotkeys for quick in-game module toggling.
enable_hotkeys = true
# Global gravity acceleration in m/s^2 (Minecraft default is ~28.0-32.0, Earth is 9.81).
gravity = 24.0

[ragdoll]
# Enable skeletal Ragdoll & Creature Physics (replaces vanilla tipping-over death animation
# and adds articulated procedural physical reactions on hit/blast).
enabled = true
# Enable partial active-ragdoll flinching/stagger when living mobs take damage.
active_hit_reaction = true
# Enable full skeletal ragdoll simulation when a mob or player dies.
death_ragdoll = true
# XPBD constraint solver iterations per frame (higher = stiffer joints).
solver_iterations = 8
# Joint angular stiffness (0.0 = loose, 1.0 = rigid).
joint_stiffness = 0.85
# Bounce/restitution of limbs against world blocks.
limb_restitution = 0.18
# Surface friction of ragdoll bodies sliding on the ground.
ground_friction = 0.72
# Multiplier for incoming weapon/explosion impulse transferred to ragdoll bones.
impulse_scale = 1.35

[item_physics]
# Enable 3D Rigid-Body Dropped Item Physics (items tumble in air, rest flat on surfaces,
# align to slopes, slide on ice, and float/sink in fluids according to material density).
enabled = true
# Angular spin multiplier while airborne.
air_spin_speed = 1.0
# Lay flat on the ground instead of floating & spinning vertically.
lay_flat_on_ground = true
# Apply realistic ice/packed-ice/blue-ice low-friction sliding and water buoyancy on server ticks.
realistic_surface_motion = true
# Tilt items slightly when resting on stairs, slabs, or uneven ledges.
align_to_surface_slopes = true

[debris]
# Enable procedural 3D non-cubic Voronoi / convex polyhedron fracture shards when blocks break.
enabled = true
# Maximum number of active 3D physical fracture shards in the world at once.
max_active_shards = 320
# Number of Voronoi polyhedral shards generated per broken block.
shards_per_block = 9
# Shard lifetime in seconds before smoothly fading/settling out.
shard_lifetime_secs = 8.5
# Enable shard-to-world voxel collision, rolling, and pile settling.
world_collision = true
# Spawn fine settling dust motes after heavy impacts or structural collapses.
settling_dust = true

[structural]
# Enable Structural Integrity & Building Collapse (unsupported overhangs and severed spans
# detach under gravity, tilt as compound structures, and shatter on impact).
enabled = true
# Maximum BFS search nodes when analyzing structural foundation support.
max_support_search_nodes = 256
# Maximum blocks detached in a single structural collapse event.
max_collapse_cluster_size = 48
# Minimum number of unsupported blocks required to trigger a structural collapse.
min_cluster_size = 2
# Material cantilever/overhang multiplier (1.0 = realistic material strength, 2.0 = forgiving).
strength_multiplier = 1.0
# Spawn kinematic 3D tilting collapse clusters on the client in addition to server block edits.
cinematic_tilt_clusters = true

[explosions]
# Enable Realistic Shockwaves, Voxel Raycast Shielding, Acoustic Delay & Camera Shake.
enabled = true
# Scale factor for directional blast wave impulse applied to entities, items, ragdolls & debris.
shockwave_impulse_scale = 1.65
# Raycast through world blocks to shield entities behind solid blast walls.
voxel_occlusion = true
# Shatter vulnerable glass panes/blocks at extended shockwave radii.
extended_glass_shatter = true
# Enable distance-attenuated camera shake and speed-of-sound (343 m/s) shockwave propagation.
camera_shake = true
camera_shake_intensity = 1.0
"#;

/// Identifier for each independent physics subsystem.
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

    pub fn id(self) -> &'static str {
        match self {
            PhysicsModule::Ragdoll => "ragdoll",
            PhysicsModule::ItemPhysics => "item_physics",
            PhysicsModule::Debris => "debris",
            PhysicsModule::Structural => "structural",
            PhysicsModule::Explosions => "explosions",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            PhysicsModule::Ragdoll => "Ragdoll & Creature Physics",
            PhysicsModule::ItemPhysics => "Dropped Item 3D Physics",
            PhysicsModule::Debris => "Voronoi Fracture & Debris",
            PhysicsModule::Structural => "Structural Integrity & Collapse",
            PhysicsModule::Explosions => "Shockwaves & Explosion Physics",
        }
    }

    pub fn from_name(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "ragdoll" | "ragdolls" | "mob" | "mobs" | "creature" | "creatures" | "1" => {
                Some(PhysicsModule::Ragdoll)
            }
            "item" | "items" | "item_physics" | "itemphysics" | "drop" | "drops" | "2" => {
                Some(PhysicsModule::ItemPhysics)
            }
            "debris" | "fracture" | "shards" | "shatter" | "voronoi" | "3" => {
                Some(PhysicsModule::Debris)
            }
            "structural" | "structure" | "collapse" | "destruction" | "integrity" | "4" => {
                Some(PhysicsModule::Structural)
            }
            "explosion" | "explosions" | "shockwave" | "blast" | "5" => {
                Some(PhysicsModule::Explosions)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RagdollConfig {
    pub enabled: bool,
    pub active_hit_reaction: bool,
    pub death_ragdoll: bool,
    pub solver_iterations: usize,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemPhysicsConfig {
    pub enabled: bool,
    pub air_spin_speed: f32,
    pub lay_flat_on_ground: bool,
    pub realistic_surface_motion: bool,
    pub align_to_surface_slopes: bool,
}

impl Default for ItemPhysicsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            air_spin_speed: 1.0,
            lay_flat_on_ground: true,
            realistic_surface_motion: true,
            align_to_surface_slopes: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DebrisConfig {
    pub enabled: bool,
    pub max_active_shards: usize,
    pub shards_per_block: usize,
    pub shard_lifetime_secs: f32,
    pub world_collision: bool,
    pub settling_dust: bool,
}

impl Default for DebrisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_active_shards: 320,
            shards_per_block: 9,
            shard_lifetime_secs: 8.5,
            world_collision: true,
            settling_dust: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StructuralConfig {
    pub enabled: bool,
    pub max_support_search_nodes: usize,
    pub max_collapse_cluster_size: usize,
    pub min_cluster_size: usize,
    pub strength_multiplier: f32,
    pub cinematic_tilt_clusters: bool,
}

impl Default for StructuralConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_support_search_nodes: 256,
            max_collapse_cluster_size: 48,
            min_cluster_size: 2,
            strength_multiplier: 1.0,
            cinematic_tilt_clusters: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExplosionsConfig {
    pub enabled: bool,
    pub shockwave_impulse_scale: f32,
    pub voxel_occlusion: bool,
    pub extended_glass_shatter: bool,
    pub camera_shake: bool,
    pub camera_shake_intensity: f32,
}

impl Default for ExplosionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shockwave_impulse_scale: 1.65,
            voxel_occlusion: true,
            extended_glass_shatter: true,
            camera_shake: true,
            camera_shake_intensity: 1.0,
        }
    }
}

/// Complete configuration for all Vibe Physics subsystems.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VibePhysicsConfig {
    pub general: GeneralConfig,
    pub ragdoll: RagdollConfig,
    pub item_physics: ItemPhysicsConfig,
    pub debris: DebrisConfig,
    pub structural: StructuralConfig,
    pub explosions: ExplosionsConfig,
}

impl VibePhysicsConfig {
    /// Parse from TOML string, falling back gracefully to defaults on malformed fields.
    pub fn from_toml(text: &str) -> Self {
        toml::from_str(text).unwrap_or_default()
    }

    /// Serialize current config to TOML string.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_else(|_| DEFAULT_CONFIG_TOML.to_owned())
    }

    /// Check if a specific module is active (both `general.master_enabled` and module flag).
    pub fn is_active(&self, module: PhysicsModule) -> bool {
        if !self.general.master_enabled {
            return false;
        }
        self.module_enabled(module)
    }

    /// Raw module enabled flag regardless of `master_enabled`.
    pub fn module_enabled(&self, module: PhysicsModule) -> bool {
        match module {
            PhysicsModule::Ragdoll => self.ragdoll.enabled,
            PhysicsModule::ItemPhysics => self.item_physics.enabled,
            PhysicsModule::Debris => self.debris.enabled,
            PhysicsModule::Structural => self.structural.enabled,
            PhysicsModule::Explosions => self.explosions.enabled,
        }
    }

    /// Set a module's enabled state.
    pub fn set_module(&mut self, module: PhysicsModule, enabled: bool) {
        match module {
            PhysicsModule::Ragdoll => self.ragdoll.enabled = enabled,
            PhysicsModule::ItemPhysics => self.item_physics.enabled = enabled,
            PhysicsModule::Debris => self.debris.enabled = enabled,
            PhysicsModule::Structural => self.structural.enabled = enabled,
            PhysicsModule::Explosions => self.explosions.enabled = enabled,
        }
    }

    /// Toggle a module and return its new state.
    pub fn toggle_module(&mut self, module: PhysicsModule) -> bool {
        let next = !self.module_enabled(module);
        self.set_module(module, next);
        next
    }

    /// Enable ONLY the specified modules and disable all others (e.g., `/vp only item_physics`).
    pub fn enable_only(&mut self, active: &[PhysicsModule]) {
        self.general.master_enabled = true;
        for m in PhysicsModule::ALL {
            self.set_module(m, active.contains(&m));
        }
    }

    /// Apply a named preset.
    pub fn apply_preset(&mut self, preset: &str) -> bool {
        match preset.trim().to_ascii_lowercase().as_str() {
            "all" | "full" | "realistic" => {
                *self = Self::default();
                true
            }
            "cinematic" => {
                *self = Self::default();
                self.ragdoll.impulse_scale = 1.75;
                self.debris.max_active_shards = 480;
                self.debris.shards_per_block = 12;
                self.structural.max_collapse_cluster_size = 64;
                self.explosions.shockwave_impulse_scale = 2.1;
                self.explosions.camera_shake_intensity = 1.35;
                true
            }
            "light" | "performance" => {
                *self = Self::default();
                self.structural.enabled = false;
                self.debris.max_active_shards = 120;
                self.debris.shards_per_block = 5;
                self.ragdoll.solver_iterations = 5;
                true
            }
            "items_only" | "items" => {
                self.enable_only(&[PhysicsModule::ItemPhysics]);
                true
            }
            "off" | "vanilla" | "none" => {
                for m in PhysicsModule::ALL {
                    self.set_module(m, false);
                }
                true
            }
            _ => false,
        }
    }

    /// Format a one-line summary of all modules for chat / HUD display.
    pub fn status_summary(&self) -> String {
        if !self.general.master_enabled {
            return "[VibePhysics] MASTER OFF (all physics paused)".to_owned();
        }
        let flag = |b: bool| if b { "ON" } else { "OFF" };
        format!(
            "[VibePhysics] Ragdoll: {} | Items: {} | Debris: {} | Structural: {} | Explosions: {}",
            flag(self.ragdoll.enabled),
            flag(self.item_physics.enabled),
            flag(self.debris.enabled),
            flag(self.structural.enabled),
            flag(self.explosions.enabled),
        )
    }
}
