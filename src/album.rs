use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::ops::Range;
use std::path::{Path, PathBuf};

pub(crate) const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "ico", "pbm", "pgm", "ppm", "pam",
    "pnm",
];

pub(crate) struct Page {
    pub(crate) id: usize,
    pub(crate) path: PathBuf,
}

struct Move {
    page: Page,
    destination: PathBuf,
    index: usize,
}

#[derive(Default)]
pub(crate) struct Album {
    pages: Vec<Page>,
    active: usize,
    spread_shifted: bool,
    history: Vec<Move>,
}

impl Album {
    pub(crate) fn open(path: &Path, recurse: bool) -> io::Result<Self> {
        let path = path.canonicalize()?;
        let kind = fs::metadata(&path)?.file_type();
        if kind.is_dir() {
            return Self::scan([Ok(path)], recurse);
        }
        if !kind.is_file() || !is_image(&path) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "expected a folder or supported image file",
            ));
        }
        let folder = path.parent().unwrap().to_path_buf();
        let mut album = Self::scan([Ok(folder)], recurse)?;
        album.active = album
            .pages
            .iter()
            .position(|page| page.path == path)
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "image not found in its folder"))?;
        Ok(album)
    }

    pub(crate) fn open_folders(folders: &[PathBuf], recurse: bool) -> io::Result<Self> {
        let roots = folders.iter().map(|path| {
            path.canonicalize()
                .map_err(|error| folder_error(path, error))
        });
        Self::scan(roots, recurse)
    }

    fn scan(
        roots: impl IntoIterator<Item = io::Result<PathBuf>>,
        recurse: bool,
    ) -> io::Result<Self> {
        let mut album = Self::default();
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        for root in roots {
            pending.push(root?);
            while let Some(folder) = pending.pop() {
                if seen.contains(&folder) {
                    continue;
                }
                album
                    .append_images(&folder, recurse, &mut pending)
                    .map_err(|error| folder_error(&folder, error))?;
                seen.insert(folder);
            }
        }
        Ok(album)
    }

    fn append_images(
        &mut self,
        folder: &Path,
        recurse: bool,
        pending: &mut Vec<PathBuf>,
    ) -> io::Result<()> {
        let first_page = self.pages.len();
        let first_dir = pending.len();
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_file() {
                let path = entry.path();
                if is_image(&path) {
                    self.pages.push(Page {
                        id: self.pages.len(),
                        path,
                    });
                }
            } else if recurse && kind.is_dir() && !kind.is_symlink() {
                pending.push(entry.path());
            }
        }
        self.pages[first_page..].sort_unstable_by(|a, b| compare_names(&a.path, &b.path));
        pending[first_dir..].sort_unstable_by(|a, b| compare_names(b, a));
        Ok(())
    }

    pub(crate) fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub(crate) fn active(&self) -> usize {
        self.active
    }

    pub(crate) fn visible(&self, spread: bool, cover: bool) -> Range<usize> {
        let len = self.pages.len();
        if len == 0 {
            return 0..0;
        }
        let active = self.active;
        let first_pair = usize::from(cover != self.spread_shifted);
        if !spread || active < first_pair {
            return active..active + 1;
        }
        let start = active - (active - first_pair) % 2;
        start..(start + 2).min(len)
    }

    pub(crate) fn shift_spread(&mut self, cover: bool, forward: bool) -> bool {
        let start = self.visible(true, cover).start;
        let start = if forward {
            (start + 1 < self.pages.len()).then_some(start + 1)
        } else {
            start.checked_sub(1)
        };
        let Some(start) = start else {
            return false;
        };
        self.spread_shifted = start % 2 != usize::from(cover);
        self.select(start);
        true
    }

    pub(crate) fn select(&mut self, index: usize) {
        self.active = index.min(self.pages.len().saturating_sub(1));
    }

    pub(crate) fn move_active(&mut self, folder: &Path) -> io::Result<()> {
        let index = self.active;
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "no active image to move"))?;
        fs::create_dir_all(folder)?;
        let destination = folder.canonicalize()?.join(page.path.file_name().unwrap());
        move_file(&page.path, &destination)?;

        let page = self.pages.remove(index);
        self.history.push(Move {
            page,
            destination,
            index,
        });
        self.select(index);
        Ok(())
    }

    pub(crate) fn undo(&mut self) -> io::Result<()> {
        let moved = self
            .history
            .last()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "no move to undo"))?;
        move_file(&moved.destination, &moved.page.path)?;

        let moved = self.history.pop().unwrap();
        self.pages.insert(moved.index, moved.page);
        self.active = moved.index;
        Ok(())
    }
}

fn folder_error(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("scanning {}: {error}", path.display()),
    )
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            EXTENSIONS
                .iter()
                .any(|image| ext.eq_ignore_ascii_case(image))
        })
}

fn compare_names(a: &Path, b: &Path) -> Ordering {
    let a = a.file_name().unwrap();
    let b = b.file_name().unwrap();
    natord::compare_ignore_case(&a.to_string_lossy(), &b.to_string_lossy()).then_with(|| a.cmp(b))
}

fn move_file(source: &Path, destination: &Path) -> io::Result<()> {
    if source == destination {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "the source is already in the destination folder",
        ));
    }
    match renamore::rename_exclusive(source, destination) {
        Err(error) if error.kind() == ErrorKind::CrossesDevices => {}
        result => return result,
    }

    let mut input = File::open(source)?;
    let mut temp = tempfile::NamedTempFile::new_in(destination.parent().unwrap())?;
    io::copy(&mut input, temp.as_file_mut())?;
    #[cfg(unix)]
    temp.as_file()
        .set_permissions(input.metadata()?.permissions())?;
    drop(input);
    temp.as_file().sync_all()?;
    drop(
        temp.persist_noclobber(destination)
            .map_err(|error| error.error)?,
    );

    // If deletion fails, leave the complete destination copy and the album unchanged.
    fs::remove_file(source).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "copied to {} but could not remove {}: {error}",
                destination.display(),
                source.display(),
            ),
        )
    })
}
