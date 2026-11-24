mod compress;
mod config;
mod game_detect;
mod gallery;
mod hotkey;
mod recorder;
mod tray;

use recorder::{GsrRecorder, Recorder};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).cloned().unwrap_or_else(|| "start".to_string());

    match command.as_str() {
        "start" => run_start(),
        "stop" => run_stop(),
        "save" => save_recording(),
        "detect" => run_detect(),
        "autostart" => run_autostart(),
        "gallery" => {
            gallery::run();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("reclip: unknown command `{other}`, expected `start` or `stop`");
            ExitCode::from(2)
        }
    }
}

fn config_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".config")
        .join("reclip")
        .join("config.json")
}

fn pid_file_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("reclip")
        .join("reclip.pid")
}

fn run_start() -> ExitCode {
    let config = match config::load(&config_path()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "reclip: failed to load config {}: {err}",
                config_path().display()
            );
            return ExitCode::FAILURE;
        }
    };

    // Background tasks: global hotkey (Alt+S) and tray icon are always available.
    let on_activate: hotkey::ActivateCallback = Arc::new(Mutex::new(Some(
        Box::new(|| {
            let _ = save_recording();
        }) as Box<dyn Fn() + Send + Sync>,
    )));
    std::thread::spawn(|| {
        async_io::block_on(async {
            if let Err(err) = hotkey::run(on_activate).await {
                eprintln!("reclip: hotkey task failed: {err}");
            }
        });
    });
    std::thread::spawn(|| {
        async_io::block_on(async {
            if let Err(err) = tray::run().await {
                eprintln!("reclip: tray task failed: {err}");
            }
        });
    });

    println!("reclip: waiting for a game (Steam library or manual_games)...");
    let mut recorder: Option<GsrRecorder> = None;
    let mut active_game: Option<String> = None;
    loop {
        let game = game_detect::detect(&config);
        if let Some(game) = game {
            if recorder.is_none() {
                let mut candidate = GsrRecorder::new();
                match candidate.start(&config) {
                    Ok(()) => {
                        if let Some(pid) = candidate.pid() {
                            if let Some(dir) = pid_file_path().parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            let _ = std::fs::write(&pid_file_path(), pid.to_string());
                        }
                        println!("reclip: game detected: {} (pid {}) - recording", game.name, game.pid);
                        send_notification(&format!("ReClip aktif: {}", game.name), false);
                        recorder = Some(candidate);
                        active_game = Some(game.name);
                    }
                    Err(err) => eprintln!("reclip: failed to start recorder: {err}"),
                }
            }
        } else if let Some(mut running) = recorder.take() {
            if let Some(pid) = running.pid() {
                let _ = std::process::Command::new("kill")
                    .arg("-INT")
                    .arg(pid.to_string())
                    .status();
                let _ = running.wait();
            }
            let _ = std::fs::remove_file(&pid_file_path());
            println!("reclip: game closed - recording stopped");
            if let Some(name) = active_game.take() {
                send_notification(&format!("ReClip berhenti: {name}"), false);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

fn run_detect() -> ExitCode {
    let config = match config::load(&config_path()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("reclip: failed to load config: {err}");
            return ExitCode::FAILURE;
        }
    };
    match game_detect::detect(&config) {
        Some(game) => println!("game detected: {} (pid {})", game.name, game.pid),
        None => println!("no game detected"),
    }
    ExitCode::SUCCESS
}

fn run_autostart() -> ExitCode {
    let config_dir = {
        let home = env::var("HOME").unwrap_or_default();
        std::path::PathBuf::from(home)
            .join(".config")
            .join("systemd")
            .join("user")
    };
    if let Err(err) = std::fs::create_dir_all(&config_dir) {
        eprintln!("reclip: failed to create {}: {err}", config_dir.display());
        return ExitCode::FAILURE;
    }
    let unit = format!(
        "[Unit]\nDescription=ReClip daemon\nAfter=graphical-session.target\n\n[Service]\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/home/rifubel/Projects/ReClip/target/debug/reclip".to_string())
    );
    let unit_path = config_dir.join("reclip.service");
    if let Err(err) = std::fs::write(&unit_path, unit) {
        eprintln!("reclip: failed to write {}: {err}", unit_path.display());
        return ExitCode::FAILURE;
    }
    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "reclip"])
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => {
            eprintln!("reclip: systemctl --user enable reclip failed (may need systemd running)");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn save_recording() -> ExitCode {
    let socket_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let socket_path = format!("{}/.reclip-gsr.sock", socket_dir);
    let mut recorder = GsrRecorder::new();
    // GSR writes replays asynchronously. If it is still busy, retry shortly
    // instead of dropping the save (so every Alt+S press produces a clip).
    let mut attempt = 0;
    let saved = loop {
        match recorder.save(&socket_path) {
            Ok(path) => break path,
            Err(err) => {
                let message = err.to_string();
                if message.contains("already being saved") && attempt < 20 {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
                eprintln!("reclip: save failed: {err}");
                return ExitCode::FAILURE;
            }
        }
    };

    let config = match config::load(&config_path()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "reclip: failed to load config {}: {err}",
                config_path().display()
            );
            return ExitCode::FAILURE;
        }
    };
    let save_dir_full = if config.save_folder.starts_with("~/") {
        format!("{}/{}", env::var("HOME").unwrap_or_default(), &config.save_folder[2..])
    } else {
        config.save_folder.clone()
    };
    let save_dir = save_dir_full.trim_end_matches('/').to_string();
    let saved = saved.trim().to_string();
    let old_path = if saved.contains('/') {
        saved
    } else {
        format!("{}/{}", save_dir, saved)
    };
    let filename: &str = old_path.rsplit('/').next().unwrap_or(old_path.as_str());
    let new_filename = filename.replace("Replay_", "ReClip_");
    let new_path = format!("{}/{}", save_dir, new_filename);
    if let Err(err) = std::fs::rename(&old_path, &new_path) {
        eprintln!("reclip: failed to rename {old_path} to {new_path}: {err}");
        return ExitCode::FAILURE;
    }
    send_notification("Clip captured!", true);
    ExitCode::SUCCESS
}

