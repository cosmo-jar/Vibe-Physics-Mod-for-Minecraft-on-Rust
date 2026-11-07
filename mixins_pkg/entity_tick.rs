fn vp_entity_tick(game: &mut crate::game::Game<'_>, dimension: &str, entity: u32, kind: &str) { crate::game::native_vibe_physics::on_entity_tick_post(game, dimension, entity, kind); }
