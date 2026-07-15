# Infinite Backlot World Art Bible

## Core identity

A tired but inhabited urban world where retrofit bureaucracy has accreted over older brick architecture. Impossible systems are treated as ordinary maintenance problems. The look is stylized, legible, slightly theatrical, and designed for portrait-oriented character staging rather than photoreal simulation.

## Shape language

- Large simple architectural masses establish silhouette and navigation.
- Repeated vertical pipes, narrow frames, stacked boxes, rails, and canopies communicate retrofit history.
- Public objects use clipped rectangles, chamfered corners, and overbuilt protective cages.
- Hero doors, counters, shelters, and signs use layered frames rather than flat planes.
- Mild impossibility appears through misaligned levels, too many conduits, odd service bridges, and infrastructure with unclear destinations.

## Architectural influences

Pre-war brick apartments, 1970s municipal additions, small independent storefronts, transit furniture, loading alleys, and industrial-edge infrastructure. Avoid direct replicas of real locations or protected commercial identities.

## Palette

| Family | Use |
|---|---|
| Burgundy, oxblood, rust, brick red | Primary architecture and repaired surfaces |
| Deep teal, blue-green | Utilities, doors, cabinets, transit furniture |
| Brass, dirty gold, warm steel | Trim, frames, civic hardware |
| Cyan and pale blue | Practical lights, impossible-system indicators |
| Warm cream, tired tan, concrete grey | Interior balance and public surfaces |
| Near-black navy | Asphalt, recesses, industrial depth |

Reserve saturated cyan and warm gold for navigation, practicals, and story emphasis.

## Materials

Use a compact shared family: painted brick, worn plaster, rough concrete, patched asphalt, painted metal, oxidized brass, dark steel, institutional tile, rubber, glass, paper, grime, and controlled emission. Variation comes from geometry layers, repair patches, decals, roughness changes, and color relatives—not a unique shader per object.

## Signage

Condensed institutional typography, high contrast, limited colors, clipped metal panels, engraved directory strips, and small geometric symbols. Recurring language includes building management, municipal access, service classifications, and Odd Hours. Copy should be sparse and readable; one strong notice is better than a wall of jokes.

## Lighting

Warm practical pools against cool cyan utility accents. Interiors require a readable key and fill. Streets use localized pools plus restrained ambient lift. Alleys are cooler and higher contrast without crushed blacks. Store lighting is brighter and cooler than the street. Emissive meshes identify fixtures; Bevy semantic lights provide illumination.

## Environmental storytelling

- Contradictory municipal notices.
- Mismatched repairs and replacement panels.
- Deliveries, recycling, and temporary barriers that imply daily use.
- Overbuilt conduits and cabinets with impossible service labels.
- Odd Hours branding repeated at the store, transit notices, and product displays.
- Building numbers and floor labels that are almost—but not quite—sequential.

## Recurring motifs

1. A divided diamond geometric symbol used by management and transit.
2. Teal utility boxes with brass ID plates.
3. Cyan strip practicals under burgundy architecture.
4. Triple conduit runs that terminate somewhere implausible.
5. Numbering such as `3¼`, `14½`, and `B-0` presented without comment.
6. Conflicting arrows and access classifications.

Use two or three motifs per location, not all six.

## Stylization and detail density

- **Background / >25 m:** silhouette, palette blocks, roofline, two or three window rhythms.
- **Mid-distance / 8–25 m:** material separation, canopies, pipes, large signs, doors, major props.
- **Character range / 2–8 m:** bevels, handles, notices, repair patches, product groupings, practical fixtures.
- **Insert range / <2 m:** reserved for authored hero props; avoid making every background object insert-ready.

Geometry should read at the target camera distance. Bevels and layered silhouettes matter more than invisible topology.

## Imported or generated assets

- License and provenance are mandatory.
- Normalize scale, axes, transforms, pivot, names, materials, and topology.
- Remove hidden cameras, lights, rigs, and unrelated collections.
- Adapt silhouette, palette, signage, and at least one detail layer to this bible.
- Replace real branding and recognizable logos.
- Use shared materials where possible.
- Add collision and semantic metadata only after cleanup.
- Reject assets that remain visually alien after one adaptation pass.

## Cell composition rules

Each world cell needs a clear primary route, one recognizable silhouette, one conversation pocket, one practical-light hierarchy, and sockets that visually point toward neighboring cells. Expansion exits should be visible but not demand construction of the next district. Dense cells may have one quiet surface; public pockets need clear negative space for two- and three-character blocking.
