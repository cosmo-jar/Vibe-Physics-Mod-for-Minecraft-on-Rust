fn vp_entity_tick_listened(game: &mut crate::game::Game<'_>, listened: &mut bool) {
    crate::game::native_vibe_physics::on_entity_tick_listened(game, listened);
}
