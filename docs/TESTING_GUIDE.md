# Alpha testing guide

Thanks for helping test OxyBrush! This is an early alpha: things will break,
and that's exactly what we need to hear about. Twenty minutes of trying it
and one honest report is a real contribution.

## 1. Install

Download from the [latest release](https://github.com/crisprintsstuff/OxyBrush/releases/latest).

**macOS 11 or newer (Apple Silicon or Intel)**
- Open `OxyBrush-…-macos-universal.dmg` and drag **OxyBrush** onto **Applications**.
- The app isn't notarized by Apple yet, so the first launch is blocked. Open it once, then go to **System Settings > Privacy & Security**, scroll down and click **Open Anyway**. On macOS 14 or older you can instead right-click the app in Applications and choose **Open**.
- On macOS, use **Cmd** wherever this guide says Ctrl, and **Option** for Alt.

**Windows 10/11 (64-bit)**
- Run `OxyBrush-…-windows-x86_64-setup.exe`. It installs for your user only and doesn't need admin rights.
- Windows will warn that the installer is from an unknown publisher, because it isn't code-signed yet. Click **More info**, then **Run anyway**.
- Prefer no installer? Unzip `…-windows-x86_64-portable.zip` and run `oxybrush.exe`.

**Linux**
- Flatpak (recommended):
  ```bash
  flatpak install --user OxyBrush-0.0.1-alpha-linux-x86_64.flatpak
  flatpak run io.github.crisprintsstuff.OxyBrush
  ```
  The first install downloads the freedesktop runtime from Flathub, so you need the Flathub remote (`flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`).
- Or unpack `…-linux-x86_64.tar.gz` and run `./oxybrush`. Needs a Vulkan or OpenGL driver and glibc 2.35 or newer.

Optional: check your download against `SHA256SUMS.txt` (`sha256sum -c SHA256SUMS.txt` on Linux, `Get-FileHash` on Windows).

## 2. First ten minutes

1. **Welcome window.** Click **Sphere** (or **Start sculpting**). Pick a **Layout** there too; on a touchscreen or tablet, try **Touch friendly**. The window only appears on the first start; reopen it from **Help > Welcome screen**, and switch layouts any time in **Settings > Layout**. Try **Settings > Arrange panels** (e.g. brushes at the bottom) and drag a brush in the shelf to a new position. Tell us if anything in it is confusing.
2. **Look around.** Drag on empty space to orbit, scroll to zoom, right-drag to pan. Press **F** to frame the model, **F1** for all shortcuts.
3. **Sculpt.** Drag on the sphere. Try brushes **1–9** and **0** (see the left shelf). Hold **Shift** to smooth, **Ctrl** to mask, **Alt** to invert (or switch **Raise / Carve** in the toolbar). Resize the brush with **[** and **]**. Click the brush, alpha or material preview at the top left to pick from thumbnails, and try the axis gizmo (top right of the viewport) and the drag buttons in the strip beside it.
4. **Undo** with **Ctrl+Z**, redo with **Ctrl+Shift+Z**.
5. **More resolution:** press **Ctrl+D** to add a subdivision level (a few times), sculpt some fine detail, then press **Shift+D** to go down a level and make a broad change; press **D** to go back up and check that the fine detail followed. **Remesh** in the right panel rebuilds even topology (it removes the levels).
6. **Pieces:** in the right panel, **+ Add > Sphere** adds a piece. Sculpt one, Alt+click the other to switch, then **Merge** to fuse them.
7. **Save** with **Ctrl+S** (a `.oxb` project), and **Export** with **Ctrl+E** (OBJ or STL, e.g. for 3D printing).
8. **New in 0.1.0:** pick an alpha (right panel > Alpha, e.g. Scales), set Stroke mode to **Stamp** and drag on the model. Try the **Stabilizer** slider for smooth lines. **Ctrl+drag** on empty space to mask a rectangle (press **L** for lasso), **Ctrl+Shift+drag** to show only part of the model.

## 3. What we most want to know

Each of these is something we couldn't check without you:

- **Does it start?** If it fails, which GPU and driver do you have? (Help > Copy system info.)
- **Pen tablets:** does pressure change brush strength (it's on by default), and does pressure → radius work when enabled? Tell us your tablet model, driver and OS (Windows Ink on or off, OpenTabletDriver, Wayland or X11).
- **Performance:** the status bar shows the triangle count and milliseconds per frame. At what triangle count does sculpting stop feeling smooth on your machine?
- **Crash recovery:** sculpt something, wait 2 minutes, then force-close OxyBrush (Task Manager / `kill`). Does it offer to recover your work on the next start?
- **Installers:** do the Start-menu entry, uninstaller and `.oxb` double-click work on Windows? Does the Flatpak show up in your app menu with its icon? On macOS, does the app start after "Open Anyway", and does it run natively (Activity Monitor > Kind: "Apple" on Apple Silicon)?
- **macOS is brand new:** any report from a real Mac (which model, which macOS version, does the GPU work, does a tablet work) is especially valuable.
- **Anything confusing:** if you had to guess how something works, that's a bug in the interface.

## 4. Reporting

- **Bugs:** in the app, **Help > Report a bug…** opens a GitHub issue form with your version, OS and GPU filled in. Please include steps to reproduce and attach the log file (**Help > Open log folder**; after a crash use `oxybrush.previous.log`).
- **Impressions:** **Help > Send feedback…** — what felt good, what was frustrating, what you missed.
- Don't attach files you don't have the right to share (and never ZBrush or other proprietary project files).

Log locations if the app won't start:

| Install | Log folder |
|---|---|
| Windows | `%LOCALAPPDATA%\OxyBrush\logs` |
| macOS | `~/Library/Application Support/oxybrush/logs` |
| Linux (tarball / source) | `~/.local/share/oxybrush/logs` |
| Linux (Flatpak) | `~/.var/app/io.github.crisprintsstuff.OxyBrush/data/oxybrush/logs` |

## 5. Known limitations

See the [changelog](../CHANGELOG.md) and [roadmap](ROADMAP.md). Highlights: macOS is untested on real Macs, tablet pressure is unverified, very dense meshes decimate slowly, remesh resets masks and face groups, and brush settings aren't remembered between sessions yet (interface settings are).

## Uninstalling

- **Windows:** Settings > Apps > OxyBrush > Uninstall (or delete the portable folder).
- **macOS:** drag OxyBrush from Applications to the Trash.
- **Flatpak:** `flatpak uninstall --user io.github.crisprintsstuff.OxyBrush`
- Autosaves and logs live in the folders above; delete them if you want a clean slate.
