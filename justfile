# sci-fi-scrolls developer commands. `just --list` shows this.

default: check

# Type-check every member, including tests and benches.
check:
    cargo check --workspace --all-targets

# Run the whole test suite. Four test threads a binary (eight froze the
# whole machine once, mid worldgen tests, on 2026-10-03): the worldgen and
# physics binaries run thread pools of their own under each test, and the
# full load of a default thread count is what makes this machine's CPU
# kill a binary mid-run (CLAUDE.md, the SIGSEGV note).
test:
    cargo test --workspace -- --test-threads=4

# Clippy with the workspace lint set, warnings fatal.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# Rustdoc for the workspace, with broken intra-doc links fatal.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# The slice in a window. Arguments go to the binary: `just run --seed 7`.
run *args:
    cargo run -p game -- {{args}}

# The simulation alone, no window, for ten frames. What a server runs.
run-headless *args="--frames 10":
    cargo run -p game -- --headless {{args}}

# A biome and height map of a seed to PNG.
worldmap *args:
    cargo run -p worldmap -- {{args}}

# The dedicated server on UDP 5000 (release: the sim, the streamer and the
# physics for every connected player). Arguments go to the binary:
# `just run-server --port 5001 --seed 7`.
run-server *args:
    cargo run --release -p server -- {{args}}

# A client of a local server. `just run-client --connect host:port` for
# another one; `--seed` must match the server's.
run-client *args="--connect 127.0.0.1:5000":
    cargo run --release -p game -- {{args}}

