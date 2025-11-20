use bevy::log::{BoxedLayer, tracing_subscriber::Layer};
use bevy::prelude::*;
use chrono::{Local, NaiveDate};
use std::collections::HashMap;
use std::fs::{self,File};
use std::io::{self,Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
const LOG_PREFIX: &str = "app-";

pub fn custom_layer(_app: &mut App) -> Option<BoxedLayer> {
    let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let file_appender = rolling::never("logs", format!("{LOG_PREFIX}{ts}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);
    Some(
        bevy::log::tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            .with_file(true)
            .with_line_number(true)
            .with_ansi(false)
            .boxed(),
    )
}
use zip::write::FileOptions;
pub fn archive_old_logs(log_dir: &Path) -> io::Result<()> {
    let today = Local::now().date_naive();

    // date -> list of file paths for that date
    let mut by_date: HashMap<NaiveDate, Vec<PathBuf>> = HashMap::new();

    if !log_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        // Only look at .log files
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };

        // Expect format: "run-YYYY-MM-DD_HH-MM-SS.log"
        if !file_name.starts_with(LOG_PREFIX) {
            continue;
        }

        // Extract the date substring between "run-" and the first '_'
        let rest = &file_name[LOG_PREFIX.len()..];
        let date_str = match rest.split_once('_') {
            Some((d, _)) => d,
            None => continue,
        };

        let date = match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Only archive logs from *before* today
        if date < today {
            by_date.entry(date).or_default().push(path);
        }
    }

    // For each older date, zip & delete
    for (date, files) in by_date {
        if files.is_empty() {
            continue;
        }

        // archive name: logs-YYYY-MM-DD-<archive_ts>.zip
        //let ts = Local::now().format("%H-%M-%S");
        let archive_name = format!("logs-{}.zip", date);
        let archive_path = log_dir.join(archive_name);

        let zip_file = File::create(&archive_path)?;
        let mut zip = zip::ZipWriter::new(zip_file);
        let options: FileOptions<'_, ()> =
            FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        for file_path in &files {
            let name_in_zip = match file_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let data = fs::read(file_path)?;
            zip.start_file(name_in_zip, options)?;
            zip.write_all(&data)?;
        }

        zip.finish()?; // flush zip

        // Only delete originals once archive is finalized
        for file_path in &files {
            let _ = fs::remove_file(file_path);
        }
    }

    Ok(())
}
