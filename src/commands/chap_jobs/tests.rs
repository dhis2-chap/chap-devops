use super::*;

#[test]
fn one_backtest_and_the_dataset_are_two_rows() {
    assert_eq!(
        row_paths(&[4], Some(1)),
        [
            ("backtest 4".to_string(), "/v1/crud/backtests/4".to_string()),
            ("dataset 1".to_string(), "/v1/crud/datasets/1".to_string()),
        ]
    );
}

#[test]
fn the_backtests_of_the_baselines_go_in_one_request_before_the_dataset() {
    assert_eq!(
        row_paths(&[1, 2, 3, 4], Some(1)),
        [
            (
                "backtests 1, 2, 3 and 4".to_string(),
                "/v1/crud/backtests?ids=1,2,3,4".to_string()
            ),
            ("dataset 1".to_string(), "/v1/crud/datasets/1".to_string()),
        ]
    );
    assert_eq!(
        shell_word("/v1/crud/backtests?ids=1,2"),
        "'/v1/crud/backtests?ids=1,2'"
    );
    assert_eq!(shell_word("/v1/crud/datasets/1"), "/v1/crud/datasets/1");
}

#[test]
fn nothing_made_is_nothing_to_delete() {
    assert!(row_paths(&[], None).is_empty());
}