# ADR 0001 promises that the headless facade has no renderer in it. Cargo
# features are additive and a single `bevy/bevy_render` in any crate below
# the facade silently breaks that for every consumer, with nothing but the
# compile time to show for it. This is the assertion, in two halves: the
# headless facade graph holds only the Bevy crates ADR 0001 allows below the
# render layer, and the crates below the render layer build and test on
# their own, because `cargo test --workspace` unifies Bevy's features across
# every member (apps/game turns the defaults on) and would let a test in
# veyra-sim that reaches for a render-side API pass unnoticed.
#
# Assert the dedicated-server dependency graph really has no renderer in it.
server-check:
    #!/usr/bin/env bash
    set -euo pipefail
    # ADR 0001's set below the render layer, plus the umbrella crate's
    # always-on support crates (macros, reflect, utils, platform, input,
    # diagnostic) that come with `bevy` itself at default-features = false.
    allowed="bevy_app bevy_ecs bevy_math bevy_state bevy_time bevy_transform bevy_tasks"
    allowed="$allowed bevy bevy_internal bevy_derive bevy_ecs_macros bevy_ecs_macro_logic bevy_macro_utils"
    allowed="$allowed bevy_reflect bevy_reflect_derive bevy_state_macros bevy_utils bevy_platform bevy_ptr"
    allowed="$allowed bevy_diagnostic bevy_input"
    # strata-physics: avian3d wants Bevy's log crate, its mass-property
    # helper and its transform easing crate. None of them is a renderer, a
    # window or a UI.
    allowed="$allowed bevy_log bevy_heavy bevy_transform_interpolation"
    found=$(cargo tree -p strata --no-default-features --features headless -e normal \
        | grep -oE "bevy_[a-z_]*|\bbevy\b" | sort -u || true)
    bad=""
    for crate in $found; do
        case " $allowed " in
            *" $crate "*) ;;
            *) bad="$bad $crate" ;;
        esac
    done
    if [ -n "$bad" ]; then
        echo "the headless graph pulls Bevy crates outside the ADR 0001 allowlist:" >&2
        echo "$bad" >&2
        echo "find the culprit with: cargo tree -p strata --no-default-features --features headless -e normal -i <crate>" >&2
        exit 1
    fi
    echo "headless graph holds only: $(echo $found | tr '\n' ' ')"
    # The dedicated server binary: the headless facade plus the replication
    # adapter and the transport. A denylist rather than the allowlist above,
    # because replicon and renet bring their own small set of Bevy crates
    # (app, ecs, time, log, state) and what matters is that nothing that
    # draws, opens a window or lays out UI came with them.
    # The balance harness (tools/balance) links the sim and the physics
    # the same way and gets the same denylist, and so does the game's headless
    # half (veyra-game: the household, the people, the camps), which the
    # server links beside the facade.
    for binary in server balance veyra-game; do
        server_found=$(cargo tree -p $binary -e normal \
            | grep -oE "bevy_[a-z_]*|\bbevy\b" | sort -u || true)
        server_bad=""
        for crate in $server_found; do
            case "$crate" in
                bevy_render|bevy_winit|bevy_ui|bevy_ui_render|bevy_pbr|bevy_window|bevy_text|bevy_core_pipeline|bevy_sprite|bevy_gizmos|bevy_image|bevy_asset)
                    server_bad="$server_bad $crate" ;;
            esac
        done
        if [ -n "$server_bad" ]; then
            echo "the $binary binary's graph pulls a render, window or UI crate:" >&2
            echo "$server_bad" >&2
            echo "find the culprit with: cargo tree -p $binary -e normal -i <crate>" >&2
            exit 1
        fi
        echo "$binary graph holds only: $(echo $server_found | tr '\n' ' ')"
    done
    # The combat view draws and plays: the server, the balance harness and the
    # headless facade link neither it nor the engine client under it, nor the
    # screens (strata-ui, veyra-ui).
    for graph in "-p server" "-p balance" "-p veyra-game" "-p strata --no-default-features --features headless"; do
        if cargo tree $graph -e normal --prefix none | rg '^(veyra-view|veyra-ui|veyra-dev|strata-client|strata-ui|strata-terrain|strata-atmosphere|strata-foliage|ashlar-bevy) '; then
            echo "a render-layer client crate leaked into a headless graph ($graph)" >&2
            exit 1
        fi
    done
    # The two screen crates keep the boundary the split exists for: the engine's
    # framework has no game crate and no terrain, atmosphere, foliage or client
    # over it, and the game's screens link none of those either, so a terrain
    # edit never waits on or rebuilds the screens.
    if cargo tree -p strata-ui -e normal --prefix none | rg '^(veyra-|strata-(sim|terrain|atmosphere|foliage|client)) '; then
        echo "strata-ui must not depend on a game crate or on the terrain, atmosphere, foliage or client" >&2
        exit 1
    fi
    if cargo tree -p veyra-ui -e normal --prefix none | rg '^strata-(client|terrain|atmosphere|foliage) '; then
        echo "veyra-ui must not depend on the engine client, the terrain, the atmosphere or the foliage" >&2
        exit 1
    fi
    # The development half (stills, scripts, the benchmark's flags) is the game's
    # `dev-tools` feature and nothing else's: the browser build's graph has none
    # of it, and no headless graph above has it either (the leak check names it).
    if cargo tree -p game --no-default-features --features web -e normal --prefix none | rg '^veyra-dev '; then
        echo "veyra-dev leaked into the browser build's graph (the game's dev-tools feature is off there)" >&2
        exit 1
    fi
    # Runtime collision consumes plain solids, never the native authoring kernel,
    # and nothing that plays bakes a material: the kernel, the graph engine and
    # the content step belong to `tools/content` and the previews. The game's
    # own tests bake with the kernel, which is why only its normal edges count.
    for graph in "-p server" "-p veyra-game" "-p strata --no-default-features --features headless" "-p game"; do
        if cargo tree $graph -e normal --prefix none | rg '^(ashlar-manifold|manifold-csg|manifold-csg-sys|ashlar-material|ashlar-content|ashlar-impostor|veyra-materials|veyra-flora|veyra-rocks|veyra-settlements|ashlar-showcase) '; then
            echo "an authoring tool leaked into a runtime graph ($graph)" >&2
            exit 1
        fi
    done
    # The Bevy-free row of the layering rule: no Bevy crate at all, and no
    # arrow up to a Bevy crate of the workspace.
    for crate in strata-core strata-data veyra-data strata-registry strata-worldgen strata-flora strata-weather veyra-world veyra-buildings veyra-weapons veyra-words ashlar; do
        if cargo tree -p $crate -e normal --prefix none | rg '^(bevy[a-z_]*|strata-(app|sim|stream|physics|net|net-replicon|terrain|atmosphere|foliage|client|ui)|veyra-(view|ui|dev)) '; then
            echo "$crate must not depend on Bevy or on a crate above it" >&2
            exit 1
        fi
    done
    # The rules crate and the plain-data crate under strata-app link bevy_ecs and
    # bevy_math and nothing above them: no bevy_app, no umbrella crate, no
    # state, time or transform crate, no crate of the engine core or the sim, so
    # a rule edit rebuilds nothing under the render layer and the rules compile
    # beside Bevy's own crates.
    for crate in veyra-rules strata-types; do
        if cargo tree -p $crate -e normal --prefix none | rg '^(bevy|bevy_app|bevy_state|bevy_time|bevy_transform|bevy_render|strata-(app|sim|stream|physics|net|net-replicon|terrain|atmosphere|foliage|client|ui)|veyra-(view|ui|dev)) '; then
            echo "$crate must link bevy_ecs and bevy_math only, and nothing above strata-app" >&2
            exit 1
        fi
    done
    # The engine's physics and replication adapter sit over strata-app and take the
    # game's contributions as data and plugins: the rules, the sim and the game's
    # physics are nowhere in their graphs, and none of them is a direct edge.
    for crate in strata-physics strata-net-replicon; do
        if cargo tree -p $crate -e normal --prefix none | rg '^(veyra-sim|veyra-rules|veyra-physics) '; then
            echo "$crate must not depend on the sim, the rules or the game's physics" >&2
            exit 1
        fi
        if cargo tree -p $crate -e normal --depth 1 --prefix none | rg '^veyra-'; then
            echo "$crate must have no direct veyra-* edge" >&2
            exit 1
        fi
    done
    # The world crates link the world half of the data alone, so an item,
    # schematic or quest edit never rebuilds them: no arrow from them to the
    # game's data, the game's world rules or the facade over both.
    for crate in strata-data strata-worldgen strata-flora strata-weather; do
        if cargo tree -p $crate -e normal --prefix none | rg '^(veyra-data|veyra-world|strata-registry) '; then
            echo "$crate must not depend on the game's data or the registry facade" >&2
            exit 1
        fi
    done
    cargo check -p strata --no-default-features --features headless
    cargo check -p server
    cargo test -p veyra-buildings -p strata-core -p strata-data -p veyra-data -p strata-registry -p strata-worldgen -p strata-flora -p strata-weather -p veyra-world -p strata-types -p strata-app -p veyra-rules -p veyra-sim -p strata-stream -p strata-physics -p veyra-physics -p veyra-words -p veyra-game -p strata-net -p strata-net-replicon -- --test-threads=4

