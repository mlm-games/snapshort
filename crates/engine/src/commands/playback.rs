//! Playback commands: transport, playhead, frame rate, monitor quality.

use serde_json::Value;
use snapshort_usecases::PlaybackCommand;

use crate::commands::cmd;
use crate::params::{self, require_f64};
use crate::{EngineError, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "playback.play",
            "Play",
            ["Playback", "Play"],
            Some("Space"),
            "{}",
            has_project,
            run_play
        ),
        cmd!(
            "playback.pause",
            "Pause",
            ["Playback", "Pause"],
            Some("Space"),
            "{}",
            has_project,
            run_pause
        ),
        cmd!(
            "playback.stop",
            "Stop",
            ["Playback", "Stop"],
            None::<&'static str>,
            "{}",
            has_project,
            run_stop
        ),
        cmd!(
            "playback.seek",
            "Move Playhead",
            [],
            None::<&'static str>,
            "{\"seconds\"?: number, \"us\"?: int, \"ms\"?: number, \"frame\"?: int}",
            has_project,
            run_seek
        ),
        cmd!(
            "playback.step",
            "Step Frames",
            [],
            Some("Right"),
            "{\"frames\": int}",
            has_project,
            run_step
        ),
        cmd!(
            "playback.setFps",
            "Set Frame Rate",
            [],
            None::<&'static str>,
            "{\"fps\": number}",
            has_project,
            run_set_fps
        ),
        cmd!(
            "preview.requestFrame",
            "Render Preview Frame",
            [],
            None::<&'static str>,
            "{\"seconds\"?: number, \"us\"?: int, \"ms\"?: number}",
            has_project,
            run_request_frame
        ),
        cmd!(
            "preview.setPreferProxy",
            "Monitor Quality",
            [],
            None::<&'static str>,
            "{\"prefer\": bool}",
            has_project,
            run_set_prefer_proxy
        ),
        query!("playback.inspect", "Playback", "{}", run_inspect),
    ]
}

fn run_play<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Playback(PlaybackCommand::Play))
            .await?;
        Ok(crate::inspect::playback_json(s).await)
    })
}

fn run_pause<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Playback(PlaybackCommand::Pause))
            .await?;
        Ok(crate::inspect::playback_json(s).await)
    })
}

fn run_stop<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Playback(PlaybackCommand::Stop))
            .await?;
        Ok(crate::inspect::playback_json(s).await)
    })
}

fn run_seek<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let fps = s.fps();
        let timestamp = params::require_time(p, fps, "playback.seek")?;
        s.dispatch(crate::SessionCommand::Playback(PlaybackCommand::Seek {
            timestamp,
        }))
        .await?;
        Ok(crate::inspect::playback_json(s).await)
    })
}

fn run_step<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let frames = params::require_f64(p, "frames", "playback.step")?;
        let fps = s.fps();
        let current = s.services().project.current_playhead().await;
        let mut stepper = snapshort_usecases::FrameStepper::new(fps);
        let mut total = 0i64;
        let steps = frames.round() as i64;
        for _ in 0..steps.abs() {
            let step = stepper.next_step_us();
            total += if steps < 0 { -step } else { step };
        }
        let next = miniter_domain::Timestamp::from_micros((current.as_micros() + total).max(0));
        s.dispatch(crate::SessionCommand::Playback(PlaybackCommand::Seek {
            timestamp: next,
        }))
        .await?;
        Ok(crate::inspect::playback_json(s).await)
    })
}

fn run_set_fps<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let fps = require_f64(p, "fps", "playback.setFps")?;
        if fps <= 0.0 {
            return Err(EngineError::bad("playback.setFps", "fps must be positive"));
        }
        s.dispatch(crate::SessionCommand::Playback(PlaybackCommand::SetFps {
            fps: fps.round() as i64,
        }))
        .await?;
        Ok(crate::inspect::settings_json(s).await)
    })
}

fn run_request_frame<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let fps = s.fps();
        let playhead = s.services().project.current_playhead().await;
        let timestamp = params::time_p(p, fps).unwrap_or(playhead);
        s.dispatch(crate::SessionCommand::Preview(
            snapshort_usecases::PreviewCommand::RequestFrame { timestamp },
        ))
        .await?;
        Ok(crate::inspect::playback_json(s).await)
    })
}

fn run_set_prefer_proxy<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let prefer = params::require_bool(p, "prefer", "preview.setPreferProxy")?;
        s.dispatch(crate::SessionCommand::Preview(
            snapshort_usecases::PreviewCommand::SetPreferProxy { prefer },
        ))
        .await?;
        Ok(crate::inspect::settings_json(s).await)
    })
}

fn run_inspect<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::playback_json(s).await) })
}
