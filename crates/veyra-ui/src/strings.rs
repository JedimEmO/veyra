//! English for the game's own locale keys, pushed as a fallback after slotted's
//! own catalogue. The table itself is `veyra-words`, which the headless
//! household links too; this module is the slotted half: the `Localizer` that
//! answers from it.

use bevy::ecs::prelude::*;
use slotted::ui::{LocArgs, LocKey, Localization, Localizer};

pub use strata_ui::TITLE;
pub use veyra_words::{BAR_KEY_WORDS, counted, credits, english, grouped};

/// The English fallback for `strata.*` keys.
#[derive(Debug, Default, Clone, Copy)]
pub struct StrataStrings;

impl Localizer for StrataStrings {
    fn resolve(&self, key: &LocKey, _args: &LocArgs) -> Option<String> {
        english(&key.0).map(str::to_owned)
    }
}

/// `Startup`: pushes the fallback.
pub fn install(mut localization: ResMut<Localization>) {
    localization.push_fallback(StrataStrings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_key_is_the_frameworks() {
        assert!(english(TITLE).is_some());
    }

    #[test]
    fn every_settings_label_has_english() {
        let spec = strata_client::ui::settings::spec();
        for tab in &spec.tabs {
            assert!(english(&tab.label.0).is_some(), "{}", tab.label.0);
            for row in &tab.rows {
                if let slotted::menu::SettingsRow::Select { options, .. }
                | slotted::menu::SettingsRow::Radio { options, .. } = row
                {
                    for option in options {
                        assert!(english(&option.label.0).is_some(), "{}", option.label.0);
                    }
                }
                let label = match row {
                    slotted::menu::SettingsRow::Toggle { label, .. }
                    | slotted::menu::SettingsRow::Slider { label, .. }
                    | slotted::menu::SettingsRow::Select { label, .. }
                    | slotted::menu::SettingsRow::Radio { label, .. }
                    | slotted::menu::SettingsRow::Text { label, .. }
                    | slotted::menu::SettingsRow::Heading(label) => label.0.as_str(),
                    _ => continue,
                };
                assert!(english(label).is_some(), "{label}");
            }
        }
    }
}
