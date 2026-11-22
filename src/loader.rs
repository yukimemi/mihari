use std::{
    collections::HashMap,
    io::{self, BufRead, Cursor, Seek},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread,
};

use eframe::egui;
use image::{
    DynamicImage, ImageDecoder, ImageError, ImageReader, Limits,
    error::{ParameterError, ParameterErrorKind},
};
use jxl_oxide::integration::JxlDecoder;

use crate::album::{Album, PageSource};

const MAX_TEXTURE_SIDE: usize = 4096;
const MAX_TEXTURE_PIXELS: u64 = 8 * 1024 * 1024;

struct Job {
    source: PageSource,
    jpeg_xl: bool,
    max_side: u32,
}

type Decoded = Result<egui::ColorImage, String>;

struct Cached {
    texture: Result<egui::TextureHandle, String>,
    keep: bool,
}

pub(crate) struct Loader {
    jobs: SyncSender<Job>,
    completed: Receiver<Option<Decoded>>,
    // Keep at most one outstanding job, even after cancellation, until the worker acknowledges it.
    pending: Option<usize>,
    cancelled: Arc<AtomicBool>,
    cache: HashMap<usize, Cached>,
    stopped: bool,
}

impl Loader {
    pub(crate) fn new(ctx: egui::Context) -> io::Result<Self> {
        let (jobs, work) = mpsc::sync_channel::<Job>(1);
        let (done, completed) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("mihari-image-loader".into())
            .spawn(move || {
                while let Ok(job) = work.recv() {
                    let result = (!worker_cancelled.load(Ordering::Relaxed))
                        .then(|| decode(job).map_err(|err| err.to_string()));
                    if done.send(result).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })?;
        drop(worker);
        Ok(Self {
            jobs,
            completed,
            pending: None,
            cancelled,
            cache: HashMap::new(),
            stopped: false,
        })
    }

    pub(crate) fn update(
        &mut self,
        ctx: &egui::Context,
        album: &Album,
        visible: Range<usize>,
        preload: usize,
    ) {
        let pages = album.pages();
        let start = visible.start.min(pages.len());
        let end = visible.end.min(pages.len()).max(start);
        let (behind, ahead) = if start < end && preload > 0 {
            (
                start.saturating_sub(2),
                end.saturating_add(preload).min(pages.len()),
            )
        } else {
            (start, end)
        };

        let mut keep_pending = false;
        for page in &pages[behind..ahead] {
            if let Some(entry) = self.cache.get_mut(&page.id) {
                entry.keep = true;
            }
            keep_pending |= self.pending == Some(page.id);
        }
        self.cache
            .retain(|_, entry| std::mem::take(&mut entry.keep));

        let max_side = ctx
            .input(|input| input.max_texture_side)
            .min(MAX_TEXTURE_SIDE);
        if let Some(id) = self.pending {
            if !keep_pending {
                self.cancelled.store(true, Ordering::Relaxed);
            }
            match self.completed.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    if !self.cancelled.load(Ordering::Relaxed)
                        && let Some(result) = result
                        && !result.as_ref().is_ok_and(|image| {
                            image.width() > max_side || image.height() > max_side
                        })
                    {
                        let texture = result.map(|image| {
                            ctx.load_texture(
                                format!("page-{id}"),
                                image,
                                egui::TextureOptions::LINEAR,
                            )
                        });
                        self.cache.insert(
                            id,
                            Cached {
                                texture,
                                keep: false,
                            },
                        );
                    }
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.stopped = true;
                }
            }
        }

