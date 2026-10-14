# CLAUDE.md: LUAN-ARTCRAFT-SUITE

Guidance for Claude Code (and Cursor agents) working in this monorepo. The owner writes in
Vietnamese; answer in Vietnamese unless asked otherwise.

## What this repo is

One git repository holding the source of the 13 ArtCraft apps from https://getartcraft.com/apps,
each imported from its official upstream at `github.com/storytold/<id>` with **git subtree
(--squash)**. There are no submodules.

| id | App | Upstream |
|----|-----|----------|
| photocraft | PhotoCraft (image editor) | storytold/photocraft |
| vectorcraft | VectorCraft (vector illustration) | storytold/vectorcraft |
| filmcraft | FilmCraft (video editor) | storytold/filmcraft |
| lightcraft | LightCraft (photo library, raw) | storytold/lightcraft |
| pdfcraft | PdfCraft (PDF workbench) | storytold/pdfcraft |
| effectcraft | EffectCraft (motion graphics, VFX) | storytold/effectcraft |
| designcraft | DesignCraft (page layout) | storytold/designcraft |
| figcraft | FigCraft (UI design, prototyping) | storytold/figcraft |
| wordcraft | WordCraft (word processor) | storytold/wordcraft |
| soundcraft | SoundCraft (audio recording, mixing) | storytold/soundcraft |
| deckcraft | DeckCraft (presentations) | storytold/deckcraft |
| cadcraft | CADCraft (CAD, drafting) | storytold/cadcraft |
| gridcraft | GridCraft (spreadsheets) | storytold/gridcraft |

`config/apps.json` is the source of truth: upstream URL, branch, imported commit, version,
rust-version, license, build packages/features, automation entry points, cross-app deps.

## Architecture

```
apps/<id>/          one complete upstream repo, unchanged layout. Each is its OWN Cargo workspace
  Cargo.toml        [workspace]: members crates/*, apps/*, xtask
  apps/<id>/        the GUI app (egui/eframe)  -> <id>.exe
  apps/<id>-cli/    headless CLI + MCP server  -> <id>-cli.exe
  apps/<id>-web/    WebAssembly build
  crates/           engine, formats, ui-egui, automation/mcp, ...
  packaging/windows package.ps1 (+ WiX .wxs) = official Windows build
  AGENTS.md / CLAUDE.md   upstream's own agent rules for that app: READ AND FOLLOW them
config/apps.json    manifest
scripts/            build / test / affected / upstream sync (PowerShell)
docs/               audit, setup, build, MCP, upstream sync, conflicts
builds/             local build output (git-ignored)
```

- There is **no root Cargo workspace**, on purpose. The apps pin different versions of egui,
  craft-libs, etc.; merging them into one workspace would force dependency upgrades. Always run
  cargo **inside** `apps/<id>/`.
- All apps are pure Rust (edition 2024). Minimum Rust: 1.90 or 1.95 depending on the app
  (`rustVersion` in the manifest). Windows builds need the MSVC toolchain.
- Shared code lives outside this repo in `storytold/craft-libs` (craft-ui, craft-print,
  craft-settings, ...), pulled by Cargo as git deps at a pinned `rev`.
- Cross-app deps (also git deps at a pinned rev, NOT the local folder):
  - effectcraft uses 16 `filmcraft-*` crates (rev 2c5ba7a726)
  - designcraft uses `hayro*` crates from pdfcraft (rev b5467d93a0)
  Editing `apps/filmcraft` or `apps/pdfcraft` does not change effectcraft/designcraft builds.
  To test such a change across apps, add a temporary `[patch."https://github.com/storytold/filmcraft"]`
  pointing at `../filmcraft/crates/...` in the consumer, and tell the owner; do not commit it
  unless asked.

## Rules for editing (mandatory)

1. **Scope.** Change only the app(s) the owner names. If asked to change one app, do not touch
   the other 12. If a change would naturally apply to several apps, propose it and list them
   first; wait for a yes.
2. **Follow the app's own rules.** Before editing `apps/<id>/`, read `apps/<id>/AGENTS.md`
   (and `apps/<id>/CLAUDE.md` if present). Their conventions (commands, tests, lints, style)
   apply inside that folder.
