#![forbid(unsafe_code)]
//! # Vibe Physics — Advanced Physics & Destruction Engine (`vibe_physics`)
//!
//! A modular, full-featured native trait mod for **Horizon** (Minecraft Java Edition 26.3 in Rust).
//!
//! ## Five Independent, Runtime-Toggleable Physics Subsystems
//! 1. **[`ragdoll`] — Ragdoll & Creature Physics**:
//!    - Articulated multi-bone XPBD skeletal ragdolls (`Biped`, `Quadruped`, `Creeper`, `Arachnid`).
//!    - World voxel collision, limb restitution, ground friction, and directional hit/blast impulse transfer.
//!    - Hybrid active-ragdoll upper-body flinching when living creatures take damage (`hurt_time > 0`).
//! 2. **[`item_physics`] — Dropped Item 3D Rigid-Body Physics**:
//!    - 3D quaternion orientation and airborne tumbling (`inject_vibe_physics_item_pose`).
//!    - Flat resting pose on surfaces for 2D items (`X` pitch $\approx 90^\circ$), cubic face resting for 3D blocks,
//!      surface slope alignment, ice low-friction gliding, and material-aware water buoyancy.
//! 3. **[`debris`] — Block Fracture & Procedural 3D Voronoi Debris**:
//!    - Non-cubic 3D Sutherland–Hodgman Voronoi convex polyhedron shattering when blocks break or explode.
//!    - Material-aware fracture geometry (`GlassShards`, `WoodSplinters`, `StonePolyhedra`, `MetalPlates`,
//!      `GranularClumps`, `FoliageFragments`), 6-DoF rigid-body voxel collisions, pile settling, and dust motes.
//! 4. **[`structural`] — Structural Integrity & Building Collapse**:
//!    - 3D BFS foundation & cantilever stress graph analyzer with material overhang limits.
//!    - Unsupported building sections detach (`set_block` + `spawn_falling_block`), tilt as compound 3D
//!      rigid clusters (`CollapsingStructureCluster`), and shatter into Voronoi debris upon ground impact.
//! 5. **[`explosions`] — Realistic Explosions & Shockwaves**:
//!    - Spherical pressure wave with 3D voxel raycast shielding/occlusion (`raycast_shockwave_transmission`).
//!    - Extended shockwave glass shattering, flying crater rim boulders, directional entity/ragdoll/debris
//!      impulse, speed-of-sound ($343\text{ m/s}$) acoustic delay, and distance-attenuated `camera_shake`.

pub mod config;
pub mod debris;
pub mod explosions;
pub mod item_physics;
pub mod materials;
pub mod math_phys;
pub mod ragdoll;
pub mod structural;

use std::sync::Mutex;

use glam::Vec3;
use horizon_client::mod_api::*;

use crate::config::{ DEFAULT_CONFIG_TOML, PhysicsModule, VibePhysicsConfig };
use crate::debris::DebrisEngine;
use crate::explosions::{ ExplosionEngine, compute_weapon_knockback_impulse };
use crate::item_physics::ItemPhysicsEngine;
use crate::ragdoll::RagdollEngine;
use crate::structural::StructuralEngine;

/// Runtime state container protected by a `Mutex` so `VibePhysicsMod` implements `Hooks: Send + Sync`.
struct EngineState {
    config: VibePhysicsConfig,
    ragdoll: RagdollEngine,
    item_physics: ItemPhysicsEngine,
    debris: DebrisEngine,
    structural: StructuralEngine,
    explosions: ExplosionEngine,
}

impl EngineState {
    fn new() -> Self {
        Self {
            config: VibePhysicsConfig::default(),
            ragdoll: RagdollEngine::new(),
            item_physics: ItemPhysicsEngine::new(),
            debris: DebrisEngine::new(),
            structural: StructuralEngine::new(),
            explosions: ExplosionEngine::new(),
        }
    }

    fn clear_all_simulations(&mut self) {
        self.ragdoll.clear();
        self.item_physics.clear();
        self.debris.clear();
        self.structural.clear();
        self.explosions.clear();
    }
}

/// Top-level native mod struct implementing [`Hooks`].
pub struct VibePhysicsMod {
    state: Mutex<EngineState>,
}

