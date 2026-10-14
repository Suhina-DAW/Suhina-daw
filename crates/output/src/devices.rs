//! The system's audio hosts and output devices, and fitting a choice to what a device offers.
//! Drivers report what they like: every number from them is checked, and a host or device that
//! fails to answer is listed with the error or left out, never a panic (AGENTS.md rule 2).

use cpal::traits::{DeviceTrait, HostTrait};
use suhina_session::audio::{DeviceInfo, HostInfo, InputInfo, MAX_BUFFER_FRAMES, MAX_SAMPLE_RATE, MIN_BUFFER_FRAMES};

/// The buffer sizes offered when the driver gives no range: powers of two the engine renders
/// well in.
const BUFFER_SIZES: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

/// Sample rates looked for within a device's supported ranges.
const SAMPLE_RATES: [u32; 13] =
    [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000];

/// The buffer sizes a device with the driver range `range` can be asked for: the powers of two
/// inside it, and its ends, within [`MIN_BUFFER_FRAMES`] … [`MAX_BUFFER_FRAMES`]. A range that
/// makes no sense (empty, reversed) counts as none.
pub fn allowed_buffers(range: Option<(u32, u32)>) -> Vec<u32> {
    let Some((min, max)) = range.filter(|(min, max)| *min > 0 && min <= max) else {
        return BUFFER_SIZES.to_vec();
    };
    let inside = |n: u32| (min..=max).contains(&n) && (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(&n);
    let mut sizes: Vec<u32> = BUFFER_SIZES.iter().copied().chain([min, max]).filter(|n| inside(*n)).collect();
    sizes.sort_unstable();
    sizes.dedup();
    if sizes.is_empty() {
        // The driver only does sizes outside the engine's; the closest end the engine can use.
        sizes.push(min.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES));
    }
    sizes
}

/// The allowed size nearest to `wanted` (the larger of two as near).
pub fn fit_buffer(wanted: u32, allowed: &[u32]) -> u32 {
    allowed
        .iter()
        .copied()
        .min_by_key(|n| (n.abs_diff(wanted), std::cmp::Reverse(*n)))
        .unwrap_or_else(|| wanted.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES))
}

/// The sample rates inside the supported `ranges` (min, max): the common ones, and a range's
/// only rate when it has one. Rates of 0 or past [`MAX_SAMPLE_RATE`] are left out.
pub fn sample_rates(ranges: &[(u32, u32)]) -> Vec<u32> {
    let usable = |r: u32| r > 0 && r <= MAX_SAMPLE_RATE;
    let mut rates: Vec<u32> = SAMPLE_RATES
        .iter()
        .copied()
        .filter(|rate| ranges.iter().any(|(min, max)| (*min..=*max).contains(rate)))
        .chain(ranges.iter().filter(|(min, max)| min == max && usable(*min)).map(|(min, _)| *min))
        .collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}

/// The buffer sizes offered on `host`: WASAPI shared mode plays in the period Windows gives it,
/// whatever is asked, so there the size is the engine's block alone and any size is offered;
/// elsewhere the sizes the driver allows.
pub fn offered_buffers(host: &str, range: Option<(u32, u32)>) -> Vec<u32> {
    if host == "WASAPI" { allowed_buffers(None) } else { allowed_buffers(range) }
}

/// Fits a wanted buffer size to `host` and the driver's `range`: the frames the engine renders
/// per block, and the size to ask the device for (none where the driver decides).
pub fn fit_for_host(host: &str, wanted: u32, range: Option<(u32, u32)>) -> (u32, Option<u32>) {
    if host == "WASAPI" {
        return (wanted.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES), None);
    }
    let fitted = fit_buffer(wanted, &allowed_buffers(range));
    (fitted, range.map(|_| fitted))
}

/// Whether two names are the same, ignoring case and the space around them.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The driver's buffer range, when it reports a usable one.
pub fn buffer_range(size: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *size {
        cpal::SupportedBufferSize::Range { min, max } if min > 0 && min <= max => Some((min, max)),
        _ => None,
    }
}

/// Lists every host cpal offers here and each one's output devices.
pub fn list_hosts() -> Vec<HostInfo> {
    let default_host = cpal::default_host().id();
    #[cfg(windows)]
    let exclusive = crate::exclusive::capable_devices();
    cpal::available_hosts()
        .into_iter()
        .map(|id| {
            let mut info = HostInfo { name: id.name().to_string(), default: id == default_host, ..HostInfo::default() };
            match cpal::host_from_id(id) {
                Ok(host) => {
                    match list_devices(&host, id.name()) {
                        Ok(devices) => info.devices = devices,
                        Err(e) => info.error = Some(e),
                    }
                    info.inputs = list_inputs(&host);
                }
                Err(e) => info.error = Some(e.to_string()),
            }
            #[cfg(windows)]
            if info.name == "WASAPI" {
                for device in &mut info.devices {
                    device.exclusive = exclusive.iter().any(|name| same_name(name, &device.name));
                }
            }
            info
        })
        .collect()
}

/// The host's input devices with their channels and sample rates; none when they cannot be
/// listed.
fn list_inputs(host: &cpal::Host) -> Vec<InputInfo> {
    let default = host.default_input_device().map(|d| d.to_string());
    let Ok(devices) = host.input_devices() else { return Vec::new() };
    devices
        .map(|device| {
            let name = device.to_string();
            let mut ranges = Vec::new();
            let mut channels = 0;
            if let Ok(configs) = device.supported_input_configs() {
                for config in configs {
                    ranges.push((config.min_sample_rate(), config.max_sample_rate()));
                    channels = channels.max(config.channels());
                }
            }
            InputInfo {
                default: default.as_deref() == Some(name.as_str()),
                channels,
                sample_rates: sample_rates(&ranges),
                name,
            }
        })
        .filter(|input| input.channels > 0)
        .collect()
}

