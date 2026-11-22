# Mihari

![Mihari](assets/logo.svg)

Mihari is a keyboard-driven image viewer and sorter built in Rust.

Prebuilt binaries for Windows (x86-64), macOS (Apple Silicon), and Linux (x86-64, glibc) are available on [GitHub Releases](https://github.com/yukimemi/mihari/releases/latest).

```powershell
cargo install mihari --locked
mihari 'D:\Pictures\Manga'
mihari 'D:\Pictures\Manga\Volume 1.cbz'
mihari --recurse 'D:\Pictures'
mihari
```

An explicit folder, image, or archive takes precedence over the startup folders in the configuration. Opening an image starts on that image and scans its containing folder. With no path, Mihari opens the configured `folders` as one album, in the order listed. With no configured folders, it opens an empty window; use `o` or `O` to open an album. To build from a checkout, use `cargo install --path . --locked`.

Images are sorted naturally within each directory (`page2` before `page10`). With recursion enabled, each directory's images come before its subdirectories, which are visited in natural order. Directory links are not followed, and overlapping roots do not repeat images. `--recurse` (`-r`) and `--no-recurse` override the configuration. The effective setting also applies to folders opened from the window.

Open a `.zip` or `.cbz` directly or with `o`. Each archive is a separate read-only album: images in nested directories are included and sorted naturally by their full path inside the archive. Pages are loaded on demand without extracting them to disk, and move shortcuts do not modify the archive. Stored and Deflate-compressed ZIP entries are supported; encrypted archives and RAR files are not. Folder scans still include only loose images.

Press `j` or `l` again on the last page or spread to ask before opening the next ZIP/CBZ in the same folder, in natural filename order. Choose **Open next file** (or press `Enter`) to start on page 1, or **Cancel** (or press `Esc`) to stay. No prompt is shown if there is no next archive, and the slideshow still loops within the current archive.

| Keys | Action |
| --- | --- |
| `h` / `k`, `j` / `l` | Previous, next |
| `gg`, `G` | First, last |
| `o`, `O` | Open an image or ZIP/CBZ archive, open a folder |
| `v`, `r` | Toggle spreads, reverse reading direction |
| `[`, `]` | Shift the current spread back or forward by one page |
| `Tab` | Select the other page in a spread (the footer shows the selected side) |
| A key from `[move]`, `u` | Move the selected image, undo the last move |
| `s`, `t` | Toggle slideshow, toggle always-on-top |
| `f`, `Esc` | Toggle fullscreen; dismiss help, exit fullscreen, or stop the slideshow |
| `?` / `F1`, `q` | Help, quit |

In spread mode, `[` and `]` change the page pairing. For example, `]` shifts pages 2–3 to 3–4, and `l` then advances to 5–6. The adjusted pairing stays in effect until you shift it again or open another album.

Moves and undos never overwrite existing files. You can undo multiple moves until you open another album or quit. The slideshow loops and restores the pre-slideshow window level when stopped. Always-on-top is not supported on Wayland.

## Configuration

The default configuration directory is `%APPDATA%\mihari` on Windows, `~/Library/Application Support/mihari` on macOS, and `$XDG_CONFIG_HOME/mihari` on Linux (falling back to `~/.config/mihari`). Mihari loads `config.toml` first, then `config.*.toml` alphabetically, and `config.local.toml` last. Scalars in later files override earlier values; `folders` arrays append. No base file is required. `--config <directory>` searches an alternate directory; `--config <file>` loads only that file and its includes.

```toml
folders = ['{{ home() }}/Pictures/Manga', '{{ home() }}/Pictures/Inbox']
recurse = false
spread = true                   # Default: false
direction = 'rtl'              # 'rtl' or 'ltr'
cover = true                    # Show the first page by itself in spread mode
preload = 4                    # Pages ahead (0–8); when enabled, also keeps up to two behind
slideshow_seconds = 5
slideshow_always_on_top = true

[vars]
sorted = '{{ home() }}/Pictures/Mihari'

[move]
1 = '{{ vars.sorted }}/keep'
2 = '{{ vars.sorted }}/review'
```

[teravars](https://github.com/yukimemi/teravars) provides `[vars]`, `system.*`, and `include`. Relative folder and move paths resolve from the config directory (or the parent directory of an explicitly selected config file). `[move]` keys are single case-sensitive ASCII letters or digits that do not conflict with built-in shortcuts. Missing destinations are created on the first move. If you don't want sorted images picked up on the next recursive scan, put their destinations outside the startup folders.

Mihari supports JPEG, JPEG XL, PNG, WebP, GIF, BMP, TIFF, ICO, and PNM. JPEG XL pages use the codestream orientation, not the Exif orientation tag. Animated images are shown as stills. The current page takes priority, and decoding and preloading happen in the background.
