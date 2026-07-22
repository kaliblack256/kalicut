//! Batch selftest: probe + trim (copy/reencode) against sample videos.
//!
//! Video directory (first match wins):
//!   1. CLI: `kalicut_selftest /path/to/videos`
//!   2. Env: `KALICUT_TEST_VIDEOS`
//!   3. Default: `~/Videos` (or `%USERPROFILE%\Videos` via dirs-style home)
//!
//! Run: `cargo run --release --bin kalicut_selftest -- [videos_dir]`

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

// Same approach as the app: drive ffmpeg via subprocess for isolation.

fn resolve_videos_dir(args: &[String]) -> PathBuf {
    if let Some(p) = args.get(1) {
        return PathBuf::from(p);
    }
    if let Ok(p) = env::var("KALICUT_TEST_VIDEOS") {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    // Portable default: $HOME/Videos (or USERPROFILE on Windows)
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Videos")
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "Usage: {} [videos_dir]\n\n\
             videos_dir  optional; else $KALICUT_TEST_VIDEOS, else ~/Videos\n\
             Output temp files go under the system temp dir (kalicut-selftest/).",
            args.first().map(String::as_str).unwrap_or("kalicut_selftest")
        );
        std::process::exit(0);
    }

    let videos_dir = resolve_videos_dir(&args);
    if !videos_dir.is_dir() {
        eprintln!(
            "FAIL: video directory not found: {}\n\
             Pass a path, set KALICUT_TEST_VIDEOS, or put samples in ~/Videos.",
            videos_dir.display()
        );
        std::process::exit(1);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&videos_dir)
        .unwrap_or_else(|e| {
            eprintln!("FAIL: read {}: {e}", videos_dir.display());
            std::process::exit(1);
        })
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    let e = e.to_ascii_lowercase();
                    matches!(e.as_str(), "mp4" | "mkv" | "mov" | "webm" | "m4v")
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    if files.is_empty() {
        eprintln!("FAIL: no videos in {}", videos_dir.display());
        std::process::exit(1);
    }

    println!("=== KALICUT selftest ===");
    println!("videos: {}", videos_dir.display());
    println!("files: {}", files.len());
    println!("passes: 15 per scenario\n");

    // Unique dir per process so parallel runs don't delete each other's outputs
    let out_dir = env::temp_dir().join(format!("kalicut-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| {
        eprintln!("FAIL: create {}: {e}", out_dir.display());
        std::process::exit(1);
    });

    let mut fails = 0u32;
    let mut ok = 0u32;

    // --- 1) probe ---
    for f in &files {
        print!("PROBE {} ... ", f.file_name().unwrap().to_string_lossy());
        match probe_json(f) {
            Ok(info) => {
                println!(
                    "OK  {}s  {:?}x{:?}  v={:?} a={:?}",
                    info.duration, info.width, info.height, info.vcodec, info.acodec
                );
                ok += 1;
            }
            Err(e) => {
                println!("FAIL  {e}");
                fails += 1;
            }
        }
    }

    // --- 2) stream copy trim ×15 per file (короткий кусок) ---
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        for i in 1..=15 {
            let out = out_dir.join(format!("copy_{name}_{i}.mp4"));
            let start = 0.5 + (i as f64) * 0.07;
            print!("COPY  {name}  pass {i:02}/15  ss={start:.2} ... ");
            let t0 = Instant::now();
            match stream_copy_trim(f, &out, start, start + 0.8) {
                Ok(()) => {
                    match verify_has_video(&out) {
                        Ok(true) => {
                            println!("OK  {:.2}s", t0.elapsed().as_secs_f64());
                            ok += 1;
                        }
                        Ok(false) => {
                            println!("FAIL  no video stream in output");
                            fails += 1;
                        }
                        Err(e) => {
                            println!("FAIL  verify: {e}");
                            fails += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("FAIL  {e}");
                    fails += 1;
                }
            }
            let _ = std::fs::remove_file(&out);
        }
    }

    // --- 3) reencode presets ×15 на самом маленьком файле ---
    let small = files
        .iter()
        .min_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX))
        .unwrap();
    let sname = small.file_name().unwrap().to_string_lossy();
    let presets: &[(&str, &[&str])] = &[
        (
            "hq_h264",
            &[
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "28",
                "-pix_fmt",
                "yuv420p",
                "-an",
            ],
        ),
        (
            "web720",
            &[
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "28",
                "-vf",
                "scale=-2:720",
                "-pix_fmt",
                "yuv420p",
                "-an",
            ],
        ),
        (
            "mobile480",
            &[
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "30",
                "-vf",
                "scale=-2:480",
                "-pix_fmt",
                "yuv420p",
                "-an",
            ],
        ),
    ];

    for (pname, args) in presets {
        for i in 1..=15 {
            let out = out_dir.join(format!("re_{pname}_{i}.mp4"));
            print!("REENC {sname}  {pname}  pass {i:02}/15 ... ");
            let t0 = Instant::now();
            match reencode_trim(small, &out, 1.0, 1.5, args) {
                Ok(()) => match verify_has_video(&out) {
                    Ok(true) => {
                        println!("OK  {:.2}s", t0.elapsed().as_secs_f64());
                        ok += 1;
                    }
                    Ok(false) => {
                        println!("FAIL  no video");
                        fails += 1;
                    }
                    Err(e) => {
                        println!("FAIL  {e}");
                        fails += 1;
                    }
                },
                Err(e) => {
                    println!("FAIL  {e}");
                    fails += 1;
                }
            }
            let _ = std::fs::remove_file(&out);
        }
    }

    // --- 4) preview size resolve (логика авто) ---
    for f in &files {
        let info = match probe_json(f) {
            Ok(i) => i,
            Err(_) => continue,
        };
        for mode in ["auto", "quality", "speed"] {
            let (w, h) = resolve_preview(
                mode,
                info.width.unwrap_or(0),
                info.height.unwrap_or(0),
                info.vcodec.as_deref().unwrap_or(""),
                info.bit_rate,
            );
            print!(
                "PREV  {}  {mode:8} → {w}x{h} ... ",
                f.file_name().unwrap().to_string_lossy()
            );
            if w >= 2 && h >= 2 && w % 2 == 0 && h % 2 == 0 {
                println!("OK");
                ok += 1;
            } else {
                println!("FAIL  bad size");
                fails += 1;
            }
        }
    }

    // --- 5) libmpv load smoke (если есть) ---
    print!("MPV    init ... ");
    match libmpv_smoke(files.first().unwrap()) {
        Ok(msg) => {
            println!("OK  {msg}");
            ok += 1;
        }
        Err(e) => {
            println!("FAIL  {e}");
            fails += 1;
        }
    }

    println!("\n=== TOTAL: ok={ok}  fail={fails} ===");
    let _ = std::fs::remove_dir_all(&out_dir);
    if fails > 0 {
        std::process::exit(1);
    }
}

