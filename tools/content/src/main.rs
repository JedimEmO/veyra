//! The content step: every generated asset the game loads, written under
//! `generated/`, which the game mounts as the `generated://` asset source.
//!
//! ```text
//! just content                     # everything, each material at its own resolution
//! just content --resolution 256    # a quick pass while iterating
//! just content --weapon-sheets scratch/review/weapons    # and the weapon pictures' contact sheets
//! ```
//!
//! Its output is regenerated, never committed and never edited by hand; deleting
//! `generated/` is always safe. Every write is atomic, so a running game that
//! watches the directory never reads half a file.
use std::path::PathBuf;

use anyhow::{Context, Result};
use ashlar::BakedBuilding;
use ashlar_content::Content;
use clap::Parser;
use content::{MATERIALS, STATS, building_stats, buildings};
use strata_flora::library::{Collider, SpeciesEntry, TreeLibrary, TreeVariant, Trunk};

/// Where generated content goes by default: `generated/` at the workspace
/// root, beside `assets/`.
const WORKSPACE_GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");

/// The shipped registries the settlement's world is built from.
const WORKSPACE_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Where bakes and meshings are cached by their inputs: under `target/`, so
/// `cargo clean` clears it with everything else that is derived.
const WORKSPACE_CACHE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/content-cache");

#[derive(Parser, Debug)]
#[command(name = "content", about)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent command line switches"
)]
struct Cli {
    /// The root to write under.
    #[arg(long, default_value = WORKSPACE_GENERATED)]
    out: PathBuf,
    /// Bake every material at this resolution instead of its own.
    #[arg(long)]
    resolution: Option<u32>,
    /// The cache of bakes and meshings keyed by their inputs.
    #[arg(long, default_value = WORKSPACE_CACHE)]
    cache: PathBuf,
    /// Bake and mesh everything again, reading no cache entry and writing none.
    #[arg(long)]
    no_cache: bool,
    /// Also write the Phase 8 capacity fixture: every synthetic tree under
    /// `generated/capacity/`, for `prop-capacity` to load.
    #[arg(long)]
    capacity: bool,
    /// Also write a contact sheet of the weapon pictures per maker into this
    /// directory, for a person to judge them by: `scratch/review/weapons`.
    #[arg(long, value_name = "DIR")]
    weapon_sheets: Option<PathBuf>,
    /// Bake only the weapon pictures, the weapon models and their two
    /// indexes, so a change to a recipe does not rebake the city.
    #[arg(long)]
    weapons_only: bool,
    /// Bake only the vehicle prop, its icon and their index.
    #[arg(long)]
    vehicles_only: bool,
}

