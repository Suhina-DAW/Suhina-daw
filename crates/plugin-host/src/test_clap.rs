//! A CLAP plug-in built into the host for tests (`test:clap`): a gain, with its state (the gain
//! as 4 bytes, little-endian `f32`) and a reported latency of [`LATENCY`] frames. Loading it
//! through `clack-host` runs the host's whole CLAP path except opening a file, so the tests do
//! not need a plug-in installed.

use std::sync::atomic::{AtomicU32, Ordering};

use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

/// Latency the test plug-in reports, in frames.
pub(crate) const LATENCY: u32 = 16;

pub(crate) type Entry = SinglePluginEntry<TestGain>;

pub(crate) struct TestGain;

pub(crate) struct GainShared {
    gain: AtomicU32,
}

impl PluginShared<'_> for GainShared {}

pub(crate) struct GainMainThread<'a> {
    shared: &'a GainShared,
}

impl<'a> PluginMainThread<'a, GainShared> for GainMainThread<'a> {}

impl Plugin for TestGain {
    type AudioProcessor<'a> = GainProcessor<'a>;
    type Shared<'a> = GainShared;
    type MainThread<'a> = GainMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&GainShared>) {
        builder.register::<PluginState>().register::<PluginLatency>();
    }
}

impl DefaultPluginFactory for TestGain {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("org.loopwerk.test.gain", "Loopwerk Test Gain")
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<GainShared, PluginError> {
        Ok(GainShared { gain: AtomicU32::new(0.5f32.to_bits()) })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a GainShared,
    ) -> Result<GainMainThread<'a>, PluginError> {
        Ok(GainMainThread { shared })
    }
}

impl PluginStateImpl for GainMainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write;
        let gain = f32::from_bits(self.shared.gain.load(Ordering::Relaxed));
        output.write_all(&gain.to_le_bytes()).map_err(|_| PluginError::Message("could not write the state"))
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read;
        let mut bytes = [0u8; 4];
        input.read_exact(&mut bytes).map_err(|_| PluginError::Message("the state is 4 bytes"))?;
        let gain = f32::from_le_bytes(bytes);
        if !gain.is_finite() {
            return Err(PluginError::Message("the gain is not a finite number"));
        }
        self.shared.gain.store(gain.to_bits(), Ordering::Relaxed);
        Ok(())
    }
}

impl PluginLatencyImpl for GainMainThread<'_> {
    fn get(&self) -> u32 {
        LATENCY
    }
}

pub(crate) struct GainProcessor<'a> {
    shared: &'a GainShared,
}

impl<'a> PluginAudioProcessor<'a, GainShared, GainMainThread<'a>> for GainProcessor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &GainMainThread<'a>,
        shared: &'a GainShared,
        _config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self { shared })
    }

    fn process(&mut self, _process: Process, mut audio: Audio, _events: Events) -> Result<ProcessStatus, PluginError> {
        let gain = f32::from_bits(self.shared.gain.load(Ordering::Relaxed));
        for mut port in &mut audio {
            let Some(pairs) = port.channels()?.into_f32() else { continue };
            for pair in pairs {
                match pair {
                    ChannelPair::InputOnly(_) => {}
                    ChannelPair::OutputOnly(out) => out.fill(0.0),
                    ChannelPair::InputOutput(input, out) => {
                        for (o, i) in out.iter_mut().zip(input) {
                            *o = i * gain;
                        }
                    }
                    ChannelPair::InPlace(buffer) => buffer.iter_mut().for_each(|s| *s *= gain),
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}
