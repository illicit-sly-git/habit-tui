# Habit TUI

A local-first Rust terminal habit tracker for Linux, with a compact monochrome interface inspired by hand-drawn sketches.

## Features

- **Weekly** and **Monthly** date grids.
- **Graph** view with Week/Month range controls and a scale that leaves one increment above the largest recorded value.
- **Total** view with Week/Month options. Numeric habits show the sum of their values; yes/no habits show the number of completed days in the selected period.
- Numeric habits (pages, reps, minutes, etc.) and yes/no habits.
- Mouse support for tabs, dates, habit rows, period controls, and top-bar actions.
- Press `?` for the keyboard shortcut popup.
- Editable first-column header label.
- Add, delete, rename, change units, change habit type, and reorder habits without recreating them.
- CSV export of all historical entries.
- SQLite storage under `$XDG_DATA_HOME/habit-tui/habits.db`, falling back to `~/.local/share/habit-tui/habits.db`.

## Build from source

Install the Rust toolchain and build dependencies on Arch Linux:

```sh
sudo pacman -S --needed rust cargo base-devel
```

From the project directory:

```sh
cargo build --release
./target/release/habit-tui
```

To install locally:

```sh
sudo install -Dm755 target/release/habit-tui /usr/local/bin/habit-tui
```

## Keyboard controls

| Key | Action |
|---|---|
| `1` / `2` / `3` / `4` | Weekly / Monthly / Graph / Total |
| `Tab` | Cycle through all four views |
| `↑` / `↓` | Select a habit |
| `←` / `→` | Move date or selected period |
| `Enter` | Edit the selected entry; in Graph, cycle graph habit |
| `x` | Toggle selected yes/no habit |
| `p` | Toggle Graph Week/Month |
| `t` | Toggle Total Week/Month |
| `v` | Open the selected habit's edit menu |
| `a` / `+` | Add a habit |
| `m` | Jump to a month (`YYYY-MM`) |
| `h` | Edit the custom first-column label |
| `e` | Export all records to CSV |
| `d` | Delete selected habit, then confirm with `y` |
| `?` / `Shift+/` | Open keyboard shortcuts |
| `Esc` | Close a dialog or return from an edit field |
| `q` | Quit |

### Habit edit menu

Press `v` or click **Edit** in the top bar. In the menu:

- `1`: Rename the habit.
- `2`: Change its measurement unit.
- `3`: Toggle numeric / yes-no type.
- `4`: Move the habit up.
- `5`: Move the habit down.

Changing a habit's name, unit, type, or position preserves its historical entries. Switching numeric to yes/no changes how the existing values are interpreted; it does not erase them.

## Arch Linux / AUR packaging

The repository includes an AUR `PKGBUILD` template and `.SRCINFO`. Before publishing:

1. Create a public GitHub repository named `habit-tui` and replace `YOUR_GITHUB_USERNAME` in `PKGBUILD` and `.SRCINFO` with your actual GitHub username. Update the maintainer line too.
2. On your Arch machine, in the project root, run `cargo generate-lockfile`, then `cargo test` and `cargo build --release`. Commit `Cargo.lock` to the repository.
3. Commit all source and packaging files to GitHub, then create and push a version tag matching `pkgver`, for example:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. Check the package locally before uploading:

   ```sh
   makepkg --syncdeps --cleanbuild
   sudo pacman -U habit-tui-0.2.0-1-$(uname -m).pkg.tar.zst
   ```

   Check the actual package filename produced by `makepkg` before installing it.

5. Generate `.SRCINFO` from the final `PKGBUILD` and upload both to your AUR repository:

   ```sh
   makepkg --printsrcinfo > .SRCINFO
   ```

   Create the AUR package repository using the package name `habit-tui`, clone it, copy `PKGBUILD` and `.SRCINFO` into it, then commit and push. The AUR package's source URL must point to your real GitHub repository and a pushed tag.

**Important:** the GitHub URL in the included PKGBUILD is intentionally a placeholder. Replace it before publishing; the package cannot build until it points at your repository and the tagged release contains `Cargo.lock`.

## Data and backups

The database is independent of the executable and source checkout. Updating or reinstalling the package does not remove your habit history. Back up `habits.db` before experimenting with database changes.

Mouse support depends on terminal-emulator mouse reporting being enabled.
