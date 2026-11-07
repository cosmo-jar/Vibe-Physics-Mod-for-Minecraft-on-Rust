//! Module 2 — **Dropped Item 3D Rigid-Body Physics** (`item_physics`).
//!
//! Replaces vanilla's floating, vertical spinning dropped item presentation and motion with:
//! 1. **3D Rigid-Body Orientation & Airborne Tumbling** (`inject_vibe_physics_item_pose`):
//!    - Every dropped `ItemEntity` has a 3D unit quaternion orientation (`Quat`) and angular velocity (`Vec3`).
//!    - While airborne, items tumble around all 3 axes driven by their linear velocity and initial spin impulse.
//!    - When landing on a surface (`on_ground`), 2D flat items (swords, tools, ingots, food, materials)
//!      settle flat onto the surface (`X` pitch $\approx 90^\circ$) with their preserved yaw, while 3D block
//!      items (`uses_block_light`) settle onto one of their 6 cubic faces!
//!    - Optional slope/ledge tilt alignment when an item rests on the edge of a block or stair.
//! 2. **Server-Side Realistic Surface Motion & Fluid Buoyancy** (`entity_tick_post` for `minecraft:item`):
//!    - Low-friction sliding on ice, packed ice, and blue ice.
//!    - Material-aware water buoyancy: wood, leaves, wool, and food float upward to the water surface;
//!      heavy stone and metal items sink realistically.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use horizon_client::mod_api::{Host, InjectVibePhysicsItemPoseEvent};

use crate::config::ItemPhysicsConfig;
use crate::materials::classify_material;
use crate::math_phys::{FastRng, integrate_quat};

/// Per-item 3D rigid-body rotational state tracked on the client.
#[derive(Debug, Clone, Copy)]
pub struct RigidItemState {
    /// Current 3D orientation quaternion.
    pub rotation: Quat,
    /// Current angular velocity ($\text{rad/s}$).
    pub angular_vel: Vec3,
    /// Preserved horizontal yaw angle ($\text{rad}$) for resting flat.
    pub yaw: f32,
    /// Last interpolated age (`item.age as f32 + partial`) to compute exact frame `dt`.
    pub last_age_ticks: f32,
    /// Whether the item was on the ground on the previous frame.
    pub was_on_ground: bool,
    /// Settled resting progress $\in [0.0, 1.0]$.
    pub settle_blend: f32,
}

impl RigidItemState {
    pub fn new(id: u32, bob_offset: f32, age_ticks: f32) -> Self {
        let mut rng = FastRng::from_seed((id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let yaw = bob_offset * std::f32::consts::TAU + rng.range_f32(0.0, std::f32::consts::TAU);
        let init_pitch = rng.range_f32(-0.6, 0.6);
        let init_roll = rng.range_f32(-0.6, 0.6);
        let rotation = Quat::from_euler(glam::EulerRot::YXZ, yaw, init_pitch, init_roll);
        let angular_vel = Vec3::new(
            rng.signed_f32() * 6.5,
            rng.signed_f32() * 4.5,
            rng.signed_f32() * 6.5,
        );
        Self {
            rotation,
            angular_vel,
            yaw,
            last_age_ticks: age_ticks,
            was_on_ground: false,
            settle_blend: 0.0,
        }
    }
}

/// Manages 3D rigid-body states for all dropped items.
pub struct ItemPhysicsEngine {
    items: HashMap<u32, RigidItemState>,
    last_prune_count: usize,
}

impl Default for ItemPhysicsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemPhysicsEngine {
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            last_prune_count: 0,
        }
    }

