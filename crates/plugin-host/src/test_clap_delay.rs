// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A second CLAP plug-in built into the host for tests (`test:clap-delay`): a delay whose one
//! parameter (id 0, "Delay", 0 to [`MAX_DELAY`] frames, stepped, 0 at first) is also its
//! latency, like a plug-in whose lookahead the user sets. Its latency may change only while it
//! is deactivated (CLAP's `latency` extension), so a new value asks the host for a restart
//! (`request_restart`); the delay and the reported latency take the new value when it is
//! activated again. Until then it keeps delaying by the old one.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams, PluginMainThreadParams,
    PluginParams,
};
use clack_plugin::events::io::{InputEvents, OutputEvents};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::utils::Cookie;

/// Longest delay it takes, in frames.
pub(crate) const MAX_DELAY: u32 = 4096;
/// Channels it delays; more are passed silent.
const CHANNELS: usize = 2;
/// The id of its one parameter, the delay.
const DELAY_PARAM: ClapId = ClapId::new(0);

pub(crate) type Entry = SinglePluginEntry<TestDelay>;

pub(crate) struct TestDelay;

pub(crate) struct DelayShared {
    /// The delay the parameter asks for, in frames.
    wanted: AtomicU32,
    /// The delay in effect since it was last activated: its latency.
    active: AtomicU32,
    /// It asked for a restart that has not come yet.
    asked: AtomicBool,
}

impl PluginShared<'_> for DelayShared {}

pub(crate) struct DelayMainThread<'a> {
    shared: &'a DelayShared,
}

impl<'a> PluginMainThread<'a, DelayShared> for DelayMainThread<'a> {}

impl Plugin for TestDelay {
    type AudioProcessor<'a> = DelayProcessor<'a>;
    type Shared<'a> = DelayShared;
    type MainThread<'a> = DelayMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&DelayShared>) {
        builder.register::<PluginLatency>().register::<PluginParams>();
    }
}

impl DefaultPluginFactory for TestDelay {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("org.suhina.test.delay", "Suhina Test Delay")
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<DelayShared, PluginError> {
        Ok(DelayShared { wanted: AtomicU32::new(0), active: AtomicU32::new(0), asked: AtomicBool::new(false) })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a DelayShared,
    ) -> Result<DelayMainThread<'a>, PluginError> {
        Ok(DelayMainThread { shared })
    }
}

impl PluginLatencyImpl for DelayMainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.active.load(Ordering::Acquire)
    }
}

impl PluginMainThreadParams for DelayMainThread<'_> {
    fn count(&self) -> u32 {
        1
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        if index == 0 {
            info.set(&ParamInfo {
                id: DELAY_PARAM,
                flags: ParamInfoFlags::IS_STEPPED,
                cookie: Cookie::empty(),
                name: b"Delay",
                module: b"",
                min_value: 0.0,
                max_value: f64::from(MAX_DELAY),
                default_value: 0.0,
            });
        }
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        (id == DELAY_PARAM).then(|| f64::from(self.shared.wanted.load(Ordering::Acquire)))
    }

    fn value_to_text(&self, id: ClapId, value: f64, writer: &mut ParamDisplayWriter) -> std::fmt::Result {
        if id != DELAY_PARAM {
            return Err(std::fmt::Error);
        }
        use std::fmt::Write;
        write!(writer, "{value:.0} frames")
    }

    fn text_to_value(&self, _id: ClapId, _text: &std::ffi::CStr) -> Option<f64> {
        None
    }

    fn flush(&self, _input: &InputEvents, _output: &mut OutputEvents) {}
}

impl PluginAudioProcessorParams for DelayProcessor<'_> {
    fn flush(&mut self, _input: &InputEvents, _output: &mut OutputEvents) {}
}

pub(crate) struct DelayProcessor<'a> {
    host: HostAudioProcessorHandle<'a>,
    shared: &'a DelayShared,
    /// Per channel, the samples on their way through: always as long as the delay.
    lines: Vec<VecDeque<f32>>,
}

impl<'a> PluginAudioProcessor<'a, DelayShared, DelayMainThread<'a>> for DelayProcessor<'a> {
    fn activate(
        host: HostAudioProcessorHandle<'a>,
        _main_thread: &DelayMainThread<'a>,
        shared: &'a DelayShared,
        _config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let delay = shared.wanted.load(Ordering::Acquire);
        shared.active.store(delay, Ordering::Release);
        shared.asked.store(false, Ordering::Release);
        let lines = (0..CHANNELS).map(|_| VecDeque::from(vec![0.0; delay as usize])).collect();
        Ok(Self { host, shared, lines })
    }

    fn process(&mut self, _process: Process, mut audio: Audio, events: Events) -> Result<ProcessStatus, PluginError> {
        for event in events.input {
            if let Some(CoreEventSpace::ParamValue(param)) = event.as_core_event()
                && param.param_id() == Some(DELAY_PARAM)
                && param.value().is_finite()
            {
                let delay = param.value().round().clamp(0.0, f64::from(MAX_DELAY)) as u32;
                self.shared.wanted.store(delay, Ordering::Release);
            }
        }
        let wanted = self.shared.wanted.load(Ordering::Acquire);
        if wanted != self.shared.active.load(Ordering::Acquire) && !self.shared.asked.swap(true, Ordering::AcqRel) {
            self.host.request_restart();
        }
        for mut port in &mut audio {
            let Some(pairs) = port.channels()?.into_f32() else { continue };
            for (channel, pair) in pairs.into_iter().enumerate() {
                let mut line = self.lines.get_mut(channel);
                let mut delay = |out: &mut f32, sample: f32| match line.as_deref_mut() {
                    Some(line) if !line.is_empty() => {
                        line.push_back(sample);
                        *out = line.pop_front().unwrap_or(0.0);
                    }
                    Some(_) => *out = sample,
                    None => *out = 0.0,
                };
                match pair {
                    ChannelPair::InputOnly(_) => {}
                    ChannelPair::OutputOnly(out) => out.fill(0.0),
                    ChannelPair::InputOutput(input, out) => {
                        for (o, i) in out.iter_mut().zip(input) {
                            delay(o, *i);
                        }
                    }
                    ChannelPair::InPlace(buffer) => {
                        for sample in buffer.iter_mut() {
                            let i = *sample;
                            delay(sample, i);
                        }
                    }
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}
