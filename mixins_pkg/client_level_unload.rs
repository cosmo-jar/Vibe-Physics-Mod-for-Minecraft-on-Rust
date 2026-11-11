fn vp_client_level_unload(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::native_vibe_physics::on_client_level_unload(game, dimension);
}
