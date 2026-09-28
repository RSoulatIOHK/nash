//! Executed source snapshots for wildcard expansion and shared branch helpers.
use super::core_eval;

#[test]
fn little_wildcard_mixed_arities_trace_order() {
    let evaluated = core_eval(
        "little_wildcard_mixed_arities_trace_order",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> trace "explicit" 1
                _ -> trace "fallback" 9
        main = (choose Stop, choose Zero, choose (One 2), choose (Two 3 4))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_shared_failure_unselected() {
    let evaluated = core_eval(
        "little_wildcard_shared_failure_unselected",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> trace "explicit" 1
                _ -> trace "fallback" fail
        main = choose Stop
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_shared_failure_zero() {
    let evaluated = core_eval(
        "little_wildcard_shared_failure_zero",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> 1
                _ -> trace "fallback" fail
        main = choose Zero
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_shared_failure_one() {
    let evaluated = core_eval(
        "little_wildcard_shared_failure_one",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> 1
                _ -> trace "fallback" fail
        main = choose (One 2)
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_shared_failure_two() {
    let evaluated = core_eval(
        "little_wildcard_shared_failure_two",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> 1
                _ -> trace "fallback" fail
        main = choose (Two 3 4)
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_returns_captured_function() {
    let evaluated = core_eval(
        "little_wildcard_returns_captured_function",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : int -> choice -> (int -> int)
        choose outer subject =
            case trace "subject" subject of
                Stop -> trace "explicit" (\n -> n)
                _ -> trace "fallback" (\n -> addInteger outer n)
        main = (choose 10 Stop 5, choose 10 Zero 5, choose 10 (One 2) 5, choose 10 (Two 3 4) 5)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_binds_whole_and_captures_outer() {
    let evaluated = core_eval(
        "little_wildcard_binds_whole_and_captures_outer",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        read : choice -> int
        read value =
            case value of
                Stop -> 0
                Zero -> 0
                One a -> a
                Two a b -> addInteger (a) (b)
        choose : int -> choice -> int
        choose outer subject =
            case trace "subject" subject of
                Stop -> 1
                whole -> trace "fallback" (addInteger outer (read whole))
        main = (choose 10 Stop, choose 10 Zero, choose 10 (One 20), choose 10 (Two 30 40))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_shared_helper_receives_bound_values() {
    let evaluated = core_eval(
        "little_wildcard_shared_helper_receives_bound_values",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : (choice, int) -> int
        choose subject =
            case trace "subject" subject of
                (Stop, _) -> 0
                (_, value) -> trace "shared" value
        main = (choose (Stop, 1), choose (Zero, 11), choose (One 2, 22), choose (Two 3 4, 33))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_unknown_tag_fails_dispatch() {
    let evaluated = core_eval(
        "little_wildcard_unknown_tag_fails_dispatch",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        type rogue = R0 | R1 | R2 | R3 | R4
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> trace "explicit" 1
                _ -> trace "fallback" 9
        main = choose (coerce R4)
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_only_does_not_inspect_unknown_tag() {
    let evaluated = core_eval(
        "little_wildcard_only_does_not_inspect_unknown_tag",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        type rogue = R0 | R1 | R2 | R3 | R4
        choose : choice -> int
        choose subject =
            case trace "subject" subject of
                _ -> trace "fallback" 9
        main = choose (coerce R4)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_mixed_arities_trace_order() {
    let evaluated = core_eval(
        "big_wildcard_mixed_arities_trace_order",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> trace "explicit" 1
                _ -> trace "fallback" 9
        main = (choose Stop, choose Zero, choose (One 2), choose (Two 3 4))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_shared_failure_unselected() {
    let evaluated = core_eval(
        "big_wildcard_shared_failure_unselected",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> trace "explicit" 1
                _ -> trace "fallback" fail
        main = choose Stop
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_shared_failure_zero() {
    let evaluated = core_eval(
        "big_wildcard_shared_failure_zero",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> 1
                _ -> trace "fallback" fail
        main = choose Zero
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_shared_failure_one() {
    let evaluated = core_eval(
        "big_wildcard_shared_failure_one",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> 1
                _ -> trace "fallback" fail
        main = choose (One 2)
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_shared_failure_two() {
    let evaluated = core_eval(
        "big_wildcard_shared_failure_two",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> 1
                _ -> trace "fallback" fail
        main = choose (Two 3 4)
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_returns_captured_function() {
    let evaluated = core_eval(
        "big_wildcard_returns_captured_function",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : int -> Choice -> (int -> int)
        choose outer subject =
            case trace "subject" subject of
                Stop -> trace "explicit" (\n -> n)
                _ -> trace "fallback" (\n -> addInteger outer n)
        main = (choose 10 Stop 5, choose 10 Zero 5, choose 10 (One 2) 5, choose 10 (Two 3 4) 5)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_binds_whole_and_captures_outer() {
    let evaluated = core_eval(
        "big_wildcard_binds_whole_and_captures_outer",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        read : Choice -> int
        read value =
            case value of
                Stop -> 0
                Zero -> 0
                One a -> unIData a
                Two a b -> addInteger (unIData a) (unIData b)
        choose : int -> Choice -> int
        choose outer subject =
            case trace "subject" subject of
                Stop -> 1
                whole -> trace "fallback" (addInteger outer (read whole))
        main = (choose 10 Stop, choose 10 Zero, choose 10 (One 20), choose 10 (Two 30 40))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_shared_helper_receives_bound_values() {
    let evaluated = core_eval(
        "big_wildcard_shared_helper_receives_bound_values",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : (Choice, int) -> int
        choose subject =
            case trace "subject" subject of
                (Stop, _) -> 0
                (_, value) -> trace "shared" value
        main = (choose (Stop, 1), choose (Zero, 11), choose (One 2, 22), choose (Two 3 4, 33))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_unknown_tag_fails_dispatch() {
    let evaluated = core_eval(
        "big_wildcard_unknown_tag_fails_dispatch",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                Stop -> trace "explicit" 1
                _ -> trace "fallback" 9
        main = choose (coerce (constrData 59 []))
        "#
        ),
    );
    assert!(
        evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_only_does_not_inspect_unknown_tag() {
    let evaluated = core_eval(
        "big_wildcard_only_does_not_inspect_unknown_tag",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case trace "subject" subject of
                _ -> trace "fallback" 9
        main = choose (coerce (constrData 59 []))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_does_not_extract_missing_fields() {
    let evaluated = core_eval(
        "big_wildcard_does_not_extract_missing_fields",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | One Int | Two Int Int
        choose : Choice -> int
        choose subject =
            case subject of
                Stop -> 1
                _ -> trace "fallback" 9
        main = (choose (coerce (constrData 1 [])), choose (coerce (constrData 2 [])))
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_calls_user_continuation_only_when_selected() {
    let evaluated = core_eval(
        "little_wildcard_calls_user_continuation_only_when_selected",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : choice -> (int -> int) -> int
        choose subject continue =
            case trace "subject" subject of
                Stop -> 0
                _ -> trace "fallback" (continue 9)
        finish : int -> int
        finish n = trace "continue" (addInteger n 100)
        main = (choose Stop (\_ -> trace "wrong" fail), choose Zero finish, choose (One 2) finish, choose (Two 3 4) finish)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_inline_function_result_consumes_fields() {
    let evaluated = core_eval(
        "little_wildcard_inline_function_result_consumes_fields",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Two int int
        choose : choice -> (int -> int)
        choose subject =
            case trace "subject" subject of
                Stop -> \n -> n
                _ -> trace "fallback" (\n -> addInteger n 1)
        main = (choose Stop 10, choose (Two 3 4) 20)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_calls_user_continuation_only_when_selected() {
    let evaluated = core_eval(
        "big_wildcard_calls_user_continuation_only_when_selected",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : Choice -> (int -> int) -> int
        choose subject continue =
            case trace "subject" subject of
                Stop -> 0
                _ -> trace "fallback" (continue 9)
        finish : int -> int
        finish n = trace "continue" (addInteger n 100)
        main = (choose Stop (\_ -> trace "wrong" fail), choose Zero finish, choose (One 2) finish, choose (Two 3 4) finish)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_inline_function_result_consumes_fields() {
    let evaluated = core_eval(
        "big_wildcard_inline_function_result_consumes_fields",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Two Int Int
        choose : Choice -> (int -> int)
        choose subject =
            case trace "subject" subject of
                Stop -> \n -> n
                _ -> trace "fallback" (\n -> addInteger n 1)
        main = (choose Stop 10, choose (Two 3 4) 20)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn little_wildcard_multiple_bindings_return_function() {
    let evaluated = core_eval(
        "little_wildcard_multiple_bindings_return_function",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type choice = Stop | Zero | One int | Two int int
        choose : (choice, int, int) -> (int -> int)
        choose subject =
            case trace "subject" subject of
                (Stop, _, _) -> \n -> n
                (_, z, a) -> trace "shared" (\n -> subtractInteger (addInteger z n) a)
        main = (choose (Stop, 90, 80) 1, choose (Zero, 21, 3) 1, choose (One 2, 31, 4) 1, choose (Two 3 4, 41, 5) 1)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}

#[test]
fn big_wildcard_multiple_bindings_return_function() {
    let evaluated = core_eval(
        "big_wildcard_multiple_bindings_return_function",
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Choice = Stop | Zero | One Int | Two Int Int
        choose : (Choice, int, int) -> (int -> int)
        choose subject =
            case trace "subject" subject of
                (Stop, _, _) -> \n -> n
                (_, z, a) -> trace "shared" (\n -> subtractInteger (addInteger z n) a)
        main = (choose (Stop, 90, 80) 1, choose (Zero, 21, 3) 1, choose (One 2, 31, 4) 1, choose (Two 3 4, 41, 5) 1)
        "#
        ),
    );

    assert!(
        !evaluated.result.starts_with("error:"),
        "{}",
        evaluated.result
    );
}
