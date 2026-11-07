import os

# 1. Update horizon-mixin.toml
mixin_toml = """[mod]
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
  { name = "game", ty = "&crate::game::Game<'_>", value = "self" },
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
  { name = "game", ty = "&crate::game::Game<'_>", value = "self" },
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
  { name = "game", ty = "&crate::game::Game<'_>", value = "self" },
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
  { name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" },
  { name = "dimension", ty = "&str", value = "dimension" },
  { name = "entity", ty = "u32", value = "entity" },
  { name = "kind", ty = "&str", value = "kind" },
]
"""

with open("mixins_pkg/horizon-mixin.toml", "w", encoding="utf-8") as f:
    f.write(mixin_toml)

# 2. Update helpers
with open("mixins_pkg/camera_shake.rs", "w", encoding="utf-8") as f:
    f.write("fn vp_camera_shake(game: &crate::game::Game<'_>, partial: f32, shake: &mut crate::game::NativeShake) { crate::game::native_vibe_physics::on_camera_shake(game, partial, shake); }\n")

with open("mixins_pkg/mob_submit.rs", "w", encoding="utf-8") as f:
    f.write("fn vp_mob_submit(game: &crate::game::Game<'_>, mob: &crate::game::NativeMobRender<'_>, draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw, drawn: &mut bool) -> ::core::ops::ControlFlow<()> { if crate::game::native_vibe_physics::on_mob_submit(game, mob, draw) { *drawn = true; ::core::ops::ControlFlow::Break(()) } else { ::core::ops::ControlFlow::Continue(()) } }\n")

with open("mixins_pkg/player_submit.rs", "w", encoding="utf-8") as f:
    f.write("fn vp_player_submit(game: &crate::game::Game<'_>, player: &crate::game::NativeMobRender<'_>, who: &crate::game::NativePlayerRender<'_>, draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw, drawn: &mut bool) -> ::core::ops::ControlFlow<()> { if crate::game::native_vibe_physics::on_player_submit(game, player, who, draw) { *drawn = true; ::core::ops::ControlFlow::Break(()) } else { ::core::ops::ControlFlow::Continue(()) } }\n")

with open("mixins_pkg/entity_tick.rs", "w", encoding="utf-8") as f:
    f.write("fn vp_entity_tick(game: &mut crate::game::Game<'_>, dimension: &str, entity: u32, kind: &str) { crate::game::native_vibe_physics::on_entity_tick_post(game, dimension, entity, kind); }\n")

print("Mixins and helpers successfully updated!")
