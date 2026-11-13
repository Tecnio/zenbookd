use std::{
    io::{self, ErrorKind},
    process::{Command, ExitStatus},
};

use thiserror::Error;

const PANEL_WIDTH: u32 = 2880;
const PANEL_HEIGHT: u32 = 1800;
const NOISE_BLOCK: u32 = 4;
const FRAME_RATE: u32 = 30;
const CYCLE_COLORS: u32 = 5;
const WINDOW_TITLE: &str = "zenbookd pixel refresh";

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum Pattern {
    #[value(help = "Fullscreen random noise, fast")]
    Noise,

    #[value(help = "Black, red, green, blue and white screens in turn, slow")]
    Cycle,
}

impl Pattern {
    fn filtergraph(self, hold_seconds: u32) -> String {
        let upscale = format!("scale={PANEL_WIDTH}:{PANEL_HEIGHT}:flags=neighbor");

        match self {
            Pattern::Noise => format!(
                "nullsrc=s={}x{}:r={FRAME_RATE},geq=lum='random(1)*255':cb=128:cr=128,{upscale}",
                PANEL_WIDTH / NOISE_BLOCK,
                PANEL_HEIGHT / NOISE_BLOCK,
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
    #[error("ffplay was not found on PATH")]
    FfplayMissing,

    #[error("could not start ffplay: {0}")]
    Spawn(io::Error),

    #[error("ffplay exited with {0}")]
    Exited(ExitStatus),
}

pub fn ffplay_args(pattern: Pattern, hold_seconds: u32, minutes: Option<u32>) -> Vec<String> {
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
    args.extend(["-i".to_string(), pattern.filtergraph(hold_seconds)]);

    args
}

pub fn run(pattern: Pattern, hold_seconds: u32, minutes: Option<u32>) -> Result<(), RefreshError> {
    let status = Command::new("ffplay")
        .args(ffplay_args(pattern, hold_seconds, minutes))
        .status()
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => RefreshError::FfplayMissing,
            _ => RefreshError::Spawn(err),
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(RefreshError::Exited(status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_matches_the_original_graph() {
        let args = ffplay_args(Pattern::Noise, 5, None);

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
        let graph = Pattern::Cycle.filtergraph(7);

        assert!(graph.contains("r='st(0,mod(floor(T/7),5));255*(eq(ld(0),1)+eq(ld(0),4))'"));
        assert!(graph.contains("g='st(0,mod(floor(T/7),5));255*(eq(ld(0),2)+eq(ld(0),4))'"));
        assert!(graph.contains("b='st(0,mod(floor(T/7),5));255*(eq(ld(0),3)+eq(ld(0),4))'"));
        assert!(graph.ends_with("scale=2880:1800:flags=neighbor"));
    }

    #[test]
    fn minutes_bound_the_run() {
        let args = ffplay_args(Pattern::Cycle, 5, Some(10));

        let limit = args.iter().position(|arg| arg == "-t").unwrap();

        assert_eq!(args[limit + 1], "600");
        assert!(args.contains(&"-autoexit".to_string()));
    }
}
