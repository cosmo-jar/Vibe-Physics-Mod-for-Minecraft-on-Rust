# Vibe Physics

[![GitHub Repository](https://img.shields.io/badge/GitHub-cosmo--jar%2FVibe--Physics-blue?logo=github)](https://github.com/cosmo-jar/Vibe-Physics-Mod-for-Minecraft-on-Rust)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Minecraft Version](https://img.shields.io/badge/Minecraft-26.3-brightgreen.svg)]()
[![Engine](https://img.shields.io/badge/Engine-Horizon%20(Rust)-orange.svg)]()

**Vibe Physics** is a high-performance native physics engine for Horizon (Minecraft 26.3), written in pure Rust by **[cosmo-jar](https://github.com/cosmo-jar)**. It transforms static voxel environments into an immersive, dynamically reactive world with 144+ FPS performance, zero per-frame heap allocations, and zero JVM garbage collection pauses.

Repository: [https://github.com/cosmo-jar/Vibe-Physics-Mod-for-Minecraft-on-Rust](https://github.com/cosmo-jar/Vibe-Physics-Mod-for-Minecraft-on-Rust)

---

## ✨ Features

1. **🏢 Structural & Building Collapse (`building_collapse`)**:
   - Universal cantilever load-bearing simulation. Breaking load-bearing pillars, support columns, or foundation beams causes unsupported masonry, roofs, and balconies to fracture and collapse in real time.
   - Blast severance detection: Suspended structures (cranes, towers, overhanging roofs, suspension bridges) severed at their base will dynamically topple and crash into the ground.
   - Non-destructive protection for fragile decor: Breaking signs, lanterns, carpets, flower pots, doors, or fence gates will **never** trigger accidental house collapses.

2. **🌲 True 3D Rigid-Body Tree Felling (`tree_felling`)**:
   - Chopping the base log of a tree dynamically tips the trunk and canopy into a rigid-body physics assembly.
   - Trees fall away from the player with natural canopy center-of-mass angular momentum, crashing into terrain with audible splintering sound effects and wood particles.

3. **💨 Volumetric Dust & Smoke Effects (`dust_clouds`)**:
   - Bilinear 3D smoke and volumetric dust puffs billowing from collapsing structures, tree impacts, and explosive craters. Can be toggled on/off independently.

4. **💥 Ballistic Explosions & Shockwaves (`explosions`)**:
   - High-yield directional blast wave uproots nearby trees, shatters glass panes into flying shards, and launches crater blocks into spinning ballistic trajectories.
   - Procedural camera trauma shake based on 3-axis sinusoidal oscillation with quadratic decay.

5. **🧱 Block Fracture & Micro-Debris (`debris`)**:
   - Subvoxel crumble shards on block break. Sand, red sand, gravel, and concrete powder burst into fine interactive micro-grains.

6. **🧟 Active Mob Ragdolls & Dismemberment (`ragdoll`)**:
   - Skeletal physics system with Verlet integration and distance constraints.
   - Realistic stumbling, directional knockback reactions, and dynamic limb severing on high-damage or blast impacts.

7. **📦 3D Item Physics (`item_physics`)**:
   - Dropped items rotate realistically, align flat to terrain surface normals, slide on ice, and float with buoyant water dynamics.

8. **⚙ In-Game Settings GUI Overlay**:
   - Press **Right Shift** (`RShift`) at any time to open the in-game settings menu.
   - Directly toggle any subsystem using keys `[1]` through `[8]`, or navigate with Arrow keys and Enter/Space.
   - Clean HUD during normal gameplay with zero on-screen clutter.

---

## 🎮 Controls

| Key | Action |
| --- | --- |
| **Right Shift** (`RShift`) | Open / Close Settings Menu |
| **Escape** (`Esc`) | Close Settings Menu |
| **1 .. 8** | Directly toggle corresponding subsystem |
| **Up / Down Arrows** | Select setting in menu |
| **Enter / Space** | Toggle selected setting |

### In-Game Chat Commands
- `/vp` or `/vp menu` — Toggle settings GUI overlay
- `/vp collapse` — Toggle building / structure collapse
- `/vp trees` — Toggle tree felling
- `/vp dust` — Toggle dust and smoke clouds
- `/vp blast` — Toggle explosions and ballistics
- `/vp debris` — Toggle block shards and sand grains
- `/vp ragdoll` — Toggle mob ragdolls and stumbling
- `/vp items` — Toggle 3D item physics
- `/vp clear` — Instantly clear all active physics assemblies and debris

---

## 📦 Installation & Packaging

### For Players
Install `vibe_physics.hnmod` into your Horizon native mods directory:
```
%APPDATA%\horizon\horizon-native-mods\vibe_physics.hnmod
```
Then launch Horizon and click **Build** on the Mods tab.

### For Developers & Maintainers
Build the `.hnmod` bundle with:
```bash
python package.py
```
This generates `vibe_physics-1.0.0.hnmod` with valid `horizon-native.json` manifest and computes its SHA-256 checksum.

---

## 👤 Author

Developed by **[cosmo-jar](https://github.com/cosmo-jar)**  
GitHub: [https://github.com/cosmo-jar/Vibe-Physics-Mod-for-Minecraft-on-Rust](https://github.com/cosmo-jar/Vibe-Physics-Mod-for-Minecraft-on-Rust)

---

## 📄 License

Licensed under the [MIT License](LICENSE).  
Copyright (c) 2026 cosmo-jar.
