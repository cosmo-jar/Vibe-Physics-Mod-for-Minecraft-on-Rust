import re

path = "mixins_pkg/vibe_physics.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# 1. Update submit_mob and submit_player in RagdollEngine
ragdoll_submit_old = """        pub fn submit_mob(&mut self, mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, cfg: &RagdollConfig) -> bool {
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
        }"""

ragdoll_submit_new = """        pub fn submit_mob(&mut self, game: &crate::game::Game<'_>, mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, cfg: &RagdollConfig) -> bool {
            if !cfg.enabled || mob.death_time <= 0 {
                return false;
            }
            let head_block = match mob.kind {
                "minecraft:skeleton" => "minecraft:skeleton_skull",
                "minecraft:wither_skeleton" => "minecraft:wither_skeleton_skull",
                "minecraft:zombie" | "minecraft:drowned" | "minecraft:husk" => "minecraft:zombie_head",
                "minecraft:creeper" => "minecraft:creeper_head",
                "minecraft:piglin" | "minecraft:piglin_brute" | "minecraft:zombified_piglin" => "minecraft:piglin_head",
                _ => "minecraft:skeleton_skull",
            };

            let t = (mob.death_time as f32 + mob.partial).min(20.0);
            let collapse_angle = (t / 20.0) * 90.0;
            let tumble_spin = t * 18.0;

            // Tumbling head
            draw.push();
            draw.translate([0.0, 1.2 - (t / 20.0) * 0.9, 0.0]);
            draw.rotate_about([1.0, 0.2, 0.1], tumble_spin, [0.0, 0.0, 0.0]);
            draw.scale([0.65, 0.65, 0.65]);
            draw.translate([-0.5, -0.5, -0.5]);
            if !game.native_draw_block(draw, head_block, &[]) {
                game.native_draw_block(draw, "minecraft:skeleton_skull", &[]);
            }
            draw.pop();

            // Torso
            draw.push();
            draw.translate([0.0, 0.6 - (t / 20.0) * 0.45, 0.0]);
            draw.rotate_about([0.0, 0.0, 1.0], collapse_angle, [0.0, 0.0, 0.0]);
            draw.scale([0.5, 0.8, 0.35]);
            draw.translate([-0.5, -0.5, -0.5]);
            game.native_draw_block(draw, "minecraft:bone_block", &[]);
            draw.pop();

            // Left leg
            draw.push();
            draw.translate([-0.2, 0.2 - (t / 20.0) * 0.15, 0.0]);
            draw.rotate_about([1.0, 0.0, 0.0], collapse_angle * 0.8, [0.0, 0.0, 0.0]);
            draw.scale([0.22, 0.45, 0.22]);
            draw.translate([-0.5, -0.5, -0.5]);
            game.native_draw_block(draw, "minecraft:bone_block", &[]);
            draw.pop();

            // Right leg
            draw.push();
            draw.translate([0.2, 0.2 - (t / 20.0) * 0.15, 0.0]);
            draw.rotate_about([-1.0, 0.0, 0.0], collapse_angle * 0.8, [0.0, 0.0, 0.0]);
            draw.scale([0.22, 0.45, 0.22]);
            draw.translate([-0.5, -0.5, -0.5]);
            game.native_draw_block(draw, "minecraft:bone_block", &[]);
            draw.pop();

            true
        }

        pub fn submit_player(&mut self, game: &crate::game::Game<'_>, player: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, cfg: &RagdollConfig) -> bool {
            if !cfg.enabled || player.death_time <= 0 {
                return false;
            }
            let t = (player.death_time as f32 + player.partial).min(20.0);
            let collapse_angle = (t / 20.0) * 90.0;
            let tumble_spin = t * 18.0;

            // Tumbling head
            draw.push();
            draw.translate([0.0, 1.2 - (t / 20.0) * 0.9, 0.0]);
            draw.rotate_about([1.0, 0.2, 0.1], tumble_spin, [0.0, 0.0, 0.0]);
            draw.scale([0.65, 0.65, 0.65]);
            draw.translate([-0.5, -0.5, -0.5]);
            if !game.native_draw_block(draw, "minecraft:player_head", &[]) {
                game.native_draw_block(draw, "minecraft:skeleton_skull", &[]);
            }
            draw.pop();

            // Torso
            draw.push();
            draw.translate([0.0, 0.6 - (t / 20.0) * 0.45, 0.0]);
            draw.rotate_about([0.0, 0.0, 1.0], collapse_angle, [0.0, 0.0, 0.0]);
            draw.scale([0.5, 0.8, 0.35]);
            draw.translate([-0.5, -0.5, -0.5]);
            game.native_draw_block(draw, "minecraft:bone_block", &[]);
            draw.pop();

            true
        }"""

