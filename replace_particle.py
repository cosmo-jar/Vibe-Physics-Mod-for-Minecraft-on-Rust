with open("mixins_pkg/vibe_physics.rs", "r", encoding="utf-8") as f:
    text = f.read()

count = text.count("game.native_add_particle")
text = text.replace("game.native_add_particle", "game.particles.add")

with open("mixins_pkg/vibe_physics.rs", "w", encoding="utf-8") as f:
    f.write(text)

print(f"Replaced {count} occurrences of native_add_particle -> particles.add!")
