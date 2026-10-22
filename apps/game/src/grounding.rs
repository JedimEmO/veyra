//! Opt-in terrain fixtures: Veyra's study buildings, baked by `just content`,
//! fitted to three real sites of seed 42: a flat one, a slope and uneven
//! ground within 350 m of each other on the plain 3 km from Port Serein
//! towards `+x` and `-z`. They were on the desert plateau at (-1500,
//! -12000) until ticket 0004's step 2b ran a mountain range through it (the
//! sites stood at 1 300 to 1 900 m on its flank), then at (1000, -3620),
//! (1020, -3800) and (1200, -2480) until its step 4 gave the granite plain
//! domes (the flat site's ground there now ranges 0.39 m, and the uneven
//! one no longer fits the corporate style). Each was found by measurement,
//! both styles fitting, to be what the old ones were: under the outpost's
//! footprint the ground's range is under 5 cm, over a metre and over 1.7 m
//! (0.037, 1.50 and 2.58 m after step 4; 0.046, 1.51 and 2.66 m since its
//! review, which moved the plain by metres: the flat site keeps its place
//! with 4 mm to spare, and the flattest within a kilometre reads 0.036).
use anyhow::{Context, Result, ensure};
use ashlar::collision_file::{self, CollisionFile};
use ashlar::{ConvexSolid, Grounding, fit_ground};
use ashlar_bevy::prelude::{AshlarBuilding, MaterialLibraryAsset};
use bevy::prelude::*;
use strata::physics::{
    avian3d::prelude::{Position, RigidBody, Rotation},
    grounding::{collider, sample_patch},
};
use strata::prelude::*;
use strata::registry::AssetSource as _;
use strata::sim::footprints::{Footprint, VegetationExclusions};

const SITES: [(&str, [f64; 2], f64); 3] = [
    ("flat", [1080.0, -2600.0], 0.0),
    ("slope", [1040.0, -2760.0], 0.65),
    ("uneven", [1080.0, -2440.0], 0.65),
];

/// The material library the content step writes, in the `generated://` source.
const LIBRARY: &str = "generated://materials/library.materials.ron";

#[derive(clap::ValueEnum, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingStyle {
    #[default]
    Outpost,
    Corporate,
}

impl BuildingStyle {
    /// Where `just content` writes this building, relative to `generated/`.
    fn baked(self) -> &'static str {
        match self {
            Self::Outpost => "buildings/outpost.ashlar",
            Self::Corporate => "buildings/corporate.ashlar",
        }
    }
}

