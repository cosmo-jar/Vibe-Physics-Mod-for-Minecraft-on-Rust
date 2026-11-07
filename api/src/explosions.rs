//! Module 5 — **Realistic Explosions & Shockwaves** (`explosions`).
//!
//! Upgrades vanilla explosions into a physical pressure wave & acoustic simulation:
//! 1. **Spherical Shockwave Expansion & Extended Glass Shattering** (`explosion_detonate`):
//!    - Estimates blast power and outer shockwave radius from the detonation block list.
//!    - Raycasts outward (`raycast_shockwave_transmission`) to shatter fragile blocks (glass,
//!      stained glass, panes, ice, glowstone, lanterns) at extended overpressure radii,
//!      adding them to `event.blocks` and spawning 3D Voronoi glass shards (`DebrisEngine`).
//!    - Launches a subset of outer crater rim blocks into ballistic trajectories via
//!      `host.spawn_falling_block` so explosions throw real flying boulders & masonry!
//! 2. **Voxel Raycast Blast Shielding & Directional Impulse** (`explosion_knockback`):
//!    - Computes line-of-sight transmission from the blast epicenter to each affected entity.
//!    - Solid blast walls (obsidian, reinforced stone, metal blocks) shield entities behind them,
//!      while unobstructed entities receive an amplified radial + upward pressure impulse.
//! 3. **Speed-of-Sound ($343\text{ m/s}$) Acoustic Wavefront & Camera Shake** (`camera_shake`, `world_submit`):
//!    - Tracks expanding shockwave fronts (`ActiveShockwave`).
//!    - Applies distance-attenuated, frequency-modulated camera shake (`NativeShake` pitch/yaw/roll/offset)
//!      when the shockwave front reaches the player's camera position.
//!    - Renders a subtle expanding ground pressure ring in `Hooks::world_submit`.

use glam::Vec3;
use horizon_client::mod_api::{
    CameraShakeEvent, ExplosionDetonateEvent, ExplosionKnockbackEvent, Host, NativeFalling,
    NativeWorldDraw,
};

use crate::config::{DebrisConfig, ExplosionsConfig};
use crate::debris::DebrisEngine;
use crate::item_physics::ItemPhysicsEngine;
use crate::materials::{classify_material, is_air_or_fluid, is_unbreakable_anchor};
use crate::math_phys::{FastRng, raycast_shockwave_transmission};
use crate::ragdoll::RagdollEngine;

/// Speed of sound in air ($\text{m/s}$) for shockwave arrival calculation.
pub const SPEED_OF_SOUND_MPS: f32 = 343.0;

/// An active expanding spherical shockwave front in the world.
#[derive(Debug, Clone, Copy)]
pub struct ActiveShockwave {
    /// World-space epicenter of the blast.
    pub center: Vec3,
    /// Estimated blast radius ($\text{m}$).
    pub base_radius: f32,
    /// Outer overpressure radius ($\text{m}$).
    pub max_radius: f32,
    /// Elapsed time since detonation ($\text{s}$).
    pub elapsed_secs: f32,
    /// Total duration of shockwave + aftershock rumble ($\text{s}$).
    pub duration_secs: f32,
    /// Peak intensity factor.
    pub intensity: f32,
}

/// Manages active shockwaves, blast occlusion, and camera shake.
pub struct ExplosionEngine {
    shockwaves: Vec<ActiveShockwave>,
    last_time_key: Option<(i64, f32)>,
    blast_counter: u64,
}

impl Default for ExplosionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExplosionEngine {
    pub fn new() -> Self {
        Self {
            shockwaves: Vec::new(),
            last_time_key: None,
            blast_counter: 0,
        }
    }

    pub fn active_shockwave_count(&self) -> usize {
        self.shockwaves.len()
    }

    pub fn clear(&mut self) {
        self.shockwaves.clear();
    }

    /// Handle `Hooks::explosion_detonate` on the server.
    /// - Shatters fragile glass/ice blocks along unobstructed shockwave rays beyond the core crater.
    /// - Spawns ballistic `FallingBlock` rubble from the outer crater rim.
    /// - Spawns 3D Voronoi fracture shards (`DebrisEngine`) and registers an expanding `ActiveShockwave`.
    pub fn on_explosion_detonate(
        &mut self,
        host: &mut dyn Host,
        event: &mut ExplosionDetonateEvent<'_, '_>,
        cfg: &ExplosionsConfig,
        debris_cfg: &DebrisConfig,
        debris_engine: &mut DebrisEngine,
        ragdoll_engine: &mut RagdollEngine,
        item_engine: &mut ItemPhysicsEngine,
    ) {
        if !cfg.enabled {
            return;
        }

        let center = Vec3::new(
            event.center[0] as f32,
            event.center[1] as f32,
            event.center[2] as f32,
        );
        let block_count = event.blocks.len();
        let estimated_radius = ((block_count as f32).cbrt() * 1.45).clamp(3.0, 16.0);
        let shockwave_radius = estimated_radius * 2.4;

        self.blast_counter = self.blast_counter.wrapping_add(1);
        let mut rng = FastRng::from_seed(self.blast_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15));