#[allow(
    clippy::too_many_lines,
    reason = "the content step's stages in order, each a few lines"
)]
fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.weapons_only {
        let content = Content::new(&cli.out).resolution(cli.resolution);
        weapons(&content, cli.weapon_sheets.as_deref())?;
        return weapon_models(&content);
    }
    if cli.vehicles_only {
        let content = Content::new(&cli.out).resolution(cli.resolution);
        return vehicle_models(&content);
    }
    let mut buildings = buildings()?;
    let (city, layout, plan) = content::settlement(std::path::Path::new(WORKSPACE_ASSETS))?;
    buildings.extend(city);
    let (site_files, site_layouts) = content::site_groups(&buildings)?;
    buildings.extend(site_files);
    let (crossing_files, crossing_kit) = content::crossings()?;
    buildings.extend(crossing_files);
    let (roadside_files, roadside_kit) = content::roadside()?;
    buildings.extend(roadside_files);
    let started = std::time::Instant::now();
    let content = Content::new(&cli.out)
        .resolution(cli.resolution)
        .cache((!cli.no_cache).then_some(&cli.cache));
    let shipped = content.ship(
        &content::graphs(),
        &content::definitions(),
        MATERIALS,
        &buildings,
        &content::ladder(),
    )?;
    println!(
        "materials: {} sets baked, {} from the cache, {} definitions, {:.1} MB",
        shipped.materials.sets.len(),
        shipped.materials.cached.len(),
        shipped.materials.library.materials.len(),
        mb(shipped.materials.bytes)
    );
    for (path, written) in &shipped.buildings {
        let how = if written.cached {
            "from the cache".to_owned()
        } else {
            format!("meshed in {:.2?}", written.elapsed)
        };
        println!(
            "{path}: triangles by level {:?}, {:.1} MB, {how}",
            written.triangles,
            mb(written.bytes),
        );
    }
    let skyline = skyline(&content, &plan, &shipped.materials.library)?;
    // What each building costs, from the files as written, for a reviewer.
    let stats = shipped
        .buildings
        .iter()
        .map(|(path, _)| path.as_str())
        .chain(layout.skyline.as_deref())
        .map(|path| {
            let bytes = std::fs::read(cli.out.join(path))?;
            Ok(building_stats(path, &BakedBuilding::read(&bytes)?))
        })
        .collect::<Result<Vec<_>>>()?;
    content.write_file(
        STATS,
        ron::ser::to_string_pretty(&stats, ron::ser::PrettyConfig::default())?.as_bytes(),
    )?;
    content.write_file(
        content::SETTLEMENT_LAYOUT,
        ron::ser::to_string_pretty(&layout, ron::ser::PrettyConfig::default())?.as_bytes(),
    )?;
    println!(
        "{}: {} buildings, layout in {}, skyline {} pieces and {} triangles, {:.1} MB, {}",
        layout.id,
        layout.buildings.len(),
        content::SETTLEMENT_LAYOUT,
        skyline.pieces,
        skyline.written.triangles.iter().sum::<usize>(),
        mb(skyline.written.bytes),
        if skyline.written.cached {
            "from the cache".to_owned()
        } else {
            format!("meshed in {:.2?}", skyline.written.elapsed)
        },
    );
    for group in &site_layouts {
        let path = strata_worldgen::sites::group_layout_path(&group.group);
        content.write_file(
            &path,
            ron::ser::to_string_pretty(group, ron::ser::PrettyConfig::default())?.as_bytes(),
        )?;
        println!(
            "{}: {} arrangements in {path}",
            group.group,
            group.arrangements.len()
        );
    }
    content.write_file(
        strata_worldgen::crossings::KIT_PATH,
        ron::ser::to_string_pretty(&crossing_kit, ron::ser::PrettyConfig::default())?.as_bytes(),
    )?;
    println!(
        "crossings: {} bridge classes, {} piers, {} files in {}",
        crossing_kit.bridges.len(),
        crossing_kit.piers.len(),
        crossing_kit.files().len(),
        strata_worldgen::crossings::KIT_PATH
    );
    content.write_file(
        strata_worldgen::roadside::ROADSIDE_PATH,
        ron::ser::to_string_pretty(&roadside_kit, ron::ser::PrettyConfig::default())?.as_bytes(),
    )?;
    println!(
        "roadside: {} files in {}",
        roadside_kit.files().len(),
        strata_worldgen::roadside::ROADSIDE_PATH
    );
    weapons(&content, cli.weapon_sheets.as_deref())?;
    weapon_models(&content)?;
    vehicle_models(&content)?;
    let trees = trees(&content)?;
    println!(
        "plants and rocks: {} species, {} variants, {} near triangles in all, {:.1} MB, library version {:016x}",
        trees.library.species.len(),
        trees.variants,
        trees.triangles,
        mb(trees.bytes),
        trees.library.version,
    );
    if cli.capacity {
        let mut triangles = 0;
        let mut bytes = 0;
        let (graphs, definitions) = (content::graphs(), content::definitions());
        for species in 0..veyra_flora::capacity::SPECIES {
            // One impostor per species, from its first variant: from past
            // the last mesh level the variants of a species are the same
            // few pixels, and an atlas per variant would cost eight times
            // the texture memory for nothing a camera sees.
            let impostor = content.write_impostor(
                &veyra_flora::capacity::impostor_directory(species),
                &veyra_flora::capacity::tree(species, 0),
                &graphs,
                &definitions,
                ashlar_content::ImpostorSettings {
                    frames: 12,
                    frame_px: 64,
                    hemispherical: true,
                },
            )?;
            bytes += impostor.bytes;
            for variant in 0..veyra_flora::capacity::VARIANTS {
                let mut prop = veyra_flora::capacity::tree(species, variant);
                prop.levels.push(ashlar::prop::PropLevel {
                    until: None,
                    cast_shadows: false,
                    pieces: Vec::new(),
                    impostor: Some(impostor.impostor.clone()),
                });
                let written =
                    content.write_prop(&veyra_flora::capacity::path(species, variant), &prop)?;
                triangles += written.triangles[0];
                bytes += written.bytes;
            }
        }
        println!(
            "capacity: {} trees, {triangles} near triangles in all, {:.1} MB",
            veyra_flora::capacity::SPECIES * veyra_flora::capacity::VARIANTS,
            mb(bytes)
        );
    }
    println!("wrote {} in {:.1?}", cli.out.display(), started.elapsed());
    Ok(())
}