fn send_notification(body: &str, beep: bool) {
    let body = body.to_string();
    let id = if beep { "reclip-save" } else { "reclip-game" };
    std::thread::spawn(move || {
        async_io::block_on(async {
            use ashpd::desktop::notification::{Notification, NotificationProxy};
            let proxy = match NotificationProxy::new().await {
                Ok(proxy) => proxy,
                Err(err) => {
                    eprintln!("reclip: failed to connect to notification portal: {err}");
                    return;
                }
            };
            let notification = Notification::new("ReClip").body(Some(body.as_str()));
            if let Err(err) = proxy.add_notification(id, notification).await {
                eprintln!("reclip: failed to send notification: {err}");
            }
            if beep {
                play_beep();
            }
        });
    });
}

fn beep_wav() -> Vec<u8> {
    const RATE: u32 = 44100;
    const DURATION: f32 = 0.18;
    let samples = (RATE as f32 * DURATION) as usize;
    let mut pcm = Vec::with_capacity(samples * 2);
    for i in 0..samples {
        let t = i as f32 / RATE as f32;
        let freq = if t < 0.07 { 880.0 } else { 1174.66 };
        let attack = (t / 0.008).min(1.0);
        let release = ((DURATION - t) / 0.05).min(1.0);
        let envelope = attack * release;
        let value = (t * freq * 2.0 * std::f32::consts::PI).sin() * envelope * 0.18;
        pcm.extend_from_slice(&((value * 32767.0) as i16).to_le_bytes());
    }

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

fn play_beep() {
    use std::io::Write;
    let wav = beep_wav();
    let mut child = match std::process::Command::new("pw-play")
        .args(["--volume", "0.35", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("reclip: failed to play beep: {err}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(err) = stdin.write_all(&wav) {
            eprintln!("reclip: failed to write beep audio: {err}");
            return;
        }
    }
    match child.wait_with_output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => eprintln!(
            "reclip: beep playback failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(err) => eprintln!("reclip: beep playback failed: {err}"),
    }
}

pub(crate) fn run_stop() -> ExitCode {
    let path = pid_file_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(_) => {
            eprintln!("reclip: no running instance found (missing {})", path.display());
            return ExitCode::FAILURE;
        }
    };
    let Ok(pid) = contents.trim().parse::<u32>() else {
        eprintln!("reclip: invalid pid file {}", path.display());
        return ExitCode::FAILURE;
    };
    let status = std::process::Command::new("kill")
        .arg("-INT")
        .arg(pid.to_string())
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => {
            eprintln!("reclip: failed to stop gpu-screen-recorder (pid {pid})");
            ExitCode::FAILURE
        }
    }
}
