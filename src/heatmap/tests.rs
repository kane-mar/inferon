use super::*;

fn cell(requests: i64, errors: i64, p95: Option<f64>) -> Cell {
    Cell { requests, errors, p95_ms: p95 }
}

fn buckets() -> Vec<String> {
    vec!["2026-10-29".into(), "2026-10-30".into(), "2026-10-31".into()]
}

#[test]
fn a_missing_bucket_is_empty_not_zero() {
    let rows = vec![(
        "deepseek-flash".to_string(),
        "2026-10-30".to_string(),
        cell(10, 1, Some(120.0)),
    )];
    let pivoted = pivot(&buckets(), &rows);
    assert_eq!(pivoted.len(), 1);
    let cells = &pivoted[0].1;
    assert_eq!(cells[0], None, "no traffic is not a fast day");
    assert_eq!(cells[1], Some(cell(10, 1, Some(120.0))));
    assert_eq!(cells[2], None);
}

#[test]
fn every_model_gets_one_cell_per_bucket() {
    let rows = vec![
        ("a".to_string(), "2026-10-29".to_string(), cell(1, 0, Some(10.0))),
        ("b".to_string(), "2026-10-31".to_string(), cell(1, 0, Some(20.0))),
    ];
    let pivoted = pivot(&buckets(), &rows);
    assert_eq!(pivoted.len(), 2);
    assert!(pivoted.iter().all(|(_, cells)| cells.len() == 3));
}

#[test]
fn the_busiest_model_comes_first() {
    let rows = vec![
        ("quiet".to_string(), "2026-10-29".to_string(), cell(1, 0, Some(10.0))),
        ("busy".to_string(), "2026-10-29".to_string(), cell(100, 2, Some(30.0))),
    ];
    let pivoted = pivot(&buckets(), &rows);
    assert_eq!(pivoted[0].0, "busy");
    assert_eq!(pivoted[1].0, "quiet");
}

#[test]
fn the_error_rate_is_per_bucket() {
    assert_eq!(cell(0, 0, None).error_rate(), None);
    assert_eq!(cell(4, 1, None).error_rate(), Some(0.25));
}

#[test]
fn only_hour_and_day_are_bucket_steps() {
    assert_eq!(bucket_expr("hour"), "hour");
    assert_eq!(bucket_expr("day"), "day");
    assert_eq!(bucket_expr("'; drop table traces; --"), "day");
}
