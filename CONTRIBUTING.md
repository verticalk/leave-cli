# Contributing

The public project remains gated on written Cognition approval. Until that gate
passes, treat this checkout as a private research prototype.

## Development rules

- Keep integrations on documented Devin CLI and ACP surfaces.
- Do not inspect private Desktop databases, copy tokens, or parse the Devin TUI.
- Do not weaken the crypto release gate or add plaintext remote fallbacks.
- Update `docs/compatibility.md` with every adapter change.
- Add tests for protocol, authorization, path, persistence, and offline changes.
- Run Rust formatting, Clippy, workspace tests, TypeScript checks, web tests, and
  the production PWA build before requesting review.

## Developer Certificate of Origin

Every commit must include a `Signed-off-by` line certifying the Developer
Certificate of Origin 1.1:

```text
Signed-off-by: Your Name <you@example.com>
```

Use `git commit -s`. The project uses the DCO and no contributor license
agreement.
