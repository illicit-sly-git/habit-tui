use anyhow::{Context, Result};
use chrono::{Datelike, Duration, Local, NaiveDate};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph},
    Frame, Terminal,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug)]
struct Habit {
    id: i64,
    name: String,
    unit: String,
    is_bool: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Weekly,
    Monthly,
    Graph,
    Total,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphPeriod {
    Week,
    Month,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    AddName,
    AddUnit,
    EditValue,
    MonthYear,
    EditHeader,
    EditHabitMenu,
    EditHabitName,
    EditHabitUnit,
    ConfirmDelete,
}

#[derive(Clone, Copy, Debug)]
enum HitAction {
    SwitchPage(Page),
    MonthYear,
    EditHeader,
    Export,
    AddHabit,
    EditHabit,
    SelectHabit(usize),
    TotalPeriod(GraphPeriod),
    SelectDate(NaiveDate),
    EditCell(usize, NaiveDate),
    GraphPeriod(GraphPeriod),
    GraphHabit(usize),
}

#[derive(Clone, Copy, Debug)]
struct HitBox {
    rect: Rect,
    action: HitAction,
}

struct App {
    db: Connection,
    habits: Vec<Habit>,
    page: Page,
    graph_period: GraphPeriod,
    total_period: GraphPeriod,
    mode: Mode,
    selected: usize,
    date: NaiveDate,
    input: String,
    header_text: String,
    pending_name: String,
    message: String,
    graph_habit: usize,
    show_help: bool,
    should_quit: bool,
    hits: Vec<HitBox>,
}

fn data_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .context("Cannot find data directory")?;
    let path = base.join("habit-tui");
    fs::create_dir_all(&path)?;
    Ok(path)
}

