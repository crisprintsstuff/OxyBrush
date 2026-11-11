# RFC 0015: Pen gestures in panels and the viewport

- **Status:** proposed for the maintainer; implemented in the same change for review

## Problem

Testers with drawing tablets "can't scroll with their pens". On Linux, and usually on macOS, a pen reaches OxyBrush as a mouse with pressure readings and no scroll wheel:

- **Panels and lists don't scroll.** egui scrolls them by dragging only for touch screens.
- **There's no press-and-hold.** Touch screens get a tooltip and a context menu on a long press; a pen gets neither, so menus need the side button.
- **The side buttons do little.** In panels they do nothing. In the viewport they follow the mouse presets, and none of those is laid out for a pen: the lower button pans by default.

On Windows with Windows Ink, winit already delivers the pen as touch, so panels scroll there. The changes below cover the other platforms and drivers.

## Proposal

A pen is recognised by its pressure: OxyBrush has seen a pressure reading within the last 1.5 s.

| Gesture | Where | What it does |
|---|---|---|
| Drag | A panel or list | Once the press moves past a click's distance (6 pt), the panel under it scrolls, unless egui gave the drag to a control (a slider, a brush tile being moved). Labels don't start a text selection while pen gestures are on. |
| Hold still | A control | After egui's longest click (0.8 s), a right-click there: it opens the control's menu, if it has one, and pins its tooltip. egui no longer counts the release as a click, so the control doesn't also activate. |
| Side button + drag | A panel or list | Scrolls it, as middle-drag already does with a mouse. A right-click that doesn't move still opens a menu. |
| Side buttons | The viewport | A new **Pen side buttons** navigation preset (Preferences > Keymap, or the button in Preferences > Pen & tablet). The lower button (right-click) orbits, Shift + it pans and Ctrl + it zooms; the upper button (middle-click) pans; Alt while orbiting snaps to 45°. |

**Preferences > Pen & tablet > Drag empty space to scroll** turns the panel gestures on:
- **With a pen** (the default);
- **Always**, also with a mouse, for pen drivers that send no pressure;
- **Never**.

It's saved as `panel_drag_scroll`.

## How

- **Drag and hold:** the app layer (`app.rs`) tracks a pen press on a panel (`PenPress`). Movement past the click distance becomes mouse-wheel events, the same mechanism middle-drag scrolling uses. A press held for 0.8 s becomes a secondary click sent to egui, marked for one frame so the tooltip helper pins the control's tip.
- **Why not pretend the pen is a touch?** egui's touch mode is tested with real touch hardware. Fed a synthetic touch next to mouse events, its scroll bars and drag areas didn't scroll reliably in tests, so the app does the two gestures itself.
- **The viewport preset** is an ordinary navigation preset (`keymap::Preset::Pen`), saved like the others.

## Not in this change

- **Pen tilt and the eraser end.** winit doesn't report them on every platform.
- **Flicks and other recognised strokes.**

## Not verified

These need real tablets to confirm: Wacom, Huion and XP-Pen, on Linux (X11/XWayland), macOS and Windows with and without Windows Ink. The logic is covered by unit tests, but no pen was used.
