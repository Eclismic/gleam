use crate::ast::{Assignment, Pattern};
use cov_mark;

use super::*;

pub fn inline_local_variable(
    module: &Module,
    params: &lsp::CodeActionParams,
    actions: &mut Vec<CodeAction>,
) {
    let uri = &params.text_document.uri;
    let line_numbers = LineNumbers::new(&module.code);
    let byte_index = line_numbers.byte_index(params.range.start.line, params.range.start.character);

    let edits = match module.find_node(byte_index) {
        Some(Located::Expression(e)) => {
            if let Some(Definition::Function(f)) =
                module.ast.find_containing_definition_for_node(byte_index)
            {
                inline_usage(e, f, line_numbers, module)
            } else {
                None
            }
        }
        Some(Located::Statement(Statement::Assignment(a)))
            if matches!(a.pattern, Pattern::Variable { .. }) =>
        {
            if let Some(Definition::Function(f)) =
                module.ast.find_containing_definition_for_node(byte_index)
            {
                inline_let(&a, f, line_numbers)
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(edits) = edits {
        CodeActionBuilder::new("Inline Variable Refactor")
            .kind(lsp_types::CodeActionKind::REFACTOR_INLINE)
            .changes(uri.clone(), edits)
            .preferred(true)
            .push_to(actions);
    }
}

fn inline_usage(
    e: &TypedExpr,
    f: &Function<Arc<Type>, TypedExpr>,
    line_numbers: LineNumbers,
    module: &Module,
) -> Option<Vec<lsp_types::TextEdit>> {
    if let Some(let_loc) = e.definition_location() {
        let let_loc = module.find_node(let_loc.span.start);
        if let Some(let_loc) = let_loc {
            if let Located::Statement(Statement::Assignment(let_assignment)) = let_loc {
                let let_val = &*let_assignment.value;
                let let_val_str = let_val.to_string()?;

                let usages: Vec<_> = f
                    .body
                    .iter()
                    .filter_map(|statement| match statement {
                        Statement::Expression(e) => usage_search(&let_assignment.pattern, e),
                        Statement::Assignment(a) => usage_search(&let_assignment.pattern, &a.value),
                        Statement::Use(_) => None,
                    })
                    .collect();

                let delete_let = usages.len() == 1;

                let mut edits: Vec<lsp_types::TextEdit> = Vec::new();

                if delete_let {
                    edits.push(lsp_types::TextEdit {
                        range: src_span_to_lsp_range(let_assignment.location, &line_numbers),
                        new_text: "".into(),
                    });
                }

                edits.push(lsp_types::TextEdit {
                    range: src_span_to_lsp_range(e.location(), &line_numbers),
                    new_text: let_val_str.to_string(),
                });
                return Some(edits);
            }
        }
    }

    None
}

fn inline_let(
    assignment: &Assignment<Arc<Type>, TypedExpr>,
    f: &Function<Arc<Type>, TypedExpr>,
    line_numbers: LineNumbers,
) -> Option<Vec<lsp_types::TextEdit>> {
    let usages: Vec<_> = f
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::Expression(e) => usage_search(&assignment.pattern, e),
            Statement::Assignment(a) => usage_search(&assignment.pattern, &a.value),
            Statement::Use(_) => None,
        })
        .flatten()
        .collect();

    if usages.is_empty() {
        cov_mark::hit!(test_inline_local_var_do_not_inline_unused_var);
        return None;
    }

    let value_to_inline = assignment.value.to_string()?;

    let mut edits: Vec<lsp_types::TextEdit> = Vec::with_capacity(usages.len() + 1);
    edits.push(lsp_types::TextEdit {
        range: src_span_to_lsp_range(assignment.location, &line_numbers),
        new_text: "".into(),
    });

    edits.extend(usages.iter().map(|usage| lsp_types::TextEdit {
        range: src_span_to_lsp_range(usage.location(), &line_numbers),
        new_text: value_to_inline.to_string(),
    }));

    Some(edits)
}

fn usage_search<'a>(
    pattern: &Pattern<Arc<Type>>,
    expr: &'a TypedExpr,
) -> Option<Vec<&'a TypedExpr>> {
    match expr {
        TypedExpr::Block { statements, .. } => {
            let mut results = vec![];
            for statement in statements {
                match statement {
                    Statement::Expression(e) => {
                        if let Some(exprs) = usage_search(pattern, e) {
                            results.extend(exprs);
                        }
                    }
                    Statement::Assignment(a) => {
                        if let Some(exprs) = usage_search(pattern, &a.value) {
                            results.extend(exprs);
                        }
                    }
                    Statement::Use(_) => {}
                }
            }
            Some(results)
        }
        TypedExpr::Pipeline {
            assignments,
            finally,
            ..
        } => {
            let mut res = Vec::new();
            for a in assignments {
                if let Some(usages) = usage_search(pattern, &a.value) {
                    res.extend(usages);
                }
            }

            if let Some(usages) = usage_search(pattern, &*finally) {
                res.extend(usages);
            }
            if res.is_empty() {
                None
            } else {
                Some(res)
            }
        }
        TypedExpr::Var { constructor, .. } => {
            let mut res = vec![];
            if matches!(
                constructor.variant,
                ValueConstructorVariant::LocalVariable { location }
                if location.start == pattern.location().start && location.end == pattern.location().end
            ) {
                res.push(expr);
            }

            if res.is_empty() {
                None
            } else {
                Some(res)
            }
        }
        TypedExpr::Fn { body, .. } => {
            let res: Vec<_> = body
                .iter()
                .flat_map(|statement| match statement {
                    Statement::Expression(e) => usage_search(pattern, e),
                    Statement::Assignment(a) => usage_search(pattern, &a.value),
                    Statement::Use(_) => None,
                })
                .flatten()
                .collect();

            if res.is_empty() {
                None
            } else {
                Some(res)
            }
        }
        TypedExpr::List { elements, tail, .. } => {
            let mut res: Vec<_> = elements
                .iter()
                .flat_map(|elem| usage_search(pattern, elem))
                .flatten()
                .collect();

            if let Some(tail) = tail {
                if let Some(usage_search) = usage_search(pattern, tail) {
                    res.extend(usage_search);
                }
            }

            if res.is_empty() {
                None
            } else {
                Some(res)
            }
        }
        TypedExpr::Call { args, .. } => {
            let mut res = vec![];
            for arg in args {
                if let Some(found) = usage_search(pattern, &arg.value) {
                    res.extend(found);
                }
            }
            if !res.is_empty() {
                Some(res)
            } else {
                None
            }
        }
        TypedExpr::BinOp { left, right, .. } => {
            let mut res = vec![];
            if let Some(usages_left) = usage_search(pattern, left) {
                res.extend(usages_left);
            }

            if let Some(usages_right) = usage_search(pattern, right) {
                res.extend(usages_right);
            }

            if res.is_empty() {
                None
            } else {
                Some(res)
            }
        }
        TypedExpr::Tuple { elems, .. } => {
            let res: Vec<_> = elems
                .iter()
                .flat_map(|elem| usage_search(pattern, elem))
                .flatten()
                .collect();

            if res.is_empty() {
                None
            } else {
                Some(res)
            }
        }
        _ => None,
    }
}
