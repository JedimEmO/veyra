//! Which table values a run varies, as dotted paths into the four combat
//! tables (`weapons.Pistol.interval`, `actions.Light.duration`,
//! `actor.stamina_regen`, `opponents.rusher.cooldown`), and the Cartesian
//! product of their values: one cell per combination, one installed table
//! set per cell.
//!
//! The paths are resolved by hand, field by field, rather than through a
//! generic reflection of the structs: the set of tunables is the balancing
//! surface and belongs in one place where a wrong name is refused with the
//! list of right ones.

use veyra_rules::combat::{Action, ActionSpec, Weapon, WeaponSpec};
use veyra_rules::combat_tables::{
    ActorTable, BulwarkTable, CombatTables, DuelistTable, MarksmanTable, OpponentTable,
    RusherTable, SentryTable,
};

/// A mutable view of one numeric field.
pub enum Field<'a> {
    /// A float field.
    F32(&'a mut f32),
    /// A count field, such as a magazine.
    U16(&'a mut u16),
}

impl Field<'_> {
    /// Store `value`, which a count field must hold whole.
    fn set(self, value: f64) -> anyhow::Result<()> {
        match self {
            #[allow(clippy::cast_possible_truncation, reason = "a table value")]
            Field::F32(slot) => *slot = value as f32,
            Field::U16(slot) => {
                anyhow::ensure!(
                    value.fract() == 0. && value >= 0. && value <= f64::from(u16::MAX),
                    "{value} is not a whole count"
                );
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "checked above"
                )]
                {
                    *slot = value as u16;
                }
            }
        }
        Ok(())
    }
}

/// A struct whose numeric fields a path can name.
trait Tunable {
    /// The field names, in declaration order.
    const FIELDS: &'static [&'static str];
    /// The field called `name`, if any.
    fn field(&mut self, name: &str) -> Option<Field<'_>>;
}

macro_rules! tunable {
    ($ty:ty { f32: [$($f:ident),*] u16: [$($u:ident),*] }) => {
        impl Tunable for $ty {
            const FIELDS: &'static [&'static str] = &[$(stringify!($f),)* $(stringify!($u),)*];
            fn field(&mut self, name: &str) -> Option<Field<'_>> {
                match name {
                    $(stringify!($f) => Some(Field::F32(&mut self.$f)),)*
                    $(stringify!($u) => Some(Field::U16(&mut self.$u)),)*
                    _ => None,
                }
            }
        }
    };
}

tunable!(ActionSpec {
    f32: [contact_start, contact_end, duration, dodge_cancel, stamina, damage, posture, travel]
    u16: []
});
tunable!(WeaponSpec {
    f32: [interval, bolt_speed, damage, posture, reload, spread, aim_spread, move_spread, range, aim_time]
    u16: [magazine]
});
tunable!(ActorTable {
    f32: [
        health, shield, stamina, posture, stamina_regen, shield_regen, posture_regen,
        recovery_wait, hurt_wait, buffer, combo_window, aim_speed_scale,
        walk_speed, sprint_speed, jump_speed, gravity
    ]
    u16: []
});
tunable!(SentryTable {
    f32: [
        bolt_speed, charge, recover, cooldown, stagger_cooldown, walk_speed, hold_near,
        hold_far, fire_range
    ]
    u16: []
});
tunable!(RusherTable {
    f32: [walk_speed, reach, cooldown, initial_cooldown]
    u16: []
});
tunable!(MarksmanTable {
    f32: [
        bolt_speed, charge, lock, recover, cooldown, stagger_cooldown, walk_speed, hold_near,
        hold_far, fire_range, flee, pace
    ]
    u16: []
});
tunable!(DuelistTable {
    f32: [
        walk_speed, reach, cooldown, initial_cooldown, wary, react, rest, parry_rest,
        stagger_cooldown, slow_margin, read_reach
    ]
    u16: []
});
tunable!(BulwarkTable {
    f32: [advance_speed, reach, cooldown, initial_cooldown, rest, stagger_cooldown, pinned]
    u16: []
});
tunable!(OpponentTable {
    f32: [health, passive_health, shield, damage_scale, engage_distance, sprint_speed]
    u16: []
});