        // 1. Extended glass / fragile block shattering via 3D shockwave raycasting
        if cfg.extended_glass_shatter {
            let mut fragile_hits: Vec<[i32; 3]> = Vec::new();
            let ray_count = 36;
            for _ in 0..ray_count {
                let dir = rng.unit_vec3();
                let end = center + dir * shockwave_radius;
                let _ = raycast_shockwave_transmission(host, center, end, true, &mut fragile_hits);
            }
            for fpos in fragile_hits {
                if !event.blocks.contains(&fpos) {
                    event.blocks.push(fpos);
                }
            }
        }

        // 2. Spawn 3D Voronoi fracture shards for a sampled subset of the blasted blocks
        //    and launch outer-rim blocks as flying `FallingBlock` boulders!
        let total = event.blocks.len();
        let shard_stride = (total / 18).max(1);
        let boulder_stride = (total / 10).max(1);
        let air_state = host.default_state("minecraft:air");

        for (idx, &bpos) in event.blocks.iter().enumerate() {
            let block_id = host.block(bpos);
            if is_air_or_fluid(&block_id) || is_unbreakable_anchor(&block_id) {
                continue;
            }

            if idx % shard_stride == 0 {
                debris_engine.spawn_block_fracture(
                    bpos,
                    &block_id,
                    Some(center),
                    Vec3::ZERO,
                    debris_cfg,
                );
            } else if idx % boulder_stride == 0 {
                // Throw outer blocks as physical FallingBlock projectiles
                if let (Some(air), Some(state_id)) = (air_state, host.default_state(&block_id)) {
                    let b_center =
                        Vec3::new(bpos[0] as f32 + 0.5, bpos[1] as f32 + 0.5, bpos[2] as f32 + 0.5);
                    let delta = b_center - center;
                    let dist = delta.length().max(0.5);
                    if dist > estimated_radius * 0.55 {
                        let dir = (delta / dist + Vec3::new(0.0, 0.65, 0.0)).normalize_or(Vec3::Y);
                        let speed = (0.45 * cfg.shockwave_impulse_scale).clamp(0.2, 1.25);
                        let vel = [
                            (dir.x * speed + rng.signed_f32() * 0.08) as f64,
                            (dir.y * speed + rng.range_f32(0.12, 0.35)) as f64,
                            (dir.z * speed + rng.signed_f32() * 0.08) as f64,
                        ];
                        let _ = host.set_block(event.dimension, bpos, air, 3);
                        let _ = host.spawn_falling_block(
                            event.dimension,
                            state_id,
                            [bpos[0] as f64 + 0.5, bpos[1] as f64, bpos[2] as f64 + 0.5],
                            vel,
                            NativeFalling {
                                drop_item: true,
                                hurts: true,
                            },
                        );
                    }
                }
            }
        }

        // 3. Push all active ragdolls, items, and debris shards with the shockwave
        let wave_strength = estimated_radius * 2.2 * cfg.shockwave_impulse_scale;
        debris_engine.apply_explosion_wave(center, shockwave_radius, wave_strength);
        ragdoll_engine.apply_explosion_wave(center, shockwave_radius, wave_strength);
        item_engine.apply_explosion_wave(center, shockwave_radius, wave_strength);

