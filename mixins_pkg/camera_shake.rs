fn vp_camera_shake(game: &crate::game::Game<'_>, partial: f32, shake: &mut crate::game::NativeShake) { crate::game::native_vibe_physics::on_camera_shake(game, partial, shake); }