/// What [`skyline`] wrote, for the summary line.
struct Skyline {
    written: ashlar_content::BuildingWritten,
    pieces: usize,
}

/// Bake the settlement's skyline and write it where the layout names it.
///
/// Through [`Content::write_building`], so it is cached by its recipe and
/// ladder like every building. It binds only materials the city's buildings
/// bind, resolved to the same keys, so the library `ship` wrote already
/// holds them; that is checked, not assumed. The recipe declares no
/// collider and no portal, and the file is read back to prove it: a skyline
/// with either would spawn a second, invisible city of colliders.
fn skyline(
    content: &Content,
    plan: &veyra_settlements::SettlementPlan,
    library: &ashlar::MaterialLibrary,
) -> Result<Skyline> {
    let skyline = content::skyline(plan)?;
    for (instance, slot, binding) in skyline.bindings() {
        anyhow::ensure!(
            library.materials.contains_key(&binding.material),
            "the skyline's {instance} binds {} to {slot}, which the shipped library does not hold",
            binding.material
        );
    }
    let written = content.write_building(
        content::SETTLEMENT_SKYLINE,
        &skyline,
        &veyra_settlements::skyline::LADDER,
    )?;
    let bytes = std::fs::read(content.root().join(content::SETTLEMENT_SKYLINE))
        .context("the skyline as written")?;
    let baked = BakedBuilding::read(&bytes)?;
    anyhow::ensure!(
        baked.part_colliders.values().all(Vec::is_empty)
            && baked.part_portals.values().all(Vec::is_empty),
        "the skyline carries colliders or portals"
    );
    let pieces = building_stats(content::SETTLEMENT_SKYLINE, &baked).levels[0].pieces;
    Ok(Skyline { written, pieces })
}

/// Bake the weapon models (ticket 0007, step 8): a prop a piece, the plain
/// materials they wear and the index, and say what was written.
///
/// Not cached, like the pictures: the content cache has no entry for a prop,
/// and `write_file` leaves unchanged bytes alone.
fn weapon_models(content: &Content) -> Result<()> {
    let started = std::time::Instant::now();
    let (props, set) = content::weapon_models()?;
    let (mut triangles, mut bytes) = (0, 0);
    for (path, prop) in &props {
        let written = content.write_prop(path, prop)?;
        triangles += written.triangles.first().copied().unwrap_or(0);
        bytes += written.bytes;
    }
    content.write_materials(
        &ashlar_material::MaterialGraphLibrary::default(),
        &content::weapon_definitions(),
        content::WEAPON_MATERIALS,
    )?;
    content.write_file(
        strata_registry::models::MODELS_PATH,
        set.to_ron().as_bytes(),
    )?;
    println!(
        "weapon models: {} chassis, {} props, {triangles} triangles, {:.0} kB, library {}, index {}, in {:.2?}",
        set.chassis.len(),
        props.len(),
        mb(bytes) * 1e3,
        content::WEAPON_LIBRARY,
        strata_registry::models::MODELS_PATH,
        started.elapsed(),
    );
    Ok(())
}

/// Bake the courier bike (ticket 0009, step 2): its prop, the plain materials
/// it wears and the index, and say what was written.
fn vehicle_models(content: &Content) -> Result<()> {
    let started = std::time::Instant::now();
    let (props, index) = content::vehicle_models()?;
    let (mut triangles, mut bytes) = (0, 0);
    for (path, prop) in &props {
        let written = content.write_prop(path, prop)?;
        triangles += written.triangles.first().copied().unwrap_or(0);
        bytes += written.bytes;
    }
    content.write_materials(
        &ashlar_material::MaterialGraphLibrary::default(),
        &content::vehicle_definitions(),
        content::VEHICLE_MATERIALS,
    )?;
    let (icons, [wide, high]) = content::vehicle_icons()?;
    for (path, data) in &icons {
        content.write_file(path, data)?;
    }
    println!(
        "vehicle icon: {} files, picture {wide} by {high}",
        icons.len()
    );
    content.write_file(
        strata_registry::vehicle::MODELS_PATH,
        index.to_ron().as_bytes(),
    )?;
    println!(
        "vehicle models: {} props, {triangles} triangles, {:.0} kB, library {}, index {}, in {:.2?}",
        props.len(),
        mb(bytes) * 1e3,
        content::VEHICLE_LIBRARY,
        strata_registry::vehicle::MODELS_PATH,
        started.elapsed(),
    );
    Ok(())
}

