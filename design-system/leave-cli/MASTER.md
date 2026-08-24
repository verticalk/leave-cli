# Leave CLI design system

**Updated:** 2026-08-23

**Direction:** Quiet developer workspace, Swiss-style structure, blue-gray accent

**Design dials:** Variance 3/10, motion 5/10, density 7/10

## Product feel

Leave should feel like a trusted system tool. Use crisp geometry, restrained
surfaces, plain language, and strong information hierarchy. Avoid neon coding
motifs, colorful status dashboards, robot imagery, oversized gradients, glass
effects, and generic AI sparkles.

## Color

The interface uses one chromatic family: blue-gray. State labels must remain
clear without introducing green, yellow, or red. Pair state text with a label,
symbol, or boundary so color never carries the meaning by itself.

| Token | Dark | Light |
|---|---:|---:|
| Background | `#0C1118` | `#F4F6F8` |
| Surface | `#121923` | `#FFFFFF` |
| Raised surface | `#17212D` | `#F8FAFB` |
| Soft surface | `#1D2936` | `#E9EEF3` |
| Border | `#415368` | `#98A9BB` |
| Soft border | `#263546` | `#D5DDE6` |
| Primary text | `#F3F6F9` | `#18222D` |
| Secondary text | `#C2CCD8` | `#344456` |
| Muted text | `#91A0B2` | `#566A7D` |
| Accent | `#8CA6C2` | `#516F8F` |
| Strong accent | `#A8BFD6` | `#405F80` |

## Typography

- IBM Plex Sans for interface text.
- JetBrains Mono for commands, paths, sequence IDs, and compact product labels.
- Sentence case throughout.
- Headlines should describe the current view instead of using marketing copy.

## Logo

The Leave mark joins an `L` path to an outgoing arrow. It represents a session
leaving the desk while the repository stays on the host. Use the mark without a
tile, gradient, glow, or invented Devin branding. Pair it with the lowercase
monospace wordmark `leave` when space allows.

## Icons

- Use Phosphor outline icons with regular weight.
- Use 19 to 20px icons for navigation, 17 to 18px inside controls, and 24 to
  26px for a single empty-state anchor.
- Avoid robot, sparkle, alert-triangle, and fake brand icons.
- Hide decorative icons from assistive technology. Name every icon-only button.
- Keep each interactive target at least 44 by 44 CSS pixels.

## Components

- Cards use a one-pixel border, no gradient, and little or no shadow.
- Primary buttons use the strong blue-gray accent. Secondary actions use a
  neutral surface and border.
- Status pills use text plus a small geometric marker. Online, waiting, and
  blocked states share the same palette and retain their written labels.
- Permission requests use a key icon, neutral raised surface, command preview,
  expiry text, and explicit action labels.
- Code, preview, and terminal surfaces use the same semantic tokens in both
  themes. Light mode must not force a dark editor.

## Motion

- Route entry: 320ms fade with a 7px vertical offset.
- Primary surface entry: 300 to 360ms fade with a 10px offset.
- Permission entry: 280ms fade and small scale change.
- Decision feedback: 220ms scale settle.
- Button press feedback: 120ms, using transform without layout movement.
- Do not animate more than two major regions per view.
- Disable nonessential motion through `prefers-reduced-motion`.

## Layout

- Maintain a 4/8px spacing rhythm.
- Use 20px phone gutters, 48px desktop gutters, and readable text measures.
- Keep the five-item bottom navigation and respect device safe areas.
- Test at 375, 768, 1024, and 1440 pixels with no horizontal page overflow.

## Acceptance checklist

- Text and controls pass WCAG AA in dark and light modes.
- Keyboard focus remains visible and unobscured.
- Touch targets are at least 44px.
- Reduced motion renders every view in its final readable state.
- The console has no application errors or failed default network requests.
- The interface contains no green, yellow, red, neon, or invented Devin icons.