assert ragdoll_submit_old in content, "ragdoll_submit_old not found!"
content = content.replace(ragdoll_submit_old, ragdoll_submit_new, 1)

# 2. Update apply_camera_shake in ExplosionEngine
shake_old = """        pub fn apply_camera_shake(&self, _partial: f32, cam_pos: Vec3, shake: &mut NativeShake, cfg: &ExplosionsConfig) {
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
        }"""

shake_new = """        pub fn apply_camera_shake(&self, _partial: f32, cam_pos: Vec3, shake: &mut NativeShake, cfg: &ExplosionsConfig) {
            if !cfg.camera_shake { return; }
            for w in &self.shockwaves {
                let dist = (w.center - cam_pos).length();
                if dist < 48.0 && w.elapsed_secs < 1.2 {
                    let decay = (1.0 - w.elapsed_secs / 1.2).max(0.0);
                    let dist_factor = (1.0 - dist / 48.0).max(0.0);
                    let intensity = decay * dist_factor;
                    let freq = w.elapsed_secs * 35.0;
                    shake.pitch += freq.sin() * intensity * 2.8;
                    shake.yaw += (freq * 1.3).cos() * intensity * 2.2;
                    shake.roll += (freq * 0.8).sin() * intensity * 1.8;
                }
            }
        }"""

assert shake_old in content, "shake_old not found!"
content = content.replace(shake_old, shake_new, 1)

# 3. Update top-level imports and hooks from line 966 to end
marker = "use std::sync::Mutex;\nuse crate::mod_api::*;\nuse super::Game;"
assert marker in content, "marker not found!"

