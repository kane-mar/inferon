//! Guards for the trace insert.
//!
//! A broken INSERT is invisible to the in-memory sink tests (they never reach
//! Postgres) and only shows up as a silently dropped batch in production. These
//! tests read the statement as data, so a duplicate column or a column the
//! migrations never create fails here instead of on the deploy.

use super::TRACE_INSERT;

/// `(a, b, c)` from the statement, in order.
fn insert_columns() -> Vec<String> {
    let statement = TRACE_INSERT;
    let open = statement.find('(').expect("column list");
    let close = statement[open..]
        .find(')')
        .map(|i| open + i)
        .expect("column list ends");
    statement[open + 1..close]
        .split(',')
        .map(|column| column.trim().to_string())
        .filter(|column| !column.is_empty())
        .collect()
}

/// `$1..$n` from the VALUES clause.
fn placeholders() -> Vec<usize> {
    let values = TRACE_INSERT.split_once("VALUES").expect("VALUES clause").1;
    values
        .split('$')
        .skip(1)
        .filter_map(|value| {
            let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

/// Every column of `traces` the migrations create, in file order.
fn migrated_columns() -> Vec<String> {
    let mut columns = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir("db/init")
        .expect("db/init")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();
    for path in files {
        let sql = std::fs::read_to_string(&path).expect("migration is readable");
        // Strip comments before splitting: prose may contain a semicolon.
        let sql = sql
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for statement in sql.split(';') {
            let statement = statement.split_whitespace().collect::<Vec<_>>().join(" ");
            let words: Vec<&str> = statement.split_whitespace().collect();
            let touches_traces = words.contains(&"traces");
            if statement.starts_with("CREATE TABLE") && touches_traces {
                let open = statement.find('(').expect("column list");
                let close = statement.rfind(')').expect("column list ends");
                for column in statement[open + 1..close].split(',') {
                    if let Some(name) = column.split_whitespace().next() {
                        columns.push(name.to_string());
                    }
                }
            } else if statement.starts_with("ALTER TABLE") && touches_traces {
                let Some(added) = statement.split_once("ADD COLUMN").map(|(_, rest)| rest) else {
                    continue;
                };
                let name = added
                    .split_whitespace()
                    .find(|word| !matches!(*word, "IF" | "NOT" | "EXISTS"))
                    .expect("a column name after ADD COLUMN");
                columns.push(name.to_string());
            }
        }
    }
    columns
}

#[test]
fn the_trace_insert_names_each_column_once() {
    let mut seen = std::collections::HashSet::new();
    let duplicated: Vec<String> = insert_columns()
        .into_iter()
        .filter(|column| !seen.insert(column.clone()))
        .collect();
    assert!(
        duplicated.is_empty(),
        "columns listed more than once: {duplicated:?}"
    );
}

#[test]
fn the_trace_insert_binds_one_value_per_column() {
    let columns = insert_columns();
    let placeholders = placeholders();
    assert_eq!(
        columns.len(),
        placeholders.len(),
        "{} columns but {} placeholders",
        columns.len(),
        placeholders.len()
    );
    assert_eq!(
        placeholders,
        (1..=columns.len()).collect::<Vec<_>>(),
        "placeholders are not $1..${}",
        columns.len()
    );
}

#[test]
fn the_trace_insert_only_names_migrated_columns() {
    let migrated = migrated_columns();
    assert!(
        migrated.contains(&"tenant_id".to_string()),
        "the migration parser found no columns: {migrated:?}"
    );
    let unknown: Vec<String> = insert_columns()
        .into_iter()
        .filter(|column| !migrated.contains(column))
        .collect();
    assert!(unknown.is_empty(), "not created by any migration: {unknown:?}");
}
