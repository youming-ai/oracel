# oklch Theme Modernization Design

## Goal

Modernize the dashboard's color system from hex/rgba to oklch + `color-mix()`. Dark-only — no light mode. All 86 `var()` references in components remain unchanged; only the token definitions in `index.css` change.

## Scope

- Convert ~15 hex base colors to `oklch()` equivalents
- Replace 15 explicit rgba alpha tokens with `color-mix(in oklch, ...)` derivations
- Convert remaining hardcoded `rgba()` in gradients to oklch
- No component changes, no new features, no light/dark toggle

## Out of Scope

- Light mode / `light-dark()` / `color-scheme`
- `@property` registrations
- `prefers-contrast` / `forced-colors` overrides
- Multiple themes or user-customizable accent hue
- Component-level `--_` scoped tokens

## Token Architecture

The `:root` block has three sections:

### 1. Primitives (hex → oklch)

| Token | Current Hex | oklch | Role |
|-------|------------|-------|------|
| `--background` | `#0a0e17` | `oklch(10% 0.02 260)` | Deepest surface |
| `--secondary`, `--muted`, `--sidebar` | `#111827` | `oklch(14% 0.02 260)` | Secondary surface |
| `--card`, `--popover`, `--sidebar-accent` | `#1a2332` | `oklch(19% 0.02 250)` | Card surface |
| `--input`, `--sidebar-border` | `#1e2d3d` | `oklch(23% 0.02 240)` | Borders, inputs |
| `--primary`, `--ring`, `--sidebar-primary`, `--sidebar-ring`, `--chart-1` | `#00d4aa` | `oklch(77% 0.16 170)` | Accent green |
| `--primary-foreground`, `--sidebar-primary-foreground`, `--accent-foreground` | `#061b1b` | `oklch(12% 0.02 180)` | Text on accent |
| `--foreground`, `--card-foreground`, `--popover-foreground`, `--secondary-foreground`, `--sidebar-foreground`, `--sidebar-accent-foreground` | `#e8edf5` | `oklch(94% 0.01 260)` | Primary text |
| `--muted-foreground` | `#7b8ca3` | `oklch(62% 0.03 250)` | Secondary text |
| `--text-dim` | `#4a5568` | `oklch(42% 0.02 250)` | Dim text |
| `--destructive`, `--chart-2` | `#ff4757` | `oklch(64% 0.24 20)` | Loss/error red |
| `--chart-3` | `#ffa502` | `oklch(78% 0.17 70)` | Warning orange |
| `--chart-4` | `#2f7aa1` | `oklch(52% 0.08 230)` | Chart blue |
| `--chart-5` | `#6d7481` | `oklch(53% 0.01 250)` | Chart gray |

Tokens that share a hex value will share a single oklch value (e.g., `--card` and `--popover` both become `oklch(19% 0.02 250)`).

### 2. Semantic Aliases (unchanged structure)

Domain aliases continue to reference canonical tokens:

```css
--bg-primary: var(--background);
--accent: var(--primary);
--win: var(--primary);
--loss: var(--destructive);
--warn: var(--chart-3);
--text-primary: var(--foreground);
--text-secondary: var(--muted-foreground);
```

No changes here beyond what already exists.

### 3. Derived Alpha Scale (rgba → color-mix)

Replace explicit rgba tokens with `color-mix()` derivations:

| Current Token | Current Value | New Value |
|---------------|--------------|-----------|
| `--accent-dim` | `rgba(0, 212, 170, 0.15)` | `color-mix(in oklch, var(--primary) 15%, transparent)` |
| `--accent-a3` | `rgba(0, 212, 170, 0.03)` | `color-mix(in oklch, var(--primary) 3%, transparent)` |
| `--accent-a6` | `rgba(0, 212, 170, 0.06)` | `color-mix(in oklch, var(--primary) 6%, transparent)` |
| `--accent-a10` | `rgba(0, 212, 170, 0.1)` | `color-mix(in oklch, var(--primary) 10%, transparent)` |
| `--accent-a12` | `rgba(0, 212, 170, 0.12)` | `color-mix(in oklch, var(--primary) 12%, transparent)` |
| `--accent-a18` | `rgba(0, 212, 170, 0.18)` | `color-mix(in oklch, var(--primary) 18%, transparent)` |
| `--accent-a20` | `rgba(0, 212, 170, 0.2)` | `color-mix(in oklch, var(--primary) 20%, transparent)` |
| `--accent-a25` | `rgba(0, 212, 170, 0.25)` | `color-mix(in oklch, var(--primary) 25%, transparent)` |
| `--accent-a30` | `rgba(0, 212, 170, 0.3)` | `color-mix(in oklch, var(--primary) 30%, transparent)` |
| `--accent-a35` | `rgba(0, 212, 170, 0.35)` | `color-mix(in oklch, var(--primary) 35%, transparent)` |
| `--accent-a50` | `rgba(0, 212, 170, 0.5)` | `color-mix(in oklch, var(--primary) 50%, transparent)` |
| `--accent-a80` | `rgba(0, 212, 170, 0.8)` | `color-mix(in oklch, var(--primary) 80%, transparent)` |
| `--border-a15` | `rgba(30, 45, 61, 0.15)` | `color-mix(in oklch, var(--input) 15%, transparent)` |
| `--border-a35` | `rgba(30, 45, 61, 0.35)` | `color-mix(in oklch, var(--input) 35%, transparent)` |
| `--border-a50` | `rgba(30, 45, 61, 0.5)` | `color-mix(in oklch, var(--input) 50%, transparent)` |
| `--border-a60` | `rgba(30, 45, 61, 0.6)` | `color-mix(in oklch, var(--input) 60%, transparent)` |

### 4. Gradient Hardcoded Colors

Remaining raw `rgba()` values in the component styles that are not yet tokenized:

| Location | Current | Replacement |
|----------|---------|-------------|
| `.header-hud` gradient | `rgba(0, 100, 200, 0.04)` | `color-mix(in oklch, oklch(52% 0.15 250) 4%, transparent)` |
| `.header-hud` gradient | `rgba(17, 24, 39, 0.95)` | `oklch(14% 0.02 260 / 0.95)` |
| `.header-hud` gradient | `rgba(10, 14, 23, 0.98)` | `oklch(10% 0.02 260 / 0.98)` |
| `.hud-card` gradient | `rgba(26, 35, 50, 0.85)` | `oklch(19% 0.02 250 / 0.85)` |
| `.hud-card` gradient | `rgba(17, 24, 39, 0.92)` | `oklch(14% 0.02 260 / 0.92)` |
| `.header-data-module` bg | `rgba(17, 24, 39, 0.6)` | `oklch(14% 0.02 260 / 0.6)` |
| `.hud-card:hover` shadow | `rgba(0, 0, 0, 0.15)` | `oklch(0% 0 0 / 0.15)` |
| `.header-logo-mark` inset | `rgba(255, 255, 255, 0.15)` | `oklch(100% 0 0 / 0.15)` |
| Logo gradient stop | `#00a88a` | `oklch(64% 0.12 170)` |

## Visual Impact

Negligible. The oklch equivalents are perceptually matched to the current hex values. At the low-chroma, low-lightness values used in this dark theme, hex-to-oklch conversion produces near-identical results. The `color-mix()` alpha derivations may differ very slightly from rgba (oklch mixing is perceptually uniform vs rgba's linear-light mixing), but at these sub-20% alpha levels the difference is imperceptible.

## Verification

- `bun run build` must pass after each task
- Visual spot-check: header, cards, charts, and table should look identical before and after
- No component files should be modified

## Browser Compatibility

- `oklch()`: supported in all major browsers since 2023 (Chrome 111+, Firefox 113+, Safari 15.4+)
- `color-mix()`: supported since 2023 (Chrome 111+, Firefox 113+, Safari 16.2+)
- Dashboard is a personal tool, not public-facing — no legacy browser concern
