//! The rivers' per-mass files, fetched through the asset server.
//!
//! A browser's data bundle carries the masses the first view samples and the
//! rest sit beside the page (`xtask bundle`), so the sim's fetch ahead of the
//! observer asks for them here, and the engine's own HTTP (or, natively, the
//! `generated://` directory) brings them. The asset is an empty marker: the
//! sim wants bytes, which a loader can only hand to a shared table, so the
//! loader leaves them there under the path and the fetcher takes them out.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use bevy::asset::{AssetLoader, LoadContext, LoadState, io::Reader};
use bevy::prelude::*;
use strata::sim::{MassFetch, MassFetcher, WaterFetcher};

/// What a loaded file leaves as an asset: nothing, its bytes are in the table.
#[derive(Asset, TypePath, Debug)]
struct MassFile;

type Table = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// Loads `*.mass`.
#[derive(TypePath)]
struct MassLoader(Table);

impl AssetLoader for MassLoader {
    type Asset = MassFile;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<MassFile, std::io::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(context.path().path().to_string_lossy().into_owned(), bytes);
        Ok(MassFile)
    }

    fn extensions(&self) -> &[&str] {
        &["mass"]
    }
}

/// The fetcher: a handle held per path while the file loads.
struct Fetcher {
    server: AssetServer,
    table: Table,
    asked: Mutex<HashMap<String, Handle<MassFile>>>,
}

impl MassFetcher for Fetcher {
    fn fetch(&self, path: &str) -> MassFetch {
        // The files are mounted as `generated://`, whose root is the
        // logical `generated/`.
        let rooted = path.trim_start_matches("generated/");
        let mut asked = self.asked.lock().unwrap_or_else(PoisonError::into_inner);
        let handle = asked
            .entry(path.to_owned())
            .or_insert_with(|| self.server.load(format!("generated://{rooted}")));
        match self.server.load_state(handle.id()) {
            LoadState::Loaded => {
                asked.remove(path);
                let bytes = self
                    .table
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(rooted);
                bytes.map_or(MassFetch::Missing, MassFetch::Ready)
            }
            LoadState::Failed(_) => {
                asked.remove(path);
                MassFetch::Missing
            }
            LoadState::NotLoaded | LoadState::Loading => MassFetch::Pending,
        }
    }
}

/// Registers the loader and installs the [`WaterFetcher`] the sim's fetch
/// ahead asks. After the asset plugin, which builds the server.
pub fn install(app: &mut App) {
    let table = Table::default();
    app.init_asset::<MassFile>()
        .register_asset_loader(MassLoader(Arc::clone(&table)));
    let server = app.world().resource::<AssetServer>().clone();
    app.insert_resource(WaterFetcher(Arc::new(Fetcher {
        server,
        table,
        asked: Mutex::new(HashMap::new()),
    })));
}