impl App {
    fn new() -> Result<Self> {
        let db = Connection::open(data_dir()?.join("habits.db"))?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS habits(
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE,
                 unit TEXT NOT NULL DEFAULT '',
                 is_bool INTEGER NOT NULL DEFAULT 0,
                 position INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS entries(
                 habit_id INTEGER NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
                 date TEXT NOT NULL,
                 value REAL NOT NULL,
                 PRIMARY KEY(habit_id,date)
             );
             CREATE TABLE IF NOT EXISTS settings(
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             INSERT OR IGNORE INTO settings(key,value) VALUES('header_text','custom text');",
        )?;

        // Migrate databases created by earlier versions without losing habit data.
        let has_position = {
            let mut stmt = db.prepare("PRAGMA table_info(habits)")?;
            let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
            columns.collect::<rusqlite::Result<Vec<_>>>()?.iter().any(|name| name == "position")
        };
        if !has_position {
            db.execute("ALTER TABLE habits ADD COLUMN position INTEGER NOT NULL DEFAULT 0", [])?;
        }

        if db.query_row("SELECT COUNT(*) FROM habits", [], |r| r.get::<_, i64>(0))? == 0 {
            db.execute("INSERT INTO habits(name,unit,is_bool,position) VALUES('Read book','pages',0,0)", [])?;
            db.execute("INSERT INTO habits(name,unit,is_bool,position) VALUES('Push-ups','reps',0,1)", [])?;
        }
        // Compact positions on every launch while preserving the current sort order.
        let ordered_ids = {
            let mut stmt = db.prepare("SELECT id FROM habits ORDER BY position,id")?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (position, id) in ordered_ids.iter().enumerate() {
            db.execute("UPDATE habits SET position=?1 WHERE id=?2", params![position as i64, id])?;
        }

        let header_text: String = db.query_row(
            "SELECT value FROM settings WHERE key='header_text'",
            [],
            |row| row.get(0),
        )?;

        let mut app = Self {
            db,
            habits: Vec::new(),
            page: Page::Weekly,
            graph_period: GraphPeriod::Week,
            total_period: GraphPeriod::Week,
            mode: Mode::Normal,
            selected: 0,
            date: Local::now().date_naive(),
            input: String::new(),
            header_text,
            pending_name: String::new(),
            message: String::new(),
            graph_habit: 0,
            show_help: false,
            should_quit: false,
            hits: Vec::new(),
        };
        app.reload_habits()?;
        Ok(app)
    }

    fn reload_habits(&mut self) -> Result<()> {
        let mut stmt = self.db.prepare("SELECT id,name,unit,is_bool FROM habits ORDER BY position,id")?;
        self.habits = stmt
            .query_map([], |r| {
                Ok(Habit {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    unit: r.get(2)?,
                    is_bool: r.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.selected = self.selected.min(self.habits.len().saturating_sub(1));
        self.graph_habit = self.graph_habit.min(self.habits.len().saturating_sub(1));
        Ok(())
    }

    fn value(&self, habit: &Habit, date: NaiveDate) -> Result<Option<f64>> {
        Ok(self
            .db
            .query_row(
                "SELECT value FROM entries WHERE habit_id=?1 AND date=?2",
                params![habit.id, date.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn set_value(&self, habit: &Habit, date: NaiveDate, value: f64) -> Result<()> {
        self.db.execute(
            "INSERT INTO entries(habit_id,date,value) VALUES(?1,?2,?3)
             ON CONFLICT(habit_id,date) DO UPDATE SET value=excluded.value",
            params![habit.id, date.to_string(), value],
        )?;
        Ok(())
    }

    fn add_habit(&mut self, name: String, unit: String) -> Result<()> {
        if name.trim().is_empty() {
            self.message = "Habit name cannot be empty.".into();
            return Ok(());
        }
        let unit = unit.trim();
        let next_position: i64 = self.db.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM habits", [], |r| r.get(0)
        )?;
        self.db.execute(
            "INSERT INTO habits(name,unit,is_bool,position) VALUES(?1,?2,?3,?4)",
            params![name.trim(), unit, if unit.is_empty() { 1 } else { 0 }, next_position],
        )?;
        self.reload_habits()?;
        self.selected = self.habits.len().saturating_sub(1);
        self.graph_habit = self.selected;
        self.message = format!("Added '{}'.", name.trim());
        Ok(())
    }

    fn export(&mut self) -> Result<()> {
        let path = data_dir()?.join(format!(
            "habit-export-{}.csv",
            Local::now().format("%Y%m%d-%H%M%S")
        ));
        let mut writer = csv::Writer::from_path(&path)?;
        writer.write_record(["date", "habit", "unit", "value"])?;
        let mut stmt = self.db.prepare(
            "SELECT e.date,h.name,h.unit,e.value
             FROM entries e JOIN habits h ON h.id=e.habit_id
             ORDER BY e.date,h.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (date, name, unit, value) = row?;
            writer.write_record([date, name, unit, value.to_string()])?;
        }
        writer.flush()?;
        self.message = format!("CSV exported to {}", path.display());
        Ok(())
    }

    fn edit_selected(&mut self) -> Result<()> {
        if let Some(habit) = self.habits.get(self.selected).cloned() {
            let current = self.value(&habit, self.date)?;
            if habit.is_bool {
                let next = if current.unwrap_or(0.0) == 0.0 { 1.0 } else { 0.0 };
                self.set_value(&habit, self.date, next)?;
                self.message = format!(
                    "{} · {} · {}",
                    habit.name,
                    self.date.format("%d %b"),
                    if next == 1.0 { "done" } else { "not done" }
                );
            } else {
                self.input = current.map(format_number).unwrap_or_default();
                self.mode = Mode::EditValue;
            }
        }
        Ok(())
    }

    fn days(&self) -> Vec<NaiveDate> {
        if self.page == Page::Weekly {
            let offset = self.date.weekday().num_days_from_monday() as i64;
            let start = self.date - Duration::days(offset);
            (0..7).map(|i| start + Duration::days(i)).collect()
        } else {
            let first = self.date.with_day(1).unwrap_or(self.date);
            let next = if self.date.month() == 12 {
                NaiveDate::from_ymd_opt(self.date.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(self.date.year(), self.date.month() + 1, 1).unwrap()
            };
            let count = next.signed_duration_since(first).num_days();
            (0..count).map(|i| first + Duration::days(i)).collect()
        }
    }

    fn set_page(&mut self, page: Page) {
        self.page = page;
        self.message.clear();
    }

    fn begin_add(&mut self) {
        self.input.clear();
        self.mode = Mode::AddName;
    }

    fn begin_month_jump(&mut self) {
        self.input = self.date.format("%Y-%m").to_string();
        self.mode = Mode::MonthYear;
    }

    fn begin_edit_header(&mut self) {
        self.input = self.header_text.clone();
        self.mode = Mode::EditHeader;
    }

    fn begin_edit_habit(&mut self) {
        if let Some(habit) = self.habits.get(self.selected) {
            self.message = format!("Editing '{}'. Choose an option in the habit menu.", habit.name);
            self.mode = Mode::EditHabitMenu;
        } else {
            self.message = "Add a habit before editing one.".into();
        }
    }

    fn reorder_selected(&mut self, delta: isize) -> Result<()> {
        if self.habits.is_empty() { return Ok(()); }
        let from = self.selected;
        let to = (from as isize + delta).clamp(0, self.habits.len() as isize - 1) as usize;
        if from == to { self.message = "Habit is already at that position.".into(); return Ok(()); }
        let first = self.habits[from].id;
        let second = self.habits[to].id;
        let transaction = self.db.unchecked_transaction()?;
        transaction.execute("UPDATE habits SET position=?1 WHERE id=?2", params![to as i64, first])?;
        transaction.execute("UPDATE habits SET position=?1 WHERE id=?2", params![from as i64, second])?;
        transaction.commit()?;
        self.selected = to;
        self.reload_habits()?;
        self.message = "Habit order updated.".into();
        Ok(())
    }

    fn toggle_habit_type(&mut self) -> Result<()> {
        if let Some(habit) = self.habits.get(self.selected).cloned() {
            if habit.is_bool {
                let unit = if habit.unit.trim().is_empty() { "count" } else { habit.unit.as_str() };
                self.db.execute("UPDATE habits SET is_bool=0, unit=?1 WHERE id=?2", params![unit, habit.id])?;
                self.message = format!("'{}' is now numeric (unit: {}).", habit.name, unit);
            } else {
                self.db.execute("UPDATE habits SET is_bool=1, unit='' WHERE id=?1", [habit.id])?;
                self.message = format!("'{}' is now yes/no.", habit.name);
            }
            self.reload_habits()?;
        }
        Ok(())
    }

    fn shift_date(&mut self, direction: i64) {
        match self.page {
            Page::Weekly => self.date += Duration::days(direction),
            Page::Monthly => self.date = shift_month(self.date, direction as i32),
            Page::Graph => match self.graph_period {
                GraphPeriod::Week => self.date += Duration::days(direction * 7),
                GraphPeriod::Month => self.date = shift_month(self.date, direction as i32),
            },
            Page::Total => match self.total_period {
                GraphPeriod::Week => self.date += Duration::days(direction * 7),
                GraphPeriod::Month => self.date = shift_month(self.date, direction as i32),
            },
        }
        self.message.clear();
    }
}

fn inset(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(x),
        y: area.y.saturating_add(y),
        width: area.width.saturating_sub(x.saturating_mul(2)),
        height: area.height.saturating_sub(y.saturating_mul(2)),
    }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && y >= rect.y && x < rect.x.saturating_add(rect.width) && y < rect.y.saturating_add(rect.height)
}

fn add_hit(hits: &mut Vec<HitBox>, rect: Rect, action: HitAction) {
    if rect.width > 0 && rect.height > 0 {
        hits.push(HitBox { rect, action });
    }
}

fn button(
    f: &mut Frame,
    rect: Rect,
    label: &str,
    active: bool,
    action: HitAction,
    hits: &mut Vec<HitBox>,
) {
    let (fg, border, bg) = if active {
        (Color::Rgb(245, 245, 245), Color::Rgb(245, 245, 245), Color::Rgb(100, 100, 100))
    } else {
        (Color::Rgb(205, 205, 205), Color::Rgb(100, 100, 100), Color::Reset)
    };
    let style = Style::default().fg(fg).bg(bg);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(bg));
    f.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style.add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }))
            .block(block),
        rect,
    );
    add_hit(hits, rect, action);
}

fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    let area = f.area();
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
        area,
    );
    if area.width < 65 || area.height < 14 {
        f.render_widget(
            Paragraph::new("Resize the terminal to at least 65 columns × 14 rows.")
                .alignment(Alignment::Center),
            inset(area, 1, 1),
        );
        return;
    }

    let inner = inset(area, 1, 1);
    let top_y = inner.y;
    let gap = 1u16;
    let compact = inner.width < 92;
    let left_widths = if compact { [9u16, 10, 8, 8] } else { [12u16, 13, 10, 10] };
    let right_widths = if compact { [10u16, 7, 6, 5] } else { [14u16, 10, 8, 7] };
    let right_total = right_widths.iter().sum::<u16>() + gap * 3;

    let mut x = inner.x;
    button(f, Rect::new(x, top_y, left_widths[0], 3), "Weekly", app.page == Page::Weekly, HitAction::SwitchPage(Page::Weekly), &mut app.hits);
    x += left_widths[0] + gap;
    button(f, Rect::new(x, top_y, left_widths[1], 3), "Monthly", app.page == Page::Monthly, HitAction::SwitchPage(Page::Monthly), &mut app.hits);
    x += left_widths[1] + gap;
    button(f, Rect::new(x, top_y, left_widths[2], 3), "Graph", app.page == Page::Graph, HitAction::SwitchPage(Page::Graph), &mut app.hits);
    x += left_widths[2] + gap;
    button(f, Rect::new(x, top_y, left_widths[3], 3), "Total", app.page == Page::Total, HitAction::SwitchPage(Page::Total), &mut app.hits);

    let right_x = inner.x + inner.width.saturating_sub(right_total);
    let mut rx = right_x;
    button(
        f,
        Rect::new(rx, top_y, right_widths[0], 3),
        &format!("{}", app.date.format("%b %Y")),
        false,
        HitAction::MonthYear,
        &mut app.hits,
    );
    rx += right_widths[0] + gap;
    button(f, Rect::new(rx, top_y, right_widths[1], 3), "Export", false, HitAction::Export, &mut app.hits);
    rx += right_widths[1] + gap;
    button(f, Rect::new(rx, top_y, right_widths[2], 3), "Edit", false, HitAction::EditHabit, &mut app.hits);
    rx += right_widths[2] + gap;
    button(f, Rect::new(rx, top_y, right_widths[3], 3), "+", false, HitAction::AddHabit, &mut app.hits);

    let rule_y = top_y + 3;
    if inner.width > 0 {
        f.render_widget(
            Paragraph::new("─".repeat(inner.width as usize)).style(Style::default().fg(Color::Rgb(100, 100, 100))),
            Rect::new(inner.x, rule_y, inner.width, 1),
        );
    }

    let body_y = top_y + 4;
    let body_height = inner.height.saturating_sub(5);
    let body = Rect::new(inner.x, body_y, inner.width, body_height);
    match app.page {
        Page::Graph => draw_graph_page(f, app, body),
        Page::Total => draw_total_page(f, app, body),
        Page::Weekly | Page::Monthly => draw_habit_grid(f, app, body),
    }

    if !app.message.is_empty() {
        let status_y = inner.y + inner.height.saturating_sub(1);
        f.render_widget(
            Paragraph::new(app.message.as_str())
                .style(Style::default().fg(Color::Rgb(205, 205, 205))),
            Rect::new(inner.x, status_y, inner.width, 1),
        );
    }

    if app.show_help {
        draw_help(f, area);
    } else if app.mode != Mode::Normal {
        draw_dialog(f, app, area);
    }

}

fn draw_grid_cell(
    f: &mut Frame,
    rect: Rect,
    label: &str,
    border: Color,
    text_style: Style,
    borders: Borders,
    action: Option<HitAction>,
    hits: &mut Vec<HitBox>,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(border));
    f.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(text_style)
            .block(block),
        rect,
    );
    if let Some(action) = action {
        add_hit(hits, rect, action);
    }
}