/// Bake the weapon pictures (ticket 0008), write them and their index, and
/// say what was written; with a directory, write the contact sheets there
/// too.
///
/// Rasterised on every run: the content cache holds materials, buildings and
/// impostors and has no entry for a plain file, so the line says how long
/// the work took instead of where it came from. Files whose bytes have not
/// changed are left alone by [`Content::write_file`] all the same.
fn weapons(content: &Content, sheets: Option<&std::path::Path>) -> Result<()> {
    let started = std::time::Instant::now();
    let (files, set) = content::weapons()?;
    for (path, data) in &files {
        content.write_file(path, data)?;
    }
    content.write_file(
        strata_registry::pictures::PICTURES_PATH,
        set.to_ron().as_bytes(),
    )?;
    let all = || set.chassis.values().flat_map(|c| &c.pieces);
    let pieces = all().count();
    println!(
        "weapons: {} chassis, {pieces} pieces ({} hidden by their chassis, {} the same picture as another) and {} whole, an icon each, in {} files, {:.0} kB, index in {}, rasterised in {:.2?} (no cache holds pictures)",
        set.chassis.len(),
        all().filter(|piece| piece.hidden).count(),
        all().filter(|piece| piece.same_as.is_some()).count(),
        set.chassis.len(),
        files.len(),
        mb(files.iter().map(|(_, data)| data.len()).sum()) * 1e3,
        strata_registry::pictures::PICTURES_PATH,
        started.elapsed(),
    );
    if let Some(directory) = sheets {
        // Atomic writes for these too: a viewer left open on a sheet reloads
        // a whole file.
        let review = Content::new(directory);
        let sheets = content::sheet::weapon_sheets(&files, &set)?;
        for (name, data) in &sheets {
            review.write_file(name, data)?;
        }
        let names: Vec<&str> = sheets.iter().map(|(name, _)| name.as_str()).collect();
        println!(
            "weapon sheets: {} in {}",
            names.join(", "),
            directory.display()
        );
    }
    Ok(())
}

/// What [`trees`] wrote, for the summary line.
struct Trees {
    library: TreeLibrary,
    variants: usize,
    triangles: usize,
    bytes: usize,
}

/// One species the content step bakes into the library: a tree, an
/// understory plant or a rock, whichever generator grew it.
struct Baked {
    /// The flora registry id it is placed by.
    species: &'static str,
    /// The directory under `generated/` its props go to.
    directory: &'static str,
    /// Its short name, for file names.
    name: &'static str,
    /// Whether it gets a far picture; small plants fade out before one would show.
    impostor: bool,
    /// Each variant's prop and height.
    variants: Vec<(ashlar::prop::Prop, f64)>,
}

/// The most generator threads at once: the development machine's CPU (an
/// i9-13900KF) crashes processes under heavy multi-thread load, so a bake
/// runs on a fixed few, not a thread per species.
const BAKE_THREADS: usize = 16;

/// Every species the generators grow, variants grown in parallel on at most
/// [`BAKE_THREADS`] threads.
fn baked() -> Vec<Baked> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    let flora = veyra_flora::recipes();
    let rocks = veyra_rocks::recipes();
    let mut jobs: Vec<Box<dyn Fn() -> Baked + Sync + '_>> = Vec::new();
    for recipe in &flora {
        jobs.push(Box::new(move || Baked {
            species: recipe.species,
            directory: "flora",
            name: recipe.name(),
            impostor: recipe.impostor(),
            variants: (0..veyra_flora::VARIANTS)
                .map(|v| {
                    let grown = veyra_flora::grow(recipe, v);
                    (grown.prop, grown.height)
                })
                .collect(),
        }));
    }
    for recipe in &rocks {
        jobs.push(Box::new(move || Baked {
            species: recipe.species,
            directory: "rocks",
            name: recipe.name(),
            impostor: recipe.impostor(),
            variants: (0..veyra_rocks::VARIANTS)
                .map(|v| {
                    let grown = veyra_rocks::grow(recipe, v);
                    (grown.prop, grown.height)
                })
                .collect(),
        }));
    }
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, Baked)>> = Mutex::new(Vec::new());
    thread::scope(|scope| {
        let workers: Vec<_> = (0..BAKE_THREADS.min(jobs.len()))
            .map(|_| {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(i) else { break };
                        let baked = job();
                        done.lock().expect("a generator thread").push((i, baked));
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("a generator thread");
        }
    });
    let mut done = done.into_inner().expect("a generator thread");
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, baked)| baked).collect()
}

