use ksni::menu::{MenuItem, StandardItem};
use ksni::{Tray, TrayMethods};

struct ReclipTray;

impl Tray for ReclipTray {
    fn id(&self) -> String {
        "reclip".into()
    }

    fn icon_name(&self) -> String {
        "camera-video".into()
    }

    fn title(&self) -> String {
        "ReClip".into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Save clip (Alt+S)".into(),
                icon_name: "document-save".into(),
                activate: Box::new(|_| {
                    let _ = crate::save_recording();
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit ReClip".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| {
                    let _ = crate::run_stop();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub async fn run() -> std::io::Result<()> {
    let tray = ReclipTray;
    match tray.spawn().await {
        Ok(_handle) => println!("tray: ReClip icon registered (StatusNotifierItem)"),
        Err(err) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to register tray icon: {err}"),
            ));
        }
    }
    std::future::pending::<()>().await;
    Ok(())
}
