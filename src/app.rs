use std::{
    io,
    ops::Range,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align2, Color32, Context, FontId, Painter, Rect, StrokeKind, TextureHandle,
    ViewportCommand, WindowLevel, pos2, vec2,
};

use crate::{
    album::{Album, EXTENSIONS},
    config::{Config, Direction},
    input::Action,
    loader::Loader,
};

const BACKGROUND: Color32 = Color32::from_gray(18);
const TEXT: Color32 = Color32::from_gray(216);

struct Slideshow {
    deadline: Option<Instant>,
    on_top: bool,
}

pub(crate) struct Viewer {
    album: Album,
    loader: Loader,
    config: Config,
    spread: bool,
    direction: Direction,
    fullscreen: bool,
    manual_top: bool,
    slideshow: Option<Slideshow>,
    g_pending: bool,
    help: bool,
    sort_help: String,
    notice: Option<String>,
    status: String,
}

impl Viewer {
    pub(crate) fn new(ctx: Context, album: Album, config: Config) -> io::Result<Self> {
        let sort_help = if config.move_keys.is_empty() {
            "No sort keys configured  /  u  undo".to_owned()
        } else {
            let keys: Vec<_> = config.move_keys.keys().map(char::to_string).collect();
            format!("{}  move selected  /  u  undo", keys.join(", "))
        };
        let mut viewer = Self {
            album,
            loader: Loader::new(ctx.clone())?,
            spread: config.spread,
            direction: config.direction,
            config,
            fullscreen: false,
            manual_top: false,
            slideshow: None,
            g_pending: false,
            help: false,
            sort_help,
            notice: None,
            status: String::new(),
        };
        viewer.refresh_status(&ctx);
        Ok(viewer)
    }

    fn visible(&self) -> Range<usize> {
        self.album.visible(self.spread, self.config.cover)
    }

    fn on_top(&self) -> bool {
        self.slideshow
            .as_ref()
            .map_or(self.manual_top, |slide| slide.on_top)
    }

