use lsp_types::{CodeAction, Range, Url};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeActionData {
    pub id: String,
    pub code_action_params: lsp_types::CodeActionParams,
}

pub enum CodeActionHandler {
    defer_calc,
    pipeline,
    inline_localvar,
}

#[derive(Debug)]
pub struct CodeActionBuilder {
    action: CodeAction,
}

impl CodeActionBuilder {
    pub fn new(title: &str) -> Self {
        Self {
            action: CodeAction {
                title: title.to_string(),
                kind: None,
                diagnostics: None,
                edit: None,
                command: None,
                is_preferred: None,
                disabled: None,
                data: None,
            },
        }
    }

    pub fn kind(mut self, kind: lsp_types::CodeActionKind) -> Self {
        self.action.kind = Some(kind);
        self
    }

    pub fn changes(mut self, uri: Url, edits: Vec<lsp_types::TextEdit>) -> Self {
        let mut edit = self.action.edit.take().unwrap_or_default();
        let mut changes = edit.changes.take().unwrap_or_default();
        _ = changes.insert(uri, edits);

        edit.changes = Some(changes);
        self.action.edit = Some(edit);
        self
    }

    pub fn preferred(mut self, is_preferred: bool) -> Self {
        self.action.is_preferred = Some(is_preferred);
        self
    }

    pub fn data(mut self, id: String, code_action_params: lsp_types::CodeActionParams) -> Self {
        let code_action_data = CodeActionData {
            id,
            code_action_params,
        };
        let js = serde_json::to_value(code_action_data).unwrap_or_default();
        self.action.data = Some(js);
        self
    }

    pub fn push_to(self, actions: &mut Vec<CodeAction>) {
        actions.push(self.action);
    }

    pub fn resolve(
        mut self,
        resolve: bool,
        uri: Url,
        edits: Vec<lsp_types::TextEdit>,
        id: String,
        code_action_params: lsp_types::CodeActionParams,
    ) -> Self {
        if resolve {
            let mut edit = self.action.edit.take().unwrap_or_default();
            let mut changes = edit.changes.take().unwrap_or_default();
            _ = changes.insert(uri, edits);
            edit.changes = Some(changes);
            self.action.edit = Some(edit);
        } else {
            let code_action_data = CodeActionData {
                id,
                code_action_params,
            };
            let js = serde_json::to_value(code_action_data).unwrap_or_default();
            self.action.data = Some(js);
        }
        self
    }
}
