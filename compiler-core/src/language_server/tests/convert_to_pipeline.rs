use crate::line_numbers::LineNumbers;
use lsp_types::{
    CodeActionContext, CodeActionParams, PartialResultParams, Position, Range,
    TextDocumentIdentifier, Url, WorkDoneProgressParams, WorkspaceEdit,
};

use super::*;

macro_rules! assert_code_action {
    ($src:expr, $position_start:expr, $position_end:expr) => {
        let result = convert_to_pipeline($src, $position_start, $position_end);
        insta::assert_snapshot!(insta::internals::AutoName, result, $src);
    };
}

fn convert_to_pipeline(src: &str, position_start: Position, position_end: Position) -> String {
    let io = LanguageServerTestIO::new();
    let mut engine = setup_engine(&io);

    _ = io.src_module(
        "list",
        r#"
            pub fn map(list: List(a), with fun: fn(a) -> b) -> List(b) {
                do_map(list, fun, [])
            }
            fn do_map(list: List(a), fun: fn(a) -> b, acc: List(b)) -> List(b) {
                case list {
                    [] -> reverse(acc)
                    [x, ..xs] -> do_map(xs, fun, [fun(x), ..acc])
                }
            }
            pub fn take_while(
                in list: List(a),
                satisfying predicate: fn(a) -> Bool,
              ) -> List(a) {
                do_take_while(list, predicate, [])
            }
            fn do_take_while(
                list: List(a),
                predicate: fn(a) -> Bool,
                acc: List(a),
              ) -> List(a) {
                case list {
                  [] -> reverse(acc)
                  [first, ..rest] ->
                    case predicate(first) {
                      True -> do_take_while(rest, predicate, [first, ..acc])
                      False -> reverse(acc)
                    }
                }
              }
            
            pub fn reverse(xs: List(a)) -> List(a) {
                do_reverse(xs)
            }
            
            fn do_reverse(list) {
                do_reverse_acc(list, [])
            }
            
            fn do_reverse_acc(remaining, accumulator) {
                case remaining {
                    [] -> accumulator
                    [item, ..rest] -> do_reverse_acc(rest, [item, ..accumulator])
                }
            }
            
            pub fn map2(
                list1: List(a),
                list2: List(b),
                with fun: fn(a, b) -> c,
              ) -> List(c) {
                do_map2(list1, list2, fun, [])
              }
              
              fn do_map2(
                list1: List(a),
                list2: List(b),
                fun: fn(a, b) -> c,
                acc: List(c),
              ) -> List(c) {
                case list1, list2 {
                  [], _ | _, [] -> reverse(acc)
                  [a, ..as_], [b, ..bs] -> do_map2(as_, bs, fun, [fun(a, b), ..acc])
                }
            }

            fn do_zip(xs: List(a), ys: List(b), acc: List(#(a, b))) -> List(#(a, b)) {
                case xs, ys {
                  [x, ..xs], [y, ..ys] -> do_zip(xs, ys, [#(x, y), ..acc])
                  _, _ -> reverse(acc)
                }
              }
              
              pub fn zip(list: List(a), with other: List(b)) -> List(#(a, b)) {
                do_zip(list, other, [])
              }
        "#,
    );

    _ = io.src_module("app", src);
    engine.compile_please().result.expect("compiled");

    // create the code action request
    let path = Utf8PathBuf::from(if cfg!(target_family = "windows") {
        r"\\?\C:\src\app.gleam"
    } else {
        "/src/app.gleam"
    });

    let url = Url::from_file_path(path).unwrap();

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier::new(url.clone()),
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        range: Range::new(position_start, position_end),
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let response = engine.action(params).result.unwrap().and_then(|actions| {
        actions
            .into_iter()
            .find(|action| action.title == "Apply Pipeline Rewrite")
    });
    if let Some(action) = response {
        apply_code_action(src, &url, &action)
    } else {
        panic!("No code action produced by the engine")
    }
}

fn apply_code_action(src: &str, url: &Url, action: &lsp_types::CodeAction) -> String {
    match &action.edit {
        Some(WorkspaceEdit { changes, .. }) => match changes {
            Some(changes) => apply_code_edit(src, url, changes),
            None => panic!("No text edit found"),
        },
        _ => panic!("No workspace edit found"),
    }
}

// This function replicates how the text editor applies TextEdit
fn apply_code_edit(
    src: &str,
    url: &Url,
    changes: &HashMap<Url, Vec<lsp_types::TextEdit>>,
) -> String {
    let mut result = src.to_string();
    let line_numbers = LineNumbers::new(src);
    let mut offset = 0;
    for (change_url, change) in changes {
        if url != change_url {
            panic!("Unknown url {}", change_url)
        }
        for edit in change {
            let start =
                line_numbers.byte_index(edit.range.start.line, edit.range.start.character) - offset;
            let end =
                line_numbers.byte_index(edit.range.end.line, edit.range.end.character) - offset;
            let range = (start as usize)..(end as usize);
            offset += end - start;
            result.replace_range(range, &edit.new_text);
        }
    }
    result
}

#[test]
fn test_converting_assign_to_pipeline() {
    assert_code_action!(
        r#"
import list

fn main() {
  let result = list.reverse(list.zip([1,2,3], [4,5,6]))
}
"#,
        Position::new(4, 15),
        Position::new(4, 61)
    );
}
#[test]
fn test_converting_assign_to_pipeline2() {
    assert_code_action!(
        r#"
import list

fn main() {
  let result = list.reverse(list.zip(list.reverse([1,2,3]), list.reverse([4,5,6])))
}
"#,
        Position::new(4, 15),
        Position::new(4, 63)
    );
}

#[test]
fn test_suggest_pipeline_assignment_func_chaining_func_as_input_pipeline() {
    assert_code_action!(
        r#"
import list

fn main() {
  let result = list.reverse(list.map(buildlist(), add1))
}

fn buildlist() -> List(Int) {
    [1, 2, 3]
}

fn add1(i: Int) -> Int{
    i + 1
}
"#,
        Position::new(4, 15),
        Position::new(4, 63)
    );
    // With Pipeline Operator
    //     let expected = "
    // import list

    // fn main() {
    //   let result =
    // buildlist()
    // |> list.map(fn(x) { x * 2 })
    // |> list.reverse()
    // }

    // fn buildlist() -> List(Int) {
    //   [1, 2, 3]
    // }
    // ";
}

#[test]
fn test_converting_expr_to_pipeline() {
    assert_code_action!(
        r#"
import list

fn main() {
  list.reverse(list.zip(list.reverse([1,2,3]), list.reverse([4,5,6])))
}
"#,
        Position::new(4, 2),
        Position::new(4, 47)
    );
}

// #[test]
// fn test_converting_assign_to_pipeline() {
//     assert_code_action!(
//         r#"
// import list

// fn main() {
//   let result = list.reverse(list.map([1,2,3], fn(x) {x * 2}))
// }
// "#,
//         Position::new(4, 2),
//         Position::new(4, 61)
//     );
// }

// #[test]
// fn test_converting_expr_to_pipeline() {
//     assert_code_action!(
//         r#"
// import list

// fn main() {
//   list.reverse(list.map([1,2,3], fn(x) {x * 2}))
// }
// "#,
//         Position::new(4, 2),
//         Position::new(4, 47)
//     );
// }

//1)
//wat te doen wanneer een function call meerdere argumenten heeft?
//string_builder.do_stuff(string_builder.reverse(string_builder.from_string(string)), add('x'))

//2)
//Kijken naar een testsituatie waarbij het niet lukt om een typedexpr te converteren naar string.
// Geen implementatie voor het to_string'en van fn(x,y) {x + y}
// #[test]
// fn test_converting_assign_to_pipeline() {
//     assert_code_action!(
//         r#"
// import list

// fn main() {
//   let result = list.reverse(list.map2([1,2,3], [4,5,6], fn(x,y) {x + y}))
// }
// "#,
//         Position::new(4, 2),
//         Position::new(4, 61)
//     );
// }
