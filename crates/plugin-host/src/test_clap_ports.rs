//! A CLAP plug-in built into the host for tests (`test:clap-ports`) whose audio is laid out
//! like a modular rack's: a main input and a main output of [`MAIN_CHANNELS`] channels each,
//! and a stereo side input and output that are not main. It copies each input port to the
//! output port of the same index, channel by channel.
//!
//! Like such plug-ins it reads every channel of every port it declared, so it refuses a block
//! whose ports are not laid out as it said (an error, not a crash): a host must give each port
//! the channels the plug-in declared, whatever the engine's own layout.

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts, PluginAudioPortsImpl,
};
use clack_plugin::prelude::*;

/// Channels of the main input and of the main output.
pub(crate) const MAIN_CHANNELS: u32 = 8;
/// Channels of the side input and of the side output.
const SIDE_CHANNELS: u32 = 2;

pub(crate) type Entry = SinglePluginEntry<TestPorts>;

pub(crate) struct TestPorts;

pub(crate) struct PortsShared;

impl PluginShared<'_> for PortsShared {}

pub(crate) struct PortsMainThread;

impl PluginMainThread<'_, PortsShared> for PortsMainThread {}

impl Plugin for TestPorts {
    type AudioProcessor<'a> = PortsProcessor;
    type Shared<'a> = PortsShared;
    type MainThread<'a> = PortsMainThread;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&PortsShared>) {
        builder.register::<PluginAudioPorts>();
    }
}

impl DefaultPluginFactory for TestPorts {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("org.suhina.test.ports", "Suhina Test Ports")
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<PortsShared, PluginError> {
        Ok(PortsShared)
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        _shared: &'a PortsShared,
    ) -> Result<PortsMainThread, PluginError> {
        Ok(PortsMainThread)
    }
}

/// The channels of each port, for inputs and outputs alike.
const DECLARED: [u32; 2] = [MAIN_CHANNELS, SIDE_CHANNELS];

impl PluginAudioPortsImpl for PortsMainThread {
    fn count(&self, _is_input: bool) -> u32 {
        DECLARED.len() as u32
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        let Some(&channel_count) = DECLARED.get(index as usize) else { return };
        let main = index == 0;
        writer.set(&AudioPortInfo {
            id: ClapId::new(index),
            name: match (main, is_input) {
                (true, true) => b"Main In",
                (true, false) => b"Main Out",
                (false, true) => b"Side In",
                (false, false) => b"Side Out",
            },
            channel_count,
            flags: if main { AudioPortFlags::IS_MAIN } else { AudioPortFlags::empty() },
            port_type: (!main).then_some(AudioPortType::STEREO),
            in_place_pair: None,
        });
    }
}

pub(crate) struct PortsProcessor;

impl<'a> PluginAudioProcessor<'a, PortsShared, PortsMainThread> for PortsProcessor {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &PortsMainThread,
        _shared: &'a PortsShared,
        _config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self)
    }

    fn process(&mut self, _process: Process, mut audio: Audio, _events: Events) -> Result<ProcessStatus, PluginError> {
        let inputs = audio.input_ports_infos().map(|p| p.channel_count()).eq(DECLARED);
        let outputs = audio.output_ports_infos().map(|p| p.channel_count()).eq(DECLARED);
        if !inputs || !outputs {
            return Err(PluginError::Message("the ports are not laid out as declared"));
        }
        for mut pair in &mut audio {
            let Some(pairs) = pair.channels()?.into_f32() else { continue };
            for channel in pairs {
                match channel {
                    ChannelPair::InputOnly(_) => {}
                    ChannelPair::OutputOnly(out) => out.fill(0.0),
                    ChannelPair::InputOutput(input, out) => out.iter_mut().zip(input).for_each(|(o, i)| *o = *i),
                    ChannelPair::InPlace(_) => {}
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}
