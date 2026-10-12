# Habit TUI

A TUI habit tracker written in Rust. Habit TUI has a compact monochrome interface inspired by a hand-drawn layout, with weekly and monthly tracking, graphs, period totals, and mouse support. Well isn't there already enough of these habit trackers already? Yes there is but I could never stick to one because I can't stick to boring stuff, maybe that's just me. This app is not for everyone. It is for people who opens their laptop everyday, for work, for gaming, or whatever. Why would you use this? Just because it's cool I guess. I made it for myself and I started to keep up with my habits just because I wanted the monthly graph to look cool. This app is for that kind of people. Do it for the joy of it people. The motivation or will power is only in the mind. As long as the rest of you doesn't align with that, you'll fail. So align yourself by using fun !! GOOD DAY !

- **Repository:** https://github.com/illicit-sly-git/habit-tui
- **License:** MIT
- **Current version:** 0.2.1

## Features

- **Weekly view** — record habits by day of the week.
- **Monthly view** — review and update entries across a calendar month.
- **Graph view** — chart one habit across a week or month.
- **Total view** — see each habit's total for a week or month. Numeric habits are summed; yes/no habits count completed days.
- **Numeric and yes/no habits** — track things such as pages read, repetitions, or whether you completed a habit.
- **Custom header** — edit the label above the habit list, type something that'll get you going.
- **Mouse support** — click tabs, dates, cells, habits, and on-screen controls. Mouse reporting must be supported by your terminal emulator.
- **CSV export** — export recorded entries for use in a spreadsheet or another tool.
- **Local SQLite database** — your habit history stays on your computer; no account or cloud service is required.

## Install on Arch Linux

The AUR package is not published yet. Until it becomes available, you can build Habit TUI from source.

### 1. Install dependencies

```sh
sudo pacman -S --needed base-devel rust cargo sqlite pkgconf git
```

### 2. Download and build

```sh
git clone https://github.com/illicit-sly-git/habit-tui.git
cd habit-tui
cargo build --release --locked
```

### 3. Install for your user

```sh
mkdir -p ~/.local/bin
install -Dm755 target/release/habit-tui ~/.local/bin/habit-tui
```

If `~/.local/bin` is not already on your Fish path, add it with:

```fish
fish_add_path ~/.local/bin
```

Launch the app from a terminal:

```sh
habit-tui
```

## Keyboard controls

Press `?` (or `Shift+/`) inside the app to open the built-in shortcut help.

| Key | Action |
|---|---|
| `1` / `2` / `3` / `4` | Open Weekly / Monthly / Graph / Total |
| `Tab` | Cycle through the four views |
| `↑` / `↓` | Select a habit |
| `←` / `→` | Move through dates or periods |
| `Enter` | Edit the selected entry; in Graph, select the next habit to graph |
| `x` | Toggle a selected yes/no habit |
| `p` | Toggle Graph between Week and Month |
| `t` | Toggle Total between Week and Month |
| `v` | Open the selected habit's edit menu |
| `a` / `+` | Add a habit |
| `m` | Jump to a month by entering `YYYY-MM` |
| `h` | Edit the custom header label |
| `e` | Export entries to CSV |
| `d` | Delete the selected habit (confirmation required) |
| `?` / `Shift+/` | Open keyboard shortcuts |
| `Esc` | Close a dialog or cancel the current edit |
| `q` | Quit |

### Edit a habit

Press `v` or click **Edit** to open the habit editor.

| Key | Action |
|---|---|
| `1` | Rename the habit |
| `2` | Change its measurement unit |
| `3` | Switch between numeric and yes/no |
| `4` | Move the habit up |
| `5` | Move the habit down |
| `Esc` | Close the editor |

Changing a habit's name, unit, type, or position does not delete its historical entries. Switching between numeric and yes/no changes how existing values are interpreted, so review past entries after changing a habit's type.

## Data and backups

Habit TUI stores data locally in SQLite:

- `$XDG_DATA_HOME/habit-tui/habits.db`, when `XDG_DATA_HOME` is set
- Otherwise, `~/.local/share/habit-tui/habits.db`

CSV exports are saved in the same application data directory. Back up `habits.db` regularly, especially before making manual changes or testing new builds. Reinstalling the executable does not remove the database.

## Build and test from source

To run the test suite and build the release binary:

```sh
cargo test --locked
cargo build --release --locked
```

## License

## Habit TUI is distributed under the MIT License. See [LICENSE](LICENSE) for the full text.
