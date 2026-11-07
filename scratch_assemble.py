import os, sys, shutil, subprocess, zipfile, json

# 1. Paths
WORKSPACE_ROOT = r"c:\Users\cosmo\Documents\Antigravity\VibePhysics"
SRC_DIR = os.path.join(WORKSPACE_ROOT, "api", "src")
MIXINS_DIR = os.path.join(WORKSPACE_ROOT, "mixins_pkg")
os.makedirs(MIXINS_DIR, exist_ok=True)

# 2. Clean component code
def read_and_clean(fname):
    p = os.path.join(SRC_DIR, fname)
    with open(p, "r", encoding="utf-8") as f:
        code = f.read()
    lines = code.splitlines()
    res = []
    for l in lines:
        if l.strip().startswith("//!") or l.strip().startswith("#!["):
            continue
        l = l.replace("crate::config::", "super::config::")
        l = l.replace("crate::materials::", "super::materials::")
        l = l.replace("crate::math_phys::", "super::math_phys::")
        l = l.replace("crate::ragdoll::", "super::ragdoll::")
        l = l.replace("crate::item_physics::", "super::item_physics::")
        l = l.replace("crate::debris::", "super::debris::")
        l = l.replace("crate::structural::", "super::structural::")
        l = l.replace("crate::explosions::", "super::explosions::")
        l = l.replace("horizon_client::mod_api::", "crate::mod_api::")
        res.append(l)
    return "\n".join(res)

cfg_code = read_and_clean("config.rs")
mat_code = read_and_clean("materials.rs")
math_code = read_and_clean("math_phys.rs")
rag_code = read_and_clean("ragdoll.rs")
item_code = read_and_clean("item_physics.rs")
deb_code = read_and_clean("debris.rs")
struct_code = read_and_clean("structural.rs")
exp_code = read_and_clean("explosions.rs")

