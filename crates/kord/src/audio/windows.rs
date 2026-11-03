//! Core Audio (WASAPI) implementation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::warn;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, IAudioCaptureClient, IAudioClient, IAudioRenderClient,
    IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eCapture, eCommunications,
    eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows::core::{GUID, HSTRING, PCWSTR};

use super::{Phase, TestState};

pub const SUPPORTED: bool = true;

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
/// Shared-mode buffer length, in 100 ns units (200 ms).
const BUFFER_DURATION: i64 = 2_000_000;

fn enumerator() -> Result<IMMDeviceEnumerator> {
    // SAFETY: COM initialisation and object creation have no preconditions;
    // initialising an already initialised thread is harmless.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        Ok(CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?)
    }
}

fn device(id: &str) -> Result<IMMDevice> {
    // SAFETY: plain COM call.
    Ok(unsafe { enumerator()?.GetDevice(&HSTRING::from(id))? })
}

fn id_of(device: &IMMDevice) -> Option<String> {
    // SAFETY: the returned string is COM-allocated and freed here exactly once.
    unsafe {
        let id = device.GetId().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const _));
        text
    }
}

pub fn default_output() -> Result<Option<String>> {
    // SAFETY: plain COM call; failing just means there's no default device.
    Ok(unsafe { enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole) }.ok().and_then(|d| id_of(&d)))
}

pub fn default_input() -> Result<Option<String>> {
    // SAFETY: as above.
    Ok(unsafe { enumerator()?.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok().and_then(|d| id_of(&d)))
}

/// An active output that isn't one of `exclude`, e.g. the speakers to fall
/// back to when the headphones come off.
pub fn other_output(exclude: &[String]) -> Result<Option<String>> {
    // SAFETY: plain COM calls on valid interfaces.
    unsafe {
        let collection = enumerator()?.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for index in 0..collection.GetCount()? {
            if let Some(id) = collection.Item(index).ok().and_then(|d| id_of(&d))
                && !exclude.contains(&id)
            {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

/// Windows' own (undocumented but long-stable) interface for choosing the
/// default device, used by the Sound control panel.
#[allow(non_snake_case)]
mod policy {
    use std::ffi::c_void;

    use windows::Win32::Media::Audio::ERole;
    use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        unsafe fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        unsafe fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        unsafe fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        unsafe fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        unsafe fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, period: *mut i64, min: *mut i64) -> HRESULT;
        unsafe fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        unsafe fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        unsafe fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        unsafe fn GetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
        unsafe fn SetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
        pub unsafe fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        unsafe fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    pub const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
}

use self::policy::{IPolicyConfig, POLICY_CONFIG_CLIENT};

/// Makes `id` the default device (for its direction) for every role.
pub fn set_default(id: &str) -> Result<()> {
    let wide: Vec<u16> = id.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated device id that outlives the calls.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let policy: IPolicyConfig = CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)?;
        for role in [eConsole, eMultimedia, eCommunications] {
            policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).ok()?;
        }
    }
    Ok(())
}

pub fn volume(id: &str) -> Result<u8> {
    // SAFETY: plain COM calls.
    unsafe {
        let volume: IAudioEndpointVolume = device(id)?.Activate(CLSCTX_ALL, None)?;
        Ok((volume.GetMasterVolumeLevelScalar()? * 100.0).round() as u8)
    }
}

pub fn set_volume(id: &str, percent: u8) -> Result<()> {
    // SAFETY: plain COM calls; a null event context is allowed.
    unsafe {
        let volume: IAudioEndpointVolume = device(id)?.Activate(CLSCTX_ALL, None)?;
        volume.SetMasterVolumeLevelScalar(f32::from(percent.min(100)) / 100.0, std::ptr::null())?;
    }
    Ok(())
}

/// Sample layout of a shared-mode stream.
#[derive(Clone, Copy)]
struct Format {
    channels: usize,
    rate: u32,
    float: bool,
    bytes_per_sample: usize,
}

/// An initialised shared-mode stream on a device.
struct Stream {
    client: IAudioClient,
    format: Format,
}

impl Stream {
    fn open(id: &str) -> Result<Self> {
        // SAFETY: the mix format is COM-allocated and freed after Initialize copied it.
        unsafe {
            let client: IAudioClient = device(id)?.Activate(CLSCTX_ALL, None)?;
            let mix = client.GetMixFormat()?;
            let format = read_format(mix);
            let result = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, BUFFER_DURATION, 0, mix, None);
            CoTaskMemFree(Some(mix as *const _));
            result.context("opening the audio stream")?;
            Ok(Self { client, format })
        }
    }
}