fn habit_label(habit: &Habit) -> String {
    if habit.is_bool {
        habit.name.clone()
    } else {
        format!("{} · {}", habit.name, habit.unit)
    }
}

fn draw_habit_grid(f: &mut Frame, app: &mut App, area: Rect) {
    let days = app.days();
    if days.is_empty() || area.height < 6 || area.width < 20 {
        return;
    }

    let monthly = app.page == Page::Monthly;
    let name_width = if monthly {
        22u16.min(area.width.saturating_sub(4))
    } else {
        24u16.min(area.width.saturating_sub(8 * 5))
    };
    let gap = if monthly { 0u16 } else { 1u16 };
    let date_area_width = area.width.saturating_sub(name_width).saturating_sub(gap * days.len().saturating_sub(1) as u16);
    let base_date_width = (date_area_width / days.len() as u16).max(1);
    let extra = date_area_width % days.len() as u16;
    let head_rect = Rect::new(area.x, area.y, name_width, 3);
    draw_grid_cell(
        f,
        head_rect,
        app.header_text.as_str(),
        Color::Rgb(205, 205, 205),
        Style::default().fg(Color::Rgb(245, 245, 245)).add_modifier(Modifier::BOLD),
        if monthly { Borders::RIGHT | Borders::BOTTOM } else { Borders::ALL },
        Some(HitAction::EditHeader),
        &mut app.hits,
    );

    let mut dx = area.x + name_width;
    let mut day_rects = Vec::with_capacity(days.len());
    for (i, day) in days.iter().enumerate() {
        let w = base_date_width + if (i as u16) < extra { 1 } else { 0 };
        let rect = Rect::new(dx, area.y, w, 3);
        let label = if monthly {
            format!("{:02}", day.day())
        } else if w >= 9 {
            format!("{:02}/{}", day.day(), day.format("%a").to_string().to_lowercase())
        } else if w >= 7 {
            format!("{:02} {}", day.day(), day.format("%a").to_string().chars().next().unwrap_or(' '))
        } else {
            format!("{:02}", day.day())
        };
        let selected_day = *day == app.date;
        let text_style = if selected_day {
            Style::default().fg(Color::Rgb(245, 245, 245)).bg(Color::Rgb(100, 100, 100)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(205, 205, 205))
        };
        let borders = if monthly { Borders::RIGHT | Borders::BOTTOM } else { Borders::ALL };
        draw_grid_cell(
            f,
            rect,
            &label,
            if selected_day { Color::Rgb(245, 245, 245) } else { Color::Rgb(100, 100, 100) },
            text_style,
            borders,
            Some(HitAction::SelectDate(*day)),
            &mut app.hits,
        );
        day_rects.push(rect);
        dx += w + gap;
    }

    let row_height = 3u16;
    let row_gap = if monthly { 0u16 } else { 1u16 };
    let visible_rows = (if monthly {
        area.height.saturating_sub(3) / row_height
    } else if area.height >= 7 {
        1 + area.height.saturating_sub(7) / (row_height + row_gap)
    } else {
        0
    }) as usize;
    let first_row = if app.selected >= visible_rows && visible_rows > 0 {
        app.selected - visible_rows + 1
    } else {
        0
    };

    for (offset, habit_index) in (first_row..app.habits.len()).take(visible_rows).enumerate() {
        let habit = &app.habits[habit_index];
        let y = area.y + 3 + row_gap + offset as u16 * (row_height + row_gap);
        if y + row_height > area.y + area.height {
            break;
        }
        let selected_habit = habit_index == app.selected;
        let name_style = if selected_habit {
            Style::default().fg(Color::Rgb(245, 245, 245)).bg(Color::Rgb(100, 100, 100)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(205, 205, 205))
        };
        draw_grid_cell(
            f,
            Rect::new(area.x, y, name_width, row_height),
            &habit_label(habit),
            if selected_habit { Color::Rgb(245, 245, 245) } else { Color::Rgb(100, 100, 100) },
            name_style,
            if monthly { Borders::RIGHT | Borders::BOTTOM } else { Borders::ALL },
            Some(HitAction::SelectHabit(habit_index)),
            &mut app.hits,
        );

        for (day_index, day) in days.iter().enumerate() {
            let value = app.value(habit, *day).ok().flatten();
            let label = if habit.is_bool {
                if value.unwrap_or(0.0) == 0.0 { "0".to_string() } else { "1".to_string() }
            } else {
                value.map(format_number).unwrap_or_else(|| "—".to_string())
            };
            let selected_day = *day == app.date;
            let text_style = if selected_day || selected_habit {
                Style::default().fg(Color::Rgb(245, 245, 245)).bg(if selected_day { Color::Rgb(100, 100, 100) } else { Color::Reset })
            } else if value.is_some() {
                Style::default().fg(Color::Rgb(245, 245, 245))
            } else {
                Style::default().fg(Color::Rgb(100, 100, 100))
            };
            draw_grid_cell(
                f,
                Rect::new(day_rects[day_index].x, y, day_rects[day_index].width, row_height),
                &label,
                if selected_day { Color::Rgb(245, 245, 245) } else { Color::Rgb(100, 100, 100) },
                text_style,
                if monthly { Borders::RIGHT | Borders::BOTTOM } else { Borders::ALL },
                Some(HitAction::EditCell(habit_index, *day)),
                &mut app.hits,
            );
        }
    }
}

