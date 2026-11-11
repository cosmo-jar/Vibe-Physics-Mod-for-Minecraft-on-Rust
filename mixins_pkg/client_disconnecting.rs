fn vp_client_disconnecting(game: &crate::game::Game<'_>) {
    crate::game::native_vibe_physics::on_client_disconnecting(game);
}
