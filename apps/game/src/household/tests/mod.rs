//! The household's tests that need the game binary: the ones that build the
//! whole app from a command line, or drive the scripts. The rest are
//! `veyra_game`'s own, and the helpers they share are `veyra_game::test_support`.

use super::*;
use clap::Parser;
use std::path::Path;
use strata::registry::item::LotOf;
use strata::registry::progression::{AuthoredSheets, XpKind};
use strata::sim::ability::Bar;
use veyra_game::household::Out;
use veyra_game::test_support::*;

mod bar;
mod quest;
mod script;
mod slots;
