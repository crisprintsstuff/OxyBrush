# RFC 0014: One Theme window

- **Status:** proposed for the maintainer; implemented in the same change for review

## Problem

Testers found the theme settings "all over the place and hard to find":

- **Preferences > Appearance** mixed interface scale, three quick presets, nine themes, accent colors, the wireframe color and opacity, corner rounding, two brush-shelf options and the viewport background in one long column.
- **View** had its own **Background** menu and a **Background colors** window, plus an **Appearance & Theme…** item that only opened the Preferences tab. An older **Theme & Appearance** window was still reachable from the command palette.
- **The presets overlapped the themes' names.** "Default Studio" was not the **Studio** theme, "Workstation" was both a layout and a preset, and the presets also changed shortcuts, navigation and the material, which a look shouldn't do.
- **The command palette** offered every theme but **Studio**.

## Proposal

One **Theme** window (**View > Theme…**, the command palette, or the button in **Preferences > Interface**) with three tabs. Changes apply at once and are remembered, as before.

| Tab | Contents |
|---|---|
| **Themes** | A gallery of the nine themes. Each card is a small picture drawn from the theme's own colors (panels, a window with text and a button, the current accent). Below the gallery are **Looks**, the old quick presets, each a theme with the accent, corners and viewport background that suit it: **Classic** (the default), **Workstation**, **Midnight**. |
| **Colors** | The accent color (eight swatches, with a picker for the custom one) and the viewport background (gradient or flat, its colors, reset). |
| **Size & shape** | Interface scale, corner rounding as four previews, and slider thickness (Thin, Normal, Thick) as three previews. |

Every tab has a footer with **Reset everything**, which goes back to the default look: Graphite, terracotta, rounded corners, 100% scale and the default background.

**Preferences > Appearance** becomes **Preferences > Interface**. It keeps the brush-shelf options (brush names, narrow shelf) and links to the Theme window.

**Looks only change how OxyBrush looks.** The old presets also set shortcuts, navigation and the material. Those stay where they're set today: the keymap presets in **Preferences > Keymap**, and the material in the material picker. "Default Studio" is renamed **Classic**, so it no longer sounds like the Studio theme. Its background is now the real default, where it had been a lighter grey.

**Wireframe settings stay in View > Wireframe**: they're a viewport overlay, not part of the theme.

## Not in this change

- **Saving and sharing custom themes** (a name, colors and shape, as a file or a code like brush presets). It's a natural next step once the window exists.
- **Editing a theme's individual colors.**

## Compatibility

The window edits the same settings as before (`theme`, `accent`, `custom_accent`, `corner_style`, `ui_scale`, the background), plus one new one, `slider_size` (`thin`, `normal` or `thick`; missing means `normal`, so older settings files load unchanged). Commands that opened the old windows (**Theme & Appearance**, **Viewport background**) now open the Theme window, on the right tab.
