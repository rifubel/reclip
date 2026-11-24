use std::path::PathBuf;
use std::process::Command;

fn ffprobe_values(input: &PathBuf, entries: &str, select: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("ffprobe");
    cmd.args([
        "-v", "error",
        "-show_entries", entries,
        "-of", "default=noprint_wrappers=1:nokey=1",
    ]);
    if let Some(stream) = select {
        cmd.args(["-select_streams", stream]);
    }
    let output = cmd.arg(input).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn duration_seconds(input: &PathBuf) -> Option<f64> {
    ffprobe_values(input, "format=duration", None)?.parse().ok()
}

fn video_height(input: &PathBuf) -> Option<u32> {
    let values = ffprobe_values(input, "stream=width,height", Some("v:0"))?;
    values.lines().nth(1)?.trim().parse().ok()
}

fn output_path(input: &PathBuf, suffix: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "clip".to_string());
    let mut candidate = input.with_file_name(format!("{stem}_{suffix}.mp4"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = input.with_file_name(format!("{stem}_{suffix}{counter}.mp4"));
        counter += 1;
    }
    candidate
}

fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let status = Command::new("nice")
        .args(["-n", "10", "ffmpeg", "-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .map_err(|err| format!("failed to run ffmpeg: {err}"))?;
    if !status.success() {
        return Err("ffmpeg failed (see terminal output)".to_string());
    }
    Ok(())
}

pub fn export_full(input: &PathBuf, start: f64, end: f64) -> Result<String, String> {
    let output = output_path(input, "full");
    let start_s = start.to_string();
    let duration_s = (end - start).to_string();
    let input_s = input.to_string_lossy().to_string();
    let output_s = output.to_string_lossy().to_string();
    run_ffmpeg(&[
        "-ss", &start_s,
        "-i", &input_s,
        "-t", &duration_s,
        "-c", "copy",
        "-avoid_negative_ts", "make_zero",
        &output_s,
    ])?;
    Ok(output.file_name().unwrap_or_default().to_string_lossy().to_string())
}

pub fn validate_size(input: &PathBuf, duration: f64, target_mb: f64) -> Result<(), String> {
    if duration <= 0.0 {
        return Err("Invalid trim range".to_string());
    }
    let target_bits = target_mb * 1024.0 * 1024.0 * 8.0 * 0.95;
    let video_kbps = (target_bits / duration / 1000.0) - 128.0;
    let min_kbps = match video_height(input) {
        Some(h) if h >= 1080 => 2500.0,
        Some(h) if h >= 720 => 1500.0,
        _ => 800.0,
    };
    if video_kbps < min_kbps {
        return Err(format!(
            "Durasi terlalu panjang untuk {target_mb:.0} MB. Butuh minimal ~{min_kbps:.0} kbps video, tersedia ~{video_kbps:.0} kbps."
        ));
    }
    Ok(())
}

pub fn export_with_size(
    input: &PathBuf,
    start: f64,
    end: f64,
    target_mb: f64,
) -> Result<String, String> {
    validate_size(input, end - start, target_mb)?;

    let duration = end - start;
    let target_bits = target_mb * 1024.0 * 1024.0 * 8.0 * 0.95;
    let video_kbps = (target_bits / duration / 1000.0) - 128.0;

    let output = output_path(input, &format!("{}mb", target_mb as u64));
    let cache = {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(format!("{home}/.cache/reclip"))
    };
    let _ = std::fs::create_dir_all(&cache);
    let pass_log = cache.join(format!("pass_{}", std::process::id()));
    let pass_log_s = pass_log.to_string_lossy().to_string();
    let start_s = start.to_string();
    let duration_s = duration.to_string();
    let input_s = input.to_string_lossy().to_string();
    let output_s = output.to_string_lossy().to_string();
    let bitrate = format!("{video_kbps:.0}k");

    run_ffmpeg(&[
        "-ss", &start_s,
        "-i", &input_s,
        "-t", &duration_s,
        "-an",
        "-c:v", "libx264",
        "-preset", "veryfast",
        "-b:v", &bitrate,
        "-pass", "1",
        "-passlogfile", &pass_log_s,
        "-f", "mp4",
        "/dev/null",
    ])?;
    run_ffmpeg(&[
        "-ss", &start_s,
        "-i", &input_s,
        "-t", &duration_s,
        "-c:v", "libx264",
        "-preset", "veryfast",
        "-b:v", &bitrate,
        "-pass", "2",
        "-passlogfile", &pass_log_s,
        "-c:a", "aac",
        "-b:a", "128k",
        "-movflags", "+faststart",
        &output_s,
    ])?;
    let _ = std::fs::remove_file(format!("{pass_log_s}-0.log"));
    let _ = std::fs::remove_file(format!("{pass_log_s}-0.log.mbtree"));

    Ok(output.file_name().unwrap_or_default().to_string_lossy().to_string())
}
