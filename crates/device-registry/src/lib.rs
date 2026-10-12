//! Every built-in device by its stable id (ADR 0004): what the model stores as
//! `DeviceKind::Builtin(id)` turns into a running device here. The engine builds device chains
//! through this crate; front ends list what can be added.

use loopwerk_device_api::{Device, DeviceInfo};

/// The built-in devices, in the order a device browser lists them.
pub const DEVICES: [&DeviceInfo; 6] = [
    &loopwerk_device_fm_synth::INFO,
    &loopwerk_device_utility::INFO,
    &loopwerk_device_auto_pan::INFO,
    &loopwerk_device_dj_eq::INFO,
    &loopwerk_device_vinyl_distortion::INFO,
    &loopwerk_device_overdrive::INFO,
];

/// What the device with this id is, or `None` for an unknown id.
pub fn info(id: &str) -> Option<&'static DeviceInfo> {
    DEVICES.iter().copied().find(|info| info.id == id)
}

/// A new device with this id at its defaults, prepared for 48 kHz, or `None` for an unknown
/// id. Allocates: call it off the audio thread.
pub fn create(id: &str) -> Option<Box<dyn Device>> {
    Some(match id {
        "fm-synth" => Box::new(loopwerk_device_fm_synth::FmSynth::new()),
        "utility" => Box::new(loopwerk_device_utility::Utility::new()),
        "auto-pan" => Box::new(loopwerk_device_auto_pan::AutoPan::new()),
        "dj-eq" => Box::new(loopwerk_device_dj_eq::DjEq::new()),
        "vinyl-distortion" => Box::new(loopwerk_device_vinyl_distortion::VinylDistortion::new()),
        "overdrive" => Box::new(loopwerk_device_overdrive::Overdrive::new()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_device_can_be_made_and_ids_are_unique() {
        for (i, info) in DEVICES.iter().enumerate() {
            let device = create(info.id).unwrap_or_else(|| panic!("{} cannot be made", info.id));
            assert_eq!(device.info(), *info);
            assert_eq!(super::info(info.id), Some(*info));
            assert!(DEVICES.iter().skip(i + 1).all(|other| other.id != info.id), "{} twice", info.id);
        }
        assert!(create("no-such-device").is_none());
        assert!(info("").is_none());
    }
}
