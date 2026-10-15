//! The weapon in the hand drawn from what is fitted (ticket 0007, step 8).
//!
//! The content step bakes every chassis, every mount's standard part and
//! every module as a prop in the recipe's frame, and an index saying which
//! file is which. Here the client turns a weapon's record into the files
//! that make it up and the point a bolt leaves from. No rule and no recipe
//! lives here: the sim's `Hands` say what is fitted, the index says what to
//! draw. Without the index, or for a chassis it lacks, the old art stays.
//!
//! The pieces are stood ([`stand`]) and waited for ([`Pieces`]) here too,
//! once, for the two places that draw a weapon from them: the hand
//! (`combat_art`) and the stage the fitting screen's model turns on.

use ashlar_bevy::prelude::{AshlarFailed, AshlarProp, AshlarSpawned, MaterialLibraryAsset};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use strata_registry::item::{ItemCatalogue, ItemKind, Record};
use strata_registry::models::{MODELS_PATH, ModelSet};

/// Every baked weapon model. Empty until the game reads the index, and
/// empty for good on a clone that has not run `just content`.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct WeaponModels(pub ModelSet);

impl WeaponModels {
    /// The index under `generated`, the content step's output root, as a
    /// source rooted there.
    ///
    /// A missing or unreadable index is no error: it is said once at info
    /// and the old art is drawn. The models are no simulation state.
    #[must_use]
    pub fn read(generated: &strata_registry::files::Files) -> Self {
        let path = std::path::Path::new(MODELS_PATH);
        let read = generated
            .read_string(MODELS_PATH)
            .map_err(|error| error.to_string())
            .and_then(|text| ModelSet::from_ron(&text).map_err(|error| error.to_string()));
        match read {
            Ok(set) => {
                tracing::info!(
                    path = %path.display(),
                    chassis = set.chassis.len(),
                    "weapon models"
                );
                Self(set)
            }
            Err(error) => {
                tracing::info!(
                    path = %path.display(),
                    %error,
                    "no weapon models: the old weapon art is drawn until `just content` runs"
                );
                Self::default()
            }
        }
    }
}

/// What one weapon in the hand is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    /// The chassis's key.
    pub chassis: String,
    /// The prop files, relative to the generated root: the chassis, then a
    /// piece a mount.
    pub files: Vec<String>,
    /// Where a bolt leaves, in the recipe's frame.
    pub tip: [f32; 3],
}

/// The pieces a record's weapon is drawn from: the chassis, then for each
/// mount the module fitted there when the record has one and the index
/// knows it, else the part the mount is built with. `None` for an item that
/// is no weapon or a chassis the index lacks.
#[must_use]
pub fn wanted(models: &ModelSet, items: &ItemCatalogue, record: &Record) -> Option<Wanted> {
    let ItemKind::Weapon {
        chassis, mounts, ..
    } = &items.get(&record.item)?.kind
    else {
        return None;
    };
    let set = models.chassis.get(chassis)?;
    let mut files = vec![set.piece(None, None)?.file.clone()];
    let mut muzzle = None;
    for mount in mounts {
        let part = record
            .fitted
            .get(&mount.slot)
            .and_then(|fitted| match &items.get(&fitted.item)?.kind {
                ItemKind::Module { part, .. } => Some(part.as_str()),
                _ => None,
            })
            .filter(|part| set.piece(Some(&mount.slot), Some(part)).is_some());
        let piece = set
            .piece(Some(&mount.slot), part)
            .or_else(|| set.piece(Some(&mount.slot), None));
        if let Some(piece) = piece {
            files.push(piece.file.clone());
        }
        if mount.slot == "muzzle" {
            muzzle = part;
        }
    }
    Some(Wanted {
        chassis: chassis.clone(),
        files,
        tip: set.tip(muzzle),
    })
}

/// Stands the pieces of `wanted` as props under a new entity carrying
/// `root`, in the recipe's frame, and gives that entity. The pieces load
/// and spawn over the frames that follow; [`Pieces::standing`] says when.
pub fn stand(
    commands: &mut Commands,
    server: &AssetServer,
    models: &ModelSet,
    wanted: &Wanted,
    root: impl Bundle,
) -> Entity {
    let library: Handle<MaterialLibraryAsset> =
        server.load(format!("generated://{}", models.materials));
    commands
        .spawn(root)
        .with_children(|parent| {
            for file in &wanted.files {
                parent.spawn(AshlarProp {
                    prop: server.load(format!("generated://{file}")),
                    materials: library.clone(),
                });
            }
        })
        .id()
}

/// How far a model stood by [`stand`] has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Some piece has not spawned yet.
    Loading,
    /// Every piece has spawned: the model can take the place of the one on
    /// show without a frame of nothing.
    Ready,
    /// A piece failed (a stale index, a partial `generated/`): the model is
    /// to be thrown away.
    Failed,
}