#[derive(Debug)]
struct Info {
    duration: f64,
    width: Option<u32>,
    height: Option<u32>,
    vcodec: Option<String>,
    acodec: Option<String>,
    bit_rate: Option<u64>,
}

fn probe_json(path: &Path) -> Result<Info, String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let duration = v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let bit_rate = v["format"]["bit_rate"]
        .as_str()
        .and_then(|s| s.parse().ok());
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams.iter().find(|s| s["codec_type"] == "video");
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");
    Ok(Info {
        duration,
        width: video.and_then(|s| s["width"].as_u64().map(|x| x as u32)),
        height: video.and_then(|s| s["height"].as_u64().map(|x| x as u32)),
        vcodec: video.and_then(|s| s["codec_name"].as_str().map(|x| x.to_string())),
        acodec: audio.and_then(|s| s["codec_name"].as_str().map(|x| x.to_string())),
        bit_rate,
    })
}

fn stream_copy_trim(input: &Path, output: &Path, start: f64, end: f64) -> Result<(), String> {
    let dur = (end - start).max(0.05);
    let out = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{start:.3}"),
            "-i",
        ])
        .arg(input)
        .args([
            "-t",
            &format!("{dur:.3}"),
            "-map",
            "0:v?",
            "-map",
            "0:a?",
            "-map",
            "0:s?",
            "-c",
            "copy",
            "-ignore_unknown",
            "-avoid_negative_ts",
            "make_zero",
            "-movflags",
            "+faststart",
        ])
        .arg(output)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn reencode_trim(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    extra: &[&str],
) -> Result<(), String> {
    let dur = (end - start).max(0.05);
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-ss", &format!("{start:.3}"), "-t", &format!("{dur:.3}")]);
    for a in extra {
        cmd.arg(a);
    }
    cmd.arg(output);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn verify_has_video(path: &Path) -> Result<bool, String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn resolve_preview(
    mode: &str,
    sw: u32,
    sh: u32,
    codec: &str,
    bit_rate: Option<u64>,
) -> (u32, u32) {
    let pixels = sw.saturating_mul(sh);
    let heavy_codec = codec.contains("hevc") || codec.contains("h265") || codec.contains("av1");
    let uhd = pixels >= 3_000_000 || sw >= 3000;
    let high_br = bit_rate.is_some_and(|b| b >= 25_000_000);
    let heavy = uhd || (heavy_codec && pixels >= 1_500_000) || high_br;
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let max_long = match mode {
        "speed" => {
            if heavy {
                640
            } else {
                854
            }
        }
        "quality" => 1920,
        _ => {
            if heavy && cores <= 4 {
                854
            } else if heavy && cores <= 8 {
                1280
            } else if heavy {
                1600
            } else if sh >= 1080 && cores >= 6 {
                1920
            } else if sh >= 720 {
                1280
            } else {
                960
            }
        }
    };
    let long = sw.max(sh).max(1);
    if long <= max_long {
        return (sw & !1, sh & !1);
    }
    let scale = max_long as f64 / long as f64;
    let w = (((sw as f64) * scale).round() as u32) & !1;
    let h = (((sh as f64) * scale).round() as u32) & !1;
    (w.max(2), h.max(2))
}

fn libmpv_smoke(path: &Path) -> Result<String, String> {
    use libmpv2::Mpv;
    let mpv = Mpv::with_initializer(|init| {
        init.set_option("vo", "null")?;
        init.set_option("ao", "null")?;
        init.set_option("terminal", "no")?;
        init.set_option("msg-level", "all=error")?;
        init.set_option("pause", true)?;
        Ok(())
    })
    .map_err(|e| format!("init: {e}"))?;
    let p = path.to_string_lossy().to_string();
    mpv.command("loadfile", &[&p, "replace"])
        .map_err(|e| format!("load: {e}"))?;
    // дать демуксу время
    std::thread::sleep(std::time::Duration::from_millis(400));
    let dur = mpv
        .get_property::<f64>("duration")
        .map_err(|e| format!("duration: {e}"))?;
    if dur <= 0.0 {
        return Err("duration=0".into());
    }
    mpv.command("seek", &["1.0", "absolute"])
        .map_err(|e| format!("seek: {e}"))?;
    Ok(format!("duration={dur:.2}s seek ok"))
}
