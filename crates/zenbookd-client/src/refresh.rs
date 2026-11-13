use std::{
    fmt, fs,
    io::{self, ErrorKind},
    path::Path,
    process::{Child, Command, ExitStatus},
    str::FromStr,
};

use thiserror::Error;

const DRM_CLASS: &str = "/sys/class/drm";
const INTERNAL_PANEL: &str = "eDP";
const NOISE_BLOCK: u32 = 4;
const FRAME_RATE: u32 = 30;
const CYCLE_COLORS: u32 = 5;
const WINDOW_TITLE: &str = "zenbookd pixel refresh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

#[derive(Debug, Error, PartialEq)]
#[error("expected WIDTHxHEIGHT, such as 2880x1800")]
pub struct InvalidResolution;

impl FromStr for Resolution {
    type Err = InvalidResolution;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (width, height) = text.split_once('x').ok_or(InvalidResolution)?;

        let digits = |part: &str| {
            let end = part
                .find(|character: char| !character.is_ascii_digit())
                .unwrap_or(part.len());

            part[..end]
                .parse::<u32>()
                .ok()
                .filter(|value| *value >= NOISE_BLOCK)
        };

        Ok(Resolution {
            width: digits(width).ok_or(InvalidResolution)?,
            height: digits(height).ok_or(InvalidResolution)?,
        })
    }
}

#[derive(Debug)]
struct Connector {
    name: String,
    preferred: Resolution,
}

fn connector(path: &Path) -> Option<Connector> {
    let status = fs::read_to_string(path.join("status")).ok()?;

    if status.trim() != "connected" {
        return None;
    }

    let modes = fs::read_to_string(path.join("modes")).ok()?;
    let preferred = modes.lines().next()?.trim().parse().ok()?;

    let name = path.file_name()?.to_string_lossy().into_owned();

    Some(Connector { name, preferred })
}

fn pick(mut connectors: Vec<Connector>) -> Option<Resolution> {
    connectors.sort_by(|a, b| a.name.cmp(&b.name));

    let internal = connectors
        .iter()
        .find(|connector| connector.name.contains(INTERNAL_PANEL));

    internal
        .or(connectors.first())
        .map(|connector| connector.preferred)
}

pub fn detect_resolution() -> Result<Resolution, RefreshError> {
    let entries = fs::read_dir(DRM_CLASS).map_err(RefreshError::DrmUnreadable)?;

    let connectors = entries
        .filter_map(Result::ok)
        .filter_map(|entry| connector(&entry.path()))
        .collect();

    pick(connectors).ok_or(RefreshError::DisplayNotFound)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Noise,
    Cycle,
}