/// Every path a run may vary, for `balance list` and for the error on a
/// wrong one.
#[must_use]
pub fn paths() -> Vec<String> {
    let mut out = Vec::new();
    for action in Action::ALL {
        for field in ActionSpec::FIELDS {
            out.push(format!("actions.{}.{field}", action.name()));
        }
    }
    for weapon in Weapon::GUNS {
        for field in WeaponSpec::FIELDS {
            out.push(format!("weapons.{}.{field}", weapon.name()));
        }
    }
    out.extend(ActorTable::FIELDS.iter().map(|f| format!("actor.{f}")));
    out.extend(
        OpponentTable::FIELDS
            .iter()
            .map(|f| format!("opponents.{f}")),
    );
    out.extend(
        SentryTable::FIELDS
            .iter()
            .map(|f| format!("opponents.sentry.{f}")),
    );
    out.extend(
        RusherTable::FIELDS
            .iter()
            .map(|f| format!("opponents.rusher.{f}")),
    );
    out.extend(
        MarksmanTable::FIELDS
            .iter()
            .map(|f| format!("opponents.marksman.{f}")),
    );
    out.extend(
        DuelistTable::FIELDS
            .iter()
            .map(|f| format!("opponents.duelist.{f}")),
    );
    out.extend(
        BulwarkTable::FIELDS
            .iter()
            .map(|f| format!("opponents.bulwark.{f}")),
    );
    out.extend(
        ["share", "arc", "stamina", "posture", "parry", "riposte"]
            .iter()
            .map(|f| format!("opponents.bulwark.guard.{f}")),
    );
    out.extend(
        ["share", "stamina", "posture"]
            .iter()
            .map(|f| format!("opponents.bulwark.guard.bolts.{f}")),
    );
    out
}

fn variant<T: Copy>(all: &[T], name: &str, of: impl Fn(T) -> &'static str) -> Option<T> {
    all.iter()
        .copied()
        .find(|v| of(*v).eq_ignore_ascii_case(name))
}

/// The field at `path`.
///
/// # Errors
/// A path that names no field; the message lists the tables.
pub fn field<'a>(tables: &'a mut CombatTables, path: &str) -> anyhow::Result<Field<'a>> {
    let parts: Vec<&str> = path.split('.').collect();
    let found = match parts.as_slice() {
        ["actions", action, field] => variant(&Action::ALL, action, Action::name)
            .and_then(|action| tables.actions.actions.get_mut(&action))
            .and_then(|row| row.field(field)),
        ["weapons", weapon, field] => variant(&Weapon::GUNS, weapon, Weapon::name)
            .and_then(|weapon| tables.weapons.weapons.get_mut(&weapon))
            .and_then(|row| row.field(field)),
        ["actor", field] => tables.actor.field(field),
        ["opponents", "sentry", field] => tables.opponents.sentry.field(field),
        ["opponents", "rusher", field] => tables.opponents.rusher.field(field),
        ["opponents", "marksman", field] => tables.opponents.marksman.field(field),
        ["opponents", "duelist", field] => tables.opponents.duelist.field(field),
        ["opponents", "bulwark", field] => tables.opponents.bulwark.field(field),
        ["opponents", "bulwark", "guard", field] => tables
            .opponents
            .bulwark
            .guard
            .field_mut(field)
            .map(Field::F32),
        ["opponents", "bulwark", "guard", "bolts", field] => tables
            .opponents
            .bulwark
            .guard
            .bolts
            .as_mut()
            .and_then(|bolts| bolts.field_mut(field))
            .map(Field::F32),
        ["opponents", field] => tables.opponents.field(field),
        _ => None,
    };
    found.ok_or_else(|| {
        anyhow::anyhow!(
            "no combat table field at `{path}`; the paths are actions.<Action>.<field>, \
             weapons.<Weapon>.<field>, actor.<field>, opponents.<field>, \
             opponents.sentry.<field>, opponents.rusher.<field>, opponents.marksman.<field> and \
             opponents.duelist.<field>, opponents.bulwark.<field>, \
             opponents.bulwark.guard.<field> and opponents.bulwark.guard.bolts.<field> (`balance list` prints them)"
        )
    })
}