fn list_devices(host: &cpal::Host, host_name: &str) -> Result<Vec<DeviceInfo>, String> {
    let default = host.default_output_device().map(|d| d.to_string());
    let devices = host.output_devices().map_err(|e| e.to_string())?;
    Ok(devices
        .map(|device| {
            let name = device.to_string();
            let mut info = DeviceInfo { default: default.as_deref() == Some(name.as_str()), ..DeviceInfo::default() };
            let mut ranges = Vec::new();
            if let Ok(configs) = device.supported_output_configs() {
                for config in configs {
                    ranges.push((config.min_sample_rate(), config.max_sample_rate()));
                    info.channels = info.channels.max(config.channels());
                    info.buffer_range = info.buffer_range.or_else(|| buffer_range(config.buffer_size()));
                }
            }
            if let Ok(config) = device.default_output_config() {
                info.default_sample_rate = Some(config.sample_rate()).filter(|r| *r > 0 && *r <= MAX_SAMPLE_RATE);
                info.channels = info.channels.max(config.channels());
                info.buffer_range = info.buffer_range.or_else(|| buffer_range(config.buffer_size()));
            }
            info.sample_rates = sample_rates(&ranges);
            info.buffer_sizes = offered_buffers(host_name, info.buffer_range);
            info.name = name;
            info
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_are_the_drivers_powers_of_two_and_ends() {
        assert_eq!(allowed_buffers(None), BUFFER_SIZES.to_vec());
        assert_eq!(allowed_buffers(Some((32, 1024))), vec![64, 128, 256, 512, 1024]);
        assert_eq!(allowed_buffers(Some((96, 2000))), vec![96, 128, 256, 512, 1024, 2000]);
        // Nonsense from a driver: as if it said nothing.
        assert_eq!(allowed_buffers(Some((0, 0))), BUFFER_SIZES.to_vec());
        assert_eq!(allowed_buffers(Some((2048, 64))), BUFFER_SIZES.to_vec());
        // Only sizes the engine cannot use: the nearest it can.
        assert_eq!(allowed_buffers(Some((8, 32))), vec![64]);
        assert_eq!(allowed_buffers(Some((8192, 16384))), vec![4096]);
    }

    #[test]
    fn a_wanted_buffer_fits_the_nearest_allowed_one() {
        let allowed = [64, 128, 256, 512, 1024];
        assert_eq!(fit_buffer(512, &allowed), 512);
        assert_eq!(fit_buffer(300, &allowed), 256);
        assert_eq!(fit_buffer(384, &allowed), 512, "a tie goes to the larger");
        assert_eq!(fit_buffer(4096, &allowed), 1024);
        assert_eq!(fit_buffer(10, &allowed), 64);
        assert_eq!(fit_buffer(10, &[]), MIN_BUFFER_FRAMES);
        assert_eq!(fit_buffer(100_000, &[]), MAX_BUFFER_FRAMES);
    }

    #[test]
    fn wasapi_shared_takes_any_size_for_the_engine_and_asks_the_device_for_none() {
        // Windows plays shared WASAPI in its own period (480 frames here), whatever is asked.
        assert_eq!(fit_for_host("WASAPI", 256, Some((480, 480))), (256, None));
        assert_eq!(fit_for_host("WASAPI", 10_000, Some((480, 480))), (4096, None));
        assert_eq!(offered_buffers("WASAPI", Some((480, 480))), BUFFER_SIZES.to_vec());
        // A driver that sets its buffer (ASIO) gets the nearest size it allows.
        assert_eq!(fit_for_host("ASIO", 300, Some((64, 2048))), (256, Some(256)));
        assert_eq!(fit_for_host("ASIO", 300, None), (256, None));
        assert_eq!(offered_buffers("ASIO", Some((256, 512))), vec![256, 512]);
    }

    #[test]
    fn sample_rates_come_from_the_ranges() {
        assert_eq!(sample_rates(&[(44_100, 44_100)]), vec![44_100]);
        assert_eq!(sample_rates(&[(44_100, 48_000)]), vec![44_100, 48_000]);
        assert_eq!(sample_rates(&[(8_000, 96_000), (44_100, 44_100)]).len(), 9);
        // A driver's odd fixed rate is kept; nonsense is not.
        assert_eq!(sample_rates(&[(47_999, 47_999)]), vec![47_999]);
        assert!(sample_rates(&[(0, 0), (u32::MAX, u32::MAX)]).is_empty());
        assert!(sample_rates(&[(96_000, 44_100)]).is_empty(), "a reversed range holds nothing");
        assert!(sample_rates(&[]).is_empty());
    }

    #[test]
    fn names_match_without_case() {
        assert!(same_name("Speakers (Universal Audio Thunderbolt WDM)", " speakers (universal audio thunderbolt wdm)"));
        assert!(!same_name("Speakers", "Speakers 2"));
    }

    /// Listing works on any machine, with or without devices, and says something about each
    /// host.
    #[test]
    fn listing_the_hosts_never_fails() {
        let hosts = list_hosts();
        for host in &hosts {
            assert!(!host.name.is_empty());
            for device in &host.devices {
                assert!(!device.buffer_sizes.is_empty(), "{}", device.name);
                assert!(device.buffer_sizes.windows(2).all(|w| w[0] < w[1]));
            }
        }
        assert!(hosts.iter().filter(|h| h.default).count() <= 1);
    }
}