/// The pieces of the models stood by [`stand`], for the one rule of when a
/// model is whole.
#[derive(SystemParam)]
pub struct Pieces<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    spawned: Query<'w, 's, (Has<AshlarSpawned>, Has<AshlarFailed>)>,
}

impl Pieces<'_, '_> {
    /// How far `model` has come. One with no pieces yet is still loading.
    #[must_use]
    pub fn standing(&self, model: Entity) -> Standing {
        let states: Vec<_> = self
            .children
            .get(model)
            .map(|pieces| {
                pieces
                    .iter()
                    .filter_map(|piece| self.spawned.get(piece).ok())
                    .collect()
            })
            .unwrap_or_default();
        if states.iter().any(|(_, failed)| *failed) {
            Standing::Failed
        } else if !states.is_empty() && states.iter().all(|(spawned, _)| *spawned) {
            Standing::Ready
        } else {
            Standing::Loading
        }
    }
}

/// Where the recipe's grip origin (top front of the grip, under the
/// receiver) sits in the hand's prop frame, metres: in the palm, the
/// barrel running out of the fist.
pub const PALM: Vec3 = Vec3::new(0.0, 0.02, 0.03);

/// The old art is scaled 0.9 under its mount; a baked weapon is in metres
/// and is drawn at its true size, so the model undoes that scale.
pub const OLD_MOUNT_SCALE: f32 = 0.9;

/// The recipe's frame (X across, Y up, Z along the barrel to the muzzle)
/// in the hand's prop frame (barrel on +Y, the grip hanging to +Z): a
/// quarter turn about X, then the palm. Scale undoes the mount's.
#[must_use]
pub fn frame() -> Transform {
    Transform::from_translation(PALM)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::splat(1.0 / OLD_MOUNT_SCALE))
}

/// A melee weapon's grip origin along the hand bone and across it, metres:
/// where the thin shape's grip stood.
pub const MELEE_GRIP: Vec3 = Vec3::new(0.0, 0.05, 0.005);

/// The recipe's frame for a melee weapon: the tip along the hand's +Y as
/// the thin shape ran, at true size, with no mount scale to undo.
#[must_use]
pub fn melee_frame() -> Transform {
    Transform::from_translation(MELEE_GRIP)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
}

