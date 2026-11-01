//! The desktop client: one window, talking to `apc serve` over loopback HTTP.
//!
//! All the work happens in the page (`ui/index.html`); this binary only opens the window and hands
//! it the token, so there is no second agent loop and no second store.

#![forbid(unsafe_code)]

use std::path::PathBuf;

/// Where `apc serve` writes its token. Kept in step with `apc-cli`'s `serve::token_path`.
fn token_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("dev", "aplix", "aplix")?;
    Some(dirs.data_dir().join("serve.token"))
}

fn main() {
    // A missing token is not fatal: the window still opens and the page says so.
    let token = token_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();

    tauri::Builder::default()
        .setup(move |app| {
            // The token never leaves the process: it is injected before the page boots, so the
            // page never has to ask for it over IPC.
            let script = format!("window.APC_TOKEN = {token:?};");
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Aplix Code")
            .inner_size(960.0, 760.0)
            .initialization_script(&script)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("apc-desktop could not start");
}
