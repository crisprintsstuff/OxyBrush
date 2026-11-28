# RFC 0061 — Dockable menus

- **Status:** First implementation
- **Scope:** Materials, Pieces, Mesh, Tools, Groups, Mask, and View.

Open a supported menu and drag its heading onto the highlighted left or right
side of the window. The **Dock left** and **Dock right** buttons do the same
thing by click or keyboard. On macOS, the system menu bar exposes the dock
commands and the in-app palette supports dragging. The original menu remains
in the menu bar.

Docked menus stack as collapsible sections. When the settings inspector is
visible on the same side, its scroll area contains those sections. The
opposite side gets a scrollable tray. Move a menu to the other side or undock
it from its section. OxyBrush stores placements in application settings so
they apply to every project and survive a restart. Tab hides and restores
the trays with the rest of the panels.

Materials moves its existing Matcaps, Glass, and Fur tabs. **Glass settings…**
opens the Glass tab at the chosen location. Menu commands use the same
handlers in the dock and the menu bar, including their enabled state and undo.
