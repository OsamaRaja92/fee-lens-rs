//! A minimal fixed-width table renderer.
//!
//! Written rather than taken from a dependency because the requirement is
//! narrow: numbers right-aligned, text left-aligned, and column widths measured
//! on the text itself. Everything here is plain text, so there are no colour
//! escapes to measure around.

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

pub struct Column {
    pub header: &'static str,
    pub align: Align,
}

fn pad(text: &str, width: usize, align: Align) -> String {
    let padding = " ".repeat(width.saturating_sub(text.chars().count()));
    match align {
        Align::Right => format!("{padding}{text}"),
        Align::Left => format!("{text}{padding}"),
    }
}

/// Renders a table, indented by two spaces to sit inside a section.
#[must_use]
pub fn render(columns: &[Column], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .chain(std::iter::once(column.header.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let line = |cells: &[String]| -> String {
        let joined = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                pad(
                    cell,
                    widths[index],
                    columns.get(index).map_or(Align::Left, |c| c.align),
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        format!("  {}", joined.trim_end())
    };

    let headers: Vec<String> = columns.iter().map(|c| (*c.header).to_owned()).collect();
    let rule: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();

    let mut out = vec![line(&headers), line(&rule)];
    out.extend(rows.iter().map(|row| line(row)));
    out.join("\n")
}