master_vibe_physics = f"""// Vibe Physics — Advanced Physics & Destruction Engine for Horizon

pub mod config {{
{cfg_code}
}}

pub mod materials {{
{mat_code}
}}

pub mod math_phys {{
{math_code}
}}

pub mod ragdoll {{
{rag_code}
}}

pub mod item_physics {{
{item_code}
}}

pub mod debris {{
{deb_code}
}}

pub mod structural {{
{struct_code}
}}

pub mod explosions {{
{exp_code}
}}

use std::sync::Mutex;
use crate::mod_api::*;
use super::Game;

pub struct VpState {{
    pub config: config::VibePhysicsConfig,
    pub ragdoll: ragdoll::RagdollEngine,
    pub item_physics: item_physics::ItemPhysicsEngine,
    pub debris: debris::DebrisEngine,
    pub structural: structural::StructuralEngine,
    pub explosions: explosions::ExplosionEngine,
}}

impl VpState {{
    pub fn new() -> Self {{
        Self {{
            config: config::VibePhysicsConfig::default(),
            ragdoll: ragdoll::RagdollEngine::new(),
            item_physics: item_physics::ItemPhysicsEngine::new(),
            debris: debris::DebrisEngine::new(),
            structural: structural::StructuralEngine::new(),
            explosions: explosions::ExplosionEngine::new(),
        }}
    }}
}}

static STATE: Mutex<Option<VpState>> = Mutex::new(None);

pub fn with_state<R>(f: impl FnOnce(&mut VpState) -> R) -> R {{
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {{
        *guard = Some(VpState::new());
    }}
    f(guard.as_mut().unwrap())
}}

pub fn on_tick(game: &mut Game<'_>) {{
    with_state(|st| {{
        if !st.config.general.master_enabled {{ return; }}
        let dt = 0.05f32;
        let gravity = st.config.general.gravity;
        if st.config.ragdoll.enabled {{
            st.ragdoll.tick(dt, gravity, |pos| !game.native_block_collision(pos).is_empty());
        }}
        if st.config.debris.enabled {{
            st.debris.tick(dt, gravity, |pos| !game.native_block_collision(pos).is_empty());
        }}
        if st.config.structural.enabled {{
            st.structural.tick(dt, gravity, |pos| !game.native_block_collision(pos).is_empty(), &mut st.debris);
        }}
        if st.config.explosions.enabled {{
            st.explosions.tick(dt);
        }}
    }});
}}

pub fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut crate::extras_hud::Corners) {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.general.show_hud_status {{ return; }}
        let tag = |on: bool| if on {{ "ON" }} else {{ "OFF" }};
        let line1 = format!(
            "VibePhysics [F6-F10] | Ragdoll:{{}} Items:{{}} Debris:{{}}",
            tag(st.config.ragdoll.enabled),
            tag(st.config.item_physics.enabled),
            tag(st.config.debris.enabled),
        );
        let line2 = format!(
            "Struct:{{}} Blast:{{}} | Shards:{{}} Clusters:{{}}",
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
    }});
}}

pub fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {{
    if !pressed {{ return; }}
    with_state(|st| {{
        if !st.config.general.enable_hotkeys {{ return; }}
        let toggled = match key {{
            "key.keyboard.f6" => Some(config::PhysicsModule::Ragdoll),
            "key.keyboard.f7" => Some(config::PhysicsModule::ItemPhysics),
            "key.keyboard.f8" => Some(config::PhysicsModule::Debris),
            "key.keyboard.f9" => Some(config::PhysicsModule::Structural),
            "key.keyboard.f10" => Some(config::PhysicsModule::Explosions),
            _ => None,
        }};
        if let Some(m) = toggled {{
            let on = st.config.toggle_module(m);
            let msg = format!("[VibePhysics] {{}} -> {{}}", m.display_name(), if on {{ "ON" }} else {{ "OFF" }});
            game.native_chat(&msg);
        }}
    }});
}}

pub fn on_command(game: &mut Game<'_>, command: &str, ran: &mut bool) {{
    let trimmed = command.trim().strip_prefix('/').unwrap_or(command.trim());
    if trimmed.starts_with("vp") || trimmed.starts_with("vibephysics") {{
        let reply = with_state(|st| {{
            st.config.status_summary()
        }});
        game.native_chat(&format!("[VibePhysics] {{}}", reply));
        *ran = true;
    }}
}}

pub fn on_destroy_block(game: &mut Game<'_>, pos: [i32; 3], state: BlockStateId) {{
    with_state(|st| {{
        if !st.config.general.master_enabled {{ return; }}
        let block_name = game.native_block(pos);
        if st.config.debris.enabled {{
            st.debris.spawn_shards_for_block(pos, &block_name, [0.0, 0.5, 0.0].into(), &st.config);
        }}
        if st.config.structural.enabled {{
            let mut detached = Vec::new();
            let is_solid = |p: [i32; 3]| !game.native_block_collision(p).is_empty();
            let get_mat = |p: [i32; 3]| materials::classify_material(&game.native_block(p));
            let gravity = st.config.general.gravity;
            st.structural.on_block_removed(pos, &mut detached, is_solid, get_mat, gravity, &st.config);
        }}
    }});
}}

pub fn on_explosion_detonate(game: &mut Game<'_>, dimension: &str, center: [f64; 3], blocks: &mut Vec<[i32; 3]>) {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.explosions.enabled {{ return; }}
        let c = glam::Vec3::new(center[0] as f32, center[1] as f32, center[2] as f32);
        st.explosions.trigger_shockwave(c, 4.0, 14.0);
        if st.config.debris.enabled {{
            for pos in blocks.iter().take(12) {{
                let bname = game.native_block(*pos);
                let dir = (glam::Vec3::new(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5) - c).normalize_or_zero();
                st.debris.spawn_shards_for_block(*pos, &bname, dir * 8.0, &st.config);
            }}
        }}
    }});
}}

pub fn on_camera_shake(partial: f32, shake: &mut NativeShake) {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.explosions.enabled {{ return; }}
        let cam_pos = glam::Vec3::ZERO;
        st.explosions.apply_camera_shake(partial, cam_pos, shake, &st.config);
    }});
}}

pub fn on_mob_submit(mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw) -> bool {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled {{ return false; }}
        st.ragdoll.render_mob_ragdoll(mob, draw, &st.config)
    }})
}}

pub fn on_player_submit(player: &NativeMobRender<'_>, who: &NativePlayerRender<'_>, draw: &mut BlockEntityDraw) -> bool {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled {{ return false; }}
        st.ragdoll.render_player_ragdoll(player, who, draw, &st.config)
    }})
}}

pub fn on_living_knockback(entity: i32, kind: &str, strength: f64, x: f64, z: f64) {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled {{ return; }}
        let impulse = glam::Vec3::new(x as f32, 0.4, z as f32) * (strength as f32);
        st.ragdoll.apply_impulse_to_entity(entity as u32, impulse);
    }});
}}

pub fn on_living_damage(entity: i32, kind: &str, amount: f32) {{
    with_state(|st| {{
        if !st.config.general.master_enabled || !st.config.ragdoll.enabled {{ return; }}
        st.ragdoll.apply_damage_flinch(entity as u32, amount);
    }});
}}

pub fn on_entity_tick_post(dimension: &str, entity: u32, kind: &str) {{
    if kind == "minecraft:item" {{
        with_state(|st| {{
            if !st.config.general.master_enabled || !st.config.item_physics.enabled {{ return; }}
            st.item_physics.track_entity_tick(entity);
        }});
    }}
}}
"""