hooks_new = """use std::sync::Mutex;
use crate::mod_api::*;
use super::Game;
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_world::registry::BlockStateId;

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
    static GREETED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !GREETED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        game.native_chat("§a[Vibe Physics 0.1.0] §fEngine Active! F6:Ragdoll F7:Items F8:Debris F9:Collapse F10:Blast");
    }

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

        // 1. Spawning dynamic block crumble debris & dust
        if st.config.debris.enabled {
            let center = [pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5];
            if let Some((sprite, tint)) = game.particle_sprite(state, pos) {
                let opt = ParticleOptions::block(ParticleType::Block, sprite, tint);
                for i in 0..16 {
                    let angle = (i as f64) * std::f64::consts::PI * 2.0 / 16.0;
                    let speed = 0.16 + ((i * 5) % 8) as f64 * 0.02;
                    let vx = angle.cos() * speed;
                    let vy = 0.18 + ((i * 11) % 7) as f64 * 0.02;
                    let vz = angle.sin() * speed;
                    game.native_add_particle(&opt, center, [vx, vy, vz]);
                }
            }
            let smoke = ParticleOptions::simple(ParticleType::Smoke);
            game.native_add_particle(&smoke, center, [0.0, 0.08, 0.0]);
        }

        // 2. Structural collapse: check unsupported blocks above
        if st.config.structural.enabled {
            let dim = game.dim.id().to_string();
            for dy in 1..=5 {
                let check_pos = [pos[0], pos[1] + dy, pos[2]];
                let upper_state = game.world.block(check_pos[0], check_pos[1], check_pos[2]);
                if game.res.facts().is_air(upper_state) {
                    break;
                }
                let has_horizontal_support = [
                    [check_pos[0] + 1, check_pos[1], check_pos[2]],
                    [check_pos[0] - 1, check_pos[1], check_pos[2]],
                    [check_pos[0], check_pos[1], check_pos[2] + 1],
                    [check_pos[0], check_pos[1], check_pos[2] - 1],
                ].iter().any(|&nb| {
                    let s = game.world.block(nb[0], nb[1], nb[2]);
                    !game.res.facts().is_air(s)
                });

                if !has_horizontal_support {
                    game.native_set_block(&dim, check_pos, BlockStateId(0), 3);
                    game.native_spawn_falling_block(
                        &dim,
                        upper_state,
                        [check_pos[0] as f64 + 0.5, check_pos[1] as f64, check_pos[2] as f64 + 0.5],
                        [0.0, -0.05, 0.0],
                        crate::game::native_falling::NativeFalling { drop_item: true, hurts: true },
                    );
                } else {
                    break;
                }
            }
        }
    });
}

pub fn on_explosion_detonate(game: &mut Game<'_>, dimension: &str, center: [f64; 3], blocks: &mut Vec<[i32; 3]>) {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.explosions.enabled { return; }
        let c = glam::Vec3::new(center[0] as f32, center[1] as f32, center[2] as f32);
        st.explosions.trigger_shockwave(c, 4.0, 24.0);

        // Explosion visual effects
        let exp = ParticleOptions::simple(ParticleType::Explosion);
        game.native_add_particle(&exp, center, [0.0, 0.1, 0.0]);
        let lsmoke = ParticleOptions::simple(ParticleType::LargeSmoke);
        for i in 0..8 {
            let angle = (i as f64) * std::f64::consts::PI * 2.0 / 8.0;
            let vx = angle.cos() * 0.25;
            let vz = angle.sin() * 0.25;
            game.native_add_particle(&lsmoke, center, [vx, 0.2, vz]);
        }

        // Fling crater blocks outward as physical falling blocks
        let take_n = blocks.len().min(12);
        for (i, &pos) in blocks.iter().take(take_n).enumerate() {
            let bstate = game.world.block(pos[0], pos[1], pos[2]);
            if !game.res.facts().is_air(bstate) {
                let dx = (pos[0] as f64 + 0.5) - center[0];
                let dy = (pos[1] as f64 + 0.5) - center[1];
                let dz = (pos[2] as f64 + 0.5) - center[2];
                let dist = (dx*dx + dy*dy + dz*dz).sqrt().max(0.1);
                let speed = 0.32;
                let vx = (dx / dist) * speed;
                let vy = 0.42 + ((i % 4) as f64) * 0.08;
                let vz = (dz / dist) * speed;

                game.native_set_block(dimension, pos, BlockStateId(0), 3);
                game.native_spawn_falling_block(
                    dimension,
                    bstate,
                    [pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5],
                    [vx, vy, vz],
                    crate::game::native_falling::NativeFalling { drop_item: true, hurts: true },
                );
            }
        }
    });
}

pub fn on_camera_shake(game: &Game<'_>, partial: f32, shake: &mut NativeShake) {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.explosions.enabled { return; }
        let cam_pos = if let Some(p) = game.native_player() {
            glam::Vec3::new(p.x as f32, p.y as f32, p.z as f32)
        } else {
            glam::Vec3::ZERO
        };
        st.explosions.apply_camera_shake(partial, cam_pos, shake, &st.config.explosions);
    });
}

pub fn on_mob_submit(game: &Game<'_>, mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw) -> bool {
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled { return false; }
        st.ragdoll.submit_mob(game, mob, draw, &st.config.ragdoll)
    })
}

pub fn on_player_submit(game: &Game<'_>, player: &NativeMobRender<'_>, who: &NativePlayerRender<'_>, draw: &mut BlockEntityDraw) -> bool {
    let _ = who;
    with_state(|st| {
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled { return false; }
        st.ragdoll.submit_player(game, player, draw, &st.config.ragdoll)
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

pub fn on_entity_tick_post(game: &mut Game<'_>, dimension: &str, entity: u32, kind: &str) {
    let _ = dimension;
    if kind == "minecraft:item" {
        with_state(|st| {
            if !st.config.general.master_enabled || !st.config.item_physics.enabled { return; }
            st.item_physics.track_entity_tick(entity, 0.05, &st.config.item_physics);
        });

        if let Some(i) = game.dropped.iter().position(|e| e.id == entity) {
            let item = &mut game.dropped[i];
            let speed = (item.velocity.x * item.velocity.x + item.velocity.z * item.velocity.z).sqrt();
            if !item.on_ground {
                item.rotation[0] = (item.rotation[0] + speed as f32 * 45.0 + 3.0) % 360.0;
                item.rotation[1] = (item.rotation[1] + speed as f32 * 30.0 + 2.0) % 360.0;
            } else {
                item.rotation[0] = 0.0;
            }

            let bx = item.position.x.floor() as i32;
            let by = (item.position.y - 0.2).floor() as i32;
            let bz = item.position.z.floor() as i32;
            let under = game.native_block([bx, by, bz]);
            if under.contains("ice") && item.on_ground {
                item.velocity.x *= 1.03;
                item.velocity.z *= 1.03;
            }
        }
    }
}
"""

idx = content.find(marker)
content = content[:idx] + hooks_new

with open(path, "w", encoding="utf-8") as f:
    f.write(content)

print("vibe_physics.rs successfully updated!")
