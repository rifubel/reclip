use crate::config::Config;
use std::io;
use std::process::{Child, Command, Stdio};

pub trait Recorder {
    fn start(&mut self, config: &Config) -> io::Result<()>;
    fn stop(&mut self) -> io::Result<()>;
    fn save(&mut self, socket_path: &str) -> io::Result<String>;
}

pub struct GsrRecorder {
    child: Option<Child>,
}

impl GsrRecorder {
    pub fn new() -> Self {
        GsrRecorder { child: None }
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|child| child.id())
    }

    fn expand_tilde(path: &str) -> String {
        if path == "~" {
            std::env::var("HOME").unwrap_or_default()
        } else if let Some(rest) = path.strip_prefix("~/") {
            format!("{}/{}", std::env::var("HOME").unwrap_or_default(), rest)
        } else {
            path.to_string()
        }
    }

    pub fn save_replay(&self, socket_path: &str) -> io::Result<String> {
        let output = Command::new("gsr-cli")
            .args(["-ipc", socket_path, "save-replay"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "gsr-cli save-replay failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn spawn(&self, config: &Config) -> io::Result<Child> {
        let save_dir = Self::expand_tilde(&config.save_folder);
        let mut cmd = Command::new("gpu-screen-recorder");
        cmd.arg("-w").arg("screen");
        cmd.arg("-f").arg("60");
        cmd.arg("-c").arg("mp4");
        cmd.arg("-r").arg(config.buffer_seconds.to_string());
        cmd.arg("-bm").arg("cbr");
        cmd.arg("-q").arg(config.quality.bitrate_kbps().to_string());
        for source in config.audio_mode.source_args() {
            cmd.arg(source);
        }
        cmd.arg("-o").arg(format!("{}/", save_dir.trim_end_matches('/')));
        let socket_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        let socket_path = format!("{}/.reclip-gsr.sock", socket_dir);
        cmd.arg("-ipc").arg(&socket_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());
        eprintln!(
            "reclip: gsr args: gpu-screen-recorder {}",
            cmd
                .get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        );
        cmd.spawn()
    }
}

impl Default for GsrRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl GsrRecorder {
    pub fn wait(&mut self) -> io::Result<()> {
        match self.child.as_mut() {
            Some(child) => child.wait().map(|_| ()),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "gpu-screen-recorder is not running",
            )),
        }
    }
}

impl Recorder for GsrRecorder {
    fn start(&mut self, config: &Config) -> io::Result<()> {
        if self.child.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "gpu-screen-recorder is already running",
            ));
        }
        let child = self.spawn(config)?;
        eprintln!("reclip: started gpu-screen-recorder with pid {}", child.id());
        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        if let Some(mut child) = self.child.take() {
            let pid = child.id().to_string();
            let status = Command::new("kill")
                .args(["-INT", pid.as_str()])
                .status()?;
            if status.success() {
                let _ = child.wait();
            }
        }
        Ok(())
    }

    fn save(&mut self, socket_path: &str) -> io::Result<String> {
        self.save_replay(socket_path)
    }
}