/// Set the field at `path`.
///
/// # Errors
/// A wrong path, or a fraction into a count.
pub fn set(tables: &mut CombatTables, path: &str, value: f64) -> anyhow::Result<()> {
    field(tables, path)?
        .set(value)
        .map_err(|error| anyhow::anyhow!("{path}: {error}"))
}

/// Read the field at `path`.
///
/// # Errors
/// A wrong path.
pub fn get(tables: &mut CombatTables, path: &str) -> anyhow::Result<f64> {
    Ok(match field(tables, path)? {
        Field::F32(slot) => f64::from(*slot),
        Field::U16(slot) => f64::from(*slot),
    })
}

/// One path and the values it takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Variation {
    /// The dotted path.
    pub path: String,
    /// Its values, in order.
    pub values: Vec<f64>,
}

impl std::str::FromStr for Variation {
    type Err = anyhow::Error;

    /// `path=v1,v2,v3`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (path, values) = text
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("`{text}`: want path=value,value,..."))?;
        let values = values
            .split(',')
            .map(|v| {
                v.trim()
                    .parse::<f64>()
                    .map_err(|error| anyhow::anyhow!("`{text}`: {v}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        anyhow::ensure!(!values.is_empty(), "`{text}`: no values");
        Ok(Self {
            path: path.trim().to_owned(),
            values,
        })
    }
}

/// One point of the grid: a value per varied path, in the variations'
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// `(path, value)` pairs.
    pub values: Vec<(String, f64)>,
}

impl Cell {
    /// `base` with this cell's values written over it.
    ///
    /// # Errors
    /// A wrong path.
    pub fn apply(&self, base: &CombatTables) -> anyhow::Result<CombatTables> {
        let mut tables = base.clone();
        for (path, value) in &self.values {
            set(&mut tables, path, *value)?;
        }
        Ok(tables)
    }
}

/// The Cartesian product of the variations: one cell with no values when
/// there are none, so a plain run is a grid of one.
#[must_use]
pub fn cells(variations: &[Variation]) -> Vec<Cell> {
    let mut cells = vec![Cell { values: Vec::new() }];
    for variation in variations {
        cells = cells
            .iter()
            .flat_map(|cell| {
                variation.values.iter().map(move |value| {
                    let mut values = cell.values.clone();
                    values.push((variation.path.clone(), *value));
                    Cell { values }
                })
            })
            .collect();
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_path_resolves_and_round_trips() {
        let mut tables = CombatTables::shipped();
        for path in paths() {
            let before = get(&mut tables, &path).expect(&path);
            set(&mut tables, &path, before).expect(&path);
            assert!((get(&mut tables, &path).expect(&path) - before).abs() < 1e-9);
        }
        assert!(get(&mut tables, "weapons.Rifle.interval").is_ok());
        assert!(get(&mut tables, "weapons.Blade.interval").is_err());
        assert!(get(&mut tables, "actor.nothing").is_err());
        assert!(set(&mut tables, "weapons.Pistol.magazine", 1.5).is_err());
        set(&mut tables, "weapons.pistol.magazine", 9.).expect("case-insensitive variant");
        assert_eq!(tables.weapons.spec(Weapon::Pistol).magazine, 9);
    }

    #[test]
    fn the_grid_is_the_product_of_the_variations_in_order() {
        let variations = [
            "weapons.Pistol.interval=0.12,0.15"
                .parse::<Variation>()
                .expect("parses"),
            "actions.Light.duration=0.35,0.40,0.45"
                .parse()
                .expect("parses"),
        ];
        let grid = cells(&variations);
        assert_eq!(grid.len(), 6);
        assert_eq!(
            grid[0].values[0],
            ("weapons.Pistol.interval".to_owned(), 0.12)
        );
        assert_eq!(
            grid[1].values[1],
            ("actions.Light.duration".to_owned(), 0.40)
        );
        assert_eq!(
            grid[5].values,
            vec![
                ("weapons.Pistol.interval".to_owned(), 0.15),
                ("actions.Light.duration".to_owned(), 0.45)
            ]
        );
        let applied = grid[5].apply(&CombatTables::shipped()).expect("applies");
        assert!((applied.actions.spec(Action::Light).duration - 0.45).abs() < 1e-6);
        assert_eq!(cells(&[]).len(), 1);
        assert!("nonsense".parse::<Variation>().is_err());
    }
}