        if self.stopped || max_side == 0 {
            return;
        }
        if let Some(page) = pages[start..ahead]
            .iter()
            .chain(pages[behind..start].iter().rev())
            .find(|page| !self.cache.contains_key(&page.id))
        {
            self.cancelled.store(false, Ordering::Relaxed);
            let job = Job {
                source: album.source(page),
                jpeg_xl: page
                    .path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jxl")),
                max_side: max_side as u32,
            };
            if self.jobs.try_send(job).is_ok() {
                self.pending = Some(page.id);
            } else {
                self.stopped = true;
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.cache.clear();
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn invalidate(&mut self, id: usize) {
        self.cache.remove(&id);
        if self.pending == Some(id) {
            self.cancelled.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn get(&self, id: usize) -> Option<Result<&egui::TextureHandle, &str>> {
        if let Some(entry) = self.cache.get(&id) {
            Some(entry.texture.as_ref().map_err(String::as_str))
        } else if self.stopped {
            Some(Err("Image decoder stopped"))
        } else {
            None
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        // Closing the sender wakes an idle worker. An active decode exits without joining here.
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn decode(job: Job) -> image::ImageResult<egui::ColorImage> {
    match job.source {
        PageSource::File(path) => {
            decode_reader(ImageReader::open(path)?, job.jpeg_xl, job.max_side)
        }
        PageSource::Archive { archive, index } => {
            let bytes = archive.read_entry(index)?;
            decode_reader(
                ImageReader::new(Cursor::new(bytes)),
                job.jpeg_xl,
                job.max_side,
            )
        }
    }
}

fn decode_reader(
    reader: ImageReader<impl BufRead + Seek>,
    jpeg_xl: bool,
    max_side: u32,
) -> image::ImageResult<egui::ColorImage> {
    if jpeg_xl {
        decode_image(JxlDecoder::new(reader.into_inner())?, max_side)
    } else {
        decode_image(reader.with_guessed_format()?.into_decoder()?, max_side)
    }
}

fn decode_image(
    mut decoder: impl ImageDecoder,
    max_side: u32,
) -> image::ImageResult<egui::ColorImage> {
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(ImageError::Parameter(ParameterError::from_kind(
            ParameterErrorKind::DimensionMismatch,
        )));
    }
    // Match ImageReader::decode's allocation accounting while retaining access to metadata.
    let mut limits = Limits::default();
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;

    // Both limits are invariant under rotation, so reduce the buffer before transforming it.
    let pixels = u64::from(width) * u64::from(height);
    if width > max_side || height > max_side || pixels > MAX_TEXTURE_PIXELS {
        let scale = (f64::from(max_side) / f64::from(width.max(height)))
            .min((MAX_TEXTURE_PIXELS as f64 / pixels as f64).sqrt());
        let scaled = |side| (f64::from(side) * scale).floor().max(1.0) as u32;
        image = image.thumbnail_exact(scaled(width), scaled(height));
    }
    image.apply_orientation(orientation);

    let size = [image.width() as usize, image.height() as usize];
    Ok(match image {
        DynamicImage::ImageRgb8(buffer) => egui::ColorImage::from_rgb(size, buffer.as_raw()),
        DynamicImage::ImageRgba8(buffer) => {
            egui::ColorImage::from_rgba_unmultiplied(size, buffer.as_raw())
        }
        DynamicImage::ImageLuma8(buffer) => egui::ColorImage::from_gray(size, buffer.as_raw()),
        image => egui::ColorImage::from_rgba_unmultiplied(size, image.into_rgba8().as_raw()),
    })
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::*;

    #[test]
    fn archive_pages_follow_natural_path_order_and_cannot_be_moved() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("漫画.CBZ");
        let mut zip = ZipWriter::new(File::create(&path)?);
        for (name, rgb, method) in [
            ("章10/page2.ppm", [0, 0, 255], CompressionMethod::Stored),
            ("章2/page10.ppm", [0, 255, 0], CompressionMethod::Deflated),
            ("__MACOSX/page1.ppm", [0, 0, 0], CompressionMethod::Stored),
            ("章2/._page2.ppm", [0, 0, 0], CompressionMethod::Stored),
            ("章2/page2.PPM", [255, 0, 0], CompressionMethod::Stored),
        ] {
            let options = SimpleFileOptions::default().compression_method(method);
            zip.start_file(name, options)?;
            zip.write_all(b"P6\n1 1\n255\n")?;
            zip.write_all(&rgb)?;
        }
        zip.start_file("ComicInfo.xml", SimpleFileOptions::default())?;
        zip.write_all(b"<ComicInfo/>")?;
        drop(zip.finish()?);

        let mut album = Album::open(&path, false)?;
        let destination = dir.path().join("sorted");
        assert_eq!(
            album.move_active(&destination).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!destination.exists());

        let pages = album
            .pages()
            .iter()
            .map(|page| {
                let image = decode(Job {
                    source: album.source(page),
                    jpeg_xl: false,
                    max_side: 4096,
                })?;
                Ok((
                    page.path.file_name().unwrap().to_str().unwrap(),
                    image.pixels[0],
                ))
            })
            .collect::<image::ImageResult<Vec<_>>>()?;
        assert_eq!(
            pages,
            [
                ("page2.PPM", egui::Color32::RED),
                ("page10.ppm", egui::Color32::GREEN),
                ("page2.ppm", egui::Color32::BLUE),
            ]
        );
        Ok(())
    }
}