#[derive(Resource)]
pub(super) struct Prepared {
    building: &'static str,
    collision: Vec<ConvexSolid>,
    ground_material: String,
    sites: Vec<(&'static str, Grounding)>,
}

/// Bounded fixture preparation before the event loop: fitting and the
/// collision proxies, read from the baked building. Nothing is meshed here;
/// the pieces come from the same file through `AshlarPlugin`.
pub(super) fn prepare(cli: &mut super::Cli) -> Result<Prepared> {
    ensure!(cli.seed == 42, "grounding study requires --seed 42");
    let style = cli.grounding_style.unwrap_or_default();
    let (spec, policy, ground_material) = match style {
        BuildingStyle::Outpost => (
            veyra_buildings::grounding_spec(),
            veyra_buildings::grounding_policy(),
            "study:concrete",
        ),
        BuildingStyle::Corporate => (
            veyra_buildings::corporate::grounding_spec(),
            veyra_buildings::corporate::grounding_policy(),
            "metro:stone",
        ),
    };
    let files = super::data_files(cli);
    let collision = baked_collision(&files, &format!("generated/{}", style.baked()))?;
    // The loader constructs this identical immutable source from the same inputs.
    let registries = strata::registry::load_from_source(&files.under("assets"))?;
    let source = strata::worldgen::ProceduralWorld::new(Seed(cli.seed), &registries);
    let mut sites = Vec::new();
    for (name, at, yaw) in SITES {
        // The rivers under the patch first (ADR 0010), on the bake's own
        // threads: before the event loop, so on no frame.
        source.prepare_water(at[0], at[1], 0.0);
        let terrain = sample_patch(&source, at)?;
        let fit = fit_ground(&spec, policy, &terrain, at, yaw)
            .with_context(|| format!("fitting {name}"))?;
        println!(
            "grounding {name}: at={at:?}, yaw={yaw}, terrain={:?}, origin_y={:.4}, ramp_length={:.2}, grade={:.4}, toe={:?}",
            fit.terrain_range, fit.pose.translation.y, fit.ramp_length, fit.ramp_grade, fit.toe
        );
        if cli.grounding_study.as_deref() == Some(name) && cli.spawn.is_none() {
            let outward = fit.pose.rotation * bevy::math::DVec3::NEG_Z;
            let start = fit.toe + outward * 5.0;
            cli.spawn = Some((start.x, start.z));
            // The default arrival view faces the threshold; explicit --spawn/--look
            // remains the reproducible camera override used by captures.
            cli.look.get_or_insert(
                (fit.pose.rotation.as_quat().to_euler(EulerRot::YXZ).0 + std::f32::consts::PI)
                    % std::f32::consts::TAU,
            );
        }
        sites.push((name, fit));
    }
    Ok(Prepared {
        building: style.baked(),
        sites,
        collision,
        ground_material: ground_material.into(),
    })
}

/// The building's collision proxies, placed per instance, from the file the
/// content step writes beside its baked building: the same solids its pieces
/// were meshed from (ashlar ADR 0002), without parsing a render file.
fn baked_collision(files: &super::Files, path: &str) -> Result<Vec<ConvexSolid>> {
    let path = collision_file::path_for(path);
    let bytes = files
        .read(&path)
        .map_err(|error| anyhow::anyhow!("{error}"))
        .with_context(|| format!("reading {path}; run `just content` first"))?;
    let file = CollisionFile::read(&bytes).with_context(|| format!("reading {path}"))?;
    Ok(file.into_solids())
}

pub(super) fn install(app: &mut App, prepared: Prepared) {
    // Before streaming starts, append to any pre-existing world reservations.
    let mut exclusions = app
        .world_mut()
        .get_resource_or_insert_with(VegetationExclusions::default);
    for (_, fit) in &prepared.sites {
        for rect in &fit.exclusions {
            exclusions.0.push(Footprint {
                min: rect.min.to_array(),
                max: rect.max.to_array(),
            });
        }
    }
    app.insert_resource(prepared)
        .insert_resource(strata::client::hud::HudVisible(false))
        .add_systems(OnEnter(AppState::InWorld), spawn)
        .add_systems(
            Update,
            dress_groundworks.run_if(resource_exists::<Undressed>),
        );
}

/// Foundations waiting for the material library to load, and the key they wear.
#[derive(Resource)]
struct Undressed {
    library: Handle<MaterialLibraryAsset>,
    material: String,
    entities: Vec<Entity>,
}

fn spawn(
    mut commands: Commands,
    prepared: Res<Prepared>,
    server: Res<AssetServer>,
    origin: Res<RenderOrigin>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let library: Handle<MaterialLibraryAsset> = server.load(LIBRARY);
    let building = server.load(format!("generated://{}", prepared.building));
    let mut groundworks = Vec::new();
    for (name, fit) in &prepared.sites {
        let t = fit.pose.translation;
        let at = WorldPos::new(t.x, t.y, t.z);
        let rotation = fit.pose.rotation.as_quat();
        let root = commands
            .spawn((
                Name::new(format!("grounding.{name}")),
                AshlarBuilding {
                    building: building.clone(),
                    materials: library.clone(),
                },
                WorldPosition(at),
                Transform::from_translation(origin.to_render(at)).with_rotation(rotation),
                Visibility::default(),
            ))
            .id();
        groundworks.push(
            commands
                .spawn((
                    Name::new(format!("{name}/groundworks")),
                    Mesh3d(meshes.add(
                        ashlar_bevy::ground_mesh(&fit.solids).expect("valid foundation prisms"),
                    )),
                    Transform::default(),
                    ChildOf(root),
                ))
                .id(),
        );
        let mut solids = prepared.collision.clone();
        solids.extend(fit.solids.iter().map(ConvexSolid::from));
        commands.spawn((
            Name::new(format!("{name}/collision")),
            WorldPosition(at),
            Position(origin.to_render(at)),
            Rotation(rotation),
            RigidBody::Static,
            collider(&solids).expect("validated convex proxies"),
        ));
    }
    commands.insert_resource(Undressed {
        library,
        material: prepared.ground_material.clone(),
        entities: groundworks,
    });
}

/// Give the foundations their material once the library is in. The building's
/// own pieces are `AshlarPlugin`'s; the foundations are the fit's, so they are
/// dressed here from the same library.
fn dress_groundworks(
    mut commands: Commands,
    undressed: Res<Undressed>,
    libraries: Res<Assets<MaterialLibraryAsset>>,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(library) = libraries.get(&undressed.library) else {
        return;
    };
    let Some(definition) = library.0.materials.get(&undressed.material) else {
        error!(
            "the material library has no {}; the foundations stay undressed",
            undressed.material
        );
        commands.remove_resource::<Undressed>();
        return;
    };
    let material = materials.add(ashlar_bevy::create_material(definition, &server));
    for entity in &undressed.entities {
        commands
            .entity(*entity)
            .insert(MeshMaterial3d(material.clone()));
    }
    commands.remove_resource::<Undressed>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashlar::LodPolicy;
    use ashlar_manifold::ManifoldMesher;
    use clap::Parser;
    use veyra_buildings::Scene;

    /// A `generated/` holding the two buildings, baked here so the tests need
    /// no content step run first.
    fn generated() -> tempfile::TempDir {
        let root = tempfile::TempDir::new().expect("a scratch directory");
        for (style, scene) in [
            (BuildingStyle::Outpost, Scene::Outpost),
            (BuildingStyle::Corporate, Scene::Corporate),
        ] {
            let building = veyra_buildings::building(scene).expect("the study validates");
            let baked =
                ashlar_manifold::bake(&building, &LodPolicy::ladder(), &ManifoldMesher::default())
                    .expect("the study meshes");
            let path = root.path().join(style.baked());
            std::fs::create_dir_all(path.parent().expect("a relative file"))
                .expect("a writable scratch directory");
            std::fs::write(
                collision_file::path_for(path.to_str().expect("a UTF-8 path")),
                CollisionFile::from_baked(&baked)
                    .expect("a level zero")
                    .write(),
            )
            .expect("a writable scratch directory");
            std::fs::write(path, baked.write().expect("a bake that serialises"))
                .expect("a writable scratch directory");
        }
        root
    }

    #[test]
    fn corporate_fixture_fits_and_reads_its_proxies() {
        let root = generated();
        let mut cli = super::super::Cli::parse_from([
            "game",
            "--grounding-study",
            "flat",
            "--grounding-style",
            "corporate",
        ]);
        cli.generated = root.path().to_owned();
        let prepared = prepare(&mut cli).expect("corporate sites fit and the proxies read");
        assert_eq!(prepared.sites.len(), 3);
        assert_eq!(prepared.ground_material, "metro:stone");
        assert!(!prepared.collision.is_empty());
    }

    #[test]
    fn a_missing_bake_says_to_run_the_content_step() {
        let root = tempfile::TempDir::new().expect("a scratch directory");
        let mut cli = super::super::Cli::parse_from(["game", "--grounding-study", "flat"]);
        cli.generated = root.path().to_owned();
        let error = prepare(&mut cli).err().expect("nothing is baked");
        assert!(format!("{error:#}").contains("just content"), "{error:#}");
    }

    #[test]
    fn fixture_fits_real_sites_and_appends_reservations_before_streaming() {
        let root = generated();
        let mut cli =
            super::super::Cli::parse_from(["game", "--grounding-study", "flat", "--look", "0.3"]);
        cli.generated = root.path().to_owned();
        let prepared = prepare(&mut cli).expect("all real sites fit");
        assert_eq!(prepared.sites.len(), 3);
        assert_eq!(cli.look, Some(0.3));
        let relief = prepared
            .sites
            .iter()
            .map(|(_, fit)| fit.terrain_range[1] - fit.terrain_range[0])
            .collect::<Vec<_>>();
        assert!(
            relief[0] < 0.05 && relief[1] > 1.0 && relief[2] > 1.7,
            "{relief:?}"
        );
        let mut app = App::new();
        let existing = Footprint {
            min: [0.0, 0.0],
            max: [1.0, 1.0],
        };
        app.insert_resource(VegetationExclusions(vec![existing]));
        install(&mut app, prepared);
        let exclusions = app.world().resource::<VegetationExclusions>();
        assert_eq!(exclusions.0.len(), 10);
        assert!(exclusions.contains(0.5, 0.5));
        let prepared = app.world().resource::<Prepared>();
        for (_, fit) in &prepared.sites {
            assert!(exclusions.contains(fit.toe.x, fit.toe.z));
            assert!(exclusions.contains(fit.pose.translation.x, fit.pose.translation.z));
        }
        assert!(!exclusions.contains(-1000.0, -12000.0));
    }
}
