# ADR 0008: Rust building descriptions separate from geometry backends

Status: moved to ashlar on 2026-09-13. The decision stands, and the crates it
names now live in the standalone ashlar workspace beside this one, where
[ashlar ADR 0001](../../../../bevy-crates/ashlar/docs/adr/0001-recipe-domain-and-backends.md)
carries it forward: `strata-buildings` is `ashlar`, `strata-building-mesh` is
`ashlar-manifold`, `strata-building-render` is `ashlar-bevy`, and the preview
tool is `ashlar-preview` with a game-side catalog. Sockets that attach and
collision derived from geometry are
[ashlar ADR 0002](../../../../bevy-crates/ashlar/docs/adr/0002-attachment-and-derived-collision.md).
What follows is the original decision, kept as the record of why the split
exists.

Status: accepted, 2026-09-13.

## Context

Buildings should use reusable Rust recipes without baking the current corporate
concrete style into the engine. A macro language is premature before its underlying
types are useful. The earlier external-authoring prototype has been retired.

## Decision

Add `strata-buildings` as a Bevy-free domain crate beside the existing lower
crates. It depends only on shared math, Serde and typed errors. It describes local
solids, reusable parts, instances, material slots and attachment frames. Rust
functions and builders author the same editable data that Serde loads.

Finalization validates a recipe and returns an immutable description. Backends
will consume that description; the domain has no IO or renderer dependencies.
All dimensions and placements are local `f64` metres in right-handed Y-up space.
Style choices live in recipe geometry and supplied material bindings, with
instance overrides. There is no mandatory grid or fixed rotation increment.

## Consequences

The first step can be tested without a GPU. A future macro is a thin
frontend over ordinary types. Mesh generation, physics export and visual preview
remain separate implementation steps. The initial serialized recipe is experimental, not a runtime asset format.

## Mesh adapter implementation

`GeometryMesher`, `TriangleMesh` and `MeshError` define the renderer-independent
output boundary in the domain. `strata-building-mesh` evaluates expressions via
Manifold's Rust bindings, preserving shared part meshes and instance metadata.
The native C++ dependency lives entirely in that adapter, outside the domain
and the headless runtime graph. `building-preview` and the opt-in client grounding
fixture consume it before interactive startup.
The kernel can be replaced by another implementation of the port. No architectural
style, palette, grid policy or storey dimensions are introduced into the backend.

Manifold was chosen to evaluate nested arbitrary solid subtraction without
introducing a bespoke boolean kernel. Its native toolchain and first-build
source download are explicit tradeoffs; see the recipe workflow for prerequisites.

## Material and physical-detail implementation

The domain now also describes opaque PBR material libraries independently of
building recipes. Texture keys remain opaque; the preview adapter handles PNG
loading, color spaces, repeat sampling and tangent generation. Per-material
metre scale and pattern offsets preserve shared geometry across finish variants.
Optional element cut slots use native surface provenance to split outer and cut
batches; no architectural role or material type is baked into that mechanism.

Physical chamfers are an explicit box primitive, not an implicit global smoothing
policy. General edge modifiers can be added independently. The example content
lives in `veyra-buildings`, the preview texture tool and the study material RON, and demonstrates
shared modules in two structures. This does not migrate the existing world study.

## Terrain integration

Ground fitting is a pure calculation over a supplied triangulated height patch.
The domain owns footprint, entrance, policy and fitted convex-solid data. It does
not sample the world, select a style or create renderer/physics handles. Triangle
clipping checks the full support and approach surface, including extrema between
corners. World sampling and convex collision conversion sit in `strata-physics`.

`veyra-buildings` is renderer-free content shared by the standalone preview and
client fixture. `strata-building-render` shares mesh/PBR upload without a native
kernel dependency. The client fixture runs the mesher once and reuses upper meshes
across three sites. The server dependency checks explicitly reject the native
kernel. This is not yet a building streaming system.
