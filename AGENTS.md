# Project UI conventions

Before changing UI, read `DESIGN.md` and inspect an existing implementation of the same control. Preserve the application's existing visual language.

- Reuse shared classes and markup from the documented reference. Do not invent a section-specific version of a control that already exists.
- For collapsible settings and log panels, use `.ui-disclosure` and its documented children. The home page is the visual reference.
- Put shared control visuals in `src/ui.css`, theme colors in `src/styles.css`; keep feature-specific layout and behavior in feature files. Avoid ID selectors for shared control appearance.
- Use the `ui-*` primitives and variants listed in `DESIGN.md` for buttons, icon actions, fields, switches, panels, dialogs, tabs, progress, list rows and dropdowns. This includes HTML strings and DOM elements created in JavaScript. Do not recreate them with long utility-class lists or feature-prefixed visual classes.
- Use existing theme tokens, local fonts, Material Symbols, and translation keys. Support light, dark, and graphite themes, keyboard focus, disabled states, and reduced motion.
- When adding a reusable pattern, document its reference and markup in `DESIGN.md`. When changing a shared pattern, check every consumer, including the minimum supported window size.
- Do not redesign unrelated controls while making a targeted change. Report other inconsistencies separately.

