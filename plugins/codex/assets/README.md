# RightWake artwork

`right-wake.png` is the bundled marketplace and composer icon. It is a square 1254 × 1254 RGBA PNG with transparent outer corners, below the official 5 MiB icon limit. The midnight tile gives the mint conversation mark contrast on both light and dark surfaces. The pulse ends in a right-pointing arrow: wake the correct existing conversation.

The same file is referenced by `logo`, `logoDark`, and `composerIcon` in the plugin's OpenAI interface metadata. Keep those paths relative to the plugin root. See the [official icon requirements](https://developers.openai.com/plugins/deploy/submission#icons-and-screenshots) and [plugin path rules](https://developers.openai.com/plugins/build/plugins#path-rules).

## Creation

Generated with the built-in image generation tool for this repository on 2026-11-04. The generated PNG is copied without image edits. No external reference image was used. The repository's Apache-2.0 license applies.

Generation prompt:

> Use case: logo-brand. Asset type: production square marketplace app icon for an open-source developer tool named RightWake, whose meaning is 'Wake the right Agent thread.' Create ONE finished icon, not a contact sheet or a presentation. Design a precise, original, exceptionally clean flat geometric emblem on a deep midnight navy rounded-square tile, with truly transparent pixels outside the rounded corners. The emblem is a single large vivid mint-green filled conversation bubble with a confident short speech tail at its lower left. Inside the mint bubble, carve a bold midnight-navy negative-space symbol that combines a lightning pulse with a RIGHT-pointing arrow: a compact angular zigzag which terminates in a clearly right-facing arrowhead. This should read immediately as an event waking and routing into the correct conversation. Favor a compact, balanced silhouette and generous stroke widths and negative space; it must remain recognizable at 24 by 24 pixels. The bubble occupies about 65 percent of the tile, carefully optically centered. Flat solid colors only: midnight navy near #0C1623 and fresh mint near #68F0B0. Smooth professionally finished contours. Square canvas, tile fills canvas except minimal antialias-safe transparent margin. No lettering, no initials, no words, no numbers, no slogan. No gradients, texture, noise, shadows, glow, bevels, borders, mockups, surrounding decoration, extra dots or secondary symbols.
