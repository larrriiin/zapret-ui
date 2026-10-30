# UI conventions

This is an operating desktop application. Keep controls consistent across sections. The existing home page is the visual reference; preserve its appearance when extracting shared patterns.

## Sources of truth

- Theme colors: `src/styles.css` (`@theme` and theme overrides).
- Shared controls and geometry tokens: `src/ui.css`, imported by `src/styles.css`. Change shared appearance there, then check all consumers.
- Fonts: locally bundled Inter (`font-headline` / `--font-headline`).
- Icons: bundled Material Symbols Outlined; decorative icons use `aria-hidden="true"`.
- Text: `src/i18n/ru.js` and `src/i18n/en.js`; use `data-i18n` or the translation helper.
- Feature styles own layout, spacing between components, and domain-specific state. Do not copy a visual skin into a feature selector or a long Tailwind class list.

## Shared controls

| Pattern | Classes | Notes |
| --- | --- | --- |
| Regular action | `ui-button` | Outlined action, shared 40px minimum height, focus, disabled and busy states |
| Main action | `ui-button ui-button-primary` | Save, apply, confirm |
| Accent action | `ui-button ui-button-accent` | Existing secondary-color actions |
| Destructive action | `ui-button ui-button-danger` | Destructive confirmation; do not use for ordinary cancellation |
| Tertiary action | `ui-button ui-button-tertiary` | Existing IP exclusion accent |
| Compact action | `ui-button ui-button-sm` | Dense toolbars and editors |
| Icon action | `ui-icon-button` | Accessible name required; optional `ui-icon-button-sm` or `ui-tone-danger` |
| Text/number/password input | `ui-field` | Width may be set by its parent layout |
| Textarea | `ui-field ui-field-multiline` | Optional `font-mono` for technical content |
| Search with leading icon | `ui-field ui-field-search` | Reserves room for the icon |
| Checkbox/radio | `ui-check` | Theme previews and invisible switch inputs retain their own wrappers |
| Switch | `ui-switch` | Native checkbox track uses `is-on`/`is-off`; button uses `data-enabled`; optional `ui-switch-sm` |
| Select | `ui-select-trigger`, `ui-select-popup`, `ui-select-option` | Shared skin; keep feature selection/search behavior; selected option uses `dropdown-option-selected` |
| Tab/filter | `ui-tab` | Active state via `aria-selected` or `aria-pressed` |
| Tab content transition | `transitionTabContent` from `src/lib/tab-transition.js` | Fade out for 100ms, switch panels, fade in for 180ms; latest selection wins; instant with reduced motion |
| Panel | `ui-panel` | Add layout/padding utilities, e.g. `p-6`; shared frame and theme surface |
| Native dialog | `ui-dialog` | Shared popup surface and backdrop |
| Existing overlay dialog | `ui-dialog-surface` | Apply to the inner panel; preserve outer visibility, stacking and behavior |
| Overlay backdrop | `ui-overlay` | Apply to the existing outer overlay; keep its visibility and stacking classes |
| Progress bar | `ui-progress` | Keep the existing ID, value and accessible label |
| User list row | `ui-list-row` | Shared row treatment, with an icon action for deletion |

Geometry tokens: `--ui-radius-control`, `--ui-radius-panel`, `--ui-control-height`, `--ui-border`, `--ui-focus`, `--ui-duration`. Colors always resolve through the current theme; never bake a dark-theme color into a component.

Shared rules live in Tailwind's `components` layer so layout and state utilities such as `hidden`, `w-full`, and `flex-1` remain effective. Native `hidden` attributes must always hide their element, including buttons and switches.

Connection actions use `ui-button-connect` / `ui-button-status` in addition to the base button. They retain their prominent size and runtime state. The window titlebar, navigation rail, theme previews, charts, diagnostics reports, and onboarding window composition have distinct roles; share primitives inside them without forcing their layouts to match.

Do not override shared height, radius, font or padding just because a control lives in WARP or Telegram. Add a named reusable variant if an actual density or semantic difference is necessary. Preserve IDs and behavior hooks when replacing visual classes.

## Collapsible panels

Reference: `#advanced-settings` in `src/components/sections/home.html`.
Shared implementation: `.ui-disclosure*` in `src/ui.css`.
Consumers: home advanced settings, Telegram advanced settings and logs, WARP advanced settings.