/// # Safety
/// `format` must point to a valid WAVEFORMATEX (or WAVEFORMATEXTENSIBLE).
unsafe fn read_format(format: *const WAVEFORMATEX) -> Format {
    // SAFETY: the caller guarantees validity; the structs are packed, so
    // fields are copied out with unaligned reads.
    unsafe {
        let tag = std::ptr::addr_of!((*format).wFormatTag).read_unaligned();
        let bits = std::ptr::addr_of!((*format).wBitsPerSample).read_unaligned();
        let float = tag == WAVE_FORMAT_IEEE_FLOAT
            || (tag == WAVE_FORMAT_EXTENSIBLE
                && std::ptr::addr_of!((*(format as *const WAVEFORMATEXTENSIBLE)).SubFormat).read_unaligned()
                    == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
        Format {
            channels: usize::from(std::ptr::addr_of!((*format).nChannels).read_unaligned()),
            rate: std::ptr::addr_of!((*format).nSamplesPerSec).read_unaligned(),
            float,
            bytes_per_sample: usize::from(bits / 8),
        }
    }
}

/// Reads one sample (as -1..1) from interleaved data.
///
/// # Safety
/// `data` must hold at least `index + 1` samples in `format`.
unsafe fn sample(data: *const u8, index: usize, format: Format) -> f32 {
    // SAFETY: bounds guaranteed by the caller; unaligned reads are fine.
    unsafe {
        let at = data.add(index * format.bytes_per_sample);
        match (format.float, format.bytes_per_sample) {
            (true, 4) => (at as *const f32).read_unaligned(),
            (false, 2) => f32::from((at as *const i16).read_unaligned()) / 32768.0,
            (false, 4) => (at as *const i32).read_unaligned() as f32 / 2_147_483_648.0,
            _ => 0.0,
        }
    }
}

/// Writes one sample (-1..1) into interleaved data.
///
/// # Safety
/// `data` must have room for at least `index + 1` samples in `format`.
unsafe fn write_sample(data: *mut u8, index: usize, value: f32, format: Format) {
    let value = value.clamp(-1.0, 1.0);
    // SAFETY: bounds guaranteed by the caller.
    unsafe {
        let at = data.add(index * format.bytes_per_sample);
        match (format.float, format.bytes_per_sample) {
            (true, 4) => (at as *mut f32).write_unaligned(value),
            (false, 2) => (at as *mut i16).write_unaligned((value * 32767.0) as i16),
            (false, 4) => (at as *mut i32).write_unaligned((value * 2_147_483_647.0) as i32),
            _ => {}
        }
    }
}

/// Records from the microphone for a few seconds, then plays it back on the
/// headphones, reporting progress and the input level as it goes.
pub struct MicTest {
    state: Arc<Mutex<TestState>>,
    _thread: JoinHandle<()>,
}

impl MicTest {
    pub fn start(input: String, output: String, seconds: u32) -> Self {
        let state = Arc::new(Mutex::new(TestState { phase: Phase::Recording { seconds_left: seconds }, level: 0.0 }));
        let shared = state.clone();
        let thread = std::thread::spawn(move || {
            let result = run_mic_test(&input, &output, seconds, &shared);
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            state.level = 0.0;
            state.phase = match result {
                Ok(()) => Phase::Done,
                Err(err) => {
                    warn!("microphone test failed: {err:#}");
                    Phase::Failed(format!("{err:#}"))
                }
            };
        });
        Self { state, _thread: thread }
    }

    pub fn state(&self) -> TestState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn run_mic_test(input: &str, output: &str, seconds: u32, state: &Mutex<TestState>) -> Result<()> {
    let set = |phase: Option<Phase>, level: Option<f32>| {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(phase) = phase {
            state.phase = phase;
        }
        if let Some(level) = level {
            state.level = level;
        }
    };

    // Record, mixed down to mono.
    let capture = Stream::open(input)?;
    let format = capture.format;
    let mut recording: Vec<f32> = Vec::with_capacity(format.rate as usize * seconds as usize);
    // SAFETY: the capture loop follows the WASAPI protocol: every buffer
    // obtained with GetBuffer is released with the same frame count.
    unsafe {
        let client: IAudioCaptureClient = capture.client.GetService()?;
        capture.client.Start()?;
        let start = Instant::now();
        let total = Duration::from_secs(u64::from(seconds));
        let mut level = 0.0f32;
        while start.elapsed() < total {
            std::thread::sleep(Duration::from_millis(15));
            while client.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                client.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let mut peak = 0.0f32;
                for frame in 0..frames as usize {
                    let mut sum = 0.0;
                    for channel in 0..format.channels {
                        sum += sample(data, frame * format.channels + channel, format);
                    }
                    let mono = sum / format.channels.max(1) as f32;
                    peak = peak.max(mono.abs());
                    recording.push(mono);
                }
                client.ReleaseBuffer(frames)?;
                level = (level * 0.6).max(peak);
            }
            let left = total.saturating_sub(start.elapsed()).as_secs_f32().ceil() as u32;
            set(Some(Phase::Recording { seconds_left: left }), Some((level * 2.5).min(1.0)));
        }
        capture.client.Stop()?;
    }
    set(Some(Phase::Playing), Some(0.0));

    // Play back, resampled to the output's rate and copied to every channel.
    let render = Stream::open(output)?;
    let out = render.format;
    let ratio = format.rate as f64 / out.rate as f64;
    let frames_total = (recording.len() as f64 / ratio) as usize;
    let source = |frame: usize| {
        let position = frame as f64 * ratio;
        let index = position as usize;
        let next = recording.get(index + 1).copied().unwrap_or(0.0);
        let current = recording.get(index).copied().unwrap_or(0.0);
        current + (next - current) * (position - index as f64) as f32
    };
    // SAFETY: as above, for the render side.
    unsafe {
        let client: IAudioRenderClient = render.client.GetService()?;
        let buffer_frames = render.client.GetBufferSize()?;
        render.client.Start()?;
        let mut written = 0usize;
        while written < frames_total {
            let available = buffer_frames - render.client.GetCurrentPadding()?;
            let count = (available as usize).min(frames_total - written);
            if count > 0 {
                let data = client.GetBuffer(count as u32)?;
                for frame in 0..count {
                    let value = source(written + frame);
                    for channel in 0..out.channels {
                        write_sample(data, frame * out.channels + channel, value, out);
                    }
                }
                client.ReleaseBuffer(count as u32, 0)?;
                written += count;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        // Let the tail play out.
        while render.client.GetCurrentPadding()? > 0 {
            std::thread::sleep(Duration::from_millis(15));
        }
        render.client.Stop()?;
    }
    Ok(())
}

/// Plays silence on an output while alive, so a Bluetooth link never goes
/// idle and the start of short sounds isn't cut off when it wakes up.
pub struct KeepAlive {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl KeepAlive {
    pub fn start(output: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            if let Err(err) = play_silence(&output, &flag) {
                warn!("keep-alive stream stopped: {err:#}");
            }
        });
        Self { stop, thread: Some(thread) }
    }
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn play_silence(output: &str, stop: &AtomicBool) -> Result<()> {
    let render = Stream::open(output)?;
    // SAFETY: every buffer obtained is released with the same frame count.
    unsafe {
        let client: IAudioRenderClient = render.client.GetService()?;
        let buffer_frames = render.client.GetBufferSize()?;
        render.client.Start()?;
        let frame_bytes = render.format.channels * render.format.bytes_per_sample;
        while !stop.load(Ordering::Relaxed) {
            let available = buffer_frames - render.client.GetCurrentPadding()?;
            if available > 0 {
                // Real zero samples rather than the "silent" flag, so the
                // stream counts as active all the way to the headphones.
                let data = client.GetBuffer(available)?;
                std::ptr::write_bytes(data, 0, available as usize * frame_bytes);
                client.ReleaseBuffer(available, 0)?;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        render.client.Stop()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real audio devices, silently: run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn streams_open_on_default_devices() {
        let output = default_output().unwrap().expect("a default output");
        let silence = KeepAlive::start(output);
        std::thread::sleep(Duration::from_secs(1));
        drop(silence);

        let input = default_input().unwrap().expect("a default input");
        let capture = Stream::open(&input).unwrap();
        let mut frames_read = 0u32;
        // SAFETY: standard capture loop, every buffer released.
        unsafe {
            let client: IAudioCaptureClient = capture.client.GetService().unwrap();
            capture.client.Start().unwrap();
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(500) {
                std::thread::sleep(Duration::from_millis(20));
                while client.GetNextPacketSize().unwrap() > 0 {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                    client.GetBuffer(&mut data, &mut frames, &mut flags, None, None).unwrap();
                    frames_read += frames;
                    client.ReleaseBuffer(frames).unwrap();
                }
            }
            capture.client.Stop().unwrap();
        }
        assert!(frames_read > capture.format.rate / 4, "captured {frames_read} frames");
        assert!(capture.format.channels > 0 && capture.format.bytes_per_sample > 0);
    }
}
