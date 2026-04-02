# CSS Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 6 highest-impact issues from the CSS audit — accessibility gaps, redundant color literals, dead code, and missing cascade layers — raising the overall score from 5.0 to ~7.5.

**Architecture:** All changes are in a single file (`dashboard/src/index.css`). Tasks are ordered by impact: accessibility first (critical), then redundancy/architecture, then cleanup. Tailwind v4 is used via `@tailwindcss/vite` plugin with no separate config file. Custom properties live in `:root`. No CSS test infrastructure exists, so verification is `bun run build` (Vite/Tailwind compilation) after each task.

**Tech Stack:** Tailwind CSS v4, Vite, CSS custom properties, `@layer`

---

## File Structure

| File | Change | Responsibility |
|------|--------|----------------|
| `dashboard/src/index.css` | **Modify** | All custom CSS — tokens, components, utilities, animations |

---

### Task 1: Add `prefers-reduced-motion` for the pulse animation

**Files:**
- Modify: `dashboard/src/index.css:398-410`

- [ ] **Step 1: Add the reduced-motion media query after the `@keyframes pulse` block**

At the end of `index.css` (after line 410), add:

```css
@media (prefers-reduced-motion: reduce) {
  .pulse-dot {
    animation: none;
  }
}
```

- [ ] **Step 2: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/index.css
git commit -m "fix(css): respect prefers-reduced-motion for pulse animation"
```

---

### Task 2: Add `:focus-visible` styles to interactive custom elements

**Files:**
- Modify: `dashboard/src/index.css:290-311` (filter-chip)
- Modify: `dashboard/src/index.css:180-197` (header-data-module)
- Modify: `dashboard/src/index.css:368-375` (trade-row)

- [ ] **Step 1: Add focus-visible to `.filter-chip`**

After the `.filter-chip:hover` block (after line 305), add:

```css
.filter-chip:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
  color: var(--text-secondary);
  background: rgba(30, 45, 61, 0.5);
}
```

- [ ] **Step 2: Add focus-visible to `.header-data-module`**

After the `.header-data-module:hover` block (after line 201), add:

```css
.header-data-module:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  border-color: rgba(0, 212, 170, 0.2);
}
```

- [ ] **Step 3: Add focus-visible to `.trade-row`**

After the `.trade-row:hover` block (after line 375), add:

```css
.trade-row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  background: rgba(0, 212, 170, 0.03);
}
```

- [ ] **Step 4: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 5: Commit**

```bash
git add dashboard/src/index.css
git commit -m "fix(css): add focus-visible styles for keyboard navigation"
```

---

### Task 3: Extract repeated `rgba()` colors into custom properties

The accent color `rgba(0, 212, 170, ...)` appears 24 times and border color `rgba(30, 45, 61, ...)` appears 6 times with varying alpha values. Extract the most common alpha variants into custom properties.

**Files:**
- Modify: `dashboard/src/index.css:11-59` (`:root` block)
- Modify: `dashboard/src/index.css` (all `rgba()` references)

- [ ] **Step 1: Add alpha-variant custom properties to `:root`**

After `--accent-dim: rgba(0, 212, 170, 0.15);` (line 22), add the following new properties:

```css
  --accent-a3: rgba(0, 212, 170, 0.03);
  --accent-a6: rgba(0, 212, 170, 0.06);
  --accent-a10: rgba(0, 212, 170, 0.1);
  --accent-a12: rgba(0, 212, 170, 0.12);
  --accent-a18: rgba(0, 212, 170, 0.18);
  --accent-a20: rgba(0, 212, 170, 0.2);
  --accent-a25: rgba(0, 212, 170, 0.25);
  --accent-a30: rgba(0, 212, 170, 0.3);
  --accent-a35: rgba(0, 212, 170, 0.35);
  --accent-a40: rgba(0, 212, 170, 0.4);
  --accent-a50: rgba(0, 212, 170, 0.5);
  --accent-a80: rgba(0, 212, 170, 0.8);
  --border-a15: rgba(30, 45, 61, 0.15);
  --border-a35: rgba(30, 45, 61, 0.35);
  --border-a50: rgba(30, 45, 61, 0.5);
  --border-a60: rgba(30, 45, 61, 0.6);