# What CI runs. The feel contracts are gated by the `contracts` test of
# tools/balance inside `test`; `balance-check` is the same bands as a
# release run for a person, not a second gate.
ci: fmt-check lint test server-check

# Benchmark the fixtures against the plan's performance gates (release build,
# needs a GPU and a display). `just bench --record "Phase 7"` appends the
# results to docs/perf/ledger.md; `just bench --fixtures woodland` runs one.
[positional-arguments]
bench *args:
    cargo build --release -p game
    python3 tools/bench.py "$@"

# The four look fixtures (canopy, dawn, dusk street, coast) captured to
# scratch/look/ with their GPU cost appended to docs/perf/look-ledger.md:
# `just look --label "Phase 1 sun" --render-look <preset>`; `just look
# --compare <a> <b>` prints the per-fixture GPU delta between two labels.
[positional-arguments]
look *args:
    cargo build --release -p game
    python3 tools/look.py "$@"

# The duel harness (progression design, section 7): every preset matchup,
# twenty bouts each, on the real sim and physics, headless, as CSV. The
# bout rows go to stdout and the summary to stderr; the arguments replace
# the default and go to the binary whole, subcommand included:
# `just balance duel --matchup carbine-vs-rusher --vary weapons.Carbine.damage=20,24,28`,
# `just balance list` prints the matchups and the paths. A table edit
# rebuilds the sim stack first (the tables are compiled in as well as read).
balance *args="duel --bouts 20":
    cargo run --release -p balance -- {{args}}

