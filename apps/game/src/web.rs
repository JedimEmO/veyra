//! The browser's half of the entry (ticket 0013, step 3): the command line
//! from the page's query string and the two adapters the native `main`
//! installs from disk, here the data bundle `index.html` fetched and the
//! page's `localStorage`.

use clap::Parser;

use crate::{Cli, query};

/// The bundle's bytes, as `index.html` leaves them in `window.VEYRA_BUNDLE`
/// (a `Uint8Array`). A JS global rather than an exported function: the
/// wasm's start function runs inside `init()`, so the bytes have to be there
/// before it is called.
const BUNDLE_GLOBAL: &str = "VEYRA_BUNDLE";

/// The `Cli` the page's query string asks for. It is the first thing the
/// entry runs, so it also sets the panic hook that gets a message to the
/// console before Bevy's own is in place. An unreadable query falls
/// back to the defaults and says so on the console, since a browser has no
/// terminal for clap to print to.
pub fn cli() -> Cli {
    std::panic::set_hook(Box::new(|info| fail(&info.to_string())));
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    match Cli::try_parse_from(query::args_from_query(&search)) {
        Ok(cli) => cli,
        Err(error) => {
            console(&format!("query {search:?}: {error}"));
            Cli::parse_from(["game"])
        }
    }
}

/// Install the browser adapters of the two ports: the bundle as the files
/// and `localStorage` as the store.
///
/// # Panics
/// With no bundle on the page, or one that cannot be read: nothing in the
/// game can start without its data, and a panic reaches the console through the hook `cli` sets.
pub fn install_ports() {
    let window = js_sys::global();
    let value = js_sys::Reflect::get(&window, &BUNDLE_GLOBAL.into())
        .unwrap_or(wasm_bindgen::JsValue::UNDEFINED);
    assert!(
        !value.is_undefined(),
        "window.{BUNDLE_GLOBAL} is missing: index.html fetches the data bundle first"
    );
    let bytes = js_sys::Uint8Array::new(&value).to_vec();
    // Release the page's copy: the source holds its own.
    let _ = js_sys::Reflect::set(
        &window,
        &BUNDLE_GLOBAL.into(),
        &wasm_bindgen::JsValue::UNDEFINED,
    );
    let source = strata::registry::bundle::source_owned(bytes)
        .unwrap_or_else(|error| panic!("the data bundle: {error}"));
    strata::registry::files::install(Box::new(source));
    strata::sim::store::install(Box::new(strata::sim::store::LocalStorage));
}

/// Resources the app needs before its plugins read them: the pointer lock
/// waits for a click, and the settings live under a `localStorage` key.
pub fn insert_resources(app: &mut bevy::prelude::App) {
    app.insert_resource(strata::client::combat::PointerLockOnClick);
    app.insert_resource(strata::client::ui::SettingsFile(std::path::PathBuf::from(
        SETTINGS_KEY,
    )));
}

/// The settings file's key in `localStorage`, beside the household's.
const SETTINGS_KEY: &str = "veyra/settings.ron";

/// An error on the console and on the page, whose loading line it replaces
/// through `window.veyraFailed` when `index.html` has set one.
pub fn fail(line: &str) {
    web_sys::console::error_1(&line.into());
    if let Ok(hook) = js_sys::Reflect::get(&js_sys::global(), &"veyraFailed".into())
        && let Some(hook) = wasm_bindgen::JsCast::dyn_ref::<js_sys::Function>(&hook)
    {
        let _ = hook.call1(&wasm_bindgen::JsValue::NULL, &line.into());
    }
}

/// One line on the page's console.
pub fn console(line: &str) {
    web_sys::console::log_1(&line.into());
}

/// Tells the page the canvas is drawing, so its loading line goes away. The
/// bake before the app starts is the long wait; from the first frames on the
/// game shows its own loading screen.
pub fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    use wasm_bindgen::JsCast;
    app.add_systems(
        Update,
        |frames: Res<bevy::diagnostic::FrameCount>, mut told: Local<bool>| {
            if *told || frames.0 < 2 {
                return;
            }
            *told = true;
            let window = js_sys::global();
            if let Ok(ready) = js_sys::Reflect::get(&window, &"veyraReady".into())
                && let Some(ready) = ready.dyn_ref::<js_sys::Function>()
            {
                let _ = ready.call0(&wasm_bindgen::JsValue::NULL);
            }
        },
    );
}
