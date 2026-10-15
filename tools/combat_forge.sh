#!/usr/bin/env bash
# Keep every generation and provenance record inside the combat project.
set -euo pipefail
combat_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export FORGE_HOME=${FORGE_HOME:-"$combat_root/../../asset-forge"}
cd "$combat_root/assets-src/combat/forge"
exec "$combat_root/target/combat-forge/debug/forge" --project "$PWD" "$@"
