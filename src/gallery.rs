use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk4::cairo;
use gtk4::gio;
use gtk4::prelude::*;

fn save_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/Videos/ReClip", home))
}

fn thumbnail_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{home}/.cache/reclip"))
}

fn list_clips() -> Vec<PathBuf> {
    let mut clips: Vec<PathBuf> = match fs::read_dir(save_dir()) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().map(|ext| ext == "mp4").unwrap_or(false))
            .collect(),
        Err(err) => {
            eprintln!("reclip: failed to read {}: {err}", save_dir().display());
            Vec::new()
        }
    };
    clips.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    clips
}

fn request_thumbnail(
    path: PathBuf,
    time: f64,
    last: &Rc<RefCell<Instant>>,
    force: bool,
    tx: &std::sync::mpsc::Sender<String>,
) {
    let now = Instant::now();
    if !force && now.duration_since(*last.borrow()).as_millis() < 100 {
        return;
    }
    *last.borrow_mut() = now;
    let tx = tx.clone();
    std::thread::spawn(move || {
        let dir = thumbnail_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let output = dir.join(format!("scrub_{stamp}.png"));
        let status = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .arg("-ss").arg(time.to_string())
            .arg("-i").arg(&path)
            .args(["-frames:v", "1", "-vf", "scale=220:-1"])
            .arg(&output)
            .status();
        if matches!(status, Ok(status) if status.success()) {
            let _ = tx.send(output.to_string_lossy().to_string());
        } else {
            let _ = std::fs::remove_file(&output);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Grab { None, Start, End, Playhead }

struct TrimState {
    duration: f64,
    start: f64,
    end: f64,
    playhead: f64,
    grab: Grab,
}

const PAD: f64 = 14.0;

fn time_at(x: f64, width: f64, duration: f64) -> f64 {
    let usable = (width - 2.0 * PAD).max(1.0);
    ((x - PAD) / usable * duration).clamp(0.0, duration)
}

fn handle_x(time: f64, width: f64, duration: f64) -> f64 {
    let usable = (width - 2.0 * PAD).max(1.0);
    PAD + time / duration.max(0.001) * usable
}

fn draw_timeline(_area: &gtk4::DrawingArea, cr: &cairo::Context, width: i32, height: i32, state: &TrimState) {
    let w = width as f64;
    let duration = state.duration.max(0.001);
    let to_x = |time: f64| handle_x(time, w, duration);
    let bar_y = height as f64 / 2.0 - 2.0;

    // Track.
    cr.set_source_rgb(0.42, 0.45, 0.50);
    cr.rectangle(PAD, bar_y, w - 2.0 * PAD, 4.0);
    let _ = cr.fill();

    // Selected range (teal, like the reference).
    let start_x = to_x(state.start);
    let end_x = to_x(state.end);
    cr.set_source_rgb(0.10, 0.78, 0.72);
    cr.rectangle(start_x, bar_y, (end_x - start_x).max(2.0), 4.0);
    let _ = cr.fill();

    // Bracket handles at Start / End.
    for x in [start_x, end_x] {
        cr.set_source_rgb(0.55, 0.95, 0.90);
        cr.rectangle(x - 1.5, bar_y - 9.0, 3.0, 22.0);
        let _ = cr.fill();
        cr.rectangle(x - 1.5, bar_y - 9.0, 7.0, 3.0);
        let _ = cr.fill();
        cr.rectangle(x - 1.5, bar_y + 10.0, 7.0, 3.0);
        let _ = cr.fill();
    }

    // Playhead (triangle + line).
    let px = to_x(state.playhead);
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(px - 1.0, bar_y - 5.0, 2.0, 14.0);
    let _ = cr.fill();
    cr.move_to(px - 5.0, bar_y - 12.0);
    cr.line_to(px + 5.0, bar_y - 12.0);
    cr.line_to(px, bar_y - 5.0);
    cr.close_path();
    let _ = cr.fill();
}

fn show_alert(parent: &gtk4::Window, message: &str) {
    let dialog = gtk4::AlertDialog::builder().message(message).build();
    dialog.choose(Some(parent), None::<&gtk4::gio::Cancellable>, |_| {});
}

fn open_export_dialog(
    parent: &gtk4::ApplicationWindow,
    path: PathBuf,
    start: f64,
    end: f64,
    status: &gtk4::Label,
) {
    let dialog = gtk4::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Export video")
        .default_width(380)
        .build();
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    root.set_margin_start(18);
    root.set_margin_end(18);

    let heading = gtk4::Label::new(Some(&format!(
        "Export: {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    )));
    heading.set_xalign(0.0);
    root.append(&heading);

    let combo = gtk4::ComboBoxText::new();
    for option in ["Full size", "10 MB", "20 MB", "50 MB", "Custom"] {
        combo.append_text(option);
    }
    combo.set_active(Some(0));
    root.append(&combo);

    let custom_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let custom_entry = gtk4::Entry::new();
    custom_entry.set_text("25");
    custom_box.append(&gtk4::Label::new(Some("Custom MB")));
    custom_box.append(&custom_entry);
    custom_box.set_visible(false);
    root.append(&custom_box);

    let custom_box_clone = custom_box.clone();
    combo.connect_changed(move |combo| {
        custom_box_clone.set_visible(combo.active() == Some(4));
    });

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let export_button = gtk4::Button::with_label("Export");
    let cancel_button = gtk4::Button::with_label("Cancel");
    buttons.append(&export_button);
    buttons.append(&cancel_button);
    root.append(&buttons);

    let dialog_clone = dialog.clone();
    cancel_button.connect_clicked(move |_| dialog_clone.destroy());

    let dialog_clone = dialog.clone();
    let combo_clone = combo.clone();
    let custom_entry_clone = custom_entry.clone();
    let path_clone = path.clone();
    let status_clone = status.clone();
    export_button.connect_clicked(move |_| {
        let target_mb = match combo_clone.active() {
            Some(1) => Some(10.0),
            Some(2) => Some(20.0),
            Some(3) => Some(50.0),
            Some(4) => match custom_entry_clone.text().trim().parse::<f64>() {
                Ok(value) if value > 0.0 => Some(value),
                _ => {
                    show_alert(&dialog_clone, "Isi ukuran MB dengan angka lebih dari 0.");
                    return;
                }
            },
            _ => None,
        };
        if let Some(mb) = target_mb {
            if let Err(err) = crate::compress::validate_size(&path_clone, end - start, mb) {
                show_alert(&dialog_clone, &err);
                return;
            }
        }
        dialog_clone.destroy();

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let rx = Rc::new(RefCell::new(Some(rx)));
        let status_poll = status_clone.clone();
        gtk4::glib::timeout_add_local(Duration::from_millis(100), move || {
            let mut guard = rx.borrow_mut();
            if let Some(receiver) = guard.as_mut() {
                while let Ok(message) = receiver.try_recv() {
                    status_poll.set_label(&message);
                }
            }
            gtk4::glib::ControlFlow::Continue
        });

        let path_worker = path_clone.clone();
        std::thread::spawn(move || {
            let _ = tx.send("Exporting...".to_string());
            let result = match target_mb {
                Some(mb) => crate::compress::export_with_size(&path_worker, start, end, mb),
                None => crate::compress::export_full(&path_worker, start, end),
            };
            let message = match result {
                Ok(name) => format!("Saved: {name}"),
                Err(err) => format!("Error: {err}"),
            };
            let _ = tx.send(message);
        });
    });

    dialog.set_child(Some(&root));
    dialog.present();
}

fn build_ui(app: &gtk4::Application) {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(
        ".reclip-controls { background: rgba(18, 20, 24, 0.96); padding: 6px; border-radius: 4px; }",
    );
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let window = gtk4::ApplicationWindow::new(app);
    window.set_title(Some("ReClip Gallery"));
    window.set_default_size(1050, 760);

    let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    paned.set_position(320);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    for path in list_clips() {
        let row = gtk4::ListBoxRow::new();
        let label = gtk4::Label::new(Some(&path.file_name().unwrap_or_default().to_string_lossy()));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        row.set_child(Some(&label));
        row.set_widget_name(&path.to_string_lossy());
        list.append(&row);
    }
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_vexpand(true);
    paned.set_start_child(Some(&scroll));

    let video = gtk4::Video::new();
    video.set_autoplay(true);
    video.set_loop(false);
    video.set_vexpand(true);
    video.set_hexpand(true);

    // Single control bar overlaid on the player (covers GtkVideo's built-in controls).
    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    controls.add_css_class("reclip-controls");
    controls.set_valign(gtk4::Align::End);
    controls.set_hexpand(true);

    let play_button = gtk4::Button::with_label("Pause");
    play_button.set_width_request(62);
    let time_label = gtk4::Label::new(Some("0s / 0s"));
    time_label.set_width_request(100);

    let timeline = gtk4::DrawingArea::new();
    timeline.set_height_request(34);
    timeline.set_hexpand(true);

    let export_button = gtk4::Button::with_label("Export");
    controls.append(&play_button);
    controls.append(&time_label);
    controls.append(&timeline);
    controls.append(&export_button);

    // Scrub preview popup floats above the control bar.
    let preview_image = gtk4::Picture::new();
    preview_image.set_width_request(220);
    preview_image.set_height_request(124);
    let preview_time = gtk4::Label::new(Some("0.0 s"));
    let preview_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    preview_box.set_halign(gtk4::Align::Start);
    preview_box.set_valign(gtk4::Align::End);
    preview_box.set_margin_bottom(58);
    preview_box.append(&preview_image);
    preview_box.append(&preview_time);
    preview_box.set_visible(false);

    let player = gtk4::Overlay::new();
    player.set_child(Some(&video));
    player.add_overlay(&controls);
    player.add_overlay(&preview_box);
    player.set_vexpand(true);
    player.set_hexpand(true);

    let info = gtk4::Label::new(Some("Select a clip from the list"));
    info.set_xalign(0.0);
    let status = gtk4::Label::new(Some("Drag the teal brackets to trim, drag elsewhere to seek, then Export"));
    status.set_xalign(0.0);
    status.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    let right = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    right.append(&info);
    right.append(&player);
    right.append(&status);
    paned.set_end_child(Some(&right));

    let selected: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let last_thumb = Rc::new(RefCell::new(Instant::now() - Duration::from_secs(1)));
    let (thumb_tx, thumb_rx) = std::sync::mpsc::channel::<String>();
    let thumb_rx = Rc::new(RefCell::new(Some(thumb_rx)));
    let previous_thumb: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    {
        let preview_image_poll = preview_image.clone();
        let previous_thumb_poll = previous_thumb.clone();
        let thumb_rx_poll = thumb_rx.clone();
        gtk4::glib::timeout_add_local(Duration::from_millis(50), move || {
            let mut receiver = thumb_rx_poll.borrow_mut();
            if let Some(rx) = receiver.as_mut() {
                while let Ok(filename) = rx.try_recv() {
                    preview_image_poll.set_filename(Some(&filename));
                    if let Some(old) = previous_thumb_poll.borrow_mut().replace(filename) {
                        let _ = std::fs::remove_file(old);
                    }
                }
            }
            gtk4::glib::ControlFlow::Continue
        });
    }

    let state = Rc::new(RefCell::new(TrimState {
        duration: 1.0,
        start: 0.0,
        end: 1.0,
        playhead: 0.0,
        grab: Grab::None,
    }));

    {
        let state_draw = state.clone();
        timeline.set_draw_func(move |area, cr, width, height| {
            let state = state_draw.borrow();
            draw_timeline(area, cr, width, height, &state);
        });
    }

    // MediaStream: play/pause + seek + timestamp.
    let media_stream: Rc<RefCell<Option<gtk4::MediaStream>>> = Rc::new(RefCell::new(None));
    {
        let stream_notify = media_stream.clone();
        video.connect_media_stream_notify(move |video| {
            *stream_notify.borrow_mut() = video.media_stream();
        });
    }
    {
        let stream_button = media_stream.clone();
        let play_label = play_button.clone();
        play_button.connect_clicked(move |_| {
            if let Some(stream) = stream_button.borrow().as_ref() {
                if stream.is_playing() {
                    stream.pause();
                    play_label.set_label("Play");
                } else {
                    stream.play();
                    play_label.set_label("Pause");
                }
            }
        });
    }
    {
        let stream_poll = media_stream.clone();
        let state_poll = state.clone();
        let time_label_poll = time_label.clone();
        let timeline_poll = timeline.clone();
        gtk4::glib::timeout_add_local(Duration::from_millis(200), move || {
            let duration = state_poll.borrow().duration;
            if let Some(stream) = stream_poll.borrow().as_ref() {
                if !stream.is_ended() {
                    let seconds = (stream.timestamp() as f64 / 1_000_000.0).min(duration);
                    state_poll.borrow_mut().playhead = seconds;
                    time_label_poll.set_label(&format!("{seconds:.0}s / {duration:.0}s"));
                    timeline_poll.queue_draw();
                }
            }
            gtk4::glib::ControlFlow::Continue
        });
    }

    fn show_preview(
        preview_box: &gtk4::Box,
        preview_time: &gtk4::Label,
        path: &PathBuf,
        x: f64,
        time: f64,
        last: &Rc<RefCell<Instant>>,
        tx: &std::sync::mpsc::Sender<String>,
    ) {
        let margin = (x - 110.0).max(0.0) as i32;
        preview_box.set_margin_start(margin);
        preview_box.set_visible(true);
        preview_time.set_label(&format!("{time:.1} s"));
        request_thumbnail(path.clone(), time, last, false, tx);
    }

    // Selecting a clip resets trim + shows filename.
    {
        let video_clone = video.clone();
        let info_clone = info.clone();
        let selected_clone = selected.clone();
        let state_clone = state.clone();
        let time_label_clone = time_label.clone();
        let preview_box_clone = preview_box.clone();
        let timeline_clone = timeline.clone();
        list.connect_row_selected(move |_list, row| {
            let Some(row) = row else { return };
            let path = PathBuf::from(row.widget_name().to_string());
            if path.as_os_str().is_empty() { return; }
            video_clone.set_file(Some(&gio::File::for_path(&path)));
            info_clone.set_label(&path.to_string_lossy());
            if let Ok(mut guard) = selected_clone.lock() { *guard = Some(path.clone()); }
            let duration = crate::compress::duration_seconds(&path).unwrap_or(60.0);
            let mut s = state_clone.borrow_mut();
            s.duration = duration;
            s.start = 0.0;
            s.end = duration;
            s.playhead = 0.0;
            drop(s);
            time_label_clone.set_label(&format!("0s / {duration:.0}s"));
            preview_box_clone.set_visible(false);
            timeline_clone.queue_draw();
        });
    }

    // Auto-refresh: new clips saved by Alt+S appear without restarting the gallery.
    {
        let list_refresh = list.clone();
        let known: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(
            list_clips()
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        ));
        let selected_refresh = selected.clone();
        gtk4::glib::timeout_add_local(Duration::from_secs(2), move || {
            let clips = list_clips();
            let names: Vec<String> = clips
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let changed = { *known.borrow() != names };
            if changed {
                *known.borrow_mut() = names.clone();
                let keep = selected_refresh
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .map(|path| path.to_string_lossy().to_string());
                while let Some(row) = list_refresh.row_at_index(0) {
                    list_refresh.remove(&row);
                }
                for path in &clips {
                    let row = gtk4::ListBoxRow::new();
                    let label = gtk4::Label::new(Some(
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                    ));
                    label.set_xalign(0.0);
                    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                    row.set_child(Some(&label));
                    row.set_widget_name(&path.to_string_lossy());
                    list_refresh.append(&row);
                }
                let target = keep
                    .filter(|path| names.contains(path))
                    .or_else(|| names.first().cloned());
                if let Some(target) = target {
                    let count = list_refresh
                        .observe_children()
                        .n_items();
                    for index in 0..count {
                        if let Some(row) = list_refresh
                            .row_at_index(index as i32)
                            .and_then(|row| row.downcast::<gtk4::ListBoxRow>().ok())
                        {
                            if row.widget_name().to_string() == target {
                                list_refresh.select_row(Some(&row));
                                break;
                            }
                        }
                    }
                }
            }
            gtk4::glib::ControlFlow::Continue
        });
    }

    // Drag: brackets set Start/End, elsewhere seeks.
    {
        let state_drag = state.clone();
        let timeline_drag = timeline.clone();
        let selected_drag = selected.clone();
        let preview_box_drag = preview_box.clone();
        let preview_time_drag = preview_time.clone();
        let last_thumb_drag = last_thumb.clone();
        let thumb_tx_drag = thumb_tx.clone();
        let stream_drag = media_stream.clone();

        let timeline_begin = timeline.clone();
        let state_begin = state.clone();
        let drag = gtk4::GestureDrag::new();
        drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
        timeline_drag.add_controller(drag.clone());

        drag.connect_drag_begin(move |_gesture, x, _y| {
            let width = timeline_begin.width() as f64;
            let s = state_begin.borrow();
            let start_x = handle_x(s.start, width, s.duration);
            let end_x = handle_x(s.end, width, s.duration);
            let grab = if (x - start_x).abs() < 14.0 {
                Grab::Start
            } else if (x - end_x).abs() < 14.0 {
                Grab::End
            } else {
                Grab::Playhead
            };
            drop(s);
            state_begin.borrow_mut().grab = grab;
        });

        drag.connect_drag_update(move |gesture, dx, _dy| {
            let (begin_x, _) = gesture.start_point().unwrap_or((0.0, 0.0));
            let width = timeline_drag.width() as f64;
            let duration = state_drag.borrow().duration;
            let time = time_at(begin_x + dx, width, duration);
            let mut seek_to = None;
            {
                let mut s = state_drag.borrow_mut();
                match s.grab {
                    Grab::Start => s.start = time.min(s.end - 0.1).max(0.0),
                    Grab::End => s.end = time.max(s.start + 0.1).min(s.duration),
                    Grab::Playhead | Grab::None => {
                        s.playhead = time;
                        seek_to = Some(time);
                    }
                }
            }
            if let Some(time) = seek_to {
                if let Some(stream) = stream_drag.borrow().as_ref() {
                    stream.seek((time * 1_000_000.0) as i64);
                }
            }
            let s = state_drag.borrow();
            let preview_at = match s.grab {
                Grab::Start => s.start,
                Grab::End => s.end,
                _ => s.playhead,
            };
            let x = handle_x(preview_at, width, s.duration);
            drop(s);
            timeline_drag.queue_draw();
            if let Some(path) = selected_drag.lock().ok().and_then(|guard| guard.clone()) {
                show_preview(&preview_box_drag, &preview_time_drag, &path, x, preview_at, &last_thumb_drag, &thumb_tx_drag);
            }
        });
    }

    // Hover: scrub preview popup.
    {
        let timeline_hover = timeline.clone();
        let state_hover = state.clone();
        let selected_hover = selected.clone();
        let preview_box_hover = preview_box.clone();
        let preview_time_hover = preview_time.clone();
        let last_thumb_hover = last_thumb.clone();
        let thumb_tx_hover = thumb_tx.clone();
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_motion(move |_controller, x, _y| {
            let width = timeline_hover.width() as f64;
            let duration = state_hover.borrow().duration;
            let time = time_at(x, width, duration);
            let over_handle = {
                let s = state_hover.borrow();
                (x - handle_x(s.start, width, duration)).abs() < 14.0
                    || (x - handle_x(s.end, width, duration)).abs() < 14.0
            };
            if over_handle {
                timeline_hover.set_cursor_from_name(Some("ew-resize"));
            } else {
                timeline_hover.set_cursor_from_name(Some("pointer"));
            }
            if let Some(path) = selected_hover.lock().ok().and_then(|guard| guard.clone()) {
                show_preview(&preview_box_hover, &preview_time_hover, &path, x, time, &last_thumb_hover, &thumb_tx_hover);
            }
        });
        let timeline_leave = timeline.clone();
        let preview_box_leave = preview_box.clone();
        motion.connect_leave(move |_controller| {
            timeline_leave.set_cursor(None);
            preview_box_leave.set_visible(false);
        });
        timeline.add_controller(motion);
    }

    let selected_clone = selected.clone();
    let state_clone = state.clone();
    let status_clone = status.clone();
    let window_clone = window.clone();
    export_button.connect_clicked(move |_button| {
        let path = match selected_clone.lock().ok().and_then(|guard| guard.clone()) {
            Some(path) => path,
            None => { status_clone.set_label("Select a clip first"); return; }
        };
        let s = state_clone.borrow();
        open_export_dialog(&window_clone, path, s.start, s.end, &status_clone);
    });

    window.set_child(Some(&paned));
    window.present();
}

pub fn run() {
    let app = gtk4::Application::builder()
        .application_id("com.reclip.ReClip.Gallery")
        .build();
    app.connect_activate(build_ui);
    app.run_with_args::<String>(&[]);
}
