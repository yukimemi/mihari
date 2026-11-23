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

Press `j` or `l` again on the last page or spread to ask before opening the next ZIP/CBZ in the same folder, in natural filename order. Choose **Open next file** or press `Enter` after the prompt appears to start on page 1. Holding `Enter` does not confirm. Choose **Cancel** or press `Esc` to stay. No prompt is shown if there is no next archive, and the slideshow still loops within the current archive.

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

## Updates

By default, opening the viewer checks for updates in the background. After a successful check, Mihari waits 24 hours before checking again. If a newer release has a matching prebuilt binary, Mihari installs it without prompting. The viewer stays open and uses the new version on the next launch. Set `auto_update = false` in your configuration to disable this behavior.

Closing the viewer cancels any pending update requests. If executable replacement has already started, the process waits for it to finish or roll back. Startup checks use `GH_TOKEN` or `GITHUB_TOKEN` when set and otherwise check anonymously; they do not launch `gh`.

Automatic updates use GitHub release archives for both Cargo and direct installs and never rebuild from source. They skip Cargo build outputs, including explicit-target builds and custom `--target-dir` directories with Cargo's build metadata.

To check or update on demand, run these commands. On Windows, use PowerShell so you can see the result:

```powershell
mihari --check-update
mihari --self-update
```

`--check-update` compares the running version with the latest GitHub release without changing the installation. `--self-update` asks before installing a newer version. For unattended updates, use `mihari --self-update --yes`. Do not combine the update flags with each other or with image paths, `--config`, `--recurse`, or `--no-recurse`.

- **Cargo installs:** Mihari detects executables in the default `~/.cargo/bin` or in `$CARGO_HOME/bin`. For a manual `--self-update`, keep `cargo` on `PATH`: the updater builds the matching crates.io version with `cargo install` and replaces the running executable.
- **Direct installs:** The latest GitHub release must have an archive for your platform named `mihari-<target>.zip` on Windows or `mihari-<target>.tar.gz` on macOS and Linux. The archive contains `mihari.exe` or `mihari`, respectively. The updater replaces the running executable, so you need write access to its directory.
- **Development builds:** If a newer version is available, self-update refuses to replace binaries under `target/debug` or `target/release`. `--check-update` still works.

Both commands query GitHub Releases. Network errors or GitHub API rate limits can make either command fail. GitHub and crates.io publish separately, so a check can report a newer release before the matching Cargo package is available. If the Cargo update fails for that reason, retry after the package appears on crates.io.

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
auto_update = true              # Set to false to skip automatic updates

[vars]
sorted = '{{ home() }}/Pictures/Mihari'

[move]
1 = '{{ vars.sorted }}/keep'
2 = '{{ vars.sorted }}/review'
```

[teravars](https://github.com/yukimemi/teravars) provides `[vars]`, `system.*`, and `include`. Relative folder and move paths resolve from the config directory (or the parent directory of an explicitly selected config file). `[move]` keys are single case-sensitive ASCII letters or digits that do not conflict with built-in shortcuts. Missing destinations are created on the first move. If you don't want sorted images picked up on the next recursive scan, put their destinations outside the startup folders.

Mihari supports JPEG, JPEG XL, PNG, WebP, GIF, BMP, TIFF, ICO, and PNM. JPEG XL pages use the codestream orientation, not the Exif orientation tag. Animated images are shown as stills. The current page takes priority, and decoding and preloading happen in the background.