impl VibePhysicsMod {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::new()),
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, EngineState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Execute a `/vibephysics` or `/vp` command and return a human-readable feedback string.
    fn execute_vp_command(&self, raw_cmd: &str) -> Option<String> {
        let trimmed = raw_cmd.trim().strip_prefix('/').unwrap_or(raw_cmd.trim());
        let mut parts = trimmed.split_whitespace();
        let root = parts.next()?.to_ascii_lowercase();
        if root != "vibephysics" && root != "vp" {
            return None;
        }

        let mut st = self.lock_state();
        let sub = parts.next().unwrap_or("status").to_ascii_lowercase();

        match sub.as_str() {
            "status" | "info" | "list" => Some(format!(
                "{} | Active: [ragdolls={}, items={}, shards={}, clusters={}, waves={}]",
                st.config.status_summary(),
                st.ragdoll.active_count(),
                st.item_physics.tracked_count(),
                st.debris.active_shard_count(),
                st.structural.active_cluster_count(),
                st.explosions.active_shockwave_count(),
            )),
            "toggle" => {
                if let Some(target) = parts.next() {
                    if target.eq_ignore_ascii_case("all") || target.eq_ignore_ascii_case("master")
                    {
                        st.config.general.master_enabled = !st.config.general.master_enabled;
                        return Some(st.config.status_summary());
                    }
                    if target.eq_ignore_ascii_case("hud") {
                        st.config.general.show_hud_status = !st.config.general.show_hud_status;
                        return Some(format!(
                            "[VibePhysics] HUD status overlay: {}",
                            if st.config.general.show_hud_status {
                                "ON"
                            } else {
                                "OFF"
                            }
                        ));
                    }
                    if let Some(m) = PhysicsModule::from_name(target) {
                        let new_state = st.config.toggle_module(m);
                        return Some(format!(
                            "[VibePhysics] {} -> {} | {}",
                            m.display_name(),
                            if new_state { "ENABLED" } else { "DISABLED" },
                            st.config.status_summary()
                        ));
                    }
                    Some(
                        "[VibePhysics] Unknown module. Use: ragdoll | item_physics | debris | structural | explosions | hud | master"
                            .to_owned(),
                    )
                } else {
                    st.config.general.master_enabled = !st.config.general.master_enabled;
                    Some(st.config.status_summary())
                }
            }
            "enable" | "on" => {
                if let Some(target) = parts.next() {
                    if target.eq_ignore_ascii_case("all") {
                        st.config.general.master_enabled = true;
                        for m in PhysicsModule::ALL {
                            st.config.set_module(m, true);
                        }
                        return Some(st.config.status_summary());
                    }
                    if let Some(m) = PhysicsModule::from_name(target) {
                        st.config.set_module(m, true);
                        return Some(format!(
                            "[VibePhysics] {} -> ENABLED | {}",
                            m.display_name(),
                            st.config.status_summary()
                        ));
                    }
                }
                Some(
                    "[VibePhysics] Usage: /vp enable <ragdoll|item_physics|debris|structural|explosions|all>"
                        .to_owned(),
                )
            }
            "disable" | "off" => {
                if let Some(target) = parts.next() {
                    if target.eq_ignore_ascii_case("all") {
                        for m in PhysicsModule::ALL {
                            st.config.set_module(m, false);
                        }
                        return Some(st.config.status_summary());
                    }
                    if let Some(m) = PhysicsModule::from_name(target) {
                        st.config.set_module(m, false);
                        return Some(format!(
                            "[VibePhysics] {} -> DISABLED | {}",
                            m.display_name(),
                            st.config.status_summary()
                        ));
                    }
                }
                Some(
                    "[VibePhysics] Usage: /vp disable <ragdoll|item_physics|debris|structural|explosions|all>"
                        .to_owned(),
                )
            }
            "only" => {
                let mut selected = Vec::new();
                for arg in parts {
                    for token in arg.split(',') {
                        if let Some(m) = PhysicsModule::from_name(token) {
                            selected.push(m);
                        }
                    }
                }
                if selected.is_empty() {
                    return Some(
                        "[VibePhysics] Usage: /vp only <module1> [module2...] (e.g. `/vp only item_physics`)"
                            .to_owned(),
                    );
                }
                st.config.enable_only(&selected);
                Some(format!(
                    "[VibePhysics] Isolated selected modules! {}",
                    st.config.status_summary()
                ))
            }
            "preset" => {
                let preset = parts.next().unwrap_or("realistic");
                if st.config.apply_preset(preset) {
                    Some(format!(
                        "[VibePhysics] Applied preset '{}' -> {}",
                        preset,
                        st.config.status_summary()
                    ))
                } else {
                    Some(
                        "[VibePhysics] Presets: realistic | cinematic | performance | items_only | vanilla"
                            .to_owned(),
                    )
                }
            }
            "clear" | "reset" => {
                st.clear_all_simulations();
                Some("[VibePhysics] Cleared all active ragdolls, shards, and collapsing clusters.".to_owned())
            }
            _ => Some(
                "[VibePhysics] Commands: /vp status | /vp toggle <module> | /vp only <module> | /vp enable <module> | /vp disable <module> | /vp preset <realistic|cinematic|performance|items_only|vanilla> | /vp clear"
                    .to_owned(),
            ),
        }
    }
}

