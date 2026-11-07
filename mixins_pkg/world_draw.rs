fn vp_world_draw(
    game: &crate::game::Game<'_>,
    camera: glam::DVec3,
    alpha: f64,
    out: &mut Vec<Vertex>,
) -> ::core::ops::ControlFlow<()> {
    if crate::game::native_vibe_physics::on_world_draw(game, camera, alpha, out) {
        ::core::ops::ControlFlow::Break(())
    } else {
        ::core::ops::ControlFlow::Continue(())
    }
}