        // 4. Register expanding shockwave for camera shake & visual wavefront ring
        if self.shockwaves.len() < 16 {
            self.shockwaves.push(ActiveShockwave {
                center,
                base_radius: estimated_radius,
                max_radius: shockwave_radius * 2.2,
                elapsed_secs: 0.0,
                duration_secs: 1.85,
                intensity: (estimated_radius / 4.0).clamp(0.6, 2.5) * cfg.camera_shake_intensity,
            });
        }
    }

    /// Handle `Hooks::explosion_knockback` on the server.
    /// Applies voxel raycast blast shielding/occlusion and directional overpressure scaling.
    pub fn on_explosion_knockback(
        &mut self,
        host: &mut dyn Host,
        event: &mut ExplosionKnockbackEvent<'_, '_>,
        cfg: &ExplosionsConfig,
        ragdoll_engine: &mut RagdollEngine,
    ) {
        if !cfg.enabled {
            return;
        }

        let center = Vec3::new(
            event.center[0] as f32,
            event.center[1] as f32,
            event.center[2] as f32,
        );

        let mut transmission = 1.0_f32;
        if cfg.voxel_occlusion
            && let Some(entity_id) = event.entity
            && let Some(epos) = host.entity_position(entity_id)
        {
            let target = Vec3::new(epos[0] as f32, epos[1] as f32 + 0.8, epos[2] as f32);
            let mut dummy = Vec::new();
            transmission = raycast_shockwave_transmission(host, center, target, false, &mut dummy);
        }

        let scale = (cfg.shockwave_impulse_scale * transmission) as f64;
        event.knockback_velocity[0] *= scale;
        event.knockback_velocity[1] = (event.knockback_velocity[1] * scale
            + 0.18 * (transmission as f64))
            .clamp(-3.5, 3.5);
        event.knockback_velocity[2] *= scale;

        if let Some(entity_id) = event.entity {
            let impulse = Vec3::new(
                event.knockback_velocity[0] as f32 * 14.0,
                event.knockback_velocity[1] as f32 * 14.0,
                event.knockback_velocity[2] as f32 * 14.0,
            );
            ragdoll_engine.record_impulse(entity_id, impulse, 0);
        }
    }

    /// Handle `Hooks::camera_shake` on the client.
    /// Evaluates acoustic arrival delay ($d / 343\text{ m/s}$) and distance attenuation for every active shockwave.
    pub fn apply_camera_shake(
        &self,
        host: &dyn Host,
        event: &mut CameraShakeEvent<'_, '_>,
        cfg: &ExplosionsConfig,
    ) {
        if !cfg.enabled || !cfg.camera_shake || self.shockwaves.is_empty() {
            return;
        }

        let cam_pos = host
            .position()
            .map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
            .unwrap_or(Vec3::ZERO);

        for wave in &self.shockwaves {
            let dist = (cam_pos - wave.center).length();
            if dist > wave.max_radius {
                continue;
            }

            // Speed-of-sound arrival delay (343 m/s, scaled slightly so 30-block blasts arrive in ~0.12s)
            let arrival_delay = (dist / SPEED_OF_SOUND_MPS) * 1.4;
            let wave_time = wave.elapsed_secs - arrival_delay;
            if wave_time < 0.0 || wave_time > wave.duration_secs {
                continue;
            }

            let dist_falloff = (1.0 - dist / wave.max_radius).clamp(0.0, 1.0).powi(2);
            let time_envelope = (1.0 - wave_time / wave.duration_secs).clamp(0.0, 1.0).powi(2);
            let amp = wave.intensity * dist_falloff * time_envelope;

            let phase = (wave_time * 42.0) + (event.partial * 0.5);
            event.shake.pitch += (phase.sin() * 4.2) * amp;
            event.shake.yaw += ((phase * 1.37).cos() * 3.4) * amp;
            event.shake.roll += ((phase * 1.93).sin() * 5.0) * amp;
            event.shake.offset[0] += ((phase * 2.1).cos() * 0.08) * amp;
            event.shake.offset[1] += ((phase * 2.7).sin() * 0.11) * amp;
            event.shake.offset[2] += ((phase * 1.8).sin() * 0.08) * amp;
        }
    }

    /// Advance active shockwaves and render expanding ground shockwave rings in `Hooks::world_submit`.
    pub fn step_and_render(
        &mut self,
        host: &dyn Host,
        draw: &mut NativeWorldDraw,
        cfg: &ExplosionsConfig,
    ) {
        if !cfg.enabled {
            self.shockwaves.clear();
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

        for wave in &mut self.shockwaves {
            wave.elapsed_secs += dt;
        }
        self.shockwaves
            .retain(|w| w.elapsed_secs < w.duration_secs + 0.35);

        // Render expanding shockwave ring segments near the blast epicenter during the initial 0.55s
        for wave in &self.shockwaves {
            if wave.elapsed_secs > 0.55 {
                continue;
            }
            let progress = (wave.elapsed_secs / 0.55).clamp(0.0, 1.0);
            let ring_radius = wave.base_radius * 0.5 + progress * wave.base_radius * 2.1;
            let thickness = (0.38 * (1.0 - progress)).max(0.06);
            let segments = 16;

            draw.push();
            if draw.translate_world([
                wave.center.x as f64,
                (wave.center.y + 0.15) as f64,
                wave.center.z as f64,
            ]) {
                for i in 0..segments {
                    let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
                    let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
                    let r_in = ring_radius - thickness;
                    let r_out = ring_radius + thickness;
                    let corners = [
                        [r_in * a0.cos(), 0.0, r_in * a0.sin()],
                        [r_out * a0.cos(), 0.0, r_out * a0.sin()],
                        [r_out * a1.cos(), 0.0, r_out * a1.sin()],
                        [r_in * a1.cos(), 0.0, r_in * a1.sin()],
                    ];
                    let _ = host.draw_quad(
                        draw,
                        "block/white_stained_glass",
                        corners,
                        [0.0, 0.0, 16.0, 16.0],
                        0xF0_F4FF,
                    );
                }
            }
            draw.pop();
        }
    }
}

/// Apply material-dependent physical knockback scaling when a living entity is struck (`living_knockback_v2`).
pub fn compute_weapon_knockback_impulse(
    strength: &mut f64,
    x: &mut f64,
    z: &mut f64,
    weapon_id: Option<&str>,
) -> Vec3 {
    let mult = if let Some(w) = weapon_id {
        let bare = crate::materials::bare_id(w);
        if bare.contains("mace") || bare.contains("axe") || bare.contains("hammer") {
            1.35
        } else if bare.contains("sword") || bare.contains("trident") {
            1.15
        } else {
            1.0
        }
    } else {
        1.0
    };
    *strength *= mult;
    let dir = Vec3::new(-(*x as f32), 0.38, -(*z as f32)).normalize_or(Vec3::Y);
    dir * ((*strength as f32) * 9.5)
}