impl Default for VibePhysicsMod {
    fn default() -> Self {
        Self::new()
    }
}

/// Entrypoint called by the Horizon native mod loader.
pub fn hooks() -> Box<dyn Hooks> {
    Box::new(VibePhysicsMod::new())
}

impl Hooks for VibePhysicsMod {
    // =========================================================================
    // 1. Configuration Registration & Live Reloading (`config/vibe_physics-common.toml`)
    // =========================================================================

    fn register_configs(&self, _host: &mut dyn Host, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(NativeConfig::new(
            "vibe_physics",
            NativeConfigKind::Common,
            DEFAULT_CONFIG_TOML,
        ));
    }

    fn config_loading(&self, _host: &mut dyn Host, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == "vibe_physics" {
            let mut st = self.lock_state();
            st.config = VibePhysicsConfig::from_toml(&event.config.text);
        }
    }

    fn config_reloading(&self, _host: &mut dyn Host, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == "vibe_physics" {
            let mut st = self.lock_state();
            st.config = VibePhysicsConfig::from_toml(&event.config.text);
        }
    }

    // =========================================================================
    // 2. Client & Server Commands (`/vibephysics`, `/vp`) & Hotkeys (`F6`..`F10`)
    // =========================================================================

    fn commands(&self, _host: &mut dyn Host, event: &mut CommandsEvent<'_>) {
        event.commands.push("vibephysics");
        event.commands.push("vp");
    }

    fn command(&self, host: &mut dyn Host, event: &mut CommandEvent<'_>) -> Flow {
        if let Some(reply) = self.execute_vp_command(event.command) {
            host.client_action_bar(Text::literal(&reply), false);
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        for usage in [
            "vp status",
            "vp toggle <ragdoll|item_physics|debris|structural|explosions|hud|master>",
            "vp only <module>",
            "vp enable <module|all>",
            "vp disable <module|all>",
            "vp preset <realistic|cinematic|performance|items_only|vanilla>",
            "vp clear",
            "vibephysics status",
            "vibephysics toggle <module>",
            "vibephysics only <module>",
        ] {
            event.commands.push(NativeCommand {
                usage: usage.to_owned(),
                level: 0,
            });
        }
    }

    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        if let Some(reply) = self.execute_vp_command(event.command) {
            let _ = host.action_bar(event.player.uuid, Text::literal(&reply));
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !event.pressed {
            return;
        }
        let mut st = self.lock_state();
        if !st.config.general.enable_hotkeys {
            return;
        }
        let toggled = match event.key {
            "key.keyboard.f6" => Some(PhysicsModule::Ragdoll),
            "key.keyboard.f7" => Some(PhysicsModule::ItemPhysics),
            "key.keyboard.f8" => Some(PhysicsModule::Debris),
            "key.keyboard.f9" => Some(PhysicsModule::Structural),
            "key.keyboard.f10" => Some(PhysicsModule::Explosions),
            _ => None,
        };
        if let Some(module) = toggled {
            let enabled = st.config.toggle_module(module);
            let msg = format!(
                "[VibePhysics] {} -> {} ({})",
                module.display_name(),
                if enabled { "ON" } else { "OFF" },
                st.config.status_summary()
            );
            drop(st);
            host.client_action_bar(Text::literal(&msg), false);
        }
    }

    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let st = self.lock_state();
        if !st.config.general.show_hud_status {
            return;
        }
        let tag = |on: bool| if on && st.config.general.master_enabled { "ON" } else { "OFF" };
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
        drop(st);

        let w = host
            .text_width(&line1)
            .max(host.text_width(&line2))
            + 6.0;
        let h = 22.0;
        let (x, y) = event.corners.place(Corner::TopLeft, w, h);
        host.fill(event.batch, [x, y, w, h], 0x6800_0000);
        host.text(event.batch, &line1, [x + 3.0, y + 2.0], 0x90_EE90, true);
        host.text(event.batch, &line2, [x + 3.0, y + 12.0], 0xD0_E8FF, true);
    }