3. **Do not change original behaviour or UI** unless that is the requested change. No drive-by
   refactors, renames, reformatting of untouched code, or dependency upgrades.
4. **Never edit** `LICENSE-*`, `NOTICE`, `ATTRIBUTION.md` or copyright headers. Keep them intact.
5. **No dependency upgrades** (`cargo update`, version bumps in Cargo.toml/Cargo.lock) unless asked.
   Builds use `--locked`.
6. **Keep owner edits upstream-safe.** Prefer adding new files/modules over rewriting upstream
   files; keep diffs small and local. That keeps future `git subtree pull` merges clean.
7. **Commits.** One app per commit, message prefix `feat(<id>):`, `fix(<id>):`, `chore(<id>):`.
   Commit a checkpoint before any large change. Never mix an upstream sync with owner edits.
8. **Forbidden without explicit permission:** `git reset --hard`, `git clean -fd/-fdx`,
   `git push --force`, `git checkout -- .`, `git restore .`, deleting folders, rewriting history,
   `git subtree split/push`, pushing to any remote other than `origin`, creating GitHub repos.
10. **Publish every change.** The owner's GitHub is `origin` =
   https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE (public). After committing a change
   on `main` (tests/build checked), run `git push origin main`. Normal fast-forward pushes only.
11. **Build scope.** The owner builds 8 apps: photocraft, vectorcraft, filmcraft, pdfcraft,
   effectcraft, wordcraft, cadcraft, gridcraft. lightcraft, designcraft, figcraft, soundcraft and
   deckcraft stay in the repo (source kept, upstream-synced) but are not built unless asked.
9. **Never report success you have not verified.** If a build or test was not run (e.g. Rust not
   installed), say so.

## Common commands (PowerShell, from repo root)

```powershell
scripts\check-env.ps1                          # what's missing to build on this machine
scripts\affected.ps1                           # which apps the current changes touch
scripts\test-app.ps1 -App <id>                 # cargo test (default members) in apps/<id>
scripts\test-app.ps1 -App <id> -Mode check     # fast compile check
scripts\test-app.ps1 -App <id> -Mode full      # fmt + clippy -D warnings + all tests
scripts\build-app.ps1 -App <id>                # release .exe -> builds/<id>/<ver>/windows-x64/
scripts\build-app.ps1 -App <id> -Msi           # + official MSI/zip via upstream package.ps1
scripts\build-all.ps1                          # all 13, continues on failure, writes report
scripts\upstream-status.ps1                    # which apps have new upstream commits
scripts\upstream-update.ps1 -App <id> -DryRun  # preview a sync + likely conflicts
scripts\upstream-update.ps1 -App <id>          # sync on a new branch; stops on conflict
```

Inside an app you can also use its own tooling, e.g. `cd apps\<id>; cargo xtask ...` (see that
app's AGENTS.md).

## Workflow for a change request

1. Identify the app(s). Read `apps/<id>/AGENTS.md` and the relevant crate.
2. Find shared code: look in `apps/<id>/crates/`, then craft-libs usage (`craft-*` deps), then
   similar code in sibling apps (`rg -n "<symbol>" apps/*/crates`). Apps share patterns
   (command registry, control_server.rs, automation/mcp crate) but not code on disk.
3. Edit, then run `scripts\test-app.ps1 -App <id>` (at least `-Mode check`).
4. Run `scripts\affected.ps1` and report every app touched plus any consumer notes.
5. Commit with the `<type>(<id>):` prefix.

## Upstream sync

Never sync as a side effect of another task. Use `scripts\upstream-update.ps1`, which works on a
new branch `upstream-sync/<id>-<stamp>`, tags a restore point, and stops on conflicts. See
`docs/UPSTREAM-SYNC.md` and `docs/CONFLICTS.md`.

## Automation / MCP

Every app ships an MCP server: `<id>-cli mcp` (JSON-RPC 2.0 over stdio), headless or bridged to a
running window via its loopback control channel. Details per app: `docs/MCP-AUTOMATION.md`. Do
not change any app's MCP protocol or add a gateway unless the owner asks.
