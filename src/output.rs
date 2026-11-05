use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::cli::Options;
use crate::format::Format;
use crate::error::Error;
use crate::slug::slug_from_title;

static TEMP_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn install_interrupt_handler() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = ctrlc::set_handler(|| {
            if let Ok(mut guard) = TEMP_PATH.lock()
                && let Some(path) = guard.take()
            {
                let _ = fs::remove_file(path);
            }
            std::process::exit(130);
        });
    });
}

/// Checks that do not need the article title. `None` means the path comes from the title.
pub fn prepare_explicit_output(opts: &Options) -> Result<Option<PathBuf>, Error> {
    let Some(path) = opts.output.clone() else {
        return Ok(None);
    };
    validate_output_path(&path, opts)?;
    Ok(Some(path))
}

pub fn path_from_title(title: &str, opts: &Options) -> Result<PathBuf, Error> {
    if let Some(path) = &opts.output {
        return Ok(path.clone());
    }
    let slug = slug_from_title(title).ok_or(Error::BadSlug)?;
    let ext = agree_format(opts.format, None)?.extension();
    let path = PathBuf::from(format!("./{slug}.{ext}"));
    validate_output_path(&path, opts)?;
    Ok(path)
}

/// `--format` wins when the path has no `.pdf` or `.epub` extension.
/// Those two extensions must agree with `--format`. Omitting `--format` takes the extension.
pub fn agree_format(flag: Option<Format>, path: Option<&Path>) -> Result<Format, Error> {
    let from_path = path.and_then(known_extension);
    match (flag, from_path) {
        (Some(flag), Some(from_path)) if flag != from_path => Err(Error::Usage(format!(
            "format {} does not match output path {}",
            flag.extension(),
            path.expect("extension came from a path").display()
        ))),
        (Some(flag), _) => Ok(flag),
        (None, Some(from_path)) => Ok(from_path),
        (None, None) => Ok(Format::Epub),
    }
}

fn known_extension(path: &Path) -> Option<Format> {
    let ext = path.extension().and_then(|ext| ext.to_str())?;
    if ext.eq_ignore_ascii_case("pdf") {
        Some(Format::Pdf)
    } else if ext.eq_ignore_ascii_case("epub") {
        Some(Format::Epub)
    } else {
        None
    }
}

fn validate_output_path(path: &Path, opts: &Options) -> Result<(), Error> {
    if path_is_dash(path) {
        return Err(Error::Usage("output path '-' is not supported".to_string()));
    }
    agree_format(opts.format, Some(path))?;
    if let Some(parent) = parent_dir(path) {
        match fs::metadata(parent) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(Error::ParentNotDir(parent.display().to_string())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NoParent(parent.display().to_string()));
            }
            Err(err) => {
                return Err(Error::UnusablePath {
                    path: path.display().to_string(),
                    reason: err.to_string(),
                });
            }
        }
    }
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Err(Error::IsDir(path.display().to_string())),
        Ok(_) if !opts.force => Err(Error::Exists(path.display().to_string())),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::UnusablePath {
            path: path.display().to_string(),
            reason: err.to_string(),
        }),
    }
}

/// Unix `rename` replaces an existing file. Windows does not, so move the
/// destination aside and put it back if the new name fails.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if fs::symlink_metadata(to).is_ok() {
            return replace_via_backup(from, to);
        }
    }
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_via_backup(from: &Path, to: &Path) -> std::io::Result<()> {
    let name = to.file_name().unwrap_or_default().to_string_lossy();
    let mut backup = to.to_path_buf();
    backup.set_file_name(format!(".{name}.{}.folio-old", std::process::id()));
    fs::rename(to, &backup)?;
    if let Err(err) = fs::rename(from, to) {
        let _ = fs::rename(&backup, to);
        return Err(err);
    }
    let _ = fs::remove_file(&backup);
    Ok(())
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
        replace_file(&self.path, final_path).map_err(|err| Error::Write(err.to_string()))?;
        self.keep = true;
        if let Ok(mut guard) = TEMP_PATH.lock()
            && guard.as_ref() == Some(&self.path)
        {
            *guard = None;
        }
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
        if let Ok(mut guard) = TEMP_PATH.lock()
            && guard.as_ref() == Some(&self.path)
        {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Options;

    fn opts(format: Option<Format>, output: Option<&str>) -> Options {
        Options {
            url: "http://example.test/a".to_string(),
            format,
            output: output.map(PathBuf::from),
            force: false,
            max_pages: 15,
            verbose: false,
        }
    }

    #[test]
    fn flag_and_extension_must_agree_and_the_default_name_follows_the_format() {
        let cases = [
            (None, None, Ok(Format::Epub)),
            (None, Some("story.pdf"), Ok(Format::Pdf)),
            (None, Some("story.PDF"), Ok(Format::Pdf)),
            (None, Some("story.epub"), Ok(Format::Epub)),
            (None, Some("story.EPUB"), Ok(Format::Epub)),
            (None, Some("story.html"), Ok(Format::Epub)),
            (None, Some("story"), Ok(Format::Epub)),
            (Some(Format::Pdf), None, Ok(Format::Pdf)),
            (Some(Format::Pdf), Some("story.pdf"), Ok(Format::Pdf)),
            (Some(Format::Pdf), Some("story.PDF"), Ok(Format::Pdf)),
            (Some(Format::Pdf), Some("story.html"), Ok(Format::Pdf)),
            (Some(Format::Pdf), Some("story"), Ok(Format::Pdf)),
            (Some(Format::Epub), None, Ok(Format::Epub)),
            (Some(Format::Epub), Some("story.epub"), Ok(Format::Epub)),
            (Some(Format::Epub), Some("story.html"), Ok(Format::Epub)),
            (Some(Format::Epub), Some("story"), Ok(Format::Epub)),
            (Some(Format::Pdf), Some("story.epub"), Err(())),
            (Some(Format::Pdf), Some("story.EPUB"), Err(())),
            (Some(Format::Epub), Some("story.pdf"), Err(())),
            (Some(Format::Epub), Some("story.PDF"), Err(())),
        ];
        for (flag, path, expect) in cases {
            let path = path.map(PathBuf::from);
            let got = agree_format(flag, path.as_deref());
            match expect {
                Ok(format) => assert_eq!(got.unwrap(), format, "{flag:?} {path:?}"),
                Err(()) => assert!(got.is_err(), "{flag:?} {path:?} should disagree"),
            }
        }

        assert_eq!(
            path_from_title("Hello", &opts(Some(Format::Pdf), None)).unwrap(),
            PathBuf::from("./hello.pdf")
        );
        assert_eq!(
            path_from_title("Hello", &opts(None, None)).unwrap(),
            PathBuf::from("./hello.epub")
        );
        assert_eq!(
            path_from_title("Hello", &opts(Some(Format::Epub), None)).unwrap(),
            PathBuf::from("./hello.epub")
        );
        let mismatch = prepare_explicit_output(&opts(Some(Format::Pdf), Some("story.epub")));
        let Err(err) = mismatch else {
            panic!("a pdf flag and an epub path must exit 2");
        };
        assert_eq!(err.code(), 2);
        assert_eq!(
            err.to_string(),
            "format pdf does not match output path story.epub"
        );
    }
}
