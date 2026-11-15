//! cpal output (PipeWire, then PulseAudio, then ALSA) feeding the mixer.

use super::mixer::Mixer;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Output {
    _stream: Option<cpal::Stream>,
    _null: Option<NullOut>,
    pub host: String,
    pub rate: u32,
    /// What was asked of the backend ("default" or "N frames").
    pub buffer: String,
    /// Backend-predicted callback-to-playback time in microseconds (0 until reported). Written by the
    /// callback with one relaxed store; read for a log line only, never awaited.
    pub latency_us: Arc<AtomicU32>,
}

/// Callback period asked of PulseAudio. cpal's Default there leaves the server's own buffering, which PulseAudio
/// documents as up to about 2 s and which an owner's stream showed (tlength 2.000 s). Fixed(N) makes cpal set a
/// 2N target with adjust_latency, so 25 ms asks for about 50 ms in total. Not smaller: the mixer still decodes
/// inside the callback, and an underrun is a crackle.
const PULSE_PERIOD_MS: u32 = 25;

/// `FFXI_AUDIO_PERIOD_MS`: 0 keeps the backend default (the old behaviour); 5..=200 overrides the period.
fn period_ms() -> u32 {
    period_from(std::env::var("FFXI_AUDIO_PERIOD_MS").ok().as_deref())
}

fn period_from(v: Option<&str>) -> u32 {
    match v.map(str::trim).filter(|v| !v.is_empty()).map(str::parse::<u32>) {
        None => PULSE_PERIOD_MS,
        Some(Ok(0)) => 0,
        Some(Ok(ms)) => ms.clamp(5, 200),
        Some(Err(_)) => PULSE_PERIOD_MS,
    }
}

/// Bounded buffering only where the default is known to be unbounded (PulseAudio). Other hosts (PipeWire, ALSA,
/// WASAPI) keep their negotiated default, as before.
fn buffer_request(host: &str, supported: &cpal::SupportedBufferSize, rate: u32, period_ms: u32) -> cpal::BufferSize {
    if period_ms == 0 || !host.eq_ignore_ascii_case("PulseAudio") {
        return cpal::BufferSize::Default;
    }
    let frames = (rate as u64 * period_ms as u64 / 1000).max(1) as u32;
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } if min <= max => cpal::BufferSize::Fixed(frames.clamp(min, max)),
        _ => cpal::BufferSize::Default,
    }
}

fn describe(b: &cpal::BufferSize) -> String {
    match b {
        cpal::BufferSize::Default => "default".into(),
        cpal::BufferSize::Fixed(n) => format!("{n} frames"),
    }
}

/// Output devices as "id  description" lines, for picking one in the config.
pub fn list() -> Vec<String> {
    let mut out = Vec::new();
    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        let Ok(devs) = host.output_devices() else { continue };
        for d in devs {
            let name = d.description().map(|x| x.to_string()).unwrap_or_default();
            out.push(format!("{}  {name}", d.id().map(|i| i.to_string()).unwrap_or_default()));
        }
    }
    out
}

/// What an output stream plays: interleaved stereo f32 frames, filled whole.
pub trait Render: Send + 'static {
    fn render(&mut self, out: &mut [f32]);
}

impl Render for Mixer {
    fn render(&mut self, out: &mut [f32]) {
        Mixer::render(self, out);
    }
}

/// Stops the null output's thread when dropped.
struct NullOut(Arc<AtomicBool>);