```html
<details class="ui-disclosure glass-panel">
  <summary class="ui-disclosure-summary">
    <span class="ui-disclosure-heading">
      <span class="material-symbols-outlined" aria-hidden="true">tune</span>
      <span>
        <span class="ui-disclosure-title" data-i18n="advanced_settings"></span>
        <!-- Optional: a short, feature-specific description. -->
        <span class="ui-disclosure-description" data-i18n="advanced_settings_desc"></span>
      </span>
    </span>
    <span class="material-symbols-outlined ui-disclosure-chevron" aria-hidden="true">expand_more</span>
  </summary>
  <!-- Feature content; retain existing IDs and behavior. -->
</details>
```

The shared classes own the panel frame, header padding, typography, icon colors, hover, focus, and open arrow rotation. Descriptions are optional; use feature-specific text. Keep native `details`/`summary` keyboard interaction. Do not add `data-i18n` to the whole summary, because translation would replace its children.

Feature content may have its own layout. Compact disclosures inside a modal (for example, wizard help) have a different role and need not use a full settings panel.

Check closed and open states, keyboard toggling, theme variants, long translated labels, and the minimum supported window size after changes.

## Rule cards and action menus

Reference: `src/features/automations.js` and `src/features/automations-interactions.js`.
`ui-icon-tile` is a 48px decorative icon container using the current theme accent. Use a bundled Material Symbol with `aria-hidden="true"` inside it. Icon pickers use `ui-tab` buttons with an accessible name and `aria-pressed`.

Rule action menus reuse `ui-select-popup` and `ui-select-option` surfaces with menu/menuitem roles. An `ui-icon-button` trigger has `aria-haspopup="menu"`, `aria-expanded` and `aria-controls`. Support arrow keys, Home/End, Escape, outside clicks and dismissal on scrolling. Keep popup placement in behavior code.

Reordering uses an `ui-icon-button` handle with pointer capture and Up/Down/Home/End keyboard alternatives. Persist only on drop, announce the new position in a live region, and restore the original order on Escape, pointer cancellation or save failure.

## Application identity

Use the ZAPRET UI name without a decorative brand symbol in the main application header and setup header. Combine the application identity and window controls in one header; there is no separate titlebar strip. The navigation rail begins with Home, without a logo or reserved logo space.

## Application shell

Reference: `#app-header` and `#app-sidebar` in `src/index.html`; shell layout is in `src/styles.css`. The unified header starts 8px from the top/left/right and is 64px high. It contains the application name/status, version/update controls, and the three window buttons in one row. Use `ui-icon-button` for window buttons and preserve their IDs. The transparent header and brand/status children are native drag regions; buttons are excluded. Do not add a header background or backdrop blur. The rail starts below the header at 84px, leaving a 12px gap, and ends 12px above the bottom. The rail and scrollable content share `--app-content-top` (84px). Content begins 80px from the left and is clipped below the header. Page wrappers have horizontal/bottom padding only, so their first block starts level with the rail, including the Zapret settings wrapper. Check all navigation entries and native button actions at 900×750.

The soft background accents live in a single fixed `.app-backdrop` under the isolated body. Keep the main scroll container and page roots transparent so the backdrop continues from the header into every page without an opaque seam. Individual cards retain their own surfaces. Text-only dialog dismissal uses `ui-button` in a right-aligned footer; `ui-icon-button` is reserved for an actual icon, never a translated Close label. References: `info-help.html`, `status.html`, and the WARP information dialog footer.

An active automation uses `ui-panel ui-panel-active`: the normal panel border becomes the semantic `--color-success` green, without changing its width, background or adding a glow. The active status remains screen-reader text, not a visible label. Disabled rules and disabled observation use the ordinary border.

Automation creation/editing uses the native `#auto-editor` dialog, with `ui-dialog ui-dialog-wide` (760px maximum width), not a disclosure in the page. Put validation errors inside the dialog. Pass the dialog as `createWarpDropdown().init(dialog)`'s popup host so options remain interactive in the native modal top layer; the default host for other consumers is body. Closing cancels the draft, preserves saved rules and restores focus to the initiating action.