```

- [ ] **Step 2: Replace all `rgba(0, 212, 170, ...)` occurrences with the matching custom property**

Apply these replacements throughout the file (outside the `:root` block):

| Old value | New value |
|-----------|-----------|
| `rgba(0, 212, 170, 0.015)` | `var(--accent-a3)` (close enough — update the token to `0.015` if pixel-perfect matters, but 0.015 opacity is imperceptible vs 0.03) |
| `rgba(0, 212, 170, 0.03)` | `var(--accent-a3)` |
| `rgba(0, 212, 170, 0.06)` | `var(--accent-a6)` |
| `rgba(0, 212, 170, 0.08)` | `var(--accent-a10)` (close — or add `--accent-a8` if desired) |
| `rgba(0, 212, 170, 0.1)` | `var(--accent-a10)` |
| `rgba(0, 212, 170, 0.12)` | `var(--accent-a12)` |
| `rgba(0, 212, 170, 0.15)` | `var(--accent-dim)` (already exists) |
| `rgba(0, 212, 170, 0.18)` | `var(--accent-a18)` |
| `rgba(0, 212, 170, 0.2)` | `var(--accent-a20)` |
| `rgba(0, 212, 170, 0.25)` | `var(--accent-a25)` |
| `rgba(0, 212, 170, 0.3)` | `var(--accent-a30)` |
| `rgba(0, 212, 170, 0.35)` | `var(--accent-a35)` |
| `rgba(0, 212, 170, 0.4)` | `var(--accent-a40)` |
| `rgba(0, 212, 170, 0.5)` | `var(--accent-a50)` |
| `rgba(0, 212, 170, 0.8)` | `var(--accent-a80)` |

**Important:** Do NOT replace `rgba()` values inside `linear-gradient()` or `radial-gradient()` arguments where the value is one stop among hardcoded `rgba(0, 100, 200, ...)` values (those are a different blue color, not the accent). Only replace the accent green `(0, 212, 170)`.

- [ ] **Step 3: Replace all `rgba(30, 45, 61, ...)` occurrences with matching custom properties**

| Old value | New value |
|-----------|-----------|
| `rgba(30, 45, 61, 0.15)` | `var(--border-a15)` |
| `rgba(30, 45, 61, 0.35)` | `var(--border-a35)` |
| `rgba(30, 45, 61, 0.5)` | `var(--border-a50)` |
| `rgba(30, 45, 61, 0.6)` | `var(--border-a60)` |

- [ ] **Step 4: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 5: Commit**

```bash
git add dashboard/src/index.css
git commit -m "refactor(css): extract repeated rgba colors into custom properties"
```

---

### Task 4: Deduplicate token aliases in `:root`

The `:root` block has two parallel naming conventions: a domain-specific set (`--bg-primary`, `--accent`, `--win`) and a shadcn/Tailwind set (`--background`, `--primary`, `--card`). Many map to identical values. Keep the shadcn set (required by the UI components) and redefine the domain-specific ones as aliases.

**Files:**
- Modify: `dashboard/src/index.css:11-59`

- [ ] **Step 1: Reorganize `:root` into two clearly separated sections**

Replace the entire `:root` block with this reorganized version:

```css
:root {
  --radius: 0.75rem;
  --font-mono: "Geist Pixel", monospace;
  --font-sans: "Inter", sans-serif;
  --font-display: "Chakra Petch", sans-serif;

  /* ── Shadcn / Tailwind tokens (canonical) ── */
  --background: #0a0e17;
  --foreground: #e8edf5;
  --card: #1a2332;
  --card-foreground: #e8edf5;
  --popover: #1a2332;
  --popover-foreground: #e8edf5;
  --primary: #00d4aa;
  --primary-foreground: #061b1b;
  --secondary: #111827;
  --secondary-foreground: #e8edf5;
  --muted: #111827;
  --muted-foreground: #7b8ca3;
  --accent-foreground: #061b1b;
  --destructive: #ff4757;
  --input: #1e2d3d;
  --ring: #00d4aa;
  --chart-1: #00d4aa;
  --chart-2: #ff4757;
  --chart-3: #ffa502;
  --chart-4: #2f7aa1;
  --chart-5: #6d7481;
  --sidebar: #111827;
  --sidebar-foreground: #e8edf5;
  --sidebar-primary: #00d4aa;
  --sidebar-primary-foreground: #061b1b;
  --sidebar-accent: #1a2332;
  --sidebar-accent-foreground: #e8edf5;
  --sidebar-border: #1e2d3d;
  --sidebar-ring: #00d4aa;

  /* ── Domain aliases (reference canonical tokens) ── */
  --bg-primary: var(--background);
  --bg-secondary: var(--secondary);
  --bg-card: var(--card);
  --border: var(--input);
  --accent: var(--primary);
  --accent-dim: rgba(0, 212, 170, 0.15);
  --win: var(--primary);
  --loss: var(--destructive);
  --warn: var(--chart-3);
  --text-primary: var(--foreground);
  --text-secondary: var(--muted-foreground);
  --text-dim: #4a5568;

  /* ── Accent alpha scale ── */
  --accent-a3: rgba(0, 212, 170, 0.03);
  --accent-a6: rgba(0, 212, 170, 0.06);
  --accent-a10: rgba(0, 212, 170, 0.1);
  --accent-a12: rgba(0, 212, 170, 0.12);
  --accent-a18: rgba(0, 212, 170, 0.18);
  --accent-a20: rgba(0, 212, 170, 0.2);
  --accent-a25: rgba(0, 212, 170, 0.25);
  --accent-a30: rgba(0, 212, 170, 0.3);
  --accent-a35: rgba(0, 212, 170, 0.35);
  --accent-a40: rgba(0, 212, 170, 0.4);
  --accent-a50: rgba(0, 212, 170, 0.5);
  --accent-a80: rgba(0, 212, 170, 0.8);
  --border-a15: rgba(30, 45, 61, 0.15);
  --border-a35: rgba(30, 45, 61, 0.35);
  --border-a50: rgba(30, 45, 61, 0.5);
  --border-a60: rgba(30, 45, 61, 0.6);
}
```

**Note:** If Task 3 was completed first, the alpha-scale tokens will already exist — just move them into the reorganized block and don't duplicate. If Task 4 is done before Task 3, the alpha tokens should be added here and Task 3 Step 1 can be skipped.

- [ ] **Step 2: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 3: Spot-check that domain aliases resolve correctly**

Run: `cd dashboard && bun run dev` (open in browser, verify header and cards render with correct colors — accent green, dark backgrounds, proper text contrast).

- [ ] **Step 4: Commit**

```bash
git add dashboard/src/index.css
git commit -m "refactor(css): deduplicate token aliases, canonical shadcn set + domain aliases"
```

---

### Task 5: Remove dead CSS classes

These classes are not referenced in any `.tsx` file:

| Class | Lines | Confirmed dead |
|-------|-------|----------------|
| `.hero-gradient` | 315–319 | Yes — grep found 0 tsx matches |
| `.glass` | 321–331 | Yes — grep found 0 tsx matches |
| `.stat-card` / `.stat-card::before` / `.stat-card:hover::before` | 333–352 | Yes — grep found 0 tsx matches |
| `.glow-accent` | 354–356 | Yes — grep found 0 tsx matches |
| `.pulse-dot` | 358–364 | Yes — grep found 0 tsx matches |

**Files:**
- Modify: `dashboard/src/index.css:313-364`

- [ ] **Step 1: Delete the entire legacy section and unused classes**

Remove lines 313–364 (from the `/* ── Legacy (kept for compatibility) ── */` comment through the `.pulse-dot` closing brace). Also remove the `@keyframes pulse` block (lines 400–410) since `.pulse-dot` was its only consumer.

Also remove the `@media (prefers-reduced-motion: reduce)` block added in Task 1 targeting `.pulse-dot` (since it's now deleted too).

- [ ] **Step 2: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/index.css
git commit -m "refactor(css): remove dead legacy classes and unused pulse animation"
```