# The feel contracts: every band of assets/combat/contracts.ron run on the
# shipped tables, printed with what it measured, and a failure when a
# chosen one is out of its range. The same bands gate `just test` through
# the `contracts` test; this is the release run for a person, and on a
# cold machine it builds the sim and the physics in release first.
# docs/balance/README.md says what to do with a miss.
balance-check:
    cargo run --release -p balance -- check

# Everything a person reviews before accepting a content change, into
# scratch/review/: every building from three fixed angles by day and by night,
# with an index.html each. Asset families add their contact sheets here as they
# arrive: the weapon pictures' sheets are in scratch/review/weapons/, one per
# maker, which takes a run of the content step. The numbers are in generated/stats/
# after `just content`, and their snapshots in tools/content/tests/snapshots/.
review:
    cargo build --release -p building-preview
    ./target/release/building-preview --references scratch/review/buildings-day
    ./target/release/building-preview --references scratch/review/buildings-night --night
    cargo run --release -p content -- --weapon-sheets scratch/review/weapons

# Bake every generated asset into generated/, which the game mounts as the
# `generated://` source. Arguments go to the binary: `just content --resolution 256`.
content *args:
    cargo run --release -p content -- {{args}}

# Preview Rust building recipes independently of world streaming.
building-recipe-preview *args:
    CARGO_BUILD_JOBS=1 cargo run -p building-preview -- {{args}}

# Fit the Rust outpost to existing terrain.
building-grounding site="uneven" *args:
    CARGO_BUILD_JOBS=1 cargo run -p game -- --serial-pipelines --grounding-study {{site}} {{args}}

# Headless fixed-camera gallery of every Rust building generator (~3 s after build).
building-references *args:
    CARGO_BUILD_JOBS=1 RUST_MIN_STACK=8388608 cargo build -p building-preview
    RUST_MIN_STACK=8388608 taskset -c 0-3 nice -n 10 ./target/debug/building-preview --references target/building-references {{args}}

# Interactive procedural weapon assembly studio.
weapon-workbench *args:
    cargo run -p weapon-workbench -- {{args}}

# The web build into dist/ (ticket 0013, step 3): the wasm and its bindings, the
# data bundle (the registries and layouts the game reads through its file port),
# index.html, and the assets and baked files Bevy loads over HTTP (what is
# copied is apps/game/web/*.filter; the rest is in the bundle). Run `just
# content` first; serve dist/ with any static server.
web:
    rm -rf dist
    mkdir -p dist
    env -u CC -u CXX -u CFLAGS -u CXXFLAGS cargo build -p game --profile web --target wasm32-unknown-unknown --no-default-features --features web
    wasm-bindgen --target web --no-typescript --out-dir dist --out-name veyra target/wasm32-unknown-unknown/web/game.wasm
    ls -l dist/veyra_bg.wasm
    if command -v wasm-opt >/dev/null; then wasm-opt -O2 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-simd -o dist/veyra_bg.opt.wasm dist/veyra_bg.wasm && mv dist/veyra_bg.opt.wasm dist/veyra_bg.wasm && ls -l dist/veyra_bg.wasm; fi
    cargo run -p xtask -- bundle dist/data.bundle.gz
    cp apps/game/web/index.html dist/index.html
    rsync -a --prune-empty-dirs --filter='. apps/game/web/assets.filter' assets/ dist/assets/
    rsync -a --prune-empty-dirs --filter='. apps/game/web/generated.filter' generated/ dist/generated/