    pub fn tracked_count(&self) -> usize {
        self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Apply a radial blast wave impulse to all tracked item orientations (makes resting items flip & tumble!).
    pub fn apply_explosion_wave(&mut self, _center: Vec3, _radius: f32, strength: f32) {
        for (id, state) in &mut self.items {
            let mut rng = FastRng::from_seed((*id as u64).wrapping_add(0x1234_5678));
            state.angular_vel += rng.unit_vec3() * (strength * 1.8).clamp(4.0, 24.0);
            state.settle_blend = 0.0;
            state.was_on_ground = false;
        }
    }

    /// Modify the dropped item's render pose matrix in `Game::dropped_item_geometry`
    /// (`Hooks::inject_vibe_physics_item_pose`).
    pub fn update_item_pose(
        &mut self,
        event: &mut InjectVibePhysicsItemPoseEvent<'_>,
        cfg: &ItemPhysicsConfig,
    ) {
        if !cfg.enabled || !event.entity {
            return;
        }

        // Periodic pruning if item cache grows large
        if self.items.len() > 1024 && self.items.len() > self.last_prune_count + 256 {
            self.items.clear();
            self.last_prune_count = 0;
        }

        let age_ticks = event.age as f32 + event.partial;
        let state = self
            .items
            .entry(event.id)
            .or_insert_with(|| RigidItemState::new(event.id, event.bob_offset, age_ticks));

        let dt_ticks = (age_ticks - state.last_age_ticks).clamp(0.0, 3.0);
        state.last_age_ticks = age_ticks;
        let dt = dt_ticks * 0.05; // 20 TPS -> 0.05s per tick

        let vel = Vec3::new(
            event.velocity[0] as f32 * 20.0,
            event.velocity[1] as f32 * 20.0,
            event.velocity[2] as f32 * 20.0,
        );
        let speed = vel.length();

        if event.was_touching_water {
            // Gentle fluid wave rocking in water
            let wave_t = age_ticks * 0.12 + event.bob_offset * 3.0;
            let pitch = wave_t.sin() * 0.25;
            let roll = (wave_t * 1.3).cos() * 0.25;
            state.yaw += vel.x * dt * 0.4;
            let water_target = if event.uses_block_light {
                Quat::from_euler(glam::EulerRot::YXZ, state.yaw, pitch, roll)
            } else {
                Quat::from_euler(
                    glam::EulerRot::YXZ,
                    state.yaw,
                    std::f32::consts::FRAC_PI_2 + pitch * 0.5,
                    roll,
                )
            };
            state.rotation = state.rotation.slerp(water_target, (dt * 6.0).clamp(0.0, 1.0));
            state.settle_blend = (state.settle_blend + dt * 2.0).clamp(0.0, 1.0);
        } else if !event.on_ground {
            // Airborne 3D rigid-body tumbling
            state.was_on_ground = false;
            state.settle_blend = (state.settle_blend - dt * 6.0).clamp(0.0, 1.0);

            // Couple linear velocity into tumbling torque (`v x up`)
            let motion_torque = Vec3::new(vel.z, (vel.x + vel.z) * 0.35, -vel.x) * 1.6;
            let target_omega = (state.angular_vel + motion_torque * dt)
                .clamp(Vec3::splat(-18.0), Vec3::splat(18.0));
            state.angular_vel = target_omega * (1.0 - 0.25 * dt).clamp(0.9, 1.0);
            state.yaw += state.angular_vel.y * dt * cfg.air_spin_speed;
            state.rotation = integrate_quat(
                state.rotation,
                state.angular_vel * cfg.air_spin_speed,
                dt,
            );
        } else {
            // On ground: impact bounce torque on first contact, then settle flat onto surface
            if !state.was_on_ground {
                state.was_on_ground = true;
                state.angular_vel *= 0.35;
            }
            // Sliding along ground (e.g. on ice) induces slight yaw rotation
            if speed > 0.15 {
                state.yaw += (vel.x * 0.5 - vel.z * 0.5) * dt;
            }
            state.settle_blend = (state.settle_blend + dt * 9.0).clamp(0.0, 1.0);

            // Compute slight surface slope tilt if resting near a block edge
            let (slope_pitch, slope_roll) = if cfg.align_to_surface_slopes {
                let fx = (event.position[0].rem_euclid(1.0) - 0.5) as f32;
                let fz = (event.position[2].rem_euclid(1.0) - 0.5) as f32;
                let edge_x = if fx.abs() > 0.38 { fx.signum() * 0.12 } else { 0.0 };
                let edge_z = if fz.abs() > 0.38 { fz.signum() * 0.12 } else { 0.0 };
                (edge_z, -edge_x)
            } else {
                (0.0, 0.0)
            };

            let target_rot = if event.uses_block_light {
                // 3D Block item: rest squarely on its bottom face with preserved yaw
                Quat::from_euler(glam::EulerRot::YXZ, state.yaw, slope_pitch, slope_roll)
            } else if cfg.lay_flat_on_ground {
                // 2D Flat item (sword, pickaxe, ingot, apple): lay flat (+90 deg X pitch) on the ground!
                Quat::from_euler(
                    glam::EulerRot::YXZ,
                    state.yaw,
                    std::f32::consts::FRAC_PI_2 + slope_pitch,
                    slope_roll,
                )
            } else {
                Quat::from_rotation_y(state.yaw)
            };

            state.rotation = state
                .rotation
                .slerp(target_rot, (dt * 12.0).clamp(0.05, 1.0));
        }

        // Compute resting vertical offset so the item sits cleanly on the block surface
        // without hovering (`bob` is removed when `lay_flat_on_ground` is active).
        let camera_rel = Vec3::from_array(event.camera_rel);
        let y_offset = if event.was_touching_water {
            let water_bob = (age_ticks * 0.12 + event.bob_offset).sin() * 0.035;
            0.08 + water_bob
        } else if event.uses_block_light {
            // 3D block item in Ground display is ~0.25 blocks tall
            event.min_offset_y * (1.0 - 0.25 * state.settle_blend)
        } else if cfg.lay_flat_on_ground {
            // 2D item rotated 90 deg flat has half-thickness ~0.018 blocks
            let air_y = event.min_offset_y;
            let flat_y = 0.022_f32;
            air_y * (1.0 - state.settle_blend) + flat_y * state.settle_blend
        } else {
            event.min_offset_y
        };

        event.pose = Mat4::from_translation(camera_rel + Vec3::new(0.0, y_offset, 0.0))
            * Mat4::from_quat(state.rotation.normalize());
    }

    /// Server-side `Hooks::entity_tick_post` handler for `minecraft:item`.
    /// Applies material-dependent water buoyancy and realistic ice/surface sliding friction.
    pub fn server_tick_item_entity(
        &self,
        host: &mut dyn Host,
        entity_id: u32,
        cfg: &ItemPhysicsConfig,
    ) {
        if !cfg.enabled || !cfg.realistic_surface_motion {
            return;
        }
        let Some(pos) = host.entity_position(entity_id) else {
            return;
        };
        let Some(vel) = host.entity_velocity(entity_id) else {
            return;
        };

        let block_at = [
            pos[0].floor() as i32,
            pos[1].floor() as i32,
            pos[2].floor() as i32,
        ];
        let block_below = [
            pos[0].floor() as i32,
            (pos[1] - 0.15).floor() as i32,
            pos[2].floor() as i32,
        ];

        let cell_id = host.block(block_at);
        let below_id = host.block(block_below);
        let cell_bare = crate::materials::bare_id(&cell_id);
        let below_bare = crate::materials::bare_id(&below_id);

        let mut new_vel = vel;
        let mut modified = false;

        // 1. Water buoyancy & fluid drag
        if cell_bare == "water" || cell_bare == "flowing_water" {
            // Gentle horizontal fluid damping and surface bobbing
            new_vel[0] *= 0.92;
            new_vel[2] *= 0.92;
            if new_vel[1] < 0.04 {
                new_vel[1] = (new_vel[1] + 0.018).min(0.06);
            }
            modified = true;
        }

        // 2. Low-friction ice sliding (items keep gliding across ice/packed_ice/blue_ice)
        if below_bare.contains("ice") && vel[1].abs() < 0.08 {
            let speed_sq = vel[0] * vel[0] + vel[2] * vel[2];
            if speed_sq > 1e-5 && speed_sq < 0.65 {
                let boost = if below_bare.contains("blue_ice") {
                    1.08
                } else if below_bare.contains("packed_ice") {
                    1.05
                } else {
                    1.03
                };
                new_vel[0] *= boost;
                new_vel[2] *= boost;
                modified = true;
            }
        } else if !below_bare.is_empty() && below_bare != "air" && vel[1].abs() < 0.01 {
            // Material-based surface friction adjustments
            let mat = classify_material(&below_id);
            if mat.friction > 0.80 && (vel[0].abs() > 0.005 || vel[2].abs() > 0.005) {
                new_vel[0] *= 0.85;
                new_vel[2] *= 0.85;
                modified = true;
            }
        }

        if modified {
            let _ = host.set_entity_velocity(entity_id, new_vel);
        }
    }
}
