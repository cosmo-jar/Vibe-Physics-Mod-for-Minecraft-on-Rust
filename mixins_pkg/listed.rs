fn vp_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::native_hooks::NativeMod {
        id: "vibe_physics".into(),
        title: "Vibe Physics 1.0.0".into(),
        description: "True 3D physics by cosmo-jar: building collapse, tree felling, active mob ragdolls, ballistic shockwaves, 3D items, and in-game settings menu (Right Shift). https://github.com/cosmo-jar/Vibe-Physics-Mod-for-Minecraft-on-Rust".into(),
        keys: Vec::new(),
    });
}
