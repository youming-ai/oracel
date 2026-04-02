# oklch Theme Modernization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert all hex/rgba colors in `dashboard/src/index.css` to oklch/color-mix, keeping property names and visual output identical.

**Architecture:** Two tasks: first convert the `:root` token definitions (hex → oklch, rgba → color-mix), then convert remaining hardcoded rgba values in component gradient/shadow rules. No component files change — only the CSS token file.

**Tech Stack:** CSS oklch(), color-mix(in oklch), Tailwind CSS v4, Vite

---

## File Structure

| File | Change | Responsibility |
|------|--------|----------------|
| `dashboard/src/index.css` | **Modify** | All token definitions and component styles |

---

### Task 1: Convert `:root` tokens from hex/rgba to oklch/color-mix

**Files:**
- Modify: `dashboard/src/index.css:11-78`

- [ ] **Step 1: Replace the shadcn canonical tokens (lines 17-46) with oklch equivalents**

Replace the entire `/* ── Shadcn / Tailwind tokens (canonical) ── */` section with:

```css
  /* ── Shadcn / Tailwind tokens (canonical) ── */
  --background: oklch(10% 0.02 260);
  --foreground: oklch(94% 0.01 260);
  --card: oklch(19% 0.02 250);
  --card-foreground: oklch(94% 0.01 260);
  --popover: oklch(19% 0.02 250);
  --popover-foreground: oklch(94% 0.01 260);
  --primary: oklch(77% 0.16 170);
  --primary-foreground: oklch(12% 0.02 180);
  --secondary: oklch(14% 0.02 260);
  --secondary-foreground: oklch(94% 0.01 260);
  --muted: oklch(14% 0.02 260);
  --muted-foreground: oklch(62% 0.03 250);
  --accent-foreground: oklch(12% 0.02 180);
  --destructive: oklch(64% 0.24 20);
  --input: oklch(23% 0.02 240);
  --ring: oklch(77% 0.16 170);
  --chart-1: oklch(77% 0.16 170);
  --chart-2: oklch(64% 0.24 20);
  --chart-3: oklch(78% 0.17 70);
  --chart-4: oklch(52% 0.08 230);
  --chart-5: oklch(53% 0.01 250);
  --sidebar: oklch(14% 0.02 260);
  --sidebar-foreground: oklch(94% 0.01 260);
  --sidebar-primary: oklch(77% 0.16 170);
  --sidebar-primary-foreground: oklch(12% 0.02 180);
  --sidebar-accent: oklch(19% 0.02 250);
  --sidebar-accent-foreground: oklch(94% 0.01 260);
  --sidebar-border: oklch(23% 0.02 240);
  --sidebar-ring: oklch(77% 0.16 170);
```

- [ ] **Step 2: Replace the domain alias `--text-dim` with oklch**

Change line 60 from:
```css
  --text-dim: #4a5568;
```
to:
```css
  --text-dim: oklch(42% 0.02 250);
```

- [ ] **Step 3: Replace the alpha scale (lines 54, 62-77) with color-mix derivations**

Replace `--accent-dim` and the entire `/* ── Accent alpha scale ── */` section with:

```css
  /* ── Derived alpha scale (color-mix) ── */
  --accent-dim: color-mix(in oklch, var(--primary) 15%, transparent);
  --accent-a3: color-mix(in oklch, var(--primary) 3%, transparent);
  --accent-a6: color-mix(in oklch, var(--primary) 6%, transparent);
  --accent-a10: color-mix(in oklch, var(--primary) 10%, transparent);
  --accent-a12: color-mix(in oklch, var(--primary) 12%, transparent);
  --accent-a18: color-mix(in oklch, var(--primary) 18%, transparent);
  --accent-a20: color-mix(in oklch, var(--primary) 20%, transparent);
  --accent-a25: color-mix(in oklch, var(--primary) 25%, transparent);
  --accent-a30: color-mix(in oklch, var(--primary) 30%, transparent);
  --accent-a35: color-mix(in oklch, var(--primary) 35%, transparent);
  --accent-a50: color-mix(in oklch, var(--primary) 50%, transparent);
  --accent-a80: color-mix(in oklch, var(--primary) 80%, transparent);
  --border-a15: color-mix(in oklch, var(--input) 15%, transparent);
  --border-a35: color-mix(in oklch, var(--input) 35%, transparent);
  --border-a50: color-mix(in oklch, var(--input) 50%, transparent);
  --border-a60: color-mix(in oklch, var(--input) 60%, transparent);
```

