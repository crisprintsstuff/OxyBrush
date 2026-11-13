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
  flatpak install --user OxyBrush-4.0.0-beta.6-linux-x86_64.flatpak
  flatpak run io.github.crisprintsstuff.OxyBrush
  ```
  The first install downloads the freedesktop runtime from Flathub, so you need the Flathub remote (`flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`).
- Or unpack `…-linux-x86_64.tar.gz` and run `./oxybrush`. Needs a Vulkan or OpenGL driver and glibc 2.35 or newer.

Optional: check your download against `SHA256SUMS.txt` (`sha256sum -c SHA256SUMS.txt` on Linux, `Get-FileHash` on Windows).

## 2. First ten minutes

1. **Welcome window.** Click **Sphere** (or **Start sculpting**). Pick a **Layout** there too; on a touchscreen or tablet, try **Touch friendly**. The window only appears on the first start; reopen it from **Help > Welcome screen**, and switch layouts any time in **Settings > Layout**. Try **Settings > Arrange panels** (e.g. brushes at the bottom) and drag a brush in the shelf to a new position. Tell us if anything in it is confusing. If you'd like to hear about new test builds, turn on **Settings > Updates > Tell me about new versions** (it's off by default; only GitHub is contacted).
2. **Look around.** Drag on empty space to orbit (hold **Shift** for 45° steps), scroll to zoom, right-drag to pan. Press **F** to frame the model, **F1** for all shortcuts. **Settings > Keymap** rebinds any key or the mouse (the **MMB orbit** preset, and **Ctrl carves, Alt masks**). Try **View > Focal length** (or the lens button beside the viewport): the model should stay about the same size while the perspective changes, and orthographic view (O) should ignore it. Try **View > Flat shading** and **View > Align cursor to surface** (the brush ring then lies on the clay; until you turn that on, and whenever the pointer is off the model, the ring stays flat to the screen). Load a matcap image of your own from the material picker (top left). **View > Background** (also under Settings) switches the viewport between the soft gradient and one flat color; Colors… sets those colors, and Reset to default puts the original gradient back. If you prefer starting strokes beside the model, turn on **Settings > Strokes can start on empty space** and orbit with the middle mouse button.
3. **Sculpt.** Drag on the sphere. Try brushes **1–9** and **0** (see the left shelf). Hold **Shift** to smooth, **Ctrl** to mask, **Alt** to invert (or switch **Raise / Carve** in the size/strength bar). Resize the brush with **[** and **]**. Click the brush, alpha or material preview at the top left to pick from thumbnails, and try the axis gizmo (top right of the viewport) and the drag buttons in the strip beside it. The ball-shaped button in that strip switches **shading** (ambient occlusion and cavity) on and off; tell us if it looks wrong or slows your computer down.
4. **Undo** with **Ctrl+Z**, redo with **Ctrl+Shift+Z**. Press **Ctrl+K** and type a few letters of a command (try "top view" or "remesh"), or press **Space** and type part of a brush name; Enter runs it. **F2** opens the built-in guide; search it for anything you got stuck on and tell us what was missing. Right-click the model (or hold **Q**) for a quick size/strength menu at the cursor. With **Grab** (key 0), hold **Alt** to pull the surface along its normal, and try **Grab through** in the brush settings.
5. **More resolution:** press **Ctrl+D** to add a subdivision level (a few times), sculpt some fine detail, then press **Shift+D** to go down a level and make a broad change; press **D** to go back up and check that the fine detail followed. **Remesh** in the right panel rebuilds even topology (it removes the levels).
6. **Color and posing:** pick the **Paint** brush (last in the shelf), choose a color and paint; try **Fill cavities** with a dark color. Switch the material to **Pearl** to see true colors. Then mask the lower half (Ctrl+drag on empty space), press **T** and drag the handles: only the unmasked half moves. Press **T** again for rotate and scale; **Esc** during a drag cancels it.
7. **Pieces:** in the right panel, **+ Add > Sphere** adds a piece. Sculpt one, Alt+click the other to switch, then **Merge** to fuse them.
8. **Save** with **Ctrl+S** (a `.oxb` project), and **Export** with **Ctrl+E** (glTF/GLB, OBJ, STL or PLY). Try a `.glb` in another 3D tool or game engine: each piece should arrive as its own object. In the **Pieces** menu, try **Make LOD chain** and **Split into loose parts** (on a model made of separate parts, e.g. an imported OBJ).
9. **Brush presets:** in the right panel, click **Presets…**, try a starter preset, then save your own brush and right-click it > **Copy brush code**. Paste that code into the **Paste a brush code** field on another computer (or after deleting the preset) to get the brush back.
10. **Alphas and selection:** pick an alpha (right panel > Alpha, e.g. Scales), set Stroke mode to **Stamp** and drag on the model. Try the **Stabilizer** slider for smooth lines. **Ctrl+drag** on empty space to mask a rectangle (press **L** for lasso), **Ctrl+Shift+drag** to show only part of the model.

## 3. What we most want to know

Each of these is something we couldn't check without you:

- **Does it start?** If it fails, which GPU and driver do you have? (Help > Copy system info.)
- **Pen tablets:** does pressure change brush strength (it's on by default), and does pressure → radius work when enabled? Tell us your tablet model, driver and OS (Windows Ink on or off, OpenTabletDriver, Wayland or X11).
- **Performance:** the status bar shows the triangle count and milliseconds per frame. At what triangle count does sculpting stop feeling smooth on your machine?
- **Crash recovery:** sculpt something, wait 2 minutes, then force-close OxyBrush (Task Manager / `kill`). Does it offer to recover your work on the next start?
- **Installers:** do the Start-menu entry, uninstaller and `.oxb` double-click work on Windows? Does the Flatpak show up in your app menu with its icon? On macOS, does the app start after "Open Anyway", and does it run natively (Activity Monitor > Kind: "Apple" on Apple Silicon)?
- **macOS is brand new:** any report from a real Mac (which model, which macOS version, does the GPU work, does a tablet work) is especially valuable.
- **New in 4.0.0-beta.6:** with two or more pieces, sculpt on one, then another, then press **Ctrl+Alt+Z** on the first: does only that piece's stroke come back off, and does **Ctrl+Alt+Shift+Z** put it back? Try **Preferences > General > Undo works on the active piece** so plain Ctrl+Z does it. Make a Freehand stroke, then **Edit > Last stroke > Edit as curve**: does the curve sit where you drew, can you reshape it and press Enter to apply? Also try **Use as Curve Move curve** and **Use as tube path**.
- **From 4.0.0-beta.5:** open **Mesh > Deform…** and drag a few bars (Bend, Twist, Taper, Inflate, Polish) on your own model: does the preview feel smooth, and does letting go apply it as one Undo step? Mask part of the model first: does the masked part stay put? Try **Repeat on other pieces** with two or more pieces. In the **Mask** menu, try **Mask border** on an open mesh (or after hiding part of one) and **Mask group borders** on a model with face groups. Is the new interface font easier to read, and is the text too dim or about right on your screen?
- **From 4.0.0-beta.4:** in **View > Theme… > Colors**, try a **Custom button color** and **Brush icons > Match accent**: do the colors look right in every theme, and is button text still readable? sculpt one side of a model with X symmetry off, then try **Mesh > Symmetrize > +X onto −X**: is the other side an exact copy, with no seam? Mask part of a piece and use **Mask > Split masked**: does the masked part become its own piece? Select two or more pieces, keep them visible and press **Join** in the Pieces panel: are they one piece now, looking exactly the same, and does one Undo bring them back? In the retopology tool, hold **Tab** and drag a border edge: does a new quad come out? Try the new bundled plugins in the **Plugins** menu (Group by Angle, Hide Faces by Angle, Color by Group, Fill Holes, Simplify Mesh).
- **From 4.0.0-beta.3:** open **View > Theme…**: can you find everything that used to be under Preferences > Appearance and View > Background? Try **Slider thickness > Thick**. **With a pen or tablet:** drag over empty space in the brush list or the right panel: does it scroll, while sliders still move as usual? Hold the pen still on a button for about a second: does its tooltip or menu appear? Try **Preferences > Keymap > Pen side buttons** and orbit or pan with the pen's side buttons. Please say which tablet and operating system you used.
- **From 4.0.0-beta.2:** in the retopology tool, Alt+click points to pin them, then Shift+drag to relax: do the pins stay put, and are they still pinned after saving and reopening? Drag on the axis gizmo at the top right: does it orbit, and does a click on an axis end still snap? With **Show at startup** on, double-click a `.oxb`: does it open without the Home window on top? On macOS, open OxyBrush once from Applications: do `.oxb` files then show the OxyBrush icon in Finder?
- **From 4.0.0-beta.1:** start a project with **File > New > Cube** and sculpt along its edges: is it smooth now? Capture an alpha at **2048** or **4096 px** (the size picker next to **Capture from view**): are the edges smooth, and how long does it take on your model? Save the project, reopen it: is the capture still there? Try **Export alpha…**, edit the PNG in a 2D app and load it back with **Load image…**.
- **From 3.0.8:** does an OBJ that failed with "material parse error" import now? Try **File > Close project** with and without unsaved changes. Import an STL or OBJ that used to come in huge: is it now a sensible size next to the sphere, and does one Undo give back the original size? Turn on **Mirror X** and Ctrl+drag a mask box over one side: is the other side masked too? Open a window such as **Preferences** or **Keymap**: is the title bar a sensible height now, at your interface scale? In **Preferences > Appearance > Studio**, do the solid slider handles, slightly squarer corners and lighter window shadow look right? Hover and drag a slider: the handle should turn to the accent color.
- **From 3.0.7:** on Linux with a tablet, does pen pressure change the stroke now (the log says "pen pressure is coming in" once it does)? On macOS, does double-clicking a `.oxb` file open it, and does dropping a file on the window open it? Does the window reopen where you left it? Try **Preferences > Appearance > Studio** and **Show brush names**, and an in-app plugin with a live preview (**Plugins > Group by Height**, **Group by Angle**, **Hide Faces by Angle** or **Color by Group**). **Fill Holes** and **Simplify Mesh** change triangles and can be stopped.
- **From 3.0.6:** turn on **Add detail as you sculpt** (Topology) and build a shape from a sphere: do the new triangles look even and stay under the brush, and how big can the mesh get before it lags? If you use a network drive, save a project there: does it save, or does a **Couldn't save** dialog tell you why (please send us the log either way)? Does the **Interface scale** slider behave? In the Retopology window, try **Subdivide and project** on a real sculpt.
- **From 3.0.5:** try **Mesh > Retopology (draw quads)…** on a real sculpt (a head is ideal): does drawing quads feel right, and what do you miss?
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

See the [changelog](../CHANGELOG.md) and [roadmap](ROADMAP.md). Highlights: macOS is untested on real Macs, tablet pressure is unverified, and retopology is new and tried only on simple shapes.

## Uninstalling

- **Windows:** Settings > Apps > OxyBrush > Uninstall (or delete the portable folder).
- **macOS:** drag OxyBrush from Applications to the Trash.
- **Flatpak:** `flatpak uninstall --user io.github.crisprintsstuff.OxyBrush`
- Autosaves and logs live in the folders above; delete them if you want a clean slate.