fn draw_total_page(f: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 45 || area.height < 8 { return; }
    let sidebar_width = 24u16.min(area.width / 3);
    let side = Rect::new(area.x, area.y, sidebar_width, area.height);
    f.render_widget(Block::default().title(" Period ").borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(100, 100, 100))), side);
    let inner = Rect::new(side.x + 1, side.y + 1, side.width.saturating_sub(2), side.height.saturating_sub(2));
    let button_width = inner.width.saturating_sub(1) / 2;
    button(f, Rect::new(inner.x, inner.y, button_width, 3), "Week", app.total_period == GraphPeriod::Week,
        HitAction::TotalPeriod(GraphPeriod::Week), &mut app.hits);
    button(f, Rect::new(inner.x + button_width + 1, inner.y, inner.width.saturating_sub(button_width + 1), 3), "Month",
        app.total_period == GraphPeriod::Month, HitAction::TotalPeriod(GraphPeriod::Month), &mut app.hits);
    let start = match app.total_period {
        GraphPeriod::Week => app.date - Duration::days(app.date.weekday().num_days_from_monday() as i64),
        GraphPeriod::Month => app.date.with_day(1).unwrap_or(app.date),
    };
    let end = match app.total_period {
        GraphPeriod::Week => start + Duration::days(6),
        GraphPeriod::Month => {
            let next = if app.date.month() == 12 { NaiveDate::from_ymd_opt(app.date.year()+1,1,1).unwrap() }
                else { NaiveDate::from_ymd_opt(app.date.year(),app.date.month()+1,1).unwrap() };
            next - Duration::days(1)
        }
    };
    f.render_widget(Paragraph::new(format!("{}\n{}\n\nUse ← / →\nto change period.",
        if app.total_period == GraphPeriod::Week { "Weekly totals" } else { "Monthly totals" },
        if app.total_period == GraphPeriod::Week { format!("{} – {}", start.format("%d %b"), end.format("%d %b %Y")) }
        else { app.date.format("%B %Y").to_string() }
    )).style(Style::default().fg(Color::Rgb(205,205,205))), Rect::new(inner.x, inner.y + 4, inner.width, inner.height.saturating_sub(4)));

    let main_x = side.x + side.width + 1;
    let main = Rect::new(main_x, area.y, area.width.saturating_sub(side.width + 1), area.height);
    f.render_widget(Block::default().title(format!(" Habit totals · {} – {} ", start.format("%d %b %Y"), end.format("%d %b %Y")))
        .borders(Borders::ALL).border_style(Style::default().fg(Color::Rgb(100,100,100))), main);
    let content = Rect::new(main.x + 1, main.y + 1, main.width.saturating_sub(2), main.height.saturating_sub(2));
    let name_width = (content.width * 2 / 3).max(12).min(content.width.saturating_sub(10));
    let header = Line::from(vec![
        Span::styled(format!("{:<width$}", "Habit", width=name_width as usize), Style::default().fg(Color::Rgb(245,245,245)).add_modifier(Modifier::BOLD)),
        Span::styled("Total", Style::default().fg(Color::Rgb(245,245,245)).add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(header), Rect::new(content.x, content.y, content.width, 1));
    f.render_widget(Paragraph::new("─".repeat(content.width as usize)).style(Style::default().fg(Color::Rgb(100,100,100))),
        Rect::new(content.x, content.y + 1, content.width, 1));
    for (i, habit) in app.habits.iter().enumerate() {
        let y = content.y + 2 + i as u16;
        if y >= content.y + content.height { break; }
        let mut total = 0.0;
        let mut date = start;
        while date <= end {
            if let Ok(Some(value)) = app.value(habit, date) {
                if habit.is_bool { if value != 0.0 { total += 1.0; } } else { total += value; }
            }
            date += Duration::days(1);
        }
        let label = format!("{:<width$}{}", habit_label(habit), format_number(total), width=name_width as usize);
        let style = if i == app.selected { Style::default().fg(Color::Rgb(245,245,245)).bg(Color::Rgb(75,75,75)).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(Color::Rgb(205,205,205)) };
        f.render_widget(Paragraph::new(label).style(style), Rect::new(content.x, y, content.width, 1));
        add_hit(&mut app.hits, Rect::new(content.x, y, content.width, 1), HitAction::SelectHabit(i));
    }
}

fn draw_graph_page(f: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 45 || area.height < 8 {
        return;
    }
    let sidebar_width = 27u16.min(area.width / 3);
    let side = Rect::new(area.x, area.y, sidebar_width, area.height);
    f.render_widget(
        Block::default()
            .title(" Habits ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
        side,
    );
    let side_inner = Rect::new(
        side.x + 1,
        side.y + 1,
        side.width.saturating_sub(2),
        side.height.saturating_sub(2),
    );

    let period_y = side_inner.y;
    let period_w = side_inner.width.saturating_sub(1) / 2;
    button(
        f,
        Rect::new(side_inner.x, period_y, period_w, 3),
        "Week",
        app.graph_period == GraphPeriod::Week,
        HitAction::GraphPeriod(GraphPeriod::Week),
        &mut app.hits,
    );
    button(
        f,
        Rect::new(side_inner.x + period_w + 1, period_y, side_inner.width.saturating_sub(period_w + 1), 3),
        "Month",
        app.graph_period == GraphPeriod::Month,
        HitAction::GraphPeriod(GraphPeriod::Month),
        &mut app.hits,
    );

    let mut y = period_y + 4;
    for (i, habit) in app.habits.iter().enumerate() {
        if y + 3 > side_inner.y + side_inner.height {
            break;
        }
        let text = habit_label(habit);
        button(
            f,
            Rect::new(side_inner.x, y, side_inner.width, 3),
            &text,
            i == app.graph_habit,
            HitAction::GraphHabit(i),
            &mut app.hits,
        );
        y += 4;
    }

    let graph_x = side.x + side.width + 1;
    let graph_width = area.x + area.width - graph_x;
    let chart_area = Rect::new(graph_x, area.y, graph_width, area.height);
    draw_graph_chart(f, app, chart_area);
}

fn draw_graph_chart(f: &mut Frame, app: &App, area: Rect) {
    if area.width < 28 || area.height < 8 {
        return;
    }
    let Some(habit) = app.habits.get(app.graph_habit.min(app.habits.len().saturating_sub(1))) else {
        f.render_widget(
            Paragraph::new("Add a habit to graph its progress.")
                .alignment(Alignment::Center)
                .block(Block::default().title(" Graph ").borders(Borders::ALL)),
            area,
        );
        return;
    };

    let dates: Vec<NaiveDate> = match app.graph_period {
        GraphPeriod::Week => {
            let start = app.date - Duration::days(app.date.weekday().num_days_from_monday() as i64);
            (0..7).map(|i| start + Duration::days(i)).collect()
        }
        GraphPeriod::Month => {
            let first = app.date.with_day(1).unwrap_or(app.date);
            let next = if app.date.month() == 12 {
                NaiveDate::from_ymd_opt(app.date.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(app.date.year(), app.date.month() + 1, 1).unwrap()
            };
            (0..next.signed_duration_since(first).num_days())
                .map(|i| first + Duration::days(i))
                .collect()
        }
    };

    let mut points = Vec::with_capacity(dates.len());
    let mut max_value = 0.0f64;
    for (i, date) in dates.iter().enumerate() {
        let value = app.value(habit, *date).ok().flatten().unwrap_or(0.0);
        max_value = max_value.max(value);
        points.push((i as f64 + 1.0, value));
    }

    // The y-axis always leaves one full increment above the highest recorded value.
    let step = if max_value < 5.0 { 1.0 } else { (max_value / 5.0).ceil().max(1.0) };
    let upper = ((max_value / step).ceil() * step) + step;
    let y_labels = (0..=((upper / step).round() as usize))
        .map(|i| Span::raw(format_number(i as f64 * step)))
        .collect::<Vec<_>>();

    let label_indices = if dates.len() <= 7 {
        (0..dates.len()).collect::<Vec<_>>()
    } else {
        let mut v = vec![0, dates.len() / 4, dates.len() / 2, (dates.len() * 3) / 4, dates.len() - 1];
        v.sort_unstable();
        v.dedup();
        v
    };
    let x_labels = label_indices
        .iter()
        .map(|i| Span::raw(format!("{:02}", dates[*i].day())))
        .collect::<Vec<_>>();

    let title = match app.graph_period {
        GraphPeriod::Week => format!(" {} · week of {} ", habit.name, dates[0].format("%d %b %Y")),
        GraphPeriod::Month => format!(" {} · {} {} ", habit.name, app.date.format("%B"), app.date.year()),
    };
    let dataset = Dataset::default()
        .name(habit.name.as_str())
        .marker(ratatui::symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Rgb(245, 245, 245)))
        .data(&points);
    let y_title = if habit.is_bool {
        "Done (0 / 1)"
    } else if habit.unit.is_empty() {
        "Value"
    } else {
        habit.unit.as_str()
    };
    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
        )
        .x_axis(
            Axis::default()
                .title("Date")
                .bounds([1.0, dates.len().max(2) as f64])
                .labels(x_labels)
                .style(Style::default().fg(Color::Rgb(205, 205, 205))),
        )
        .y_axis(
            Axis::default()
                .title(y_title)
                .bounds([0.0, upper.max(1.0)])
                .labels(y_labels)
                .style(Style::default().fg(Color::Rgb(205, 205, 205))),
        );
    f.render_widget(chart, area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let width = 62u16.min(area.width.saturating_sub(4));
    let height = 25u16.min(area.height.saturating_sub(4));
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    f.render_widget(Clear, popup);
    let yellow = Style::default().fg(Color::Rgb(245, 245, 245)).add_modifier(Modifier::BOLD);
    let lines = vec![
        Line::from(Span::styled("KEYBOARD SHORTCUTS", Style::default().fg(Color::Rgb(245, 245, 245)).add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::from(vec![Span::styled("1 / 2 / 3 / 4", yellow), Span::raw("  Weekly / Monthly / Graph / Total")]),
        Line::from(vec![Span::styled("Tab", yellow), Span::raw("          Switch to the next view")]),
        Line::from(vec![Span::styled("↑ / ↓", yellow), Span::raw("        Select a habit")]),
        Line::from(vec![Span::styled("← / →", yellow), Span::raw("        Move date or month")]),
        Line::from(vec![Span::styled("Enter", yellow), Span::raw("        Edit value / cycle graph habit")]),
        Line::from(vec![Span::styled("x", yellow), Span::raw("            Toggle yes/no habit")]),
        Line::from(vec![Span::styled("p", yellow), Span::raw("            Toggle graph Week / Month")]),
        Line::from(vec![Span::styled("t", yellow), Span::raw("            Toggle Total Week / Month")]),
        Line::from(vec![Span::styled("v", yellow), Span::raw("            Edit selected habit settings")]),
        Line::from(vec![Span::styled("a / +", yellow), Span::raw("        Add a habit")]),
        Line::from(vec![Span::styled("m", yellow), Span::raw("            Jump to month (YYYY-MM)")]),
        Line::from(vec![Span::styled("h", yellow), Span::raw("            Edit the custom header label")]),
        Line::from(vec![Span::styled("e", yellow), Span::raw("            Export all records to CSV")]),
        Line::from(vec![Span::styled("d", yellow), Span::raw("            Delete selected habit")]),
        Line::from(vec![Span::styled("? / Esc", yellow), Span::raw("      Close this help box")]),
        Line::raw(""),
        Line::from(Span::styled("MOUSE", Style::default().fg(Color::Rgb(245, 245, 245)).add_modifier(Modifier::BOLD))),
        Line::raw("Click tabs, date cells, habits, graph period and top controls."),
        Line::from(Span::styled("Click outside this box to close it.", Style::default().fg(Color::Rgb(205, 205, 205)))),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .style(Style::default().fg(Color::Rgb(205, 205, 205)).bg(Color::Rgb(12, 12, 12)))
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(245, 245, 245)))
                    .style(Style::default().bg(Color::Rgb(12, 12, 12))),
            ),
        popup,
    );
}

fn draw_dialog(f: &mut Frame, app: &App, area: Rect) {
    let width = 54u16.min(area.width.saturating_sub(4));
    let requested_height = if app.mode == Mode::EditHabitMenu { 14 } else { 9 };
    let height = requested_height.min(area.height.saturating_sub(4));
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    f.render_widget(Clear, popup);

    let (title, line1, line2) = match app.mode {
        Mode::AddName => (" Add habit ", "Habit name".to_string(), format!("{}▏", app.input)),
        Mode::AddUnit => (
            " Habit type ",
            "Unit (pages, reps, minutes); blank means yes/no".to_string(),
            format!("{}▏", app.input),
        ),
        Mode::EditValue => {
            let name = app.habits.get(app.selected).map(|h| h.name.as_str()).unwrap_or("habit");
            (
                " Edit entry ",
                format!("{} · {}", name, app.date.format("%d %b %Y")),
                format!("{}▏", app.input),
            )
        }
        Mode::MonthYear => (
            " Jump to month ",
            "Enter a month as YYYY-MM".to_string(),
            format!("{}▏", app.input),
        ),
        Mode::EditHeader => (
            " Edit header label ",
            "Text shown in the first column header".to_string(),
            format!("{}▏", app.input),
        ),
        Mode::EditHabitMenu => {
            let name = app.habits.get(app.selected).map(|h| h.name.as_str()).unwrap_or("habit");
            (" Edit habit ", format!("Selected: {name}"), "1 Rename   2 Unit   3 Type   4 Move up   5 Move down".to_string())
        }
        Mode::EditHabitName => (" Rename habit ", "Enter a new name".to_string(), format!("{}▏", app.input)),
        Mode::EditHabitUnit => (" Edit measurement unit ", "Examples: pages, reps, minutes; blank is allowed".to_string(), format!("{}▏", app.input)),
        Mode::ConfirmDelete => {
            let name = app.habits.get(app.selected).map(|h| h.name.as_str()).unwrap_or("this habit");
            (
                " Delete habit? ",
                "This also deletes its recorded entries.".to_string(),
                format!("Delete {}?  y/N", name),
            )
        }
        Mode::Normal => ("", String::new(), String::new()),
    };
    let instruction = match app.mode {
        Mode::AddName | Mode::AddUnit | Mode::EditValue | Mode::MonthYear | Mode::EditHeader | Mode::EditHabitName | Mode::EditHabitUnit => {
            "Enter · Save/Continue    Esc · Cancel"
        }
        Mode::EditHabitMenu => "1–5 · Choose action    Esc · Close menu",
        Mode::ConfirmDelete => "y · Delete    Any other key · Cancel",
        Mode::Normal => "",
    };
    let lines = if app.mode == Mode::EditHabitMenu {
        vec![
            Line::raw(""),
            Line::from(Span::raw(line1)),
            Line::raw(""),
            Line::from(Span::styled("1  Rename habit", Style::default().fg(Color::Rgb(245, 245, 245)))),
            Line::from(Span::styled("2  Change measurement unit", Style::default().fg(Color::Rgb(245, 245, 245)))),
            Line::from(Span::styled("3  Toggle numeric / yes-no type", Style::default().fg(Color::Rgb(245, 245, 245)))),
            Line::from(Span::styled("4  Move habit up", Style::default().fg(Color::Rgb(245, 245, 245)))),
            Line::from(Span::styled("5  Move habit down", Style::default().fg(Color::Rgb(245, 245, 245)))),
            Line::raw(""),
            Line::from(Span::styled(instruction, Style::default().fg(Color::Rgb(205, 205, 205)))),
        ]
    } else {
        vec![
            Line::raw(""),
            Line::from(Span::raw(line1)),
            Line::raw(""),
            Line::from(Span::styled(line2, Style::default().fg(Color::Rgb(245, 245, 245)))),
            Line::raw(""),
            Line::from(Span::styled(instruction, Style::default().fg(Color::Rgb(205, 205, 205)))),
        ]
    };
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(245, 245, 245)))
                    .style(Style::default().bg(Color::Rgb(12, 12, 12))),
            ),
        popup,
    );
}

