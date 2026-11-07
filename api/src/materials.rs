//! Unified Material Physics Database for **Vibe Physics**.
//!
//! Maps Minecraft block and item registry identifiers (`minecraft:stone`, `minecraft:oak_planks`,
//! `minecraft:glass`, `minecraft:iron_block`, etc.) to physical properties:
//! - Density ($\text{kg/m}^3$), mass, and water buoyancy.
//! - Structural compressive/tensile strength and maximum horizontal cantilever span.
//! - Surface restitution (bounciness) and sliding friction.
//! - Procedural 3D fracture pattern (`FractureStyle`) for Voronoi debris generation.
//! - Representative block-atlas sprite path and RGB tint for shard rendering.

use glam::Vec3;

/// Controls how a broken block is sliced into 3D non-cubic Voronoi / convex polyhedra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FractureStyle {
    /// Sharp, thin, angular razor shards (glass, ice, glowstone, amethyst).
    GlassShards,
    /// Elongated anisotropic splinters along the wood grain (logs, planks, fences, doors).
    WoodSplinters,
    /// Irregular rocky polyhedra with rough facets (stone, cobblestone, ores, bricks, concrete).
    StonePolyhedra,
    /// Heavy, dense, angular plates and chunks (iron, gold, copper, diamond, netherite, anvil).
    MetalPlates,
    /// Soft, clumpy fragments that settle quickly with minimal bounce (dirt, sand, gravel, clay).
    GranularClumps,
    /// Fibrous/leafy fragments with high air drag (leaves, wool, hay, moss).
    FoliageFragments,
}

/// Complete physical profile for a material class.
#[derive(Debug, Clone, Copy)]
pub struct MaterialProfile {
    /// Material category name for debugging.
    pub name: &'static str,
    /// Density in $\text{kg/m}^3$ (water = $1000.0$; $< 1000$ floats in water).
    pub density: f32,
    /// Maximum horizontal overhang / cantilever distance (in blocks) before structural shear failure.
    pub max_overhang: i32,
    /// Maximum vertical/diagonal structural support reach (in graph steps).
    pub support_reach: i32,
    /// Blast occlusion absorption per voxel ($0.0 = \text{transparent}$, $1.0 = \text{blocks wave}$).
    pub blast_absorption: f32,
    /// Coefficient of restitution ($0.0 = \text{inelastic}$, $1.0 = \text{elastic bounce}$).
    pub restitution: f32,
    /// Dynamic friction coefficient ($0.02 = \text{blue ice}$, $0.85 = \text{rough stone}$).
    pub friction: f32,
    /// Fracture geometry pattern when shattered.
    pub fracture_style: FractureStyle,
    /// Default shard count multiplier relative to `debris.shards_per_block`.
    pub shard_multiplier: f32,
    /// Primary block texture sprite in the Minecraft blocks atlas (`block/<name>`).
    pub sprite: &'static str,
    /// RGB tint applied to rendered shards (`[1.0, 1.0, 1.0]` for untinted sprites).
    pub tint: [f32; 3],
    /// Whether this block is structural (participates in building collapse analysis).
    pub is_structural: bool,
    /// Whether this block is fragile enough to shatter from distant shockwaves.
    pub shockwave_fragile: bool,
}

impl MaterialProfile {
    /// Returns true if an item or shard of this material floats in water.
    pub fn floats_in_water(&self) -> bool {
        self.density < 980.0
    }

    /// Buoyancy acceleration factor in water ($> 0$ upwards, $< 0$ sinks).
    pub fn water_buoyancy_factor(&self) -> f32 {
        ((1000.0 - self.density) / 1000.0).clamp(-1.2, 0.85)
    }

    /// Approximate mass of a shard with volume fraction `vol_fraction` of a $1\text{ m}^3$ block.
    pub fn shard_mass(&self, vol_fraction: f32) -> f32 {
        (self.density * vol_fraction.max(0.005)).max(1.0)
    }