#[cfg(test)]
// Tips are compared with the exact values the test wrote, never computed.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use strata_registry::models::{ChassisModels, ModelPiece};

    fn piece(mount: Option<&str>, module: Option<&str>, tip: Option<[f32; 3]>) -> ModelPiece {
        let stem = match (mount, module) {
            (None, _) => "chassis".to_owned(),
            (Some(mount), None) => format!("{mount}.standard"),
            (Some(mount), Some(module)) => format!("{mount}.{module}"),
        };
        ModelPiece {
            mount: mount.map(str::to_owned),
            module: module.map(str::to_owned),
            file: format!("weapons/veyra/patrol_carbine/{stem}.ashprop"),
            tip,
        }
    }

    fn models() -> ModelSet {
        let mut pieces = vec![piece(None, None, None)];
        for mount in ["sight", "feed", "fore_end", "grip", "muzzle", "stock"] {
            let tip = (mount == "muzzle").then_some([0.0, 0.071, 0.49]);
            pieces.push(piece(Some(mount), None, tip));
        }
        pieces.push(piece(Some("sight"), Some("vigil_reflex"), None));
        pieces.push(piece(
            Some("muzzle"),
            Some("vigil_compensator"),
            Some([0.0, 0.071, 0.509]),
        ));
        let mut set = ModelSet::default();
        set.chassis.insert(
            "patrol_carbine".to_owned(),
            ChassisModels {
                muzzle: [0.0, 0.07, 0.4],
                pieces,
            },
        );
        set
    }

    fn items() -> ItemCatalogue {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        strata_registry::load_from_dir(assets)
            .expect("assets/ loads")
            .items
    }

    fn carbine(items: &ItemCatalogue, fitted: &[(&str, &str)]) -> Record {
        let hands = strata_sim::items::Hands::from_saved(
            &items
                .starting()
                .expect("the first day's stock")
                .hands
                .clone()
                .map(Some),
            items,
        )
        .expect("hands");
        let mut record =
            strata_sim::items::read(hands.0.get(0).expect("carbine"), items).expect("record");
        for (mount, module) in fitted {
            record.fitted.insert(
                (*mount).to_owned(),
                strata_registry::item::Fitted {
                    item: strata_registry::Namespaced::parse(module).expect("id"),
                    batch: 0,
                    by: None,
                    lines: std::collections::BTreeMap::default(),
                    condition: None,
                },
            );
        }
        record
    }

    #[test]
    fn a_bare_carbine_is_the_chassis_and_six_standard_pieces() {
        let items = items();
        let got = wanted(&models(), &items, &carbine(&items, &[])).expect("a carbine");
        assert_eq!(got.files.len(), 7);
        assert!(got.files[0].ends_with("chassis.ashprop"));
        assert!(got.files[1..].iter().all(|f| f.contains(".standard.")));
        assert_eq!(got.tip, [0.0, 0.071, 0.49]);
    }

    #[test]
    fn a_fitted_sight_replaces_only_its_own_piece() {
        let items = items();
        let record = carbine(&items, &[("sight", "veyra:module.vigil_reflex")]);
        let got = wanted(&models(), &items, &record).expect("a carbine");
        assert_eq!(got.files.len(), 7);
        assert_eq!(
            got.files
                .iter()
                .filter(|f| f.contains(".standard."))
                .count(),
            5
        );
        assert!(
            got.files
                .iter()
                .any(|f| f.ends_with("sight.vigil_reflex.ashprop"))
        );
    }

    #[test]
    fn a_fitted_muzzle_moves_the_tip() {
        let items = items();
        // No muzzle module is an item yet, so a module whose part the index
        // knows on the muzzle mount stands in for one.
        let mut models = models();
        models
            .chassis
            .get_mut("patrol_carbine")
            .expect("chassis")
            .pieces
            .push(piece(
                Some("muzzle"),
                Some("patrol_extended_feed"),
                Some([0.0, 0.071, 0.509]),
            ));
        let record = carbine(&items, &[("muzzle", "veyra:module.patrol_extended_feed")]);
        let got = wanted(&models, &items, &record).expect("a carbine");
        assert_eq!(got.tip, [0.0, 0.071, 0.509]);
    }

    #[test]
    fn an_unknown_chassis_gives_nothing() {
        let items = items();
        let record = carbine(&items, &[]);
        assert_eq!(wanted(&ModelSet::default(), &items, &record), None);
    }

    #[test]
    fn the_frame_turns_the_barrel_up_and_the_grip_back() {
        let turn = frame().rotation;
        assert!((turn * Vec3::Z - Vec3::Y).length() < 1e-5);
        assert!((turn * Vec3::NEG_Y - Vec3::Z).length() < 1e-5);
    }

    /// A service blade's record by hand: the items give no start for it.
    fn blade(items: &ItemCatalogue, edge: Option<&str>) -> Record {
        let id = strata_registry::Namespaced::parse("veyra:weapon.service_blade").expect("id");
        let def = items.get(&id).expect("the service blade");
        let mut record = Record {
            item: def.id.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: std::collections::BTreeMap::default(),
            condition: None,
            fitted: std::collections::BTreeMap::default(),
            origin: None,
        };
        if let Some(edge) = edge {
            record.fitted.insert(
                "edge".to_owned(),
                strata_registry::item::Fitted {
                    item: strata_registry::Namespaced::parse(edge).expect("id"),
                    batch: 0,
                    by: None,
                    lines: std::collections::BTreeMap::default(),
                    condition: None,
                },
            );
        }
        record
    }

    fn blade_models() -> ModelSet {
        let mut set = ModelSet::default();
        set.chassis.insert(
            "field_blade".to_owned(),
            ChassisModels {
                muzzle: [0.0, 0.0, 0.72],
                pieces: vec![
                    piece(None, None, None),
                    piece(Some("edge"), None, None),
                    piece(Some("grip"), None, None),
                    piece(Some("edge"), Some("blade_hooked_edge"), None),
                ],
            },
        );
        set
    }

    #[test]
    fn a_melee_weapon_is_its_chassis_then_its_standard_pieces_then_a_fitted_part() {
        let items = items();
        let bare = wanted(&blade_models(), &items, &blade(&items, None)).expect("a blade");
        assert_eq!(bare.chassis, "field_blade");
        assert_eq!(bare.files.len(), 3);
        assert!(bare.files[0].ends_with("chassis.ashprop"));
        assert!(bare.files[1].contains("edge.standard"));
        assert!(bare.files[2].contains("grip.standard"));
        let fitted = wanted(
            &blade_models(),
            &items,
            &blade(&items, Some("veyra:module.blade_hooked_edge")),
        )
        .expect("a blade");
        assert_eq!(fitted.files.len(), 3);
        assert!(fitted.files[1].contains("edge.blade_hooked_edge"));
        assert!(fitted.files[2].contains("grip.standard"));
    }

    #[test]
    fn the_melee_frame_keeps_the_tip_on_the_hands_long_axis_at_true_size() {
        let frame = melee_frame();
        assert!((frame.rotation * Vec3::Z - Vec3::Y).length() < 1e-5);
        assert_eq!(frame.scale, Vec3::ONE);
    }
}
