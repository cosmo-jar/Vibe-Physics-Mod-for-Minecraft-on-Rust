path = "mixins_pkg/vibe_physics.rs"
with open(path, "r", encoding="utf-8") as f:
    c = f.read()

# 1. Remove unused Vec3 in mod materials
c = c.replace("pub mod materials {\n    use glam::Vec3;\n", "pub mod materials {\n", 1)

# 2. _yaw in ArticulatedRagdoll::new
c = c.replace("pub fn new(id: u32, kind: &str, feet: Vec3, yaw: f32) -> Self {", "pub fn new(id: u32, kind: &str, feet: Vec3, _yaw: f32) -> Self {", 1)

# 3. Borrow checker fix in on_entity_tick_post
old_entity_tick = """        if let Some(i) = game.dropped.iter().position(|e| e.id == entity) {
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
        }"""

new_entity_tick = """        if let Some(i) = game.dropped.iter().position(|e| e.id == entity) {
            let (pos, on_ground) = {
                let e = &game.dropped[i];
                (e.position, e.on_ground)
            };
            let bx = pos.x.floor() as i32;
            let by = (pos.y - 0.2).floor() as i32;
            let bz = pos.z.floor() as i32;
            let is_ice = on_ground && game.native_block([bx, by, bz]).contains("ice");

            let item = &mut game.dropped[i];
            let speed = (item.velocity.x * item.velocity.x + item.velocity.z * item.velocity.z).sqrt();
            if !item.on_ground {
                item.rotation[0] = (item.rotation[0] + speed as f32 * 45.0 + 3.0) % 360.0;
                item.rotation[1] = (item.rotation[1] + speed as f32 * 30.0 + 2.0) % 360.0;
            } else {
                item.rotation[0] = 0.0;
            }

            if is_ice {
                item.velocity.x *= 1.03;
                item.velocity.z *= 1.03;
            }
        }"""

assert old_entity_tick in c, "old_entity_tick not found!"
c = c.replace(old_entity_tick, new_entity_tick, 1)

with open(path, "w", encoding="utf-8") as f:
    f.write(c)

print("Fixes applied successfully!")