    fn apply_top(&self, ctx: &Context) {
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.on_top() {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        }));
    }

    fn stop_slideshow(&mut self, ctx: &Context) {
        if self.slideshow.take().is_some() {
            self.apply_top(ctx);
        }
    }

    fn refresh_status(&mut self, ctx: &Context) {
        use std::fmt::Write as _;

        self.status.clear();
        let total = self.album.pages().len();
        let active = self.album.active();
        if let Some(page) = self.album.pages().get(active) {
            let name = page.path.file_name().unwrap_or(page.path.as_os_str());
            ctx.send_viewport_cmd(ViewportCommand::Title(format!(
                "Mihari — {}/{} — {}",
                active + 1,
                total,
                name.to_string_lossy()
            )));
            let _ = write!(&mut self.status, "{} / {}  ·  ", active + 1, total);
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Title("Mihari".to_owned()));
        }
        self.status.push_str(match (self.spread, self.direction) {
            (false, _) => "Single",
            (true, Direction::Rtl) => "Spread RTL",
            (true, Direction::Ltr) => "Spread LTR",
        });
        if self.slideshow.is_some() {
            self.status.push_str("  ·  Slideshow");
        }
        if self.on_top() {
            self.status.push_str("  ·  On top");
        }
        self.status.push_str("  ·  ? help");
    }

    fn open(&mut self, ctx: &Context, frame: &eframe::Frame, folder: bool) -> io::Result<()> {
        self.stop_slideshow(ctx);
        let mut dialog = rfd::FileDialog::new().set_parent(frame);
        if let Some(dir) = self
            .album
            .pages()
            .get(self.album.active())
            .and_then(|page| page.path.parent())
        {
            dialog = dialog.set_directory(dir);
        }
        let path = if folder {
            dialog.set_title("Open image folder").pick_folder()
        } else {
            dialog
                .set_title("Open image")
                .add_filter("Images", EXTENSIONS)
                .pick_file()
        };
        if let Some(path) = path {
            self.album = Album::open(&path, self.config.recurse)?;
            self.loader.clear();
        }
        Ok(())
    }

    fn key(&mut self, action: Action, ctx: &Context, frame: &eframe::Frame) -> bool {
        let g_pending = std::mem::take(&mut self.g_pending);
        match action {
            Action::Quit => {
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return false;
            }
            Action::Help | Action::Escape if self.help => {
                self.help = false;
                return false;
            }
            _ if self.help => return false,
            Action::FirstPrefix if !g_pending => {
                self.g_pending = true;
                return false;
            }
            Action::Move(key) if !self.config.move_keys.contains_key(&key) => return false,
            Action::Unbound => return false,
            _ => {}
        }
        self.notice = None;
        if let Some(slide) = &mut self.slideshow {
            slide.deadline = None;
        }
        let visible = self.visible();
        match action {
            Action::Previous if visible.start > 0 => {
                self.album.select(visible.start - 1);
                self.album.select(self.visible().start);
            }
            Action::Next if visible.end < self.album.pages().len() => {
                self.album.select(visible.end)
            }
            Action::FirstPrefix => self.album.select(0),
            Action::Last => self
                .album
                .select(self.album.pages().len().saturating_sub(1)),
            Action::OpenFile | Action::OpenFolder => {
                if let Err(error) = self.open(ctx, frame, matches!(action, Action::OpenFolder)) {
                    self.notice = Some(format!("Open failed: {error}"));
                }
            }
            Action::Spread => self.spread = !self.spread,
            Action::Direction => {
                self.direction = match self.direction {
                    Direction::Rtl => Direction::Ltr,
                    Direction::Ltr => Direction::Rtl,
                };
            }
            Action::SelectOther if visible.len() == 2 => {
                self.album.select(if self.album.active() == visible.start {
                    visible.start + 1
                } else {
                    visible.start
                });
            }
            Action::Move(key) => {
                self.stop_slideshow(ctx);
                if let Some(page) = self.album.pages().get(self.album.active()) {
                    let id = page.id;
                    match self.album.move_active(&self.config.move_keys[&key]) {
                        Ok(()) => {
                            self.loader.invalidate(id);
                            self.notice = Some("Moved  ·  u to undo".to_owned());
                        }
                        Err(error) => self.notice = Some(format!("Move failed: {error}")),
                    }
                }
            }
            Action::Undo => {
                self.stop_slideshow(ctx);
                match self.album.undo() {
                    Ok(()) => {
                        self.loader
                            .invalidate(self.album.pages()[self.album.active()].id);
                        self.notice = Some("Move undone".to_owned());
                    }
                    Err(error) => self.notice = Some(format!("Undo: {error}")),
                }
            }
            Action::Slideshow if self.slideshow.is_some() => self.stop_slideshow(ctx),
            Action::Slideshow if !visible.is_empty() => {
                self.slideshow = Some(Slideshow {
                    deadline: None,
                    on_top: self.manual_top || self.config.slideshow_always_on_top,
                });
                self.apply_top(ctx);
            }
            Action::Top => {
                if let Some(slide) = &mut self.slideshow {
                    slide.on_top = !slide.on_top;
                } else {
                    self.manual_top = !self.manual_top;
                }
                self.apply_top(ctx);
            }
            Action::Fullscreen => self.set_fullscreen(ctx, !self.fullscreen),
            Action::Help => self.help = true,
            Action::Escape if self.fullscreen => self.set_fullscreen(ctx, false),
            Action::Escape => self.stop_slideshow(ctx),
            _ => return false,
        }
        true
    }

    fn set_fullscreen(&mut self, ctx: &Context, fullscreen: bool) {
        self.fullscreen = fullscreen;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(fullscreen));
    }

    fn tick(&mut self, ctx: &Context, visible: Range<usize>) -> bool {
        let Some(slide) = &mut self.slideshow else {
            return false;
        };
        let pages = self.album.pages();
        if self.help
            || visible.len() == pages.len()
            || pages[visible.clone()]
                .iter()
                .any(|page| self.loader.get(page.id).is_none())
        {
            return false;
        }
        let now = Instant::now();
        let deadline = *slide
            .deadline
            .get_or_insert_with(|| now + Duration::from_secs(self.config.slideshow_seconds));
        if deadline > now {
            ctx.request_repaint_after(deadline - now);
            return false;
        }
        slide.deadline = None;
        self.album.select(if visible.end < pages.len() {
            visible.end
        } else {
            0
        });
        ctx.request_repaint();
        true
    }

    fn paint_canvas(&self, painter: &Painter, canvas: Rect) {
        painter.rect_filled(canvas, 0., BACKGROUND);
        if self.help {
            self.paint_help(painter, canvas);
            return;
        }
        let pages = &self.album.pages()[self.visible()];
        match pages {
            [] => centered(
                painter,
                canvas,
                "No images  ·  o to open a file  ·  O to open a folder",
            ),
            [page] => self.paint_page(painter, canvas, self.loader.get(page.id), false),
            [first, second] => {
                let (left, right) = match self.direction {
                    Direction::Rtl => (second, first),
                    Direction::Ltr => (first, second),
                };
                let left_image = self.loader.get(left.id);
                let right_image = self.loader.get(right.id);
                let (left_rect, right_rect) = spread_rects(canvas, left_image, right_image);
                let active_id = self.album.pages()[self.album.active()].id;
                self.paint_page(painter, left_rect, left_image, left.id == active_id);
                self.paint_page(painter, right_rect, right_image, right.id == active_id);
            }
            _ => unreachable!("a spread has at most two pages"),
        }
    }

    fn paint_page(&self, painter: &Painter, bounds: Rect, image: CachedImage<'_>, selected: bool) {
        let rect = match image {
            Some(Ok(texture)) => {
                let size = texture.size_vec2();
                let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
                let rect = Rect::from_center_size(bounds.center(), size * scale);
                painter.image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
                    Color32::WHITE,
                );
                rect
            }
            Some(Err(error)) => {
                let clipped = painter.with_clip_rect(bounds);
                centered(&clipped, bounds, error);
                bounds
            }
            None => {
                centered(painter, bounds, "Loading…");
                bounds
            }
        };
        if selected {
            painter.rect_stroke(
                rect,
                0.,
                (2., Color32::from_rgb(105, 186, 255)),
                StrokeKind::Inside,
            );
        }
    }

    fn paint_help(&self, painter: &Painter, canvas: Rect) {
        let lines = [
            "h / k  previous    j / l  next    gg / G  first / last",
            "o  open image    O  open folder",
            "v  spread    r  reading direction    Tab  select page",
            &self.sort_help,
            "s  slideshow    t  always on top    f  fullscreen",
            "? / F1 / Esc  close help    q  quit",
        ];
        let height = 21.;
        let start = canvas.center().y - height * (lines.len() - 1) as f32 / 2.;
        for (index, line) in lines.into_iter().enumerate() {
            painter.text(
                pos2(canvas.center().x, start + index as f32 * height),
                Align2::CENTER_CENTER,
                line,
                FontId::proportional(14.),
                TEXT,
            );
        }
    }
}

