use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::cli::{Format, Options};
use crate::error::Error;
use crate::slug::slug_from_title;

static TEMP_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn install_interrupt_handler() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = ctrlc::set_handler(|| {
            if let Ok(mut guard) = TEMP_PATH.lock() {
                if let Some(path) = guard.take() {
                    let _ = fs::remove_file(path);
                }
            }
            std::process::exit(130);
        });
    });
}

/// Checks that do not need the article title. `None` means the path comes from the title.
pub fn prepare_explicit_output(opts: &Options) -> Result<Option<PathBuf>, Error> {
    let Some(path) = opts.output.clone() else {
        if matches!(opts.format, Some(Format::Epub) | None) {
            return Ok(None);
        }
        return Err(Error::Usage("unsupported format".to_string()));
    };
    validate_output_path(&path, opts, true)?;
    Ok(Some(path))
}

pub fn path_from_title(title: &str, opts: &Options) -> Result<PathBuf, Error> {
    if let Some(path) = &opts.output {
        return Ok(path.clone());
    }
    let slug = slug_from_title(title).ok_or(Error::BadSlug)?;
    let path = PathBuf::from(format!("./{slug}.epub"));
    validate_output_path(&path, opts, false)?;
    Ok(path)
}

fn validate_output_path(path: &Path, opts: &Options, from_flag: bool) -> Result<(), Error> {
    if path_is_dash(path) {
        return Err(Error::Usage("output path '-' is not supported".to_string()));
    }
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        if ext.eq_ignore_ascii_case("pdf") {
            return Err(Error::Usage(format!(
                "format epub does not match output path {}",
                path.display()
            )));
        }
        if from_flag && opts.format.is_none() && !ext.eq_ignore_ascii_case("epub") {
            // No extension agreement to apply. The default format stays epub.
        }
        if matches!(opts.format, Some(Format::Epub)) && ext.eq_ignore_ascii_case("pdf") {
            return Err(Error::Usage(format!(
                "format epub does not match output path {}",
                path.display()
            )));
        }
    }
    if let Some(parent) = parent_dir(path) {
        if !parent.exists() {
            return Err(Error::NoParent(parent.display().to_string()));
        }
        if !parent.is_dir() {
            return Err(Error::NoParent(parent.display().to_string()));
        }
    }
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Err(Error::IsDir(path.display().to_string())),
        Ok(_) if !opts.force => Err(Error::Exists(path.display().to_string())),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::Write(err.to_string())),
    }
}

fn path_is_dash(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn parent_dir(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => None,
        other => other,
    }
}

pub struct TempFile {
    path: PathBuf,
    keep: bool,
}

impl TempFile {
    pub fn new(final_path: &Path) -> Result<Self, Error> {
        let name = final_path.file_name().unwrap_or_default().to_string_lossy();
        let mut path = final_path.to_path_buf();
        path.set_file_name(format!(".{name}.{}.folio-tmp", std::process::id()));
        if let Ok(mut guard) = TEMP_PATH.lock() {
            *guard = Some(path.clone());
        }
        Ok(Self { path, keep: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn persist(mut self, final_path: &Path) -> Result<(), Error> {
        if let Ok(file) = File::open(&self.path) {
            let _ = file.sync_all();
        }
        fs::rename(&self.path, final_path).map_err(|err| Error::Write(err.to_string()))?;
        self.keep = true;
        if let Ok(mut guard) = TEMP_PATH.lock() {
            if guard.as_ref() == Some(&self.path) {
                *guard = None;
            }
        }
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
        if let Ok(mut guard) = TEMP_PATH.lock() {
            if guard.as_ref() == Some(&self.path) {
                *guard = None;
            }
        }
    }
}