/// Grow every variant of every tree, understory and rock species, bake an
/// impostor per species that wants one, write each variant as a prop with
/// the impostor as its last level, and write the library the game and the
/// server place them from.
///
/// The impostor is baked from the first variant alone, as the capacity
/// fixture's is: an atlas per variant would be six times the texture memory
/// until the atlases are block compressed, and the game scales the one
/// picture to each variant's height.
fn trees(content: &Content) -> Result<Trees> {
    let (graphs, definitions) = (content::graphs(), content::definitions());
    let mut library = TreeLibrary::default();
    let (mut variants, mut triangles, mut bytes) = (0, 0, 0);
    for species in baked() {
        let impostor = if species.impostor {
            let written = content.write_impostor(
                &format!("{}/impostors/{}", species.directory, species.name),
                &species.variants[0].0,
                &graphs,
                &definitions,
                ashlar_content::ImpostorSettings::default(),
            )?;
            bytes += written.bytes;
            Some(written.impostor)
        } else {
            None
        };
        let mut entry = SpeciesEntry::default();
        for (variant, (mut prop, height)) in species.variants.into_iter().enumerate() {
            let collider = collider(&prop);
            if let Some(impostor) = &impostor {
                prop.levels.push(ashlar::prop::PropLevel {
                    until: None,
                    cast_shadows: false,
                    pieces: Vec::new(),
                    impostor: Some(impostor.clone()),
                });
            }
            let path = format!("{}/{}-{variant}.ashprop", species.directory, species.name);
            let written = content.write_prop(&path, &prop)?;
            triangles += written.triangles[0];
            bytes += written.bytes;
            variants += 1;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "metres, to f32 for placement"
            )]
            entry.variants.push(TreeVariant {
                prop: path,
                height: height as f32,
                collider,
            });
        }
        library.species.insert(species.species.to_owned(), entry);
    }
    let library = library.versioned();
    content.write_file(
        strata_flora::library::LIBRARY_PATH,
        library.to_ron().as_bytes(),
    )?;
    Ok(Trees {
        library,
        variants,
        triangles,
        bytes,
    })
}

/// What the server collides with, from the prop's colliders: an upright
/// capsule is a trunk, collided as a cylinder to its top; a lying one is a
/// log; convex hulls are a rock. A prop with none is walked through.
#[expect(
    clippy::cast_possible_truncation,
    reason = "metres, to f32 for placement"
)]
fn collider(prop: &ashlar::prop::Prop) -> Collider {
    use ashlar::prop::PropCollider;
    let point = |p: &ashlar::glam::DVec3| [p.x as f32, p.y as f32, p.z as f32];
    let hulls: Vec<Vec<[f32; 3]>> = prop
        .colliders
        .iter()
        .filter_map(|c| match c {
            PropCollider::ConvexHull(solid) => Some(solid.vertices.iter().map(point).collect()),
            _ => None,
        })
        .collect();
    if !hulls.is_empty() {
        return Collider::Hulls(hulls);
    }
    match prop.colliders.first() {
        Some(PropCollider::Capsule { a, b, radius }) => {
            let upright = (a.x - b.x).abs() < 1e-3 && (a.z - b.z).abs() < 1e-3;
            if upright {
                Collider::Cylinder(Trunk {
                    radius: *radius as f32,
                    height: (a.y.max(b.y) + radius) as f32,
                })
            } else {
                Collider::Capsule {
                    a: point(a),
                    b: point(b),
                    radius: *radius as f32,
                }
            }
        }
        Some(PropCollider::Cylinder {
            base,
            height,
            radius,
        }) => Collider::Cylinder(Trunk {
            radius: *radius as f32,
            height: (base.y + height) as f32,
        }),
        _ => Collider::None,
    }
}

#[expect(clippy::cast_precision_loss, reason = "a byte count for people")]
fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1e6
}
