//! Sound output for Windows and macOS through `cpal` (WASAPI / CoreAudio).
//!
//! The mixer thread produces 44.1 kHz stereo chunks and hands them over a small
//! channel; cpal's callback pulls them out, converting to the device's rate and
//! channel count. The channel is short (2 chunks, about 46 ms), so the mixer stays
//! only a little ahead of the speakers, like with PulseAudio on Linux.
//!
//! Rust note: a cpal `Stream` can't move between threads on every platform, so a
//! small thread of its own builds the stream and keeps it alive.

use super::RATE;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::VecDeque;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

pub struct Output {
    tx: SyncSender<Vec<i16>>,
}

impl Output {
    pub fn open() -> Option<Output> {
        let (tx, rx) = sync_channel::<Vec<i16>>(2);
        let (ok_tx, ok_rx) = sync_channel::<bool>(1);
        std::thread::spawn(move || {
            match start_stream(rx) {
                Some(stream) => {
                    let _ = ok_tx.send(true);
                    // keep the stream alive for the life of the game
                    loop {
                        std::thread::park();
                        let _ = &stream;
                    }
                }
                None => {
                    let _ = ok_tx.send(false);
                }
            }
        });
        if ok_rx.recv().unwrap_or(false) { Some(Output { tx }) } else { None }
    }

    pub fn name(&self) -> &'static str {
        if cfg!(target_os = "windows") { "wasapi" } else { "coreaudio" }
    }

    /// Blocks until there's room for the chunk. Returns false if the stream went away.
    pub fn write(&mut self, pcm: &[i16]) -> bool {
        self.tx.send(pcm.to_vec()).is_ok()
    }
}

fn start_stream(rx: Receiver<Vec<i16>>) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, rx),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, rx),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, rx),
        _ => None,
    }?;
    stream.play().ok()?;
    Some(stream)
}

/// Feeds the device from our 44.1 kHz stereo chunks, with linear resampling.
struct Feeder {
    rx: Receiver<Vec<i16>>,
    buf: VecDeque<[f32; 2]>,
    pos: f64,  // fractional read position into `buf`
    step: f64, // our frames per device frame
}

impl Feeder {
    fn next(&mut self) -> [f32; 2] {
        while self.buf.len() < 2 {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    for f in chunk.chunks_exact(2) {
                        self.buf.push_back([f[0] as f32 / 32768.0, f[1] as f32 / 32768.0]);
                    }
                }
                Err(_) => return [0.0, 0.0], // mixer is late: a moment of silence
            }
        }
        let a = self.buf[0];
        let b = self.buf[1];
        let t = self.pos as f32;
        let out = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        self.pos += self.step;
        while self.pos >= 1.0 && self.buf.len() >= 2 {
            self.buf.pop_front();
            self.pos -= 1.0;
        }
        out
    }
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, rx: Receiver<Vec<i16>>) -> Option<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let dev_rate = config.sample_rate as f64;
    let mut feed = Feeder { rx, buf: VecDeque::new(), pos: 0.0, step: RATE as f64 / dev_rate };
    device
        .build_output_stream(
            config.clone(),
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let [l, r] = feed.next();
                    for (c, s) in frame.iter_mut().enumerate() {
                        let v = match (channels, c) {
                            (1, _) => (l + r) * 0.5,
                            (_, 0) => l,
                            (_, 1) => r,
                            _ => 0.0,
                        };
                        *s = T::from_sample(v);
                    }
                }
            },
            |e| eprintln!("sound output error: {e}"),
            None,
        )
        .ok()
}