impl eframe::App for Viewer {
    fn logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        let actions: Vec<_> =
            ctx.input(|input| input.events.iter().filter_map(Action::from_event).collect());
        let mut changed = false;
        for action in actions {
            changed |= self.key(action, ctx, frame);
        }
        let visible = self.visible();
        self.loader.update(
            ctx,
            self.album.pages(),
            visible.clone(),
            self.config.preload,
        );
        changed |= self.tick(ctx, visible);
        if changed {
            self.refresh_status(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        let bottom = (rect.bottom() - 26.).max(rect.top());
        let canvas = Rect::from_min_max(rect.min, pos2(rect.right(), bottom));
        let footer = Rect::from_min_max(pos2(rect.left(), bottom), rect.max);
        self.paint_canvas(&ui.painter_at(canvas), canvas);
        let painter = ui.painter_at(footer);
        painter.rect_filled(footer, 0., Color32::from_gray(29));
        painter.text(
            pos2(footer.left() + 9., footer.center().y),
            Align2::LEFT_CENTER,
            self.notice.as_deref().unwrap_or(&self.status),
            FontId::proportional(13.),
            TEXT,
        );
    }
}

type CachedImage<'a> = Option<Result<&'a TextureHandle, &'a str>>;

fn spread_rects(canvas: Rect, left: CachedImage<'_>, right: CachedImage<'_>) -> (Rect, Rect) {
    let (bounds, split) = if let (Some(Ok(left)), Some(Ok(right))) = (left, right) {
        let left_size = left.size_vec2();
        let right_size = right.size_vec2();
        let left_ratio = left_size.x / left_size.y;
        let ratio = left_ratio + right_size.x / right_size.y;
        let height = canvas.height().min(canvas.width() / ratio);
        let bounds = Rect::from_center_size(canvas.center(), vec2(ratio * height, height));
        (bounds, bounds.left() + left_ratio * height)
    } else {
        (canvas, canvas.center().x)
    };
    (
        Rect::from_min_max(bounds.min, pos2(split, bounds.bottom())),
        Rect::from_min_max(pos2(split, bounds.top()), bounds.max),
    )
}

fn centered(painter: &Painter, rect: Rect, message: &str) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        message,
        FontId::proportional(14.),
        TEXT,
    );
}
