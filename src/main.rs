use clap::{Parser, ValueEnum};
use convert_case::{Case, Casing};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{self, BufRead, IsTerminal},
    path::PathBuf,
    process,
};
use symphonia::core::{
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{MetadataOptions, Tag},
    probe::Hint,
};

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Csv,
}

#[derive(Parser)]
#[command(author, version, about = "Show audio file technical metadata")]
struct Cli {
    /// One or more audio files (omit to read file paths from stdin)
    files: Vec<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Show only basic info (duration, bitrate, sample rate)
    #[arg(short, long)]
    basic: bool,

    /// Suppress error messages
    #[arg(short, long)]
    quiet: bool,

    /// Continue processing other files even if one fails
    #[arg(short = 'k', long)]
    keep_going: bool,

    /// Recursive directory processing
    #[arg(short, long)]
    recursive: bool,
}

#[derive(Debug)]
struct AudioInfo {
    file_path: String,
    sample_rate: Option<u32>,
    channels: Option<u8>,
    duration_seconds: Option<u64>,
    avg_bitrate_kbps: Option<u32>,
    tags: Vec<(String, String)>,
    file_size_bytes: u64,
    codec: Option<String>,
}

fn normalize_key(tag: &Tag) -> String {
    tag.std_key
        .map(|k| format!("{k:?}"))
        .unwrap_or_else(|| tag.key.clone())
        .to_case(Case::Snake)
}

fn collect_from_stdin() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    io::stdin()
        .lock()
        .lines()
        .map(|line| {
            let path = PathBuf::from(line?.trim());
            if path.is_file() && is_audio_file(&path) {
                Ok(Some(path))
            } else {
                if !path.exists() {
                    eprintln!("Warning: File not found: {}", path.display());
                }
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|files| files.into_iter().flatten().collect())
}

fn collect_audio_files(
    paths: &[PathBuf],
    recursive: bool,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();

    for path in paths {
        if path.is_file() {
            if is_audio_file(path) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            if recursive {
                files.extend(
                    walkdir::WalkDir::new(path)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().is_file())
                        .map(|e| e.path().to_path_buf())
                        .filter(is_audio_file),
                );
            } else {
                return Err(format!(
                    "{} is a directory (use --recursive to process directories)",
                    path.display()
                )
                .into());
            }
        } else {
            return Err(format!("File not found: {}", path.display()).into());
        }
    }

    Ok(files)
}

fn is_audio_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext_str| {
            matches!(
                ext_str.to_lowercase().as_str(),
                "mp3" | "flac" | "ogg" | "wav" | "aac" | "m4a" | "wma"
            )
        })
        .unwrap_or(false)
}

fn process_file(path: &PathBuf) -> Result<AudioInfo, Box<dyn std::error::Error>> {
    let file_size = fs::metadata(path)?.len();
    let reader = Box::new(File::open(path)?);
    let mss = MediaSourceStream::new(reader, Default::default());
    
    let mut hint = Hint::new();
    if let Some(ext_str) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext_str);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format.tracks().first().ok_or("No supported audio track")?;
    let params = &track.codec_params;

    let mut info = AudioInfo {
        file_path: path.display().to_string(),
        sample_rate: params.sample_rate,
        channels: params.channels.map(|ch| ch.count() as u8),
        duration_seconds: None,
        avg_bitrate_kbps: None,
        tags: Vec::new(),
        file_size_bytes: file_size,
        codec: Some(params.codec.to_string()),
    };

    // Calculate duration and bitrate
    if let (Some(time_base), Some(n_frames)) = (params.time_base, params.n_frames) {
        let duration = time_base.calc_time(n_frames);
        info.duration_seconds = Some(duration.seconds);
        
        let bitrate_bps = (file_size as f64 * 8.0) / (duration.seconds as f64);
        info.avg_bitrate_kbps = Some((bitrate_bps / 1_000.0) as u32);
    }

    // Collect tags
    info.tags = format
        .metadata()
        .current()
        .iter()
        .flat_map(|m| m.tags())
        .map(|tag| (normalize_key(tag), tag.value.to_string()))
        .collect();

    Ok(info)
}

