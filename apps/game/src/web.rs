//! The browser's half of the entry (ticket 0013, step 3): the command line
//! from the page's query string and the two adapters the native `main`
//! installs from disk, here the data bundle `index.html` fetched and the
//! page's `localStorage`; the pointer lock, which only the page can ask for
//! (ticket 0015, step 3); and the load's phases on the console.

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
/// is the page's to ask for, and the settings live under a `localStorage`
/// key.
pub fn insert_resources(app: &mut bevy::prelude::App) {
    app.insert_resource(veyra_view::combat::PointerLockOnClick::default());
    if let Some(adapter) = browser_adapter() {
        console(&format!("webgpu adapter {adapter}"));
        app.insert_resource(strata::client::ui::graphics::BrowserAdapter(adapter));
    }
    app.insert_resource(strata::client::ui::SettingsFile(std::path::PathBuf::from(
        SETTINGS_KEY,
    )));
}

/// What the page's `veyraAdapter` says of the WebGPU adapter, `vendor/
/// architecture`, for the graphics preset's default: wgpu reports only
/// `Other` on the web.
fn browser_adapter() -> Option<String> {
    js_sys::Reflect::get(&js_sys::global(), &"veyraAdapter".into())
        .ok()?
        .as_string()
        .filter(|adapter| !adapter.is_empty())
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

/// Seconds since the page began, from the browser's `performance.now()`: the
/// clock `index.html`'s own phase lines read.
fn page_seconds() -> f64 {
    let global = js_sys::global();
    js_sys::Reflect::get(&global, &"performance".into())
        .ok()
        .and_then(|performance| {
            let now = js_sys::Reflect::get(&performance, &"now".into()).ok()?;
            let now = wasm_bindgen::JsCast::dyn_ref::<js_sys::Function>(&now)?.clone();
            now.call0(&performance).ok()?.as_f64()
        })
        .map_or(0.0, |ms| ms / 1000.0)
}

/// A load phase reached, on the console with its time from the page's start,
/// so a load is read off by phase (ticket 0015, step 3).
pub fn phase(name: &str) {
    console(&format!("phase {name}: {:.2} s", page_seconds()));
}

/// The document's `pointerLockElement` is set: the browser holds the lock.
fn lock_held() -> bool {
    let global = js_sys::global();
    js_sys::Reflect::get(&global, &"document".into())
        .and_then(|document| js_sys::Reflect::get(&document, &"pointerLockElement".into()))
        .is_ok_and(|element| !element.is_null() && !element.is_undefined())
}

/// Lets the lock go: what the game does when a screen opens.
fn release_lock() {
    let global = js_sys::global();
    if let Ok(document) = js_sys::Reflect::get(&global, &"document".into())
        && let Ok(exit) = js_sys::Reflect::get(&document, &"exitPointerLock".into())
        && let Some(exit) = wasm_bindgen::JsCast::dyn_ref::<js_sys::Function>(&exit)
    {
        let _ = exit.call0(&document);
    }
}

/// `Update`: the page and the camera agree on the pointer lock. The camera
/// says each frame it runs that it wants the lock; the page's click handler
/// asks for it when `veyraWantsLock` is set; this reads back whether the
/// browser holds it and lets go when no screen-free camera wants it. A lock
/// the browser took away while the camera wanted it (Escape is swallowed by
/// the browser in a lock, so no key reaches the game) opens the pause, as
/// Escape does on a desktop.
fn pointer_lock(
    mut lock: bevy::prelude::ResMut<veyra_view::combat::PointerLockOnClick>,
    keys: bevy::prelude::Res<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>,
    mut commands: bevy::prelude::Commands,
    mut before: bevy::prelude::Local<bool>,
) {
    let wanted = std::mem::take(&mut lock.wanted);
    let held = lock_held();
    let _ = js_sys::Reflect::set(
        &js_sys::global(),
        &"veyraWantsLock".into(),
        &wasm_bindgen::JsValue::from_bool(wanted),
    );
    if held && !wanted {
        release_lock();
    }
    if *before && !held && wanted && !keys.just_pressed(bevy::prelude::KeyCode::Escape) {
        strata::client::ui::open_pause(&mut commands);
    }
    *before = held;
    if lock.held != held {
        lock.held = held;
    }
}

/// Tells the page the canvas is drawing, so its loading line goes away, and
/// puts the pointer lock's system in. The bake before the app starts used
/// to be the long wait; from the first frames on the game shows its own
/// loading screen.
pub fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    use wasm_bindgen::JsCast;
    phase("app built");
    app.add_systems(
        Update,
        (
            pointer_lock.in_set(strata::client::ui::ScreenWork),
            |frames: Res<bevy::diagnostic::FrameCount>,
             terrain: Option<Res<strata::terrain::TerrainStats>>,
             mut told: Local<[bool; 2]>| {
                if !told[0] && frames.0 >= 2 {
                    told[0] = true;
                    phase("first frame");
                    let window = js_sys::global();
                    if let Ok(ready) = js_sys::Reflect::get(&window, &"veyraReady".into())
                        && let Some(ready) = ready.dyn_ref::<js_sys::Function>()
                    {
                        let _ = ready.call0(&wasm_bindgen::JsValue::NULL);
                    }
                }
                if !told[1] && terrain.is_some_and(|stats| stats.built > 0) {
                    told[1] = true;
                    phase("first terrain region");
                }
            },
        ),
    );
}