    // =========================================================================
    // 3. Module 1 — Ragdoll & Creature Physics (`mob_submit`, `player_submit`)
    // =========================================================================

    fn player_render_listened(
        &self,
        _host: &dyn Host,
        event: &mut PlayerRenderListenedEvent<'_>,
    ) {
        let st = self.lock_state();
        if st.config.is_active(PhysicsModule::Ragdoll) {
            event.listened = true;
        }
    }

    fn mob_submit(&self, host: &dyn Host, event: &mut MobSubmitEvent<'_, '_>) -> Flow {
        let mut st = self.lock_state();
        if !st.config.is_active(PhysicsModule::Ragdoll) {
            return Flow::Continue;
        }
        let cfg = st.config.ragdoll.clone();
        let gravity = st.config.general.gravity;
        st.ragdoll
            .submit_creature(host, event.mob, None, event.draw, &cfg, gravity)
    }

    fn player_submit(&self, host: &dyn Host, event: &mut PlayerSubmitEvent<'_, '_>) -> Flow {
        let mut st = self.lock_state();
        if !st.config.is_active(PhysicsModule::Ragdoll) {
            return Flow::Continue;
        }
        let cfg = st.config.ragdoll.clone();
        let gravity = st.config.general.gravity;
        st.ragdoll.submit_creature(
            host,
            event.player,
            Some(event.who),
            event.draw,
            &cfg,
            gravity,
        )
    }

    fn living_knockback_v2(&self, event: &mut LivingKnockbackV2Event<'_>) -> Flow {
        let mut st = self.lock_state();
        if !st.config.is_active(PhysicsModule::Ragdoll) {
            return Flow::Continue;
        }
        let weapon = event.attacker_main.map(|s| s.item.as_str());
        let impulse = compute_weapon_knockback_impulse(
            &mut event.strength,
            &mut event.x,
            &mut event.z,
            weapon,
        );
        if event.entity >= 0 {
            st.ragdoll
                .record_impulse(event.entity as u32, impulse, 0);
        }
        Flow::Continue
    }

    fn living_damage_post(&self, event: &mut LivingDamagePostEvent<'_>) {
        let mut st = self.lock_state();
        if !st.config.is_active(PhysicsModule::Ragdoll) || event.entity < 0 {
            return;
        }
        // Transfer damage magnitude into a vertical/directional ragdoll jolt
        let mag = (event.amount * 0.65).clamp(1.0, 14.0);
        st.ragdoll
            .record_impulse(event.entity as u32, Vec3::new(0.0, mag * 0.45, 0.0), 0);
    }

    // =========================================================================
    // 4. Module 2 — Dropped Item 3D Physics (`inject_vibe_physics_item_pose`, `entity_tick_post`)
    // =========================================================================

    fn inject_vibe_physics_item_pose(&self, event: &mut InjectVibePhysicsItemPoseEvent<'_>) {
        let mut st = self.lock_state();
        if !st.config.is_active(PhysicsModule::ItemPhysics) {
            return;
        }
        let cfg = st.config.item_physics.clone();
        st.item_physics.update_item_pose(event, &cfg);
    }

    fn entity_tick_listened(&self, _host: &mut dyn Host, event: &mut EntityTickListenedEvent<'_>) {
        let st = self.lock_state();
        if st.config.is_active(PhysicsModule::ItemPhysics) {
            event.listened = true;
        }
    }

    fn entity_tick_post(&self, host: &mut dyn Host, event: &mut EntityTickPostEvent<'_>) {
        if event.kind != "minecraft:item" {
            return;
        }
        let st = self.lock_state();
        if !st.config.is_active(PhysicsModule::ItemPhysics) {
            return;
        }
        let cfg = st.config.item_physics.clone();
        drop(st);
        let st = self.lock_state();
        st.item_physics
            .server_tick_item_entity(host, event.entity, &cfg);
    }