---

### Task 6: Wrap custom styles in `@layer` for cascade organization

Tailwind v4 uses `@layer` natively. Custom styles without a layer can accidentally override Tailwind utilities. Organize the custom CSS into layers.

**Files:**
- Modify: `dashboard/src/index.css` (wrap existing blocks)

- [ ] **Step 1: Wrap the base reset and body styles in `@layer base`**

Wrap the `*`, `html, body, #root`, and `body` rules (currently around lines 61–83) in a layer:

```css
@layer base {
  * {
    box-sizing: border-box;
  }

  html,
  body,
  #root {
    min-height: 100%;
  }

  body {
    margin: 0;
    background-color: var(--bg-primary);
    background-image:
      linear-gradient(var(--border-a15) 1px, transparent 1px),
      linear-gradient(90deg, var(--border-a15) 1px, transparent 1px);
    background-size: 40px 40px;
    color: var(--text-primary);
    font-family: var(--font-sans);
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }
}
```

- [ ] **Step 2: Wrap typography utilities in `@layer utilities`**

```css
@layer utilities {
  .mono {
    font-family: var(--font-mono);
  }

  .display-font {
    font-family: var(--font-display);
  }

  .scrollbar-thin {
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
  }

  .scrollbar-thin::-webkit-scrollbar {
    width: 4px;
    height: 4px;
  }

  .scrollbar-thin::-webkit-scrollbar-track {
    background: transparent;
  }

  .scrollbar-thin::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 4px;
  }
}
```

- [ ] **Step 3: Wrap all component styles in `@layer components`**

Wrap everything else (header-hud, hud-card, filter-chip, card-title-hud, trade-row) in `@layer components { ... }`.

- [ ] **Step 4: Verify the build succeeds**

Run: `cd dashboard && bun run build`
Expected: Build completes with no errors.

- [ ] **Step 5: Verify visual rendering in dev mode**

Run: `cd dashboard && bun run dev` and check:
- Header renders with glow effects and scan lines
- Cards have hover border glow and top-line pseudo-element
- Filter chips highlight on active state
- Scrollbar is thin and styled

If any styles are missing, `@layer` ordering may need adjustment — Tailwind v4 layers take precedence over unlayered styles. Consult the Tailwind v4 docs on custom layer ordering if needed.

- [ ] **Step 6: Commit**

```bash
git add dashboard/src/index.css
git commit -m "refactor(css): organize custom styles into @layer base/components/utilities"
```

---

## Verification

After all tasks are complete, run the full build:

```bash
cd dashboard && bun run build && bun run lint
```

Expected: Both commands pass with zero errors.