fn shift_month(date: NaiveDate, delta: i32) -> NaiveDate {
    let index = date.year() * 12 + date.month0() as i32 + delta;
    let year = index.div_euclid(12);
    let month = index.rem_euclid(12) as u32 + 1;
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    first + Duration::days((date.day().min((next - first).num_days() as u32) - 1) as i64)
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value:.2}")
    }
}

fn do_action(app: &mut App, action: HitAction) -> Result<()> {
    match action {
        HitAction::SwitchPage(page) => app.set_page(page),
        HitAction::MonthYear => app.begin_month_jump(),
        HitAction::EditHeader => app.begin_edit_header(),
        HitAction::Export => {
            if let Err(error) = app.export() {
                app.message = format!("Export failed: {error}");
            }
        }
        HitAction::AddHabit => app.begin_add(),
        HitAction::EditHabit => app.begin_edit_habit(),
        HitAction::TotalPeriod(period) => { app.total_period = period; app.message.clear(); }
        HitAction::SelectHabit(index) => {
            app.selected = index.min(app.habits.len().saturating_sub(1));
            app.graph_habit = app.selected;
            app.message.clear();
        }
        HitAction::SelectDate(date) => {
            app.date = date;
            app.message.clear();
        }
        HitAction::EditCell(index, date) => {
            app.selected = index.min(app.habits.len().saturating_sub(1));
            app.graph_habit = app.selected;
            app.date = date;
            app.edit_selected()?;
        }
        HitAction::GraphPeriod(period) => {
            app.graph_period = period;
            app.message.clear();
        }
        HitAction::GraphHabit(index) => {
            app.graph_habit = index.min(app.habits.len().saturating_sub(1));
            app.selected = app.graph_habit;
            app.message.clear();
        }
    }
    Ok(())
}