impl Drop for NullOut {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Opens the device with this cpal id ("host:device"), "null" (silent, mixed at real time, for test runs: ALSA's
/// null device is not paced, so sounds there end at once), else the default output; `make` builds the renderer once
/// the rate is known.
pub fn open<R: Render>(device: Option<&str>, make: impl FnOnce(u32) -> R) -> Result<Output> {
    if device == Some("null") {
        return open_null(make);
    }
    let (host, device) = match device.filter(|d| !d.is_empty()) {
        Some(s) => {
            // A device saved by a build with PipeWire, in one built without it (--no-default-features): this build
            // has no such host, so the id doesn't parse (or the host isn't there) and the default output plays
            // instead of no sound at all. A malformed id or a missing device on a known host still fails.
            let found = s.parse::<cpal::DeviceId>().and_then(|id| Ok((cpal::host_from_id(id.host())?, id)));
            match found {
                Ok((host, id)) => {
                    let d = host.device_by_id(&id).with_context(|| format!("no audio device {s}"))?;
                    (host, d)
                }
                Err(e)
                    if e.kind() == cpal::ErrorKind::UnsupportedOperation
                        || e.kind() == cpal::ErrorKind::HostUnavailable =>
                {
                    log::warn!("audio device {s}: {e}; using the default output");
                    let host = cpal::default_host();
                    let d = host.default_output_device().context("no audio output device")?;
                    (host, d)
                }
                Err(e) => return Err(e.into()),
            }
        }
        None => {
            let host = cpal::default_host();
            let d = host.default_output_device().context("no audio output device")?;
            (host, d)
        }
    };
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let host_name = host.id().name().to_string();
    let request = buffer_request(&host_name, supported.buffer_size(), supported.sample_rate(), period_ms());
    let mut config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    // The renderer is built once and shared, so a refused bounded request can retry with the backend default.
    let mixer = Arc::new(Mutex::new(make(rate)));
    let latency_us = Arc::new(AtomicU32::new(0));
    let try_build = |config: &StreamConfig| -> Result<cpal::Stream> {
        let (m, l) = (mixer.clone(), latency_us.clone());
        match format {
            SampleFormat::F32 => build::<f32, R>(&device, config.clone(), m, l),
            SampleFormat::I16 => build::<i16, R>(&device, config.clone(), m, l),
            SampleFormat::I32 => build::<i32, R>(&device, config.clone(), m, l),
            SampleFormat::U16 => build::<u16, R>(&device, config.clone(), m, l),
            f => anyhow::bail!("unsupported output sample format {f}"),
        }
    };
    config.buffer_size = request;
    let stream = match try_build(&config) {
        Ok(s) => s,
        Err(e) if !matches!(config.buffer_size, cpal::BufferSize::Default) => {
            log::warn!("audio: {} buffer refused ({e:#}); using the backend default", describe(&config.buffer_size));
            config.buffer_size = cpal::BufferSize::Default;
            try_build(&config)?
        }
        Err(e) => return Err(e),
    };
    stream.play()?;
    Ok(Output {
        _stream: Some(stream),
        _null: None,
        host: host_name,
        rate,
        buffer: describe(&config.buffer_size),
        latency_us,
    })
}

fn open_null<R: Render>(make: impl FnOnce(u32) -> R) -> Result<Output> {
    const RATE: u32 = 48000;
    const BLOCK: usize = 480;
    let mut mixer = make(RATE);
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::Builder::new().name("audio-null".into()).spawn(move || {
        let mut buf = vec![0.0f32; BLOCK * 2];
        let (start, mut done) = (Instant::now(), 0u64);
        while !s.load(Ordering::Relaxed) {
            mixer.render(&mut buf);
            done += BLOCK as u64;
            if let Some(d) =
                (start + Duration::from_micros(done * 1_000_000 / RATE as u64)).checked_duration_since(Instant::now())
            {
                std::thread::sleep(d);
            }
        }
    })?;
    Ok(Output {
        _stream: None,
        _null: Some(NullOut(stop)),
        host: "null".into(),
        rate: RATE,
        buffer: format!("{BLOCK} frames"),
        latency_us: Arc::new(AtomicU32::new(0)),
    })
}

fn build<T: SizedSample + FromSample<f32>, R: Render>(
    device: &cpal::Device,
    config: StreamConfig,
    mixer: Arc<Mutex<R>>,
    latency_us: Arc<AtomicU32>,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let mut buf: Vec<f32> = Vec::new();
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let t = info.timestamp();
            latency_us.store(
                t.playback.duration_since(t.callback).as_micros().min(u32::MAX as u128) as u32,
                Ordering::Relaxed,
            );
            // Only this callback renders once a stream exists, so the lock is uncontended; never block on it.
            let Ok(mut mixer) = mixer.try_lock() else {
                for x in data.iter_mut() {
                    *x = T::from_sample(0.0);
                }
                return;
            };
            let frames = data.len() / channels.max(1);
            buf.resize(frames * 2, 0.0);
            mixer.render(&mut buf);
            for (f, s) in data.chunks_exact_mut(channels).zip(buf.chunks_exact(2)) {
                match f.len() {
                    1 => f[0] = T::from_sample((s[0] + s[1]) * 0.5),
                    _ => {
                        f[0] = T::from_sample(s[0]);
                        f[1] = T::from_sample(s[1]);
                        for x in &mut f[2..] {
                            *x = T::from_sample(0.0);
                        }
                    }
                }
            }
        },
        |e| log::warn!("audio stream: {e}"),
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pulse_gets_a_bounded_period_and_other_hosts_keep_their_default() {
        let range = cpal::SupportedBufferSize::Range { min: 1, max: 1 << 20 };
        assert_eq!(buffer_request("PulseAudio", &range, 48000, 25), cpal::BufferSize::Fixed(1200));
        assert_eq!(buffer_request("pulseaudio", &range, 44100, 25), cpal::BufferSize::Fixed(1102));
        for host in ["PipeWire", "ALSA", "WASAPI", "null"] {
            assert_eq!(buffer_request(host, &range, 48000, 20), cpal::BufferSize::Default, "{host}");
        }
        assert_eq!(
            buffer_request("PulseAudio", &range, 48000, 0),
            cpal::BufferSize::Default,
            "0 restores the old default"
        );
        assert_eq!(
            buffer_request("PulseAudio", &cpal::SupportedBufferSize::Unknown, 48000, 20),
            cpal::BufferSize::Default
        );
        let narrow = cpal::SupportedBufferSize::Range { min: 2048, max: 4096 };
        assert_eq!(
            buffer_request("PulseAudio", &narrow, 48000, 20),
            cpal::BufferSize::Fixed(2048),
            "clamped into the supported range"
        );
    }
    #[test]
    fn period_override_is_bounded_and_zero_means_default() {
        assert_eq!(period_from(None), PULSE_PERIOD_MS);
        assert_eq!(period_from(Some("")), PULSE_PERIOD_MS);
        assert_eq!(period_from(Some("junk")), PULSE_PERIOD_MS);
        assert_eq!(period_from(Some("0")), 0);
        assert_eq!(period_from(Some("1")), 5);
        assert_eq!(period_from(Some("50")), 50);
        assert_eq!(period_from(Some("9999")), 200);
    }
}
