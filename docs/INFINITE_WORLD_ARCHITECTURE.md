# Infinite World Architecture

## Status and boundary

This document records the intended expansion path for Infinite Backlot. It does not replace or weaken the proven episode-production pipeline. Whole-episode Gemma authoring, frozen accepted replay, measured audio retiming, the shared semantic GLB loader, Blender-authored sets, Bevy GPU capture, native 1080×1920 output, and FFmpeg packaging remain the production foundation.

The world system grows by assembling reviewed authored modules. It is not an LLM-generated mesh system.

## Design principles

1. **Authored identity, procedural scale.** Handcrafted hero locations carry story identity. Procedurally assembled connective areas provide breadth without flattening every location into generic output.
2. **Semantic contracts over arbitrary coordinates.** Modules expose typed sockets, staging marks, camera anchors, interactions, cutaway walls, collision volumes, and level-of-detail representations.
3. **Deterministic continuity.** A world seed plus versioned registries reproduce layout. Persistent state survives unloading and revisiting.
4. **One production truth.** Interactive and offline paths consume the same scene registry, module manifests, semantic bindings, and persistent world state.
5. **Bounded LLM authority.** The LLM requests location requirements and mood. Deterministic systems select and assemble valid reviewed content.

## Scene and module registry

The scene registry is the durable catalog for rooms, corridors, exterior blocks, terrain tiles, and hero scenes. Every entry should contain:

- Stable module or scene ID.
- Semantic type and tags.
- Source `.blend`, exported GLB, semantic sidecar, provenance, and hashes.
- Module and scene version.
- Compatible socket types and socket transforms.
- Traversable bounds, collision data, navigation surfaces, and accessibility constraints.
- Staging marks for actors, props, vehicles, and crowds.
- Camera anchors, camera-safe volumes, and portrait-framing metadata.
- Interactions and stateful semantic nodes.
- Cutaway walls and visibility groups.
- Detail tiers and distant representation.
- Validation status and reviewed reference evidence.

Registry versions must be explicit. Saved worlds retain the module version they were built with until a deterministic migration is approved. Runtime dressing state is stored separately from immutable authored geometry.

## Indoor expansion

### Authored module library

Indoor growth starts from the apartment/elevator set and adds Blender-authored modules for:

- Rooms and hallways.
- Modular corners and T-junctions.
- Four-way junctions.
- Elevators and elevator lobbies.
- Stairwells and landings.
- Apartment shells.
- Utility rooms, service corridors, laundry areas, storage, and mechanical spaces.
- Handcrafted recurring hero rooms for locations central to story identity.

### Indoor connection contract

Every modular indoor asset exposes standard connection sockets. A socket declares opening dimensions, floor elevation, orientation, clearance, wall thickness, transition type, and compatibility tags. Assembly rejects incompatible geometry rather than forcing intersections.

Each module also provides:

- Character and prop staging marks.
- Camera anchors for establishing, dialogue, reaction, insert, reveal, and transition shots.
- Camera-safe clearances for portrait capture.
- Semantic interactions such as doors, panels, lights, elevators, appliances, and destructible props.
- Cutaway walls or ceiling groups for production visibility.
- Traversal and collision metadata.

Hero rooms can have bespoke layouts and interaction logic while still exposing standard sockets to the connective system.

## Outdoor expansion

### Urban modules

The outdoor library grows through authored modules for:

- Streets and sidewalks.
- Intersections.
- Alleys.
- Courtyards.
- Parking lots.
- Building façades and service entrances.
- Parks and pocket plazas.
- Rooftops.
- Transit entrances.

Road and lot sockets describe lane count, width, curb profile, grade, sidewalk width, pedestrian crossings, parking interfaces, and compatible intersection types. Façade sockets connect reviewed building fronts to street, alley, courtyard, and roof systems.

### Terrain and wilderness modules

Later libraries can add terrain and wilderness modules: slopes, ridges, clearings, water edges, trails, roads, scrub, forest, and transitional outskirts. These modules use terrain-edge sockets, road sockets, elevation bands, biome tags, and camera-safe clearings.

Distant skyline and terrain representation should use lower-detail authored silhouettes, impostors, or streamed proxy geometry. Distant content establishes place and parallax without paying high-detail production costs outside the active area.

## World scale and deterministic layout

A world seed, registry version, and layout algorithm version fully determine the base layout. Procedural assembly chooses among authored modules using constraints such as:

- Required semantic location types.
- Socket compatibility.
- Traversal reachability.
- Spatial budget and topological goals.
- Camera and staging requirements.
- Narrative adjacency rules.
- Exterior grade, road, lot, and terrain continuity.
- Repetition limits and authored variation weights.

