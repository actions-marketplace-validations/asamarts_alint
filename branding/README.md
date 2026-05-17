# alint press kit

Brand assets for alint. Reuse these for write-ups, talks, package
listings, and social posts instead of screenshotting the site.

## Assets

| File | What | Use |
|---|---|---|
| `logo.svg` | Rounded-square mark, white lowercase "a" on alint indigo. 128 viewBox, reads down to 16px. | Favicon, avatar, anywhere the mark stands alone. |
| `logo-512.png` / `logo-256.png` / `logo-128.png` | Raster of the mark. | Package registries, GitHub org avatar, slide decks. |
| `social-card.svg` | 1200x630 Open Graph / Twitter card. Vector, tiny on the wire. | The on-site `og:image` source. |
| `social-card.png` | 1200x630 raster of the same card. | Link previews. Most scrapers (X, LinkedIn, Slack, Discord, Facebook) do not render an SVG `og:image`, so the raster is the one that actually shows up. |
| `demo.cast` | asciinema v2 recording: `check`, `config`, `list`, `suggest`, `fix`, re-check. 108x42, ~30s. | The live player on alint.org plays this. Source for a GIF (see below). |

## Brand

- Primary: `#4338ca` (indigo). Background gradient: `#0e0e10` to `#1e1b4b`.
- Accent text: `#a5b4fc`. Muted text: `#6b7280`.
- Wordmark: heavy weight, tight tracking. The SVGs name a system-UI
  font stack; a rasterizer without those fonts substitutes a clean
  sans, which is acceptable for the raster fallbacks here.

## License

Same dual license as the project (Apache-2.0 OR MIT). The marks
identify alint; do not modify them to imply a different project.

## Regenerate the raster

From the alint.org repo, with its pinned Node (`.nvmrc` = 24):

```sh
node -e "const s=require('sharp');for(const[i,o,w,h]of[['public/og-image.svg','public/og-image.png',1200,630],['public/favicon.svg','branding/logo-512.png',512,512]])s(i,{density:400}).resize(w,h,{fit:'fill'}).png().toFile(o)"
```

`sharp` requires Node >= 16; the system default may be older, so use
the nvm-pinned 24.

## Open follow-ups (not yet applied)

1. **Repoint `og:image` to the PNG.** `src/pages/*.astro` on alint.org
   still set `og:image` / `twitter:image` / JSON-LD `image` to
   `og-image.svg`. The PNG (`og-image.png`) is now deployed alongside
   it. Switching the meta tags to the PNG is the change that makes link
   previews render on the scrapers that ignore SVG. This edit touches
   the same files the SEO/JSON-LD work is in, so it is left for that
   pass to apply, not done here, to avoid a conflicting edit.
2. **The GitHub README has no demo.** When HN or Lobsters points at the
   repo, the README is the first thing seen and it currently has no
   visual. The site has the live player; the README cannot run it.
   Either link the live player, or generate a GIF from `demo.cast`:

   ```sh
   # needs agg (asciinema gif generator); not installed in this env
   cargo install --git https://github.com/asciinema/agg
   agg branding/demo.cast branding/demo.gif
   ```

   Pick one before launch (the launch URL is the repo).