Note: `--accent-dim` moves from the domain aliases section into the derived alpha scale section (it's an alpha derivation, not a semantic alias).

- [ ] **Step 4: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 5: Commit**

```bash
git add dashboard/src/index.css
git commit -m "refactor(css): convert token definitions from hex/rgba to oklch/color-mix"
```

---

### Task 2: Convert remaining hardcoded rgba in component styles to oklch

After Task 1, the `:root` tokens are all oklch/color-mix. But 9 raw `rgba()` values remain in the `@layer components` block (gradients, shadows, backgrounds). Convert these to oklch.

**Files:**
- Modify: `dashboard/src/index.css` (lines within `@layer components`)

- [ ] **Step 1: Convert `.header-hud` gradient colors**

Replace the `.header-hud` background (currently lines 110-113):

```css
  .header-hud {
    background:
      radial-gradient(ellipse at 15% 50%, var(--accent-a6) 0%, transparent 50%),
      radial-gradient(ellipse at 85% 50%, rgba(0, 100, 200, 0.04) 0%, transparent 50%),
      linear-gradient(180deg, rgba(17, 24, 39, 0.95) 0%, rgba(10, 14, 23, 0.98) 100%);
    border-bottom: 1px solid var(--accent-a12);
  }
```

with:

```css
  .header-hud {
    background:
      radial-gradient(ellipse at 15% 50%, var(--accent-a6) 0%, transparent 50%),
      radial-gradient(ellipse at 85% 50%, oklch(52% 0.15 250 / 0.04) 0%, transparent 50%),
      linear-gradient(180deg, oklch(14% 0.02 260 / 0.95) 0%, oklch(10% 0.02 260 / 0.98) 100%);
    border-bottom: 1px solid var(--accent-a12);
  }
```

- [ ] **Step 2: Convert `.header-logo-mark` colors**

Replace the `.header-logo-mark` background and box-shadow (currently lines 149-161):

```css
  .header-logo-mark {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    background: linear-gradient(135deg, var(--accent), #00a88a);
    border-radius: 8px;
    box-shadow:
      0 0 16px var(--accent-a30),
      inset 0 1px 0 rgba(255, 255, 255, 0.15);
    flex-shrink: 0;
  }
```

with:

```css
  .header-logo-mark {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    background: linear-gradient(135deg, var(--accent), oklch(64% 0.12 170));
    border-radius: 8px;
    box-shadow:
      0 0 16px var(--accent-a30),
      inset 0 1px 0 oklch(100% 0 0 / 0.15);
    flex-shrink: 0;
  }
```

- [ ] **Step 3: Convert `.header-data-module` background**

Replace the background in `.header-data-module` (currently line 198):

```css
    background: rgba(17, 24, 39, 0.6);
```

with:

```css
    background: oklch(14% 0.02 260 / 0.6);
```

- [ ] **Step 4: Convert `.hud-card` gradient and shadow colors**

Replace the `.hud-card` background (currently lines 255-256):

```css
    background:
      linear-gradient(135deg, rgba(26, 35, 50, 0.85), rgba(17, 24, 39, 0.92));
```

with:

```css
    background:
      linear-gradient(135deg, oklch(19% 0.02 250 / 0.85), oklch(14% 0.02 260 / 0.92));
```

Replace the `.hud-card:hover` box-shadow (currently line 286):

```css
    box-shadow: 0 2px 20px rgba(0, 0, 0, 0.15);
```

with:

```css
    box-shadow: 0 2px 20px oklch(0% 0 0 / 0.15);
```

- [ ] **Step 5: Verify no raw rgba() or hex color values remain outside :root**

Run: `cd /Users/kashue/Github/oracel && grep -nE 'rgba\(|#[0-9a-fA-F]{3,8}' dashboard/src/index.css`

Expected: Only matches inside `:root { }` (there should be zero matches since `:root` now uses oklch/color-mix). If any remain, fix them.

- [ ] **Step 6: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 7: Commit**

```bash
git add dashboard/src/index.css
git commit -m "refactor(css): convert remaining gradient/shadow rgba to oklch"
```

---

## Verification

After both tasks, run the full build and confirm zero raw color literals outside `:root`:

```bash
cd /Users/kashue/Github/oracel/dashboard && bun run build
```

Expected: Build passes. The file should contain `oklch()` and `color-mix()` for all color definitions, with `var()` references in all component rules.
