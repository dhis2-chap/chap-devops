use super::*;

#[test]
fn each_backtest_is_its_own_request_before_the_dataset() {
    assert_eq!(
        row_paths(&[1, 2], Some(9)),
        [
            "/v1/crud/backtests/1",
            "/v1/crud/backtests/2",
            "/v1/crud/datasets/9"
        ]
    );
}

#[test]
fn nothing_made_is_nothing_to_delete() {
    assert!(row_paths(&[], None).is_empty());
    assert_eq!(kept_line(&[], None), None);
}

#[test]
fn the_kept_line_names_the_rows_and_how_to_remove_them() {
    assert_eq!(
        kept_line(&[8], Some(7)).unwrap(),
        "kept backtest 8 and dataset 7; remove them with `chap-devops api DELETE \
         /v1/crud/backtests/8` then `chap-devops api DELETE /v1/crud/datasets/7`"
    );
    assert_eq!(
        kept_line(&[25, 26, 27, 28], Some(7)).unwrap(),
        "kept backtests 25, 26, 27 and 28 and dataset 7; remove each backtest with \
         `chap-devops api DELETE /v1/crud/backtests/ID`, then the dataset with \
         `chap-devops api DELETE /v1/crud/datasets/7`"
    );
    assert_eq!(
        kept_line(&[], Some(7)).unwrap(),
        "kept dataset 7; remove it with `chap-devops api DELETE /v1/crud/datasets/7`"
    );
}