with open(os.path.join(MIXINS_DIR, "vibe_physics.rs"), "w", encoding="utf-8") as f:
    f.write(master_vibe_physics)

helpers = {
    'tick.rs': "fn vp_tick(game: &mut crate::game::Game<'_>) { crate::game::native_vibe_physics::on_tick(game); }",
    'hud.rs': "fn vp_hud(game: &mut crate::game::Game<'_>, batch: &mut crate::hud::HudBatch, corners: &mut crate::extras_hud::Corners) { crate::game::native_vibe_physics::on_hud(game, batch, corners); }",
    'key.rs': "fn vp_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) { crate::game::native_vibe_physics::on_key(game, key, pressed); }",
    'command.rs': "fn vp_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) { crate::game::native_vibe_physics::on_command(game, command, ran); }",
    'commands.rs': 'fn vp_commands(commands: &mut Vec<&\'static str>) { commands.push("vp"); commands.push("vibephysics"); }',
    'destroy_block.rs': "fn vp_destroy_block(game: &mut crate::game::Game<'_>, pos: [i32; 3], state: horizon_world::registry::BlockStateId) { crate::game::native_vibe_physics::on_destroy_block(game, pos, state); }",
    'explosion.rs': "fn vp_explosion(game: &mut crate::game::Game<'_>, dimension: &str, center: [f64; 3], blocks: &mut Vec<[i32; 3]>) { crate::game::native_vibe_physics::on_explosion_detonate(game, dimension, center, blocks); }",
    'camera_shake.rs': "fn vp_camera_shake(partial: f32, shake: &mut crate::game::NativeShake) { crate::game::native_vibe_physics::on_camera_shake(partial, shake); }",
    'mob_submit.rs': "fn vp_mob_submit(mob: &crate::game::NativeMobRender<'_>, draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw, drawn: &mut bool) -> ::core::ops::ControlFlow<()> { if crate::game::native_vibe_physics::on_mob_submit(mob, draw) { *drawn = true; ::core::ops::ControlFlow::Break(()) } else { ::core::ops::ControlFlow::Continue(()) } }",
    'player_submit.rs': "fn vp_player_submit(player: &crate::game::NativeMobRender<'_>, who: &crate::game::NativePlayerRender<'_>, draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw, drawn: &mut bool) -> ::core::ops::ControlFlow<()> { if crate::game::native_vibe_physics::on_player_submit(player, who, draw) { *drawn = true; ::core::ops::ControlFlow::Break(()) } else { ::core::ops::ControlFlow::Continue(()) } }",
    'living_knockback.rs': "fn vp_living_knockback(entity: i32, kind: &str, strength: f64, x: f64, z: f64) { crate::game::native_vibe_physics::on_living_knockback(entity, kind, strength, x, z); }",
    'living_damage.rs': "fn vp_living_damage(entity: i32, kind: &str, amount: f32) { crate::game::native_vibe_physics::on_living_damage(entity, kind, amount); }",
    'entity_tick.rs': "fn vp_entity_tick(dimension: &str, entity: u32, kind: &str) { crate::game::native_vibe_physics::on_entity_tick_post(dimension, entity, kind); }",
    'listed.rs': 'fn vp_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) { mods.push(crate::game::native_hooks::NativeMod { id: "vibe_physics".into(), title: "Vibe Physics 0.1.0".into(), description: "Advanced Modular Physics & Destruction Engine (XPBD Ragdolls, 3D Debris, Structural Integrity, Shockwaves, Item Physics)".into(), keys: vec![("F6".into(), "Toggle Ragdolls".into()), ("F7".into(), "Toggle Item Physics".into()), ("F8".into(), "Toggle Debris Voronoi".into()), ("F9".into(), "Toggle Structural Load".into()), ("F10".into(), "Toggle Pressure Waves".into())] }); }',
}

for hname, hcode in helpers.items():
    with open(os.path.join(MIXINS_DIR, hname), "w", encoding="utf-8") as f:
        f.write(hcode)

