use std::sync::{Arc, Mutex};

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, GlobalShortcuts, NewShortcut,
};
use ashpd::Result;
use futures::StreamExt;

pub type ActivateCallback = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

const SHORTCUT_ID: &str = "reclip-save";
const SHORTCUT_TRIGGER: &str = "ALT+s";
const SHORTCUT_DESC: &str = "Save ReClip recording";

pub async fn run(on_activate: ActivateCallback) -> Result<()> {
    loop {
        match setup_and_listen(on_activate.clone()).await {
            Ok(()) => {
                eprintln!("reclip: hotkey session closed, reconnecting in 1 second");
                async_io::Timer::after(std::time::Duration::from_secs(1)).await;
            }
            Err(err) => {
                eprintln!("reclip: hotkey setup failed: {err}; retrying in 5 seconds");
                async_io::Timer::after(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn setup_and_listen(on_activate: ActivateCallback) -> Result<()> {
    let app_id = "com.reclip.ReClip".parse::<ashpd::AppID>()?;
    println!("hotkey: registering app id via host portal registry");
    if let Err(err) = ashpd::register_host_app(app_id).await {
        eprintln!("hotkey: warning: app id registration failed ({err}); continuing without it");
    }
    println!("hotkey: app id registration step finished");

    let global_shortcuts = GlobalShortcuts::new().await?;
    println!(
        "hotkey: GlobalShortcuts portal connected, version {}",
        global_shortcuts.version()
    );

    let session = global_shortcuts.create_session(CreateSessionOptions::default()).await?;
    println!("hotkey: GlobalShortcuts session created");

    let shortcuts = vec![NewShortcut::new(SHORTCUT_ID, SHORTCUT_DESC).preferred_trigger(
        SHORTCUT_TRIGGER,
    )];
    let request = global_shortcuts
        .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
        .await?;
    let response = request.response()?;
    println!("hotkey: BindShortcuts response: {response:?}");

    let mut stream = global_shortcuts.receive_activated().await?;
    println!("hotkey: listening for Activated signal");

    while let Some(activated) = stream.next().await {
        println!("hotkey: received signal: {activated:?}");
        if activated.shortcut_id() != SHORTCUT_ID {
            continue;
        }
        println!("hotkey: shortcut '{SHORTCUT_ID}' activated, running save");
        let guard = match on_activate.lock() {
            Ok(guard) => guard,
            Err(err) => {
                eprintln!("reclip: hotkey callback lock failed: {err}");
                continue;
            }
        };
        if let Some(callback) = guard.as_ref() {
            callback();
        }
    }

    // Portal menutup sesi: tutup juga di sisi kita supaya retry nggak numpuk session.
    let _ = session.close().await;
    Ok(())
}