impl Pattern {
    fn filtergraph(self, size: Resolution, hold_seconds: u32) -> String {
        let upscale = format!("scale={}:{}:flags=neighbor", size.width, size.height);

        match self {
            Pattern::Noise => format!(
                "nullsrc=s={}x{}:r={FRAME_RATE},geq=lum='random(1)*255':cb=128:cr=128,{upscale}",
                size.width / NOISE_BLOCK,
                size.height / NOISE_BLOCK,
            ),

            Pattern::Cycle => {
                let channel = |own: u32| {
                    format!(
                        "'st(0,mod(floor(T/{hold_seconds}),{CYCLE_COLORS}));\
                         255*(eq(ld(0),{own})+eq(ld(0),{white}))'",
                        white = CYCLE_COLORS - 1,
                    )
                };

                format!(
                    "nullsrc=s=16x10:r={FRAME_RATE},format=rgb24,geq=r={}:g={}:b={},{upscale}",
                    channel(1),
                    channel(2),
                    channel(3),
                )
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("could not read {DRM_CLASS}: {0}")]
    DrmUnreadable(io::Error),

    #[error("no connected display reports a preferred mode")]
    DisplayNotFound,

    #[error("ffplay was not found on PATH")]
    FfplayMissing,

    #[error("could not start ffplay: {0}")]
    Spawn(io::Error),

    #[error("ffplay exited with {0}")]
    Exited(ExitStatus),
}

pub fn ffplay_args(
    pattern: Pattern,
    size: Resolution,
    hold_seconds: u32,
    minutes: Option<u32>,
) -> Vec<String> {
    let mut args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-fs",
        "-an",
        "-window_title",
        WINDOW_TITLE,
    ]
    .map(String::from)
    .to_vec();

    if let Some(minutes) = minutes {
        args.extend(["-t".to_string(), (minutes * 60).to_string()]);
        args.push("-autoexit".to_string());
    }

    args.extend(["-f".to_string(), "lavfi".to_string()]);
    args.extend(["-i".to_string(), pattern.filtergraph(size, hold_seconds)]);

    args
}

fn launch_failure(err: io::Error) -> RefreshError {
    match err.kind() {
        ErrorKind::NotFound => RefreshError::FfplayMissing,
        _ => RefreshError::Spawn(err),
    }
}

pub fn spawn(
    pattern: Pattern,
    size: Resolution,
    hold_seconds: u32,
    minutes: Option<u32>,
) -> Result<Child, RefreshError> {
    Command::new("ffplay")
        .args(ffplay_args(pattern, size, hold_seconds, minutes))
        .spawn()
        .map_err(launch_failure)
}

pub fn finished(status: ExitStatus) -> Result<(), RefreshError> {
    if status.success() {
        Ok(())
    } else {
        Err(RefreshError::Exited(status))
    }
}

pub fn run(
    pattern: Pattern,
    size: Resolution,
    hold_seconds: u32,
    minutes: Option<u32>,
) -> Result<(), RefreshError> {
    let status = Command::new("ffplay")
        .args(ffplay_args(pattern, size, hold_seconds, minutes))
        .status()
        .map_err(launch_failure)?;

    finished(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Resolution = Resolution {
        width: 2880,
        height: 1800,
    };

    fn plugged(name: &str, preferred: &str) -> Connector {
        Connector {
            name: name.to_string(),
            preferred: preferred.parse().unwrap(),
        }
    }

    #[test]
    fn parses_sizes_and_drm_modes() {
        assert_eq!("2880x1800".parse(), Ok(PANEL));
        assert_eq!("1920x1080i".parse::<Resolution>().unwrap().height, 1080);
        assert_eq!("2880".parse::<Resolution>(), Err(InvalidResolution));
        assert_eq!("0x1800".parse::<Resolution>(), Err(InvalidResolution));
    }

    #[test]
    fn prefers_the_internal_panel() {
        let connectors = vec![
            plugged("card1-DP-1", "3840x2160"),
            plugged("card1-eDP-1", "2880x1800"),
        ];

        assert_eq!(pick(connectors), Some(PANEL));
        assert_eq!(
            pick(vec![plugged("card1-HDMI-A-1", "1920x1080")]).map(|size| size.width),
            Some(1920)
        );
        assert_eq!(pick(Vec::new()), None);
    }

    #[test]
    fn noise_scales_to_the_given_size() {
        let graph = Pattern::Noise.filtergraph(
            Resolution {
                width: 1920,
                height: 1200,
            },
            5,
        );

        assert!(graph.starts_with("nullsrc=s=480x300:r=30,"));
        assert!(graph.ends_with("scale=1920:1200:flags=neighbor"));
    }

    #[test]
    fn noise_matches_the_original_graph() {
        let args = ffplay_args(Pattern::Noise, PANEL, 5, None);

        assert_eq!(
            args.last().unwrap(),
            "nullsrc=s=720x450:r=30,geq=lum='random(1)*255':cb=128:cr=128,\
             scale=2880:1800:flags=neighbor"
        );
        assert!(!args.contains(&"-autoexit".to_string()));
        assert!(args.contains(&"-fs".to_string()));
    }

    #[test]
    fn cycle_holds_each_color_and_mixes_white() {
        let graph = Pattern::Cycle.filtergraph(PANEL, 7);

        assert!(graph.contains("r='st(0,mod(floor(T/7),5));255*(eq(ld(0),1)+eq(ld(0),4))'"));
        assert!(graph.contains("g='st(0,mod(floor(T/7),5));255*(eq(ld(0),2)+eq(ld(0),4))'"));
        assert!(graph.contains("b='st(0,mod(floor(T/7),5));255*(eq(ld(0),3)+eq(ld(0),4))'"));
        assert!(graph.ends_with("scale=2880:1800:flags=neighbor"));
    }

    #[test]
    fn minutes_bound_the_run() {
        let args = ffplay_args(Pattern::Cycle, PANEL, 5, Some(10));

        let limit = args.iter().position(|arg| arg == "-t").unwrap();

        assert_eq!(args[limit + 1], "600");
        assert!(args.contains(&"-autoexit".to_string()));
    }
}
