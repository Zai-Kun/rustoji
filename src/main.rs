use expanduser::expanduser;
use serde_json;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::io::{self, Result, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::process::{Command, Stdio};

// Constants
const PNG_EMOJIS_PATH: &str = "~/assets/emojis";
const DATA_FOLDER: &str = "~/.local/share/rustoji";
const SUPPORTED_PICKERS: [&str; 2] = ["fuzzel", "bemenu"];
const UNICODE_EMOJIS_FILE_URL: &str =
    "https://raw.githubusercontent.com/Zai-Kun/rustoji/refs/heads/master/emojis.json";

fn main() -> Result<()> {
    let expanded_png_emojis_path = expanduser(PNG_EMOJIS_PATH)?;
    let expanded_data_folder_path = expanduser(DATA_FOLDER)?;

    let unicode_emojis_file_path = expanded_data_folder_path.join("emojis.json");
    let history_file_path = expanded_data_folder_path.join("history.json");

    ensure_folder_exists(&expanded_data_folder_path)?;

    if !unicode_emojis_file_path.exists() {
        fetch_unicode_emojis_file(&unicode_emojis_file_path)?;
    }

    let mut history: HashMap<String, u32> = load_json_or_default(&history_file_path)?;
    let mut sorted_history: Vec<(&String, &u32)> = history.iter().collect();
    sorted_history.sort_by(|a, b| b.1.cmp(a.1));
    let sorted_history: Vec<&String> = sorted_history.iter().map(|&(key, _)| key).collect();

    let unicode_emojis: HashMap<String, String> = load_json_or_default(&unicode_emojis_file_path)?;
    let png_emojis = collect_png_emojis_and_filter(&expanded_png_emojis_path, &sorted_history)?;

    let (picker, copy_png_emoji_path) = parse_args();

    let output = run_picker(
        &picker,
        &unicode_emojis,
        &png_emojis,
        &sorted_history,
        &expanded_png_emojis_path,
    )?;

    if output.is_empty() {
        return Ok(());
    }

    let (emoji, emoji_name) = if output.ends_with(".png") {
        (output.clone(), output.clone())
    } else {
        match output.split_once(' ') {
            Some((emoji, emoji_name)) => (emoji.to_string(), emoji_name.to_string()),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid emoji format",
                ))
            }
        }
    };

    let status_code =
        copy_emoji_to_clipboard(&emoji, &expanded_png_emojis_path, copy_png_emoji_path)?;
    notify(&format!("Copied: {}", status_code));

    *history.entry(emoji_name).or_insert(0) += 1;

    let file = fs::File::create(&history_file_path)?;
    serde_json::to_writer_pretty(file, &history)?;

    Ok(())
}

fn notify(msg: &str) {
    Command::new("notify-send")
        .args(&[msg, "-t", "1000"])
        .status()
        .unwrap();
}

fn copy_emoji_to_clipboard(
    emoji: &str,
    expanded_png_emojis_path: &PathBuf,
    copy_png_emoji_path: bool,
) -> io::Result<ExitStatus> {
    if !emoji.ends_with(".png") {
        let cmd = Command::new("wl-copy")
            .args(&[emoji, "-t", "text/plain"])
            .status()?;
        return Ok(cmd);
    }

    let emoji_path = expanded_png_emojis_path.join(emoji);
    if copy_png_emoji_path {
        let f = "file://".to_owned() + emoji_path.to_str().unwrap();
        let cmd = Command::new("wl-copy")
            .args(&[&f, "-t", "text/uri-list"])
            .status()?;
        return Ok(cmd);
    }

    let mut file = fs::File::open(emoji_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let mut child = Command::new("wl-copy")
        .args(&["-t", "image/png"])
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&buffer)?;
    }

    let status = child.wait()?;
    Ok(status)
}

fn parse_args() -> (String, bool) {
    let args: Vec<String> = env::args().collect();

    let copy_png_emoji_path = args // copy image's path instead of copying the actual image
        .get(2)
        .map_or(true, |arg| arg.to_lowercase() != "false");

    let picker = args
        .get(1)
        .filter(|picker| SUPPORTED_PICKERS.contains(&picker.as_str()))
        .map(|picker| picker.as_str())
        .unwrap_or(SUPPORTED_PICKERS[0]);

    (picker.to_string(), copy_png_emoji_path)
}

fn ensure_folder_exists(folder: &Path) -> Result<()> {
    if !folder.exists() {
        fs::create_dir_all(folder)?;
    }
    Ok(())
}

fn load_json_or_default<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    if path.exists() {
        let file_content = fs::read_to_string(path)?;
        let parsed_data: T = serde_json::from_str(&file_content)?;
        Ok(parsed_data)
    } else {
        Ok(serde_json::from_str("{}").unwrap())
    }
}

fn fetch_unicode_emojis_file(path: &Path) -> io::Result<()> {
    if UNICODE_EMOJIS_FILE_URL.is_empty() {
        eprintln!("No URL provided for fetching the emojis file.");
        return Ok(());
    }
    let status = Command::new("wget")
        .args(&[UNICODE_EMOJIS_FILE_URL, "-O", path.to_str().unwrap()])
        .status()?;

    if !status.success() {
        eprintln!("Failed to download the emojis file.");
    }
    Ok(())
}

fn collect_png_emojis_and_filter(
    path: &Path,
    emojis_to_filter_out: &Vec<&String>,
) -> io::Result<Vec<PathBuf>> {
    let mut all_png_emojis = Vec::new();
    if path.exists() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if emojis_to_filter_out.contains(&&entry.file_name().into_string().unwrap()) {
                continue;
            }
            let path = entry.path();
            if path.is_file() && path.extension() == Some(std::ffi::OsStr::new("png")) {
                all_png_emojis.push(path);
            }
        }
    }
    Ok(all_png_emojis)
}

fn run_picker(
    picker: &str,
    unicode_emojis: &HashMap<String, String>,
    png_emojis: &Vec<PathBuf>,
    sorted_history: &Vec<&String>,
    expanded_png_emojis_path: &PathBuf,
) -> io::Result<String> {
    let mut command = Command::new(picker);

    if picker == "fuzzel" {
        command.arg("--dmenu").arg("--counter");
    }

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        for emoji in sorted_history {
            if emoji.ends_with(".png") {
                let emoji_path = expanded_png_emojis_path.join(emoji);
                let to_write = format!("{}\0icon\x1f{}", emoji, emoji_path.to_str().unwrap());
                writeln!(stdin, "{to_write}")?;
            } else {
                writeln!(stdin, "{} {emoji}", unicode_emojis.get(*emoji).unwrap())?;
            }
        }

        for emoji in png_emojis {
            let file_name = emoji.file_name().unwrap().to_str().unwrap();
            let to_write = format!("{}\0icon\x1f{}", file_name, emoji.to_str().unwrap());
            writeln!(stdin, "{to_write}")?
        }

        for (emoji, value) in unicode_emojis
            .into_iter()
            .filter(|(key, _)| !sorted_history.contains(&key))
        {
            writeln!(stdin, "{} {}", value, emoji)?;
        }
    }

    let output = child.wait_with_output()?;
    let output_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(output_str)
}