fn handle_modal_key(app: &mut App, code: KeyCode) -> Result<()> {
    if app.mode == Mode::EditHabitMenu {
        match code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Char('1') => { if let Some(h) = app.habits.get(app.selected) { app.input = h.name.clone(); app.mode = Mode::EditHabitName; } },
            KeyCode::Char('2') => { if let Some(h) = app.habits.get(app.selected) { app.input = h.unit.clone(); app.mode = Mode::EditHabitUnit; } },
            KeyCode::Char('3') => app.toggle_habit_type()?,
            KeyCode::Char('4') => app.reorder_selected(-1)?,
            KeyCode::Char('5') => app.reorder_selected(1)?,
            _ => {}
        }
        return Ok(());
    }
    match code {
        KeyCode::Esc => {
            app.mode = if matches!(app.mode, Mode::EditHabitName | Mode::EditHabitUnit) { Mode::EditHabitMenu } else { Mode::Normal };
            app.input.clear();
            app.message = "Cancelled.".into();
        }
        KeyCode::Enter => match app.mode {
            Mode::AddName => {
                let name = app.input.trim().to_string();
                if name.is_empty() {
                    app.message = "Habit name cannot be empty.".into();
                    app.mode = Mode::Normal;
                } else {
                    app.pending_name = name;
                    app.input.clear();
                    app.mode = Mode::AddUnit;
                }
            }
            Mode::AddUnit => {
                let name = std::mem::take(&mut app.pending_name);
                let unit = std::mem::take(&mut app.input);
                match app.add_habit(name, unit) {
                    Ok(()) => app.mode = Mode::Normal,
                    Err(error) => {
                        app.message = format!("Could not add habit: {error}");
                        app.mode = Mode::Normal;
                    }
                }
                app.input.clear();
            }
            Mode::EditValue => {
                if let Some(habit) = app.habits.get(app.selected).cloned() {
                    if app.input.trim().is_empty() {
                        app.db.execute(
                            "DELETE FROM entries WHERE habit_id=?1 AND date=?2",
                            params![habit.id, app.date.to_string()],
                        )?;
                        app.message = "Entry cleared.".into();
                    } else if let Ok(value) = app.input.trim().parse::<f64>() {
                        if value >= 0.0 {
                            app.set_value(&habit, app.date, value)?;
                            app.message = format!("Saved {} for {}.", format_number(value), habit.name);
                        } else {
                            app.message = "Values cannot be negative.".into();
                        }
                    } else {
                        app.message = "Enter a valid non-negative number.".into();
                    }
                }
                app.input.clear();
                app.mode = Mode::Normal;
            }
            Mode::MonthYear => {
                let text = app.input.trim().to_string();
                if let Ok(date) = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d") {
                    app.date = date;
                    app.message = format!("Showing {} {}.", date.format("%B"), date.year());
                } else {
                    app.message = "Use YYYY-MM, for example 2026-10.".into();
                }
                app.input.clear();
                app.mode = Mode::Normal;
            }
            Mode::EditHeader => {
                let text = app.input.clone();
                app.db.execute(
                    "INSERT INTO settings(key,value) VALUES('header_text',?1)
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![text],
                )?;
                app.header_text = text;
                app.input.clear();
                app.mode = Mode::Normal;
                app.message = "Header label saved.".into();
            }
            Mode::EditHabitName => {
                let text = app.input.trim().to_string();
                if text.is_empty() { app.message = "Habit name cannot be empty.".into(); }
                else if let Some(habit) = app.habits.get(app.selected).cloned() {
                    match app.db.execute("UPDATE habits SET name=?1 WHERE id=?2", params![text, habit.id]) {
                        Ok(_) => { app.message = "Habit renamed.".into(); app.reload_habits()?; app.mode = Mode::EditHabitMenu; },
                        Err(error) => { app.message = format!("Could not rename habit: {error}"); app.mode = Mode::EditHabitMenu; }
                    }
                }
                app.input.clear();
            }
            Mode::EditHabitUnit => {
                let text = app.input.trim().to_string();
                if let Some(habit) = app.habits.get(app.selected).cloned() {
                    app.db.execute("UPDATE habits SET unit=?1 WHERE id=?2", params![text, habit.id])?;
                    app.message = "Measurement unit updated.".into();
                    app.reload_habits()?;
                    app.mode = Mode::EditHabitMenu;
                }
                app.input.clear();
            }
            _ => app.mode = Mode::Normal,
        },
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(c) if app.mode != Mode::ConfirmDelete => app.input.push(c),
        KeyCode::Char('y') | KeyCode::Char('Y') if app.mode == Mode::ConfirmDelete => {
            if let Some(habit) = app.habits.get(app.selected) {
                app.db.execute("DELETE FROM habits WHERE id=?1", [habit.id])?;
            }
            app.reload_habits()?;
            app.mode = Mode::Normal;
            app.message = "Habit and its entries deleted.".into();
        }
        _ if app.mode == Mode::ConfirmDelete => {
            app.mode = Mode::Normal;
            app.message = "Deletion cancelled.".into();
        }
        _ => {}
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut app = App::new()?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal, &mut app);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result?;
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| draw(frame, app))?;
        match event::read()? {
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                if app.show_help {
                    app.show_help = false;
                    continue;
                }
                if app.mode != Mode::Normal {
                    continue;
                }
                if let Some(hit) = app
                    .hits
                    .iter()
                    .rev()
                    .find(|hit| contains(hit.rect, mouse.column, mouse.row))
                    .copied()
                {
                    do_action(app, hit.action)?;
                }
            }
            Event::Key(key) => {
                if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
                if app.show_help {
                    let is_help_key = key.code == KeyCode::Char('?')
                        || (key.code == KeyCode::Char('/') && key.modifiers.contains(KeyModifiers::SHIFT));
                    if key.code == KeyCode::Esc || is_help_key || key.code == KeyCode::Char('q') {
                        app.show_help = false;
                    }
                    continue;
                }
                if app.mode != Mode::Normal {
                    handle_modal_key(app, key.code)?;
                    continue;
                }

                let is_help_key = key.code == KeyCode::Char('?')
                    || (key.code == KeyCode::Char('/') && key.modifiers.contains(KeyModifiers::SHIFT));
                if is_help_key {
                    app.show_help = true;
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Up => {
                        app.selected = app.selected.saturating_sub(1);
                        app.graph_habit = app.selected;
                    }
                    KeyCode::Down => {
                        app.selected = (app.selected + 1).min(app.habits.len().saturating_sub(1));
                        app.graph_habit = app.selected;
                    }
                    KeyCode::Left => app.shift_date(-1),
                    KeyCode::Right => app.shift_date(1),
                    KeyCode::Char('1') => app.set_page(Page::Weekly),
                    KeyCode::Char('2') => app.set_page(Page::Monthly),
                    KeyCode::Char('3') => app.set_page(Page::Graph),
                    KeyCode::Char('4') => app.set_page(Page::Total),
                    KeyCode::Tab => {
                        app.set_page(match app.page {
                            Page::Weekly => Page::Monthly,
                            Page::Monthly => Page::Graph,
                            Page::Graph => Page::Total,
                            Page::Total => Page::Weekly,
                        });
                    }
                    KeyCode::Enter => {
                        if app.page == Page::Graph {
                            if !app.habits.is_empty() {
                                app.graph_habit = (app.graph_habit + 1) % app.habits.len();
                                app.selected = app.graph_habit;
                            }
                        } else {
                            app.edit_selected()?;
                        }
                    }
                    KeyCode::Char('x') => {
                        if let Some(habit) = app.habits.get(app.selected) {
                            if habit.is_bool {
                                app.edit_selected()?;
                            } else {
                                app.message = "Select a numeric cell and press Enter to edit it.".into();
                            }
                        }
                    }
                    KeyCode::Char('p') if app.page == Page::Graph => {
                        app.graph_period = match app.graph_period {
                            GraphPeriod::Week => GraphPeriod::Month,
                            GraphPeriod::Month => GraphPeriod::Week,
                        };
                    }
                    KeyCode::Char('t') if app.page == Page::Total => {
                        app.total_period = match app.total_period { GraphPeriod::Week => GraphPeriod::Month, GraphPeriod::Month => GraphPeriod::Week };
                    }
                    KeyCode::Char('v') => app.begin_edit_habit(),
                    KeyCode::Char('a') | KeyCode::Char('+') => app.begin_add(),
                    KeyCode::Char('m') => app.begin_month_jump(),
                    KeyCode::Char('h') => app.begin_edit_header(),
                    KeyCode::Char('e') => {
                        if let Err(error) = app.export() {
                            app.message = format!("Export failed: {error}");
                        }
                    }
                    KeyCode::Char('d') => {
                        if let Some(habit) = app.habits.get(app.selected) {
                            app.message = format!("Delete '{}' and its entries? Press y to confirm.", habit.name);
                            app.mode = Mode::ConfirmDelete;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}
