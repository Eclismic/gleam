use crate::ast::SrcSpan;

use super::*;

pub fn convert_to_pipeline(
    module: &Module,
    params: &lsp::CodeActionParams,
    actions: &mut Vec<CodeAction>,
) {
    let uri = &params.text_document.uri;
    let line_numbers = LineNumbers::new(&module.code);
    let byte_index = line_numbers.byte_index(params.range.start.line, params.range.start.character);

    let located = match module.find_node(byte_index) {
        Some(located) => located,
        None => return,
    };

    let (call_expression, location) = match located {
        Located::Expression(expr) => (expr, expr.location().start),
        _ => return,
    };

    let mut call_chain: Vec<&TypedExpr> = Vec::new();
    detect_call_chain_conversion_to_pipeline(call_expression, &mut call_chain);

    if call_chain.is_empty() {
        return;
    }

    //ook pas doen als de call_chain groter is dan 1?
    let pipeline_parts = convert_call_chain_to_pipeline(call_chain).unwrap();

    let indent = line_numbers.line_and_column_number(location).column - 1;

    let edit: lsp_types::TextEdit = create_edit(pipeline_parts, line_numbers, indent).unwrap();

    CodeActionBuilder::new("Apply Pipeline Rewrite")
        .kind(lsp_types::CodeActionKind::REFACTOR_REWRITE)
        .changes(uri.clone(), vec![edit])
        .preferred(true)
        .push_to(actions);
}

fn create_edit(
    pipeline_parts: PipelineParts,
    line_numbers: LineNumbers,
    indent: u32,
) -> Option<lsp::TextEdit> {
    let mut edit_str = EcoString::new();

    edit_str.push_str(&format!("{} \n", pipeline_parts.input));

    if let Err(()) = pipeline_parts
        .calls
        .iter()
        .try_for_each(|part| match part.to_string() {
            Some(s) => {
                for _ in 0..indent {
                    edit_str.push(' ');
                }
                edit_str.push_str(&format!("|> {}\n", s));
                Ok(())
            }
            None => Err(()),
        })
    {
        return None;
    }

    // let mut input_pipeline_str = EcoString::new();
    
    // input_pipeline_str.push_str(&format!("{} \n", pipeline_parts.input));

    Some(lsp::TextEdit {
        range: src_span_to_lsp_range(pipeline_parts.location, &line_numbers),
       // new_text: (input_pipeline_str + edit_str).to_string(),
        new_text: edit_str.to_string(),
    })
}

fn detect_call_chain_conversion_to_pipeline<'a>(
    call_expression: &'a TypedExpr,
    call_chain: &mut Vec<&'a TypedExpr>,
) {
    call_chain.push(call_expression);

    if let TypedExpr::Call { args, .. } = call_expression {
        let arg = match args.first() {
            Some(arg) => arg,
            None => return,
        };

        match &arg.value {
            TypedExpr::Call { .. } => {
                detect_call_chain_conversion_to_pipeline(&arg.value, call_chain)
            }
            _ => (),
        }
    }
}

fn convert_call_chain_to_pipeline(mut call_chain: Vec<&TypedExpr>) -> Option<PipelineParts> {
    call_chain.reverse();

    let modified_chain: Vec<_> = call_chain
        .iter()
        .filter_map(|expr| {
            if let TypedExpr::Call {
                location,
                typ,
                fun,
                args,
            } = expr
            {
                if args.len() > 0 {
                    let mut new_args = args.clone();
                    let _ = new_args.drain(..1);

                    Some(TypedExpr::Call {
                        location: location.clone(),
                        typ: typ.clone(),
                        fun: fun.clone(),
                        args: new_args,
                    })
                } else {
                    //call without args; no need to remove the first arg
                    //this is probably the input to the pipeline
                    None
                }
            } else {
                None
            }
        })
        //.filter_map(|x| x) //CHANGE THIS
        .collect();

    let first_chain = call_chain.first().expect("There is a first element");

    let input = match first_chain {
        TypedExpr::Call { args, .. } => {
            if let Some(arg) = args.first() {
                arg.value.to_string()
            } else {
                first_chain.to_string()
            }
        },
        _ => return None,
    }?;

    Some(PipelineParts {
        input: input,
        location: call_chain.last().expect("there is a last one").location(),
        calls: modified_chain,
    })
}

struct PipelineParts {
    input: EcoString,
    location: SrcSpan,
    calls: Vec<TypedExpr>,
}
