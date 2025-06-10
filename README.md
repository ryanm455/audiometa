# Audio Meta
A simple to use command line tool to read audio files' metadata.
This was made very quickly so probably has many bugs.

## Usage
```
audiometa [...FILES]
```
Supports pipelining:
```
ls | audiometa
```
Supports directories:
```
audiometa . -r
```
Example of possible complex usage:
```
audiometa . -r | grep -E "(file:|avg_bitrate_kbps:)" | paste - - | sort -k4 -n
```

## Command Line Options

| Option | Short | Long | Type | Default | Description |
|--------|-------|------|------|---------|-------------|
| `files` | - | - | `Vec<PathBuf>` | - | One or more audio files (omit to read file paths from stdin) |
| `format` | `-f` | `--format` | `text`/`json`/`csv` | `text` | Output format |
| `basic` | `-b` | `--basic` | `bool` | `false` | Show only basic info (duration, bitrate, sample rate) |
| `quiet` | `-q` | `--quiet` | `bool` | `false` | Suppress error messages |
| `keep_going` | `-k` | `--keep-going` | `bool` | `false` | Continue processing other files even if one fails |
| `recursive` | `-r` | `--recursive` | `bool` | `false` | Recursive directory processing |
