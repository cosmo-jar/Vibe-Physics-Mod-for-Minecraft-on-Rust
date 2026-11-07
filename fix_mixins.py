with open('mixins_pkg/vibe_physics.rs', 'r', encoding='utf-8') as f:
    text = f.read()

# 1. Ragdoll imports and solve_pair
old_imp = """    use std::collections::HashMap;
    use glam::{Quat, Vec3};
    use crate::mod_api::{BlockEntityDraw, NativeMobRender, NativePlayerRender};
    use super::config::RagdollConfig;
    use super::math_phys::{resolve_sphere_voxel_collision, solve_cone_constraint, solve_distance_constraint};"""

new_imp = """    use std::collections::HashMap;
    use glam::Vec3;
    use crate::mod_api::{BlockEntityDraw, NativeMobRender};
    use super::config::RagdollConfig;
    use super::math_phys::resolve_sphere_voxel_collision;"""

assert old_imp in text, "old_imp not found"
text = text.replace(old_imp, new_imp)

old_step = """            for _ in 0..cfg.solver_iterations {
                solve_distance_constraint(&mut self.bones[0].pos, &mut self.bones[1].pos, 0.55, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[1].pos, &mut self.bones[2].pos, 0.45, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[1].pos, &mut self.bones[3].pos, 0.35, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[1].pos, &mut self.bones[4].pos, 0.35, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[0].pos, &mut self.bones[5].pos, 0.75, cfg.joint_stiffness);
                solve_distance_constraint(&mut self.bones[0].pos, &mut self.bones[6].pos, 0.75, cfg.joint_stiffness);
            }"""

new_step = """            for _ in 0..cfg.solver_iterations {
                self.solve_pair(0, 1, 0.55, cfg.joint_stiffness);
                self.solve_pair(1, 2, 0.45, cfg.joint_stiffness);
                self.solve_pair(1, 3, 0.35, cfg.joint_stiffness);
                self.solve_pair(1, 4, 0.35, cfg.joint_stiffness);
                self.solve_pair(0, 5, 0.75, cfg.joint_stiffness);
                self.solve_pair(0, 6, 0.75, cfg.joint_stiffness);
            }"""

assert old_step in text, "old_step not found"
text = text.replace(old_step, new_step)

solve_pair_code = """        pub fn solve_pair(&mut self, i: usize, j: usize, target_dist: f32, stiffness: f32) {
            if i >= self.bones.len() || j >= self.bones.len() || i == j { return; }
            let p1 = self.bones[i].pos;
            let p2 = self.bones[j].pos;
            let delta = p2 - p1;
            let dist = delta.length();
            if dist > 1e-4 {
                let diff = (dist - target_dist) / dist;
                let corr = delta * (0.5 * stiffness * diff);
                self.bones[i].pos += corr;
                self.bones[j].pos -= corr;
            }
        }

        pub fn step("""

assert "        pub fn step(" in text, "step not found"
text = text.replace("        pub fn step(", solve_pair_code)

# 2. Structural imports and unused variables
old_struct_imp = """    use glam::{Quat, Vec3};
    use super::config::StructuralConfig;
    use super::debris::DebrisEngine;
    use super::materials::{MaterialProfile, is_natural_foundation, is_unbreakable_anchor};"""

new_struct_imp = """    use glam::{Quat, Vec3};
    use super::config::StructuralConfig;
    use super::debris::DebrisEngine;
    use super::materials::MaterialProfile;"""

assert old_struct_imp in text, "old_struct_imp not found"
text = text.replace(old_struct_imp, new_struct_imp)

text = text.replace("get_mat: impl Fn([i32; 3]) -> MaterialProfile,", "_get_mat: impl Fn([i32; 3]) -> MaterialProfile,")
text = text.replace("gravity: f32,\n            cfg: &StructuralConfig,", "_gravity: f32,\n            cfg: &StructuralConfig,")
text = text.replace("is_solid: impl Fn([i32; 3]) -> bool, debris: &mut DebrisEngine", "_is_solid: impl Fn([i32; 3]) -> bool, _debris: &mut DebrisEngine")

# 3. Explosions
text = text.replace("pub fn apply_camera_shake(&self, partial: f32,", "pub fn apply_camera_shake(&self, _partial: f32,")

# 4. Destroy & Explosion detonate
text = text.replace("pos: [i32; 3], state: BlockStateId)", "pos: [i32; 3], _state: BlockStateId)")
text = text.replace("dimension: &str, center: [f64; 3]", "_dimension: &str, center: [f64; 3]")

with open("mixins_pkg/vibe_physics.rs", "w", encoding="utf-8") as f:
    f.write(text)

print("mixins_pkg/vibe_physics.rs successfully updated!")
