//! Running audio through a plug-in offline, as fast as it goes: the same host process and
//! link as playback, but each block waits for the plug-in instead of a real-time deadline.

use std::time::Duration;

use loopwerk_ipc::Outcome;

use crate::{Launch, Plugin, PluginError};

/// Frames per block offline.
pub const BLOCK_FRAMES: u32 = 512;
/// How long one block may take offline before the plug-in counts as stuck.
pub const BLOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// What an offline run gave.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// Interleaved stereo: the input's length plus the tail, lined up with the input (the
    /// plug-in's latency taken out).
    pub samples: Vec<f32>,
    /// The latency the plug-in reported, in frames.
    pub latency: u32,
}

/// Runs interleaved stereo `input` through the plug-in `launch` names, then `tail_frames` of
/// silence so effects ring out. `launch.channels` and `launch.max_frames` are set here.
pub fn render(mut launch: Launch, input: &[f32], tail_frames: usize) -> Result<Rendered, PluginError> {
    launch.channels = 2;
    launch.max_frames = BLOCK_FRAMES;
    let (plugin, mut audio) = Plugin::start(launch)?;
    let latency = plugin.latency();
    let input_frames = input.len() / 2;
    let wanted = input_frames + tail_frames;
    // Run long enough that the delayed output covers what is wanted.
    let total = wanted + latency as usize;
    let block = BLOCK_FRAMES as usize;
    let mut output = Vec::with_capacity(total * 2 + block * 2);
    let mut chunk_in = vec![0.0f32; block * 2];
    let mut chunk_out = vec![0.0f32; block * 2];
    let mut frame = 0;
    while frame < total {
        let frames = block.min(total - frame);
        let samples = frames * 2;
        chunk_in.fill(0.0);
        let from = (frame * 2).min(input.len());
        let to = ((frame + frames) * 2).min(input.len());
        if let (Some(source), Some(target)) = (input.get(from..to), chunk_in.get_mut(..to - from)) {
            target.copy_from_slice(source);
        }
        let (Some(block_in), Some(block_out)) = (chunk_in.get(..samples), chunk_out.get_mut(..samples)) else {
            break;
        };
        match audio.process(block_in, block_out, BLOCK_TIMEOUT) {
            Outcome::Done => output.extend_from_slice(block_out),
            Outcome::Missed | Outcome::Busy => {
                return Err(PluginError::Timeout("a block offline (the plug-in stopped answering)"));
            }
        }
        frame += frames;
    }
    drop(plugin);
    let skip = (latency as usize * 2).min(output.len());
    let mut samples = output.split_off(skip);
    samples.truncate(wanted * 2);
    samples.resize(wanted * 2, 0.0);
    Ok(Rendered { samples, latency })
}