horizon_mixin_toml = """[mod]
id = "vibe_physics"
version = "0.1.0"
priority = 50

[compatibility]
horizon_revision = "6475f62f3c834b7489e1816bf47f47ea5eb2b879"

[[module]]
id = "vibe_physics"
file = "crates/horizon-client/src/game.rs"
name = "native_vibe_physics"
source = "vibe_physics.rs"

[[injection]]
id = "listed"
hook = "mods"
hook_version = 1
kind = "HEAD"
helper = "listed.rs"
bindings = [{ name = "mods", ty = "&mut Vec<crate::game::native_hooks::NativeMod>", value = "&mut *mods" }]

[[injection]]
id = "tick"
hook = "tick"
hook_version = 1
kind = "HEAD"
helper = "tick.rs"
bindings = [{ name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" }]

[[injection]]
id = "hud"
hook = "hud"
hook_version = 1
kind = "HEAD"
helper = "hud.rs"
bindings = [
  { name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" },
  { name = "batch", ty = "&mut crate::hud::HudBatch", value = "&mut *batch" },
  { name = "corners", ty = "&mut crate::extras_hud::Corners", value = "&mut *corners" },
]

[[injection]]
id = "key"
hook = "key"
hook_version = 1
kind = "HEAD"
helper = "key.rs"
bindings = [
  { name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" },
  { name = "key", ty = "&str", value = "key" },
  { name = "pressed", ty = "bool", value = "pressed" },
]

[[injection]]
id = "command"
hook = "command"
hook_version = 1
kind = "HEAD"
helper = "command.rs"
bindings = [
  { name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" },
  { name = "command", ty = "&str", value = "command" },
  { name = "ran", ty = "&mut bool", value = "&mut *ran" },
]

[[injection]]
id = "commands"
hook = "commands"
hook_version = 1
kind = "HEAD"
helper = "commands.rs"
bindings = [{ name = "commands", ty = "&mut Vec<&'static str>", value = "&mut *commands" }]

[[injection]]
id = "destroy_block"
hook = "destroy_block"
hook_version = 1
kind = "HEAD"
helper = "destroy_block.rs"
bindings = [
  { name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" },
  { name = "pos", ty = "[i32; 3]", value = "pos" },
  { name = "state", ty = "horizon_world::registry::BlockStateId", value = "state" },
]

[[injection]]
id = "explosion"
hook = "explosion_detonate"
hook_version = 1
kind = "HEAD"
helper = "explosion.rs"
bindings = [
  { name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" },
  { name = "dimension", ty = "&str", value = "dimension" },
  { name = "center", ty = "[f64; 3]", value = "center" },
  { name = "blocks", ty = "&mut Vec<[i32; 3]>", value = "&mut *blocks" },
]

[[injection]]
id = "camera_shake"
hook = "camera_shake"
hook_version = 1
kind = "HEAD"
helper = "camera_shake.rs"
bindings = [
  { name = "partial", ty = "f32", value = "partial" },
  { name = "shake", ty = "&mut crate::game::NativeShake", value = "&mut *shake" },
]

[[injection]]
id = "mob_submit"
hook = "mob_submit"
hook_version = 1
kind = "HEAD"
cancellable = true
helper = "mob_submit.rs"
bindings = [
  { name = "mob", ty = "&crate::game::NativeMobRender<'_>", value = "mob" },
  { name = "draw", ty = "&mut crate::game::native_block_entity_draw::BlockEntityDraw", value = "&mut *draw" },
  { name = "drawn", ty = "&mut bool", value = "&mut *drawn" },
]

[[injection]]
id = "player_submit"
hook = "player_submit"
hook_version = 1
kind = "HEAD"
cancellable = true
helper = "player_submit.rs"
bindings = [
  { name = "player", ty = "&crate::game::NativeMobRender<'_>", value = "player" },
  { name = "who", ty = "&crate::game::NativePlayerRender<'_>", value = "who" },
  { name = "draw", ty = "&mut crate::game::native_block_entity_draw::BlockEntityDraw", value = "&mut *draw" },
  { name = "drawn", ty = "&mut bool", value = "&mut *drawn" },
]

[[injection]]
id = "living_knockback"
hook = "living_knockback"
hook_version = 1
kind = "HEAD"
helper = "living_knockback.rs"
bindings = [
  { name = "entity", ty = "i32", value = "entity" },
  { name = "kind", ty = "&str", value = "kind" },
  { name = "strength", ty = "f64", value = "*strength" },
  { name = "x", ty = "f64", value = "*x" },
  { name = "z", ty = "f64", value = "*z" },
]

[[injection]]
id = "living_damage"
hook = "living_incoming_damage"
hook_version = 1
kind = "HEAD"
helper = "living_damage.rs"
bindings = [
  { name = "entity", ty = "i32", value = "entity" },
  { name = "kind", ty = "&str", value = "kind" },
  { name = "amount", ty = "f32", value = "*amount" },
]

[[injection]]
id = "entity_tick"
hook = "entity_tick_post"
hook_version = 1
kind = "HEAD"
helper = "entity_tick.rs"
bindings = [
  { name = "dimension", ty = "&str", value = "dimension" },
  { name = "entity", ty = "u32", value = "entity" },
  { name = "kind", ty = "&str", value = "kind" },
]
"""

with open(os.path.join(MIXINS_DIR, "horizon-mixin.toml"), "w", encoding="utf-8") as f:
    f.write(horizon_mixin_toml)

print("Files in MIXINS_DIR:")
for f in os.listdir(MIXINS_DIR):
    print(" ", f)
