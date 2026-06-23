use std::{
    fs::{self, File, OpenOptions},
    io::{Result, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Datelike, Local, Timelike, Utc};

use crate::config::RotationTimeFormat;

pub struct LogFile {
    file: Option<File>,
    date: DateTime<Utc>,
    base_path: PathBuf,
    rotation_time: RotationTimeFormat,
}
impl LogFile {
    pub fn new(
        base_path: PathBuf,
        rotation_time: RotationTimeFormat,
        enabled: bool,
    ) -> Result<Self> {
        let date = Utc::now();

        let file = if enabled {
            Self::archive_existing_latest(&base_path)?;
            Some(Self::open_latest(&base_path)?)
        } else {
            None
        };
        Ok(Self {
            file,
            date,
            base_path,
            rotation_time,
        })
    }

    fn latest_filename(base_path: &Path) -> PathBuf {
        base_path.join("latest.log")
    }

    fn open_latest(base_path: &Path) -> Result<File> {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(Self::latest_filename(base_path))
    }

    fn archive_filename(base_path: &Path, date: DateTime<Local>) -> PathBuf {
        let base_name = date.format("%Y-%m-%d-%H%M%S");
        let mut path = base_path.join(format!("{base_name}.log"));
        let mut suffix = 1;
        while path.exists() {
            path = base_path.join(format!("{base_name}-{suffix}.log"));
            suffix += 1;
        }
        path
    }

    fn archive_existing_latest(base_path: &Path) -> Result<()> {
        let latest = Self::latest_filename(base_path);
        let Ok(metadata) = fs::metadata(&latest) else {
            return Ok(());
        };
        if metadata.len() == 0 {
            return Ok(());
        }

        let modified = metadata.modified()?;
        let archive = Self::archive_filename(base_path, modified.into());
        fs::rename(latest, archive)
    }

    fn rotate_latest(&mut self, now: DateTime<Utc>) -> Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }

        let latest = Self::latest_filename(&self.base_path);
        if fs::metadata(&latest).is_ok_and(|metadata| metadata.len() > 0) {
            let archive = Self::archive_filename(&self.base_path, DateTime::<Local>::from(now));
            fs::rename(latest, archive)?;
        }

        self.date = now;
        self.file = Some(Self::open_latest(&self.base_path)?);
        Ok(())
    }

    fn check_time(&self, now: DateTime<Utc>) -> bool {
        match self.rotation_time {
            RotationTimeFormat::None => false,
            RotationTimeFormat::Hourly => {
                self.date.hour() != now.hour()
                    || self.date.day() != now.day()
                    || self.date.month() != now.month()
                    || self.date.year() != now.year()
            }
            RotationTimeFormat::Daily => {
                self.date.day() != now.day()
                    || self.date.month() != now.month()
                    || self.date.year() != now.year()
            }
            RotationTimeFormat::Weekly => {
                now.signed_duration_since(self.date) >= chrono::TimeDelta::weeks(1)
            }
            RotationTimeFormat::Monthly => {
                self.date.month() != now.month() || self.date.year() != now.year()
            }
        }
    }

    pub fn disable(&mut self) {
        self.file = None;
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let now = Utc::now();
        if self.check_time(now) {
            self.rotate_latest(now)?;
        }
        let Some(file) = self.file.as_mut() else {
            return Ok(buf.len());
        };
        file.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        let Some(ref mut file) = self.file else {
            return Ok(());
        };
        file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::{
        env, process,
        result::Result as StdResult,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_log_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after unix epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "steel-log-file-test-{name}-{}-{nanos}",
            process::id()
        ))
    }

    #[test]
    fn weekly_rotation_uses_elapsed_time_without_day_underflow() {
        let date = Utc
            .with_ymd_and_hms(2026, 1, 31, 0, 0, 0)
            .single()
            .expect("valid date");
        let log_file = LogFile {
            file: None,
            date,
            base_path: PathBuf::new(),
            rotation_time: RotationTimeFormat::Weekly,
        };

        let next_day = Utc
            .with_ymd_and_hms(2026, 2, 1, 0, 0, 0)
            .single()
            .expect("valid date");
        let next_week = Utc
            .with_ymd_and_hms(2026, 2, 7, 0, 0, 0)
            .single()
            .expect("valid date");

        assert!(!log_file.check_time(next_day));
        assert!(log_file.check_time(next_week));
    }

    #[test]
    fn archive_filename_uses_collision_suffixes() {
        let path = test_log_dir("collision");
        fs::create_dir_all(&path).expect("test dir should be created");

        let date = Local
            .with_ymd_and_hms(2026, 6, 18, 14, 22, 3)
            .single()
            .expect("valid date");
        fs::write(path.join("2026-06-18-142203.log"), "first")
            .expect("archive placeholder should be written");
        fs::write(path.join("2026-06-18-142203-1.log"), "second")
            .expect("archive placeholder should be written");

        let archive = LogFile::archive_filename(&path, date);

        assert_eq!(
            archive.file_name().and_then(|name| name.to_str()),
            Some("2026-06-18-142203-2.log")
        );
        fs::remove_dir_all(path).expect("test dir should be removed");
    }

    #[test]
    fn startup_archives_existing_non_empty_latest() {
        let path = test_log_dir("startup");
        fs::create_dir_all(&path).expect("test dir should be created");
        let latest = path.join("latest.log");
        fs::write(&latest, "previous run").expect("latest should be written");

        LogFile::new(path.clone(), RotationTimeFormat::Daily, true).expect("log file should open");

        assert!(latest.exists());
        assert_eq!(
            fs::read_to_string(&latest).expect("latest should be readable"),
            ""
        );
        let archives = fs::read_dir(&path)
            .expect("test dir should be readable")
            .filter_map(StdResult::ok)
            .filter(|entry| entry.file_name() != "latest.log")
            .collect::<Vec<_>>();
        assert_eq!(archives.len(), 1);
        assert_eq!(
            fs::read_to_string(archives[0].path()).expect("archive should be readable"),
            "previous run"
        );

        fs::remove_dir_all(path).expect("test dir should be removed");
    }
}
