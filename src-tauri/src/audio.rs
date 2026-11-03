use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Recorded {
    pub path: PathBuf,
    pub duration: f64,
}

pub struct Recording {
    stop_tx: Option<mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<Result<Recorded, String>>>,
    started: Instant,
}

impl Recording {
    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn stop(mut self) -> Result<Recorded, String> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        match self.handle.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| "錄音執行緒異常結束".to_string())?,
            None => Err("錄音已經結束".to_string()),
        }
    }
}

/// List the names of microphones the system exposes to WASAPI.
pub fn input_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("無法列出麥克風：{e}"))?;
    let mut names: Vec<String> = devices.map(|d| d.to_string()).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

fn pick_device(host: &cpal::Host, wanted: Option<&str>) -> Result<cpal::Device, String> {
    if let Some(wanted) = wanted.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(devices) = host.input_devices() {
            if let Some(found) = devices.into_iter().find(|d| d.to_string() == wanted) {
                return Ok(found);
            }
        }
        eprintln!("[murmur] microphone \"{wanted}\" not found, using default");
    }
    host.default_input_device()
        .ok_or_else(|| "找不到麥克風，請確認麥克風已連接並允許應用程式存取".to_string())
}

pub fn start(
    device_name: Option<String>,
    out_path: PathBuf,
    level: Arc<AtomicU32>,
) -> Result<Recording, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let started = Instant::now();

    let handle = std::thread::spawn(move || -> Result<Recorded, String> {
        let host = cpal::default_host();
        let device = match pick_device(&host, device_name.as_deref()) {
            Ok(device) => device,
            Err(err) => {
                let _ = ready_tx.send(Err(err.clone()));
                return Err(err);
            }
        };

        let supported = match device.default_input_config() {
            Ok(config) => config,
            Err(err) => {
                let msg = format!("無法取得麥克風設定：{err}");
                let _ = ready_tx.send(Err(msg.clone()));
                return Err(msg);
            }
        };

        let sample_format = supported.sample_format();
        let stream_config: cpal::StreamConfig = supported.into();
        let channels = stream_config.channels.max(1) as usize;
        let rate = stream_config.sample_rate;
        let samples: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));

        let err_fn = |err: cpal::Error| {
            eprintln!("[murmur] audio stream error: {err}");
        };

        let stream_result: Result<cpal::Stream, String> = match sample_format {
            SampleFormat::F32 => {
                let sink = samples.clone();
                let level = level.clone();
                device
                    .build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            accumulate(data.iter().copied(), channels, &sink, &level);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| e.to_string())
            }
            SampleFormat::I16 => {
                let sink = samples.clone();
                let level = level.clone();
                device
                    .build_input_stream(
                        stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            accumulate(
                                data.iter().map(|s| *s as f32 / 32768.0),
                                channels,
                                &sink,
                                &level,
                            );
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| e.to_string())
            }
            SampleFormat::U16 => {
                let sink = samples.clone();
                let level = level.clone();
                device
                    .build_input_stream(
                        stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            accumulate(
                                data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0),
                                channels,
                                &sink,
                                &level,
                            );
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| e.to_string())
            }
            other => Err(format!("不支援的麥克風格式：{other:?}")),
        };

        let stream = match stream_result {
            Ok(stream) => stream,
            Err(err) => {
                let _ = ready_tx.send(Err(err.clone()));
                return Err(err);
            }
        };

        if let Err(err) = stream.play() {
            let msg = format!("無法開始錄音：{err}");
            let _ = ready_tx.send(Err(msg.clone()));
            return Err(msg);
        }

        let _ = ready_tx.send(Ok(rate));
        let _ = stop_rx.recv();

        let _ = stream.pause();
        drop(stream);

        let data = samples.lock().map(|d| d.clone()).unwrap_or_default();
        level.store(0f32.to_bits(), Ordering::Relaxed);

        if data.is_empty() {
            return Err("沒有收到任何音訊，請確認麥克風裝置".to_string());
        }

        let duration = data.len() as f64 / rate as f64;
        write_wav(&out_path, &data, rate)?;
        Ok(Recorded {
            path: out_path,
            duration,
        })
    });

    match ready_rx.recv_timeout(Duration::from_secs(6)) {
        Ok(Ok(_rate)) => Ok(Recording {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
            started,
        }),
        Ok(Err(err)) => {
            let _ = handle.join();
            Err(err)
        }
        Err(_) => {
            let _ = stop_tx.send(());
            let _ = handle.join();
            Err("啟動麥克風逾時".to_string())
        }
    }
}

fn accumulate<I: Iterator<Item = f32>>(
    frame_samples: I,
    channels: usize,
    sink: &Mutex<Vec<f32>>,
    level: &AtomicU32,
) {
    if channels == 0 {
        return;
    }
    let mut peak = 0.0f32;
    if let Ok(mut buffer) = sink.lock() {
        let mut channel_index = 0usize;
        let mut sum = 0.0f32;
        for sample in frame_samples {
            sum += sample;
            channel_index += 1;
            if channel_index == channels {
                let mono = sum / channels as f32;
                peak = peak.max(mono.abs());
                buffer.push(mono);
                channel_index = 0;
                sum = 0.0;
            }
        }
    }
    level.store((peak * 3.5).clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("無法建立暫存資料夾：{e}"))?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("無法建立錄音檔：{e}"))?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(value)
            .map_err(|e| format!("寫入錄音檔失敗：{e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("完成錄音檔失敗：{e}"))
}