    // =========================================================================
    // 5. Module 3 & 4 — Block Fracture Voronoi Debris & Structural Collapse
    // =========================================================================

    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        let broken_id = host.block(event.pos);
        let dimension = host
            .dimension()
            .unwrap_or("minecraft:overworld")
            .to_owned();

        let mut st = self.lock_state();
        if st.config.is_active(PhysicsModule::Debris) {
            let debris_cfg = st.config.debris.clone();
            st.debris.spawn_block_fracture(
                event.pos,
                &broken_id,
                None,
                Vec3::ZERO,
                &debris_cfg,
            );
        }
        if st.config.is_active(PhysicsModule::Structural) {
            let struct_cfg = st.config.structural.clone();
            st.structural
                .on_server_block_removed(host, &dimension, event.pos, &struct_cfg);
        }
        Flow::Continue
    }

    // =========================================================================
    // 6. Module 5 — Realistic Explosions & Shockwaves (`explosion_detonate`, `explosion_knockback`, `camera_shake`)
    // =========================================================================

    fn explosion_detonate(
        &self,
        host: &mut dyn Host,
        event: &mut ExplosionDetonateEvent<'_, '_>,
    ) {
        let mut st = self.lock_state();
        if !st.config.general.master_enabled {
            return;
        }
        let EngineState {
            ref config,
            ref mut ragdoll,
            ref mut item_physics,
            ref mut debris,
            ref mut structural,
            ref mut explosions,
        } = *st;

        if config.explosions.enabled {
            explosions.on_explosion_detonate(
                host,
                event,
                &config.explosions,
                &config.debris,
                debris,
                ragdoll,
                item_physics,
            );
        } else if config.debris.enabled {
            // Even if shockwaves are off, still fracture blocks destroyed by the blast if Debris is ON
            let center = Vec3::new(
                event.center[0] as f32,
                event.center[1] as f32,
                event.center[2] as f32,
            );
            let stride = (event.blocks.len() / 14).max(1);
            for &bpos in event.blocks.iter().step_by(stride) {
                let bid = host.block(bpos);
                debris.spawn_block_fracture(bpos, &bid, Some(center), Vec3::ZERO, &config.debris);
            }
        }

        if config.structural.enabled {
            structural.on_server_explosion_blocks_removed(
                host,
                event.dimension,
                event.blocks,
                &config.structural,
            );
        }
    }

    fn explosion_knockback(
        &self,
        host: &mut dyn Host,
        event: &mut ExplosionKnockbackEvent<'_, '_>,
    ) {
        let mut st = self.lock_state();
        if !st.config.is_active(PhysicsModule::Explosions) {
            return;
        }
        let EngineState {
            ref config,
            ref mut ragdoll,
            ref mut explosions,
            ..
        } = *st;
        explosions.on_explosion_knockback(host, event, &config.explosions, ragdoll);
    }

    fn camera_shake(&self, host: &dyn Host, event: &mut CameraShakeEvent<'_, '_>) {
        let st = self.lock_state();
        if !st.config.is_active(PhysicsModule::Explosions) {
            return;
        }
        st.explosions
            .apply_camera_shake(host, event, &st.config.explosions);
    }

    // =========================================================================
    // 7. Unified World-Space 3D Physics Rendering (`Hooks::world_submit`)
    // =========================================================================

    fn world_submit(&self, host: &dyn Host, event: &mut WorldSubmitEvent<'_>) {
        let mut st = self.lock_state();
        if !st.config.general.master_enabled {
            return;
        }
        let gravity = st.config.general.gravity;
        let EngineState {
            ref config,
            ref mut debris,
            ref mut structural,
            ref mut explosions,
            ..
        } = *st;

        // 1. Step & render collapsing building clusters (which shatter into Voronoi shards on ground impact)
        if config.structural.enabled {
            structural.step_and_render(
                host,
                event.draw,
                &config.structural,
                &config.debris,
                debris,
                gravity,
            );
        }

        // 2. Step & render 3D Voronoi polyhedral fracture shards and settling dust motes
        if config.debris.enabled {
            debris.step_and_render(host, event.draw, &config.debris, gravity);
        }

        // 3. Step & render expanding shockwave pressure rings
        if config.explosions.enabled {
            explosions.step_and_render(host, event.draw, &config.explosions);
        }
    }

    fn client_level_unload(&self, _host: &mut dyn Host, _event: &mut ClientLevelUnloadEvent<'_>) {
        let mut st = self.lock_state();
        st.clear_all_simulations();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debris::build_voronoi_cell_polyhedron;
    use crate::materials::{FractureStyle, classify_material};
    use crate::math_phys::{FastRng, integrate_quat, solve_distance_constraint};

    #[test]
    fn modular_config_toggles_and_isolates_subsystems() {
        let mut cfg = VibePhysicsConfig::from_toml(DEFAULT_CONFIG_TOML);
        assert!(cfg.is_active(PhysicsModule::Ragdoll));
        assert!(cfg.is_active(PhysicsModule::ItemPhysics));
        assert!(cfg.is_active(PhysicsModule::Debris));
        assert!(cfg.is_active(PhysicsModule::Structural));
        assert!(cfg.is_active(PhysicsModule::Explosions));

        // Isolate only ItemPhysics (disable ragdoll & others)
        cfg.enable_only(&[PhysicsModule::ItemPhysics]);
        assert!(!cfg.is_active(PhysicsModule::Ragdoll));
        assert!(cfg.is_active(PhysicsModule::ItemPhysics));
        assert!(!cfg.is_active(PhysicsModule::Debris));
        assert!(!cfg.is_active(PhysicsModule::Structural));
        assert!(!cfg.is_active(PhysicsModule::Explosions));
    }

    #[test]
    fn vp_commands_control_every_module_at_runtime() {
        let m = VibePhysicsMod::new();
        let reply = m.execute_vp_command("/vp only item_physics").unwrap();
        assert!(reply.contains("Ragdoll: OFF"));
        assert!(reply.contains("Items: ON"));

        let reply2 = m.execute_vp_command("/vp toggle ragdoll").unwrap();
        assert!(reply2.contains("Ragdoll: ON"));
        assert!(reply2.contains("Items: ON"));
    }

    #[test]
    fn voronoi_polyhedron_clipping_produces_valid_non_cubic_3d_shards() {
        let mut rng = FastRng::from_seed(42);
        let seeds = vec![
            Vec3::new(0.25, 0.25, 0.25),
            Vec3::new(0.75, 0.25, 0.30),
            Vec3::new(0.30, 0.75, 0.65),
            Vec3::new(0.70, 0.70, 0.75),
        ];
        for i in 0..seeds.len() {
            let (centroid, radius, faces) =
                build_voronoi_cell_polyhedron(i, &seeds, Vec3::ONE, &mut rng);
            assert!(centroid.x >= 0.0 && centroid.x <= 1.0);
            assert!(radius > 0.05 && radius < 1.0);
            assert!(faces.len() >= 4, "convex polyhedron must have at least 4 faces");
        }
    }

    #[test]
    fn material_database_distinguishes_glass_wood_stone_ice_metal() {
        let glass = classify_material("minecraft:tinted_glass");
        assert_eq!(glass.fracture_style, FractureStyle::GlassShards);
        assert!(glass.shockwave_fragile);

        let wood = classify_material("minecraft:oak_planks");
        assert_eq!(wood.fracture_style, FractureStyle::WoodSplinters);
        assert!(wood.floats_in_water());

        let metal = classify_material("minecraft:iron_block");
        assert_eq!(metal.fracture_style, FractureStyle::MetalPlates);
        assert!(!metal.floats_in_water());

        let ice = classify_material("minecraft:blue_ice");
        assert!(ice.friction < 0.03);
    }

    #[test]
    fn xpbd_distance_constraint_converges_to_rest_length() {
        let mut p1 = Vec3::new(0.0, 0.0, 0.0);
        let mut p2 = Vec3::new(2.0, 0.0, 0.0);
        for _ in 0..8 {
            solve_distance_constraint(&mut p1, 1.0, &mut p2, 1.0, 1.0, 0.9);
        }
        let dist = (p2 - p1).length();
        assert!((dist - 1.0).abs() < 1e-3, "dist was {dist}");
        let q = integrate_quat(glam::Quat::IDENTITY, Vec3::new(0.0, 3.14, 0.0), 0.1);
        assert!((q.length() - 1.0).abs() < 1e-5);
    }
}
