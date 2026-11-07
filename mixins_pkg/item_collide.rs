fn vp_item_collide(
    game: &crate::game::Game<'_>,
) -> ::core::ops::ControlFlow<Vec<Aabb>> {
    if let Some(boxes) = crate::game::native_vibe_physics::on_item_collisions_head(game) {
        return ::core::ops::ControlFlow::Break(boxes);
    }
    ::core::ops::ControlFlow::Continue(())
}