    /// Scale vector applied to local Voronoi plane normals to produce anisotropic shapes
    /// (e.g. elongated wood splinters or thin glass shards).
    pub fn fracture_anisotropy(&self) -> Vec3 {
        match self.fracture_style {
            FractureStyle::GlassShards => Vec3::new(1.0, 1.0, 0.28),
            FractureStyle::WoodSplinters => Vec3::new(1.6, 0.32, 0.38),
            FractureStyle::StonePolyhedra => Vec3::new(1.0, 0.92, 1.05),
            FractureStyle::MetalPlates => Vec3::new(1.25, 0.45, 1.15),
            FractureStyle::GranularClumps => Vec3::new(0.95, 0.85, 0.95),
            FractureStyle::FoliageFragments => Vec3::new(1.1, 0.5, 1.1),
        }
    }
}

/// Strip `minecraft:` prefix if present.
pub fn bare_id(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// Returns true if the block ID represents air, cave air, void air, or fluid/non-solid space.
pub fn is_air_or_fluid(block_id: &str) -> bool {
    let b = bare_id(block_id);
    matches!(
        b,
        "" | "air"
            | "cave_air"
            | "void_air"
            | "water"
            | "flowing_water"
            | "lava"
            | "flowing_lava"
            | "bubble_column"
            | "light"
            | "structure_void"
    )
}

/// Returns true if the block is an unbreakable/foundation anchor in the world
/// (bedrock, obsidian, reinforced deepslate, end portal frame, barrier).
pub fn is_unbreakable_anchor(block_id: &str) -> bool {
    let b = bare_id(block_id);
    matches!(
        b,
        "bedrock"
            | "barrier"
            | "command_block"
            | "chain_command_block"
            | "repeating_command_block"
            | "structure_block"
            | "jigsaw"
            | "end_portal_frame"
            | "end_portal"
            | "nether_portal"
            | "reinforced_deepslate"
    )
}

/// Returns true if the block is natural terrain ground (stone, dirt, grass, deepslate, sand, terracotta)
/// when at or near world terrain level, or acts as a strong structural base.
pub fn is_natural_foundation(block_id: &str) -> bool {
    let b = bare_id(block_id);
    is_unbreakable_anchor(b)
        || matches!(
            b,
            "stone"
                | "deepslate"
                | "granite"
                | "diorite"
                | "andesite"
                | "tuff"
                | "calcite"
                | "dripstone_block"
                | "bedrock"
                | "obsidian"
                | "crying_obsidian"
                | "netherrack"
                | "basalt"
                | "smooth_basalt"
                | "blackstone"
                | "end_stone"
                | "dirt"
                | "grass_block"
                | "coarse_dirt"
                | "podzol"
                | "mycelium"
                | "rooted_dirt"
                | "mud"
                | "clay"
                | "sandstone"
                | "red_sandstone"
                | "terracotta"
        )
}

/// Classify any block or item ID into a rich `MaterialProfile`.
pub fn classify_material(id: &str) -> MaterialProfile {
    let b = bare_id(id);

    // 1. Ice family (super low friction, glass-like fracture, floats in water)
    if b.contains("blue_ice") || b.contains("packed_ice") || b == "ice" || b == "frosted_ice" {
        let friction = if b.contains("blue_ice") {
            0.015
        } else if b.contains("packed_ice") {
            0.03
        } else {
            0.05
        };
        let sprite = if b.contains("blue_ice") {
            "block/blue_ice"
        } else if b.contains("packed_ice") {
            "block/packed_ice"
        } else {
            "block/ice"
        };
        return MaterialProfile {
            name: "ice",
            density: 917.0,
            max_overhang: 3,
            support_reach: 8,
            blast_absorption: 0.35,
            restitution: 0.25,
            friction,
            fracture_style: FractureStyle::GlassShards,
            shard_multiplier: 1.3,
            sprite,
            tint: [0.85, 0.95, 1.0],
            is_structural: true,
            shockwave_fragile: b == "ice" || b == "frosted_ice",
        };
    }

    // 2. Glass, stained glass, panes, lanterns, glowstone, sea lantern, amethyst
    if b.contains("glass")
        || b == "glowstone"
        || b == "sea_lantern"
        || b.contains("amethyst")
        || b.contains("lantern")
        || b == "ice"
    {
        let sprite = if b.contains("white_stained") {
            "block/white_stained_glass"
        } else if b.contains("red_stained") {
            "block/red_stained_glass"
        } else if b.contains("blue_stained") {
            "block/blue_stained_glass"
        } else if b.contains("tinted_glass") {
            "block/tinted_glass"
        } else if b == "glowstone" {
            "block/glowstone"
        } else if b == "sea_lantern" {
            "block/sea_lantern"
        } else if b.contains("amethyst") {
            "block/amethyst_block"
        } else {
            "block/glass"
        };
        return MaterialProfile {
            name: "glass",
            density: 2500.0,
            max_overhang: 2,
            support_reach: 5,
            blast_absorption: 0.12,
            restitution: 0.42,
            friction: 0.35,
            fracture_style: FractureStyle::GlassShards,
            shard_multiplier: 1.6,
            sprite,
            tint: [0.92, 0.98, 1.0],
            is_structural: true,
            shockwave_fragile: true,
        };
    }

    // 3. Wood, logs, planks, stripped logs, bamboo, fences, stairs, doors, chests, barrels
    if b.contains("planks")
        || b.contains("_log")
        || b.contains("_wood")
        || b.contains("_stem")
        || b.contains("_hyphae")
        || b.contains("bamboo")
        || b.contains("fence")
        || b.contains("door")
        || b.contains("trapdoor")
        || b == "crafting_table"
        || b == "bookshelf"
        || b == "chest"
        || b == "barrel"
        || b == "ladder"
        || b.contains("sign")
        || b.contains("wooden")
        || b == "stick"
        || b == "bow"
        || b == "crossbow"
        || b == "shield"
    {
        let sprite = if b.contains("spruce") {
            "block/spruce_planks"
        } else if b.contains("birch") {
            "block/birch_planks"
        } else if b.contains("jungle") {
            "block/jungle_planks"
        } else if b.contains("acacia") {
            "block/acacia_planks"
        } else if b.contains("dark_oak") {
            "block/dark_oak_planks"
        } else if b.contains("mangrove") {
            "block/mangrove_planks"
        } else if b.contains("cherry") {
            "block/cherry_planks"
        } else if b.contains("crimson") {
            "block/crimson_planks"
        } else if b.contains("warped") {
            "block/warped_planks"
        } else if b.contains("bamboo") {
            "block/bamboo_planks"
        } else if b.contains("_log") {
            "block/oak_log"
        } else {
            "block/oak_planks"
        };
        return MaterialProfile {
            name: "wood",
            density: 620.0, // Floats in water!
            max_overhang: 5, // Wood has good tensile/bending strength for beams & roofs
            support_reach: 14,
            blast_absorption: 0.45,
            restitution: 0.28,
            friction: 0.58,
            fracture_style: FractureStyle::WoodSplinters,
            shard_multiplier: 1.15,
            sprite,
            tint: [1.0, 1.0, 1.0],
            is_structural: true,
            shockwave_fragile: false,
        };
    }

    // 4. Metals, heavy ores, anvil, chains, iron bars, netherite, copper, gold, diamond
    if b.contains("iron")
        || b.contains("gold")
        || b.contains("copper")
        || b.contains("netherite")
        || b.contains("diamond")
        || b.contains("emerald")
        || b.contains("anvil")
        || b == "chain"
        || b == "cauldron"
        || b == "hopper"
        || b == "bell"
        || b == "lodestone"
    {
        let sprite = if b.contains("gold") {
            "block/gold_block"
        } else if b.contains("copper") {
            "block/copper_block"
        } else if b.contains("netherite") {
            "block/netherite_block"
        } else if b.contains("diamond") {
            "block/diamond_block"
        } else if b.contains("emerald") {
            "block/emerald_block"
        } else {
            "block/iron_block"
        };
        return MaterialProfile {
            name: "metal",
            density: 7850.0, // Heavy! Sinks rapidly in water
            max_overhang: 8, // Strongest structural cantilever
            support_reach: 22,
            blast_absorption: 0.95,
            restitution: 0.35,
            friction: 0.48,
            fracture_style: FractureStyle::MetalPlates,
            shard_multiplier: 0.85,
            sprite,
            tint: [1.0, 1.0, 1.0],
            is_structural: true,
            shockwave_fragile: false,
        };
    }

    // 5. Foliage, leaves, wool, hay, moss, flowers, plants
    if b.contains("leaves")
        || b.contains("wool")
        || b.contains("carpet")
        || b.contains("moss")
        || b == "hay_block"
        || b == "sponge"
        || b == "wet_sponge"
        || b.contains("azalea")
        || b.contains("vine")
        || b.contains("bush")
        || b.contains("grass") && b != "grass_block"
    {
        let (sprite, tint) = if b.contains("leaves") {
            ("block/oak_leaves", [0.48, 0.76, 0.32])
        } else if b.contains("wool") || b.contains("carpet") {
            ("block/white_wool", [0.98, 0.98, 0.98])
        } else if b == "hay_block" {
            ("block/hay_block_side", [1.0, 1.0, 1.0])
        } else {
            ("block/moss_block", [0.55, 0.78, 0.38])
        };
        return MaterialProfile {
            name: "foliage",
            density: 280.0, // Very light, floats easily
            max_overhang: 3,
            support_reach: 6,
            blast_absorption: 0.10,
            restitution: 0.08,
            friction: 0.75,
            fracture_style: FractureStyle::FoliageFragments,
            shard_multiplier: 0.9,
            sprite,
            tint,
            is_structural: false,
            shockwave_fragile: b.contains("leaves"),
        };
    }

    // 6. Soft granular soils (dirt, sand, gravel, mud, clay, snow, soul sand)
    if b.contains("dirt")
        || b == "grass_block"
        || b.contains("sand") && !b.contains("sandstone")
        || b == "gravel"
        || b.contains("clay")
        || b.contains("mud")
        || b.contains("snow")
        || b.contains("soul_")
        || b == "farmland"
        || b == "dirt_path"
        || b == "podzol"
        || b == "mycelium"
    {
        let sprite = if b.contains("red_sand") {
            "block/red_sand"
        } else if b.contains("sand") {
            "block/sand"
        } else if b == "gravel" {
            "block/gravel"
        } else if b.contains("snow") {
            "block/snow"
        } else if b.contains("clay") {
            "block/clay"
        } else {
            "block/dirt"
        };
        return MaterialProfile {
            name: "granular",
            density: 1550.0,
            max_overhang: 1, // Soil cannot overhang far without collapsing
            support_reach: 4,
            blast_absorption: 0.45,
            restitution: 0.06,
            friction: 0.85,
            fracture_style: FractureStyle::GranularClumps,
            shard_multiplier: 1.0,
            sprite,
            tint: [1.0, 1.0, 1.0],
            is_structural: true,
            shockwave_fragile: false,
        };
    }

    // 7. Masonry, bricks, cobblestone, stone bricks, concrete, terracotta, quartz, prismarine, stone
    let sprite = if b.contains("cobblestone") {
        "block/cobblestone"
    } else if b.contains("stone_bricks") {
        "block/stone_bricks"
    } else if b.contains("deepslate_bricks") || b.contains("deepslate_tiles") {
        "block/deepslate_bricks"
    } else if b.contains("deepslate") {
        "block/deepslate"
    } else if b.contains("bricks") || b == "brick" {
        "block/bricks"
    } else if b.contains("sandstone") {
        "block/sandstone"
    } else if b.contains("quartz") {
        "block/quartz_block_side"
    } else if b.contains("blackstone") {
        "block/blackstone"
    } else if b.contains("netherrack") {
        "block/netherrack"
    } else if b.contains("nether_bricks") {
        "block/nether_bricks"
    } else if b.contains("end_stone") {
        "block/end_stone"
    } else if b.contains("obsidian") {
        "block/obsidian"
    } else if b.contains("granite") {
        "block/granite"
    } else if b.contains("diorite") {
        "block/diorite"
    } else if b.contains("andesite") {
        "block/andesite"
    } else if b.contains("tuff") {
        "block/tuff"
    } else if b.contains("terracotta") {
        "block/terracotta"
    } else if b.contains("concrete") {
        "block/white_concrete"
    } else {
        "block/stone"
    };

    let is_reinforced = b.contains("obsidian") || b.contains("deepslate") || b.contains("concrete");
    MaterialProfile {
        name: if is_reinforced { "reinforced_stone" } else { "stone" },
        density: if is_reinforced { 2950.0 } else { 2450.0 },
        max_overhang: if is_reinforced { 5 } else { 3 },
        support_reach: if is_reinforced { 16 } else { 11 },
        blast_absorption: if is_reinforced { 0.90 } else { 0.72 },
        restitution: 0.22,
        friction: 0.74,
        fracture_style: FractureStyle::StonePolyhedra,
        shard_multiplier: 1.0,
        sprite,
        tint: [1.0, 1.0, 1.0],
        is_structural: true,
        shockwave_fragile: false,
    }
}