The deterministic solver emits module IDs, versions, transforms chosen from valid socket connections, and stable instance IDs. It never asks the LLM for raw mesh geometry or unconstrained arbitrary world coordinates.

## Chunk streaming and detail tiers

The world is partitioned into deterministic chunks with stable IDs. Streaming maintains:

- An active high-detail production area around the current cast, camera, and scheduled interactions.
- Lower-detail surrounding areas for continuity, shadows, skyline, reflections, and traversal planning.
- Proxy or absent distant areas beyond the current production horizon.

Streaming priority combines camera visibility, scheduled actions, cast routes, audio relevance, and adjacency. Episode capture preflights every required chunk and semantic binding before frame zero so deterministic offline rendering cannot encounter asynchronous layout drift.

Unloading a chunk removes transient render/runtime entities, not its persistent state. Re-entry restores the same module instances and reapplies saved state.

## Persistent world state

Locations remain persistent when revisited. State is keyed by world seed, chunk ID, module instance ID, semantic node ID, and content version. Persisted facts include:

- Damage and repairs.
- Moved, attached, missing, or destroyed props.
- Signs, labels, graffiti, and notices.
- Door, elevator, panel, light, and utility state.
- Environmental state such as flooding, power, debris, and contamination.
- Runtime dressing selections.
- Story ownership and access changes.
- Weather residue and seasonal changes where they materially affect the location.

Persistence records semantic deltas rather than duplicating whole GLBs. Versioned migrations translate old semantic IDs when reviewed modules evolve.

## Content strategy

### Handcrafted hero locations

Recurring hero locations are authored and reviewed in Blender for recognizable composition, interaction, performance staging, and narrative identity. They can be unusually detailed and bespoke.

### Procedural connective areas

Hallways, service spaces, streets, alleys, lots, terrain transitions, and other connective areas are assembled from authored modules. Variation comes from compatible module choices, deterministic layout, and runtime dressing—not from unreviewed generated meshes.

### Runtime dressing

A deterministic dressing layer varies reviewed modules for:

- Weather.
- Time of day.
- Damage and repair.
- Clutter and occupancy.
- Seasons.
- Temporary construction or events.
- Controlled surreal events.

Dressing choices are seeded, registry-backed, persistent when story-relevant, and constrained by semantic attachment points.

### Gradual library growth

The scene library expands incrementally. New gaps discovered during production become module briefs. Blender MCP can create candidate modules offline, but those assets enter the registry only after source retention, semantic validation, provenance checks, deterministic export, visual review, and compatibility tests. Runtime production does not invoke Blender MCP to invent geometry during an episode.

## LLM boundary

The LLM may request semantic requirements such as:

- Location class and function.
- Mood, lighting intent, density, cleanliness, weather, or damage.
- Required adjacencies or traversal beats.
- Needed interactions, staging capacity, and camera intent.
- Story-specific hero-location identity.

The deterministic world system translates those requirements into registry queries, validates socket and production constraints, and selects or assembles authored modules.

The LLM does **not** directly output raw mesh geometry, arbitrary transforms, arbitrary world coordinates, unbounded procedural code, collision meshes, navigation topology, or final camera placements. Coordinates and transforms arise from typed sockets, staging marks, camera anchors, and deterministic solvers.

## Recommended progression

1. **Current apartment set.** Keep the semantic apartment/elevator GLB as the production baseline and harden its registry entry, bindings, camera anchors, cutaways, interactions, and evidence.
2. **Modular indoor floors.** Split reusable corridor, corner, junction, elevator, stairwell, apartment-shell, and utility modules behind standard indoor sockets. Assemble several deterministic floor layouts while preserving shared interactive/offline loading.
3. **Exterior street corner.** Build one reviewed hero street corner with façades, sidewalks, road sockets, an alley or courtyard connection, camera-safe areas, and distant skyline proxy.
4. **Outdoor neighborhood modules.** Add streets, intersections, alleys, courtyards, parking lots, parks, rooftops, transit entrances, façades, and road/lot socket validation. Assemble a small deterministic neighborhood around the hero corner.
5. **Chunk streaming.** Introduce stable chunk IDs, high-detail production radius, lower-detail surroundings, preflighted capture residency, and deterministic load/unload ordering.
6. **Persistent world state.** Persist module instances and semantic deltas for damage, props, signs, interactions, dressing, and environmental state; add registry-version migrations and revisit tests.
7. **Terrain and wider world.** Extend the same contracts to terrain, wilderness, road transitions, camera-safe clearings, and distant skyline/terrain representations.

Each stage must remain shippable through the existing production pipeline before the next stage expands scope.