fn output_text(infos: &[AudioInfo], basic_only: bool) {
    for (i, info) in infos.iter().enumerate() {
        if i > 0 {
            println!();
        }

        println!("file: {}", info.file_path);

        if let Some(codec) = &info.codec {
            println!("codec: {codec}");
        }

        if let Some(sr) = info.sample_rate {
            println!("sample_rate: {sr}");
        }

        if let Some(ch) = info.channels {
            println!("channels: {ch}");
        }

        match info.duration_seconds {
            Some(duration) => println!("duration: {duration:.2}s"),
            None => println!("duration: unknown"),
        }

        if let Some(bitrate) = info.avg_bitrate_kbps {
            println!("avg_bitrate_kbps: {bitrate}");
        }

        println!("file_size_bytes: {}", info.file_size_bytes);

        if !basic_only {
            for (key, value) in &info.tags {
                println!("{key}: {value}");
            }
        }
    }
}

fn output_json(infos: &[AudioInfo]) {
    let json_output = json!(infos
        .iter()
        .map(|info| json!({
            "file_path": info.file_path,
            "codec": info.codec,
            "sample_rate": info.sample_rate,
            "channels": info.channels,
            "duration_seconds": info.duration_seconds,
            "avg_bitrate_kbps": info.avg_bitrate_kbps,
            "file_size_bytes": info.file_size_bytes,
            "tags": info.tags.iter().cloned().collect::<std::collections::HashMap<_, _>>()
        }))
        .collect::<Vec<_>>());

    println!("{}", serde_json::to_string_pretty(&json_output).unwrap());
}

fn output_csv(infos: &[AudioInfo]) {
    println!("file_path,codec,sample_rate,channels,duration_seconds,avg_bitrate_kbps,file_size_bytes");
    
    for info in infos {
        println!(
            "{},{},{},{},{},{},{}",
            info.file_path,
            info.codec.as_deref().unwrap_or(""),
            info.sample_rate.map_or(String::new(), |v| v.to_string()),
            info.channels.map_or(String::new(), |v| v.to_string()),
            info.duration_seconds.map_or(String::new(), |v| format!("{v:.2}")),
            info.avg_bitrate_kbps.map_or(String::new(), |v| v.to_string()),
            info.file_size_bytes,
        );
    }
}

fn main() {
    let cli = Cli::parse();

    let use_stdin = cli.files.is_empty();
    
    if use_stdin && io::stdin().is_terminal() {
        if !cli.quiet {
            eprintln!("Error: No files provided and no data available on stdin");
        }
        process::exit(1);
    }

    let files = if use_stdin {
        collect_from_stdin()
    } else {
        collect_audio_files(&cli.files, cli.recursive)
    };

    let files = match files {
        Ok(files) => files,
        Err(e) => {
            if !cli.quiet {
                eprintln!("Error: {e}");
            }
            process::exit(1);
        }
    };

    if files.is_empty() {
        if !cli.quiet {
            eprintln!("Error: No audio files found");
        }
        process::exit(1);
    }

    let mut results = Vec::new();
    let mut had_errors = false;

    for file in &files {
        match process_file(file) {
            Ok(info) => results.push(info),
            Err(e) => {
                had_errors = true;
                if !cli.quiet {
                    eprintln!("Error with {}: {e}", file.display());
                }
                if !cli.keep_going {
                    process::exit(1);
                }
            }
        }
    }

    if !results.is_empty() {
        match cli.format {
            OutputFormat::Text => output_text(&results, cli.basic),
            OutputFormat::Json => output_json(&results),
            OutputFormat::Csv => output_csv(&results),
        }
    }

    if had_errors {
        process::exit(1);
    }
}