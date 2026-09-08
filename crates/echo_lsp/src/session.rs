//! In-process LSP session: JSON-RPC handling without stdio.
//!
//! Keeps protocol reliability testable: open/change → versioned diagnostics,
//! feature requests, multi-file open buffers via shared overlays.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::analysis::analyze_path;
use crate::document::{
    diagnostic_matches_doc, uri_to_path, ContentChange, DocumentStore, OpenDocument,
};
use crate::features::{
    analysis_product, completion, definition, document_symbols, format_edits, hover, module_for_path,
    references, rename, semantic_tokens_with_ast, signature_help, workspace_symbols, Location,
    LspRange, RenameResult, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
use crate::position::Position;
use crate::LspSeverity;

/// One outbound JSON-RPC message (response, error, or notification).
#[derive(Debug, Clone)]
pub enum Outgoing {
    Response { id: Value, result: Value },
    Error { id: Value, code: i32, message: String },
    Notification { method: String, params: Value },
}

/// Mutable language-server session state.
#[derive(Debug, Default)]
pub struct LspSession {
    store: DocumentStore,
    root: Option<PathBuf>,
    /// After `shutdown`, only `exit` is expected.
    shutdown_requested: bool,
}

impl LspSession {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn store(&self) -> &DocumentStore {
        &self.store
    }

    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Handle one JSON-RPC message; returns ordered outbound messages.
    pub fn handle(&mut self, msg: &Value) -> Vec<Outgoing> {
        let method = msg.get("method").and_then(|m| m.as_str());
        let id = msg.get("id").cloned();

        if self.shutdown_requested {
            match method {
                Some("exit") => return Vec::new(),
                Some(_) if id.is_some() => {
                    return vec![Outgoing::Error {
                        id: id.unwrap(),
                        code: -32600,
                        message: "server shut down".into(),
                    }];
                }
                _ => return Vec::new(),
            }
        }

        match method {
            Some("initialize") => {
                self.on_initialize(msg);
                vec![Outgoing::Response {
                    id: id.unwrap_or(Value::Null),
                    result: initialize_capabilities(),
                }]
            }
            Some("initialized") | Some("workspace/didChangeConfiguration") => Vec::new(),
            Some("shutdown") => {
                self.shutdown_requested = true;
                vec![Outgoing::Response {
                    id: id.unwrap_or(Value::Null),
                    result: Value::Null,
                }]
            }
            Some("exit") => Vec::new(),
            Some("textDocument/didOpen") => {
                if let Some(params) = msg.get("params") {
                    self.on_did_open(params)
                } else {
                    Vec::new()
                }
            }
            Some("textDocument/didChange") => {
                if let Some(params) = msg.get("params") {
                    self.on_did_change(params)
                } else {
                    Vec::new()
                }
            }
            Some("textDocument/didClose") => {
                if let Some(params) = msg.get("params") {
                    self.on_did_close(params)
                } else {
                    Vec::new()
                }
            }
            Some("textDocument/didSave") => {
                if let Some(params) = msg.get("params") {
                    self.on_did_save(params)
                } else {
                    Vec::new()
                }
            }
            Some("textDocument/hover") => {
                let result = self.with_doc_pos(msg, |path, text, pos, product| {
                    hover(product, path, text, pos).map(|h| {
                        json!({
                            "contents": { "kind": "markdown", "value": h.contents },
                            "range": range_json(h.range),
                        })
                    })
                });
                response(id, result.unwrap_or(Value::Null))
            }
            Some("textDocument/definition") => {
                let result = self.with_doc_pos(msg, |path, text, pos, product| {
                    definition(product, path, text, pos).map(location_json)
                });
                response(id, result.unwrap_or(Value::Null))
            }
            Some("textDocument/references") => {
                let include_decl = msg
                    .pointer("/params/context/includeDeclaration")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let result = self.with_doc_pos(msg, |path, text, pos, product| {
                    let locs = references(product, path, text, pos, include_decl);
                    Some(Value::Array(locs.into_iter().map(location_json).collect()))
                });
                response(id, result.unwrap_or_else(|| Value::Array(vec![])))
            }
            Some("textDocument/completion") => {
                let result = self.with_doc_pos(msg, |path, text, pos, product| {
                    let items = completion(product, path, text, pos);
                    Some(Value::Array(
                        items
                            .into_iter()
                            .map(|c| {
                                let mut o = json!({
                                    "label": c.label,
                                    "kind": c.kind as i32,
                                });
                                if let Some(d) = c.detail {
                                    o["detail"] = json!(d);
                                }
                                if let Some(t) = c.insert_text {
                                    o["insertText"] = json!(t);
                                }
                                o
                            })
                            .collect(),
                    ))
                });
                response(id, result.unwrap_or_else(|| Value::Array(vec![])))
            }
            Some("textDocument/signatureHelp") => {
                let result = self.with_doc_pos(msg, |path, text, pos, product| {
                    signature_help(product, path, text, pos).map(|h| {
                        json!({
                            "signatures": [{
                                "label": h.label,
                                "parameters": h.parameters.iter().map(|p| json!({"label": p})).collect::<Vec<_>>(),
                            }],
                            "activeSignature": 0,
                            "activeParameter": h.active_parameter,
                        })
                    })
                });
                response(id, result.unwrap_or(Value::Null))
            }
            Some("textDocument/rename") => {
                let new_name = msg
                    .pointer("/params/newName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match self.with_doc_pos(msg, |path, text, pos, product| {
                    Some(rename(product, path, text, pos, new_name))
                }) {
                    Some(RenameResult::Ok(edit)) => {
                        let edits: Vec<Value> = edit
                            .edits
                            .into_iter()
                            .map(|e| {
                                json!({
                                    "range": range_json(e.range),
                                    "newText": e.new_text,
                                })
                            })
                            .collect();
                        response(
                            id,
                            json!({
                                "changes": {
                                    edit.uri: edits
                                }
                            }),
                        )
                    }
                    Some(RenameResult::Err(message)) => {
                        if let Some(id) = id {
                            vec![Outgoing::Error {
                                id,
                                code: -32602,
                                message,
                            }]
                        } else {
                            Vec::new()
                        }
                    }
                    None => response(id, Value::Null),
                }
            }
            Some("textDocument/documentSymbol") => {
                let result = self.with_doc(msg, |path, text, product| {
                    let Some(m) = module_for_path(product, path) else {
                        return Some(Value::Array(vec![]));
                    };
                    let Some(file) = m.file.as_ref() else {
                        return Some(Value::Array(vec![]));
                    };
                    let syms = document_symbols(path, text, file);
                    Some(Value::Array(
                        syms.into_iter()
                            .map(|s| {
                                json!({
                                    "name": s.name,
                                    "kind": s.kind as i32,
                                    "range": range_json(s.range),
                                    "selectionRange": range_json(s.selection_range),
                                    "detail": s.container,
                                })
                            })
                            .collect(),
                    ))
                });
                response(id, result.unwrap_or_else(|| Value::Array(vec![])))
            }
            Some("workspace/symbol") => {
                let query = msg
                    .pointer("/params/query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let result = if let Some(doc) = self.store.iter().next() {
                    if let Some(path) = doc.path.as_ref() {
                        let overlays = self.store.overlays();
                        let product = analysis_product(path, &overlays, true);
                        let syms = workspace_symbols(&product, &overlays, query);
                        Value::Array(
                            syms.into_iter()
                                .map(|s| {
                                    json!({
                                        "name": s.name,
                                        "kind": s.kind as i32,
                                        "location": {
                                            "uri": s.uri,
                                            "range": range_json(s.range),
                                        },
                                        "containerName": s.container,
                                    })
                                })
                                .collect(),
                        )
                    } else {
                        Value::Array(vec![])
                    }
                } else {
                    Value::Array(vec![])
                };
                response(id, result)
            }
            Some("textDocument/formatting") => {
                let result = self.with_doc_text(msg, |path, text| match format_edits(path, text) {
                    Ok(edits) => Some(Value::Array(
                        edits
                            .into_iter()
                            .map(|e| {
                                json!({
                                    "range": range_json(e.range),
                                    "newText": e.new_text,
                                })
                            })
                            .collect(),
                    )),
                    Err(_) => Some(Value::Array(vec![])),
                });
                response(id, result.unwrap_or_else(|| Value::Array(vec![])))
            }
            Some("textDocument/semanticTokens/full") => {
                let result = self.with_doc(msg, |path, text, product| {
                    let file = module_for_path(product, path).and_then(|m| m.file.as_ref());
                    let data = semantic_tokens_with_ast(text, file);
                    Some(json!({ "data": data }))
                });
                response(id, result.unwrap_or_else(|| json!({ "data": [] })))
            }
            Some(other) if id.is_some() => {
                vec![Outgoing::Error {
                    id: id.unwrap(),
                    code: -32601,
                    message: format!("method not found: {other}"),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn on_initialize(&mut self, msg: &Value) {
        let Some(params) = msg.get("params") else {
            return;
        };
        // Prefer workspaceFolders[0], then rootUri, then rootPath.
        if let Some(folders) = params.get("workspaceFolders").and_then(|v| v.as_array()) {
            if let Some(uri) = folders
                .first()
                .and_then(|f| f.get("uri"))
                .and_then(|u| u.as_str())
                .and_then(uri_to_path)
            {
                self.root = Some(uri);
                return;
            }
        }
        if let Some(uri) = params
            .pointer("/rootUri")
            .and_then(|u| u.as_str())
            .and_then(uri_to_path)
        {
            self.root = Some(uri);
        } else if let Some(path) = params
            .pointer("/rootPath")
            .and_then(|u| u.as_str())
            .map(PathBuf::from)
        {
            self.root = Some(path);
        }
    }

    fn on_did_open(&mut self, params: &Value) -> Vec<Outgoing> {
        let doc = &params["textDocument"];
        let uri = doc["uri"].as_str().unwrap_or("").to_string();
        let version = doc["version"].as_i64().unwrap_or(0) as i32;
        let text = doc["text"].as_str().unwrap_or("").to_string();
        self.store.open(uri, version, text);
        self.publish_all_open()
    }

    fn on_did_change(&mut self, params: &Value) -> Vec<Outgoing> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let version = params["textDocument"]["version"].as_i64().unwrap_or(0) as i32;
        let changes = parse_content_changes(params.get("contentChanges"));
        if changes.is_empty() {
            return Vec::new();
        }
        self.store.apply_changes(&uri, version, &changes);
        self.publish_all_open()
    }

    fn on_did_close(&mut self, params: &Value) -> Vec<Outgoing> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        self.store.close(&uri);
        vec![diag_notification(&uri, None, Vec::new())]
    }

    fn on_did_save(&mut self, params: &Value) -> Vec<Outgoing> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if let Some(text) = params.get("text").and_then(|t| t.as_str()) {
            let ver = self.store.get(&uri).map(|d| d.version).unwrap_or(0);
            self.store.change(&uri, ver, text.to_string());
        }
        self.publish_all_open()
    }

    /// Re-analyze every open buffer with shared overlays; publish versioned diags.
    fn publish_all_open(&self) -> Vec<Outgoing> {
        let overlays = self.store.overlays();
        let mut out = Vec::new();
        // Snapshot open docs so we can publish even when iteration order varies.
        let docs: Vec<&OpenDocument> = self.store.iter().collect();
        for doc in docs {
            let Some(path) = doc.path.as_ref() else {
                continue;
            };
            let diags = analyze_path(path, &overlays, true);
            let for_doc: Vec<_> = diags
                .into_iter()
                .filter(|d| diagnostic_matches_doc(&d.uri, doc))
                .collect();
            out.push(diag_notification(
                &doc.uri,
                Some(doc.version),
                for_doc,
            ));
        }
        out
    }

    fn with_doc_pos<F, R>(&self, msg: &Value, f: F) -> Option<R>
    where
        F: FnOnce(&Path, &str, Position, &echo_pipeline::AnalysisProduct) -> Option<R>,
    {
        let params = msg.get("params")?;
        let uri = params.pointer("/textDocument/uri")?.as_str()?;
        let pos = pos_from_params(params)?;
        let doc = self.store.get(uri)?;
        let path = doc.path.as_ref()?;
        let overlays = self.store.overlays();
        let product = analysis_product(path, &overlays, true);
        f(path, &doc.text, pos, &product)
    }

    fn with_doc<F, R>(&self, msg: &Value, f: F) -> Option<R>
    where
        F: FnOnce(&Path, &str, &echo_pipeline::AnalysisProduct) -> Option<R>,
    {
        let params = msg.get("params")?;
        let uri = params.pointer("/textDocument/uri")?.as_str()?;
        let doc = self.store.get(uri)?;
        let path = doc.path.as_ref()?;
        let overlays = self.store.overlays();
        let product = analysis_product(path, &overlays, true);
        f(path, &doc.text, &product)
    }

    fn with_doc_text<F, R>(&self, msg: &Value, f: F) -> Option<R>
    where
        F: FnOnce(&Path, &str) -> Option<R>,
    {
        let params = msg.get("params")?;
        let uri = params.pointer("/textDocument/uri")?.as_str()?;
        let doc = self.store.get(uri)?;
        let path = doc.path.as_ref()?;
        f(path, &doc.text)
    }
}

fn parse_content_changes(raw: Option<&Value>) -> Vec<ContentChange> {
    let Some(arr) = raw.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|c| {
            let text = c.get("text")?.as_str()?.to_string();
            let range = c.get("range").and_then(|r| {
                let start_line = r.pointer("/start/line")?.as_u64()? as u32;
                let start_char = r.pointer("/start/character")?.as_u64()? as u32;
                let end_line = r.pointer("/end/line")?.as_u64()? as u32;
                let end_char = r.pointer("/end/character")?.as_u64()? as u32;
                Some((
                    Position {
                        line: start_line,
                        character: start_char,
                    },
                    Position {
                        line: end_line,
                        character: end_char,
                    },
                ))
            });
            Some(ContentChange { range, text })
        })
        .collect()
}

fn diag_notification(
    uri: &str,
    version: Option<i32>,
    diags: Vec<crate::LspDiagnostic>,
) -> Outgoing {
    let items: Vec<Value> = diags
        .iter()
        .map(|d| {
            let severity = match d.severity {
                LspSeverity::Error => 1,
                LspSeverity::Warning => 2,
                LspSeverity::Information => 3,
                LspSeverity::Hint => 4,
            };
            let mut obj = json!({
                "range": {
                    "start": { "line": d.start.line, "character": d.start.character },
                    "end": { "line": d.end.line, "character": d.end.character }
                },
                "severity": severity,
                "source": "echo",
                "message": d.message,
            });
            if let Some(code) = &d.code {
                obj["code"] = json!(code);
            }
            obj
        })
        .collect();

    let mut params = json!({
        "uri": uri,
        "diagnostics": items,
    });
    if let Some(v) = version {
        params["version"] = json!(v);
    }
    Outgoing::Notification {
        method: "textDocument/publishDiagnostics".into(),
        params,
    }
}

fn response(id: Option<Value>, result: Value) -> Vec<Outgoing> {
    let Some(id) = id else {
        return Vec::new();
    };
    vec![Outgoing::Response { id, result }]
}

fn initialize_capabilities() -> Value {
    json!({
        "capabilities": {
            "textDocumentSync": {
                "openClose": true,
                // Incremental (2): clients may still send full-buffer changes
                // (no range); both forms are accepted.
                "change": 2,
                "save": { "includeText": true }
            },
            "hoverProvider": true,
            "definitionProvider": true,
            "referencesProvider": true,
            "completionProvider": {
                "triggerCharacters": [".", "/", "$", "~", "#", "%"]
            },
            "signatureHelpProvider": {
                "triggerCharacters": ["(", ","]
            },
            "renameProvider": true,
            "documentSymbolProvider": true,
            "workspaceSymbolProvider": true,
            "documentFormattingProvider": true,
            "semanticTokensProvider": {
                "legend": {
                    "tokenTypes": SEMANTIC_TOKEN_TYPES,
                    "tokenModifiers": SEMANTIC_TOKEN_MODIFIERS,
                },
                "full": true,
            }
        },
        "serverInfo": {
            "name": "echo-lsp",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn range_json(r: LspRange) -> Value {
    json!({
        "start": { "line": r.start.line, "character": r.start.character },
        "end": { "line": r.end.line, "character": r.end.character },
    })
}

fn location_json(loc: Location) -> Value {
    json!({
        "uri": loc.uri,
        "range": range_json(loc.range),
    })
}

fn pos_from_params(params: &Value) -> Option<Position> {
    let line = params.pointer("/position/line")?.as_u64()? as u32;
    let character = params.pointer("/position/character")?.as_u64()? as u32;
    Some(Position { line, character })
}

/// Serialize an outbound message as a JSON-RPC object.
#[must_use]
pub fn outgoing_to_json(msg: &Outgoing) -> Value {
    match msg {
        Outgoing::Response { id, result } => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
        Outgoing::Error { id, code, message } => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
        Outgoing::Notification { method, params } => json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str, body: &str) -> (PathBuf, String) {
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-lsp-sess-{t}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(name);
        fs::write(&path, body).unwrap();
        let uri = crate::document::path_to_uri(&path);
        (path, uri)
    }

    fn first_diag_params(out: &[Outgoing]) -> Option<&Value> {
        out.iter().find_map(|m| match m {
            Outgoing::Notification { method, params }
                if method == "textDocument/publishDiagnostics" =>
            {
                Some(params)
            }
            _ => None,
        })
    }

    #[test]
    fn initialize_reports_incremental_sync() {
        let mut s = LspSession::new();
        let out = s.handle(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": "file:///tmp/proj"
            }
        }));
        assert_eq!(out.len(), 1);
        let Outgoing::Response { result, .. } = &out[0] else {
            panic!("expected response");
        };
        assert_eq!(
            result.pointer("/capabilities/textDocumentSync/change"),
            Some(&json!(2))
        );
        assert_eq!(s.root(), Some(Path::new("/tmp/proj")));
    }

    #[test]
    fn initialize_prefers_workspace_folders() {
        let mut s = LspSession::new();
        let _ = s.handle(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": "file:///tmp/old",
                "workspaceFolders": [{
                    "uri": "file:///tmp/new-root",
                    "name": "new"
                }]
            }
        }));
        assert_eq!(s.root(), Some(Path::new("/tmp/new-root")));
    }

    #[test]
    fn did_open_publishes_versioned_clean_diags() {
        let (path, uri) = temp_file("ok.echo", "$ x = 1\n");
        let mut s = LspSession::new();
        let out = s.handle(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "echo",
                    "version": 1,
                    "text": "$ x = 1\n"
                }
            }
        }));
        let params = first_diag_params(&out).expect("publishDiagnostics");
        assert_eq!(params["uri"], uri);
        assert_eq!(params["version"], 1);
        assert_eq!(params["diagnostics"].as_array().unwrap().len(), 0);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn did_change_full_reports_shadow_with_code() {
        let (path, uri) = temp_file("sh.echo", "$ x = 1\n");
        let mut s = LspSession::new();
        let _ = s.handle(&json!({
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": 1,
                    "text": "$ x = 1\n"
                }
            }
        }));
        let out = s.handle(&json!({
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": "$ x = 1\n$ x = 2\n" }]
            }
        }));
        let params = first_diag_params(&out).expect("diags");
        assert_eq!(params["version"], 2);
        let diags = params["diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "sem-shadow"),
            "{diags:?}"
        );
        // Span on second bind (line 1).
        let shadow = diags
            .iter()
            .find(|d| d["code"] == "sem-shadow")
            .unwrap();
        assert_eq!(shadow["range"]["start"]["line"], 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn incremental_change_updates_buffer_and_diags() {
        let (path, uri) = temp_file("inc.echo", "$ x = 1\n");
        let mut s = LspSession::new();
        let _ = s.handle(&json!({
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": 1,
                    "text": "$ x = 1\n"
                }
            }
        }));
        // Append a shadowing bind via incremental insert at EOF.
        let out = s.handle(&json!({
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 3 },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 0 }
                    },
                    "text": "$ x = 2\n"
                }]
            }
        }));
        assert_eq!(
            s.store().get(&uri).unwrap().text,
            "$ x = 1\n$ x = 2\n"
        );
        let params = first_diag_params(&out).expect("diags");
        assert_eq!(params["version"], 3);
        let diags = params["diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "sem-shadow"),
            "{diags:?}"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn hover_request_after_open() {
        let (path, uri) = temp_file("hov.echo", "$ answer = 42\n$ x = answer\n");
        let mut s = LspSession::new();
        let _ = s.handle(&json!({
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": 1,
                    "text": "$ answer = 42\n$ x = answer\n"
                }
            }
        }));
        // Character of 'a' in second-line `answer` (line 1, after "$ x = ").
        let out = s.handle(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 6 }
            }
        }));
        let Outgoing::Response { result, .. } = &out[0] else {
            panic!("expected hover response, got {out:?}");
        };
        let contents = result.pointer("/contents/value").and_then(|v| v.as_str());
        assert!(
            contents.is_some_and(|c| c.contains("answer")),
            "{result}"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn multi_file_overlay_diag_on_importer() {
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-lsp-mf-{t}"));
        fs::create_dir_all(&root).unwrap();
        let lib = root.join("lib.echo");
        let main = root.join("main.echo");
        fs::write(&lib, "\\ answer\n$ answer = 1\n").unwrap();
        fs::write(&main, "/ ./lib\n$ x = lib.answer\n").unwrap();
        let main_uri = crate::document::path_to_uri(&main);
        let lib_uri = crate::document::path_to_uri(&lib);

        let mut s = LspSession::new();
        let _ = s.handle(&json!({
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": main_uri,
                    "version": 1,
                    "text": "/ ./lib\n$ x = lib.answer\n"
                }
            }
        }));
        // Open lib dirty with a shadow error; republish should still attribute
        // correctly per URI when both are open.
        let out = s.handle(&json!({
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": lib_uri,
                    "version": 1,
                    "text": "\\ answer\n$ answer = 1\n$ answer = 2\n"
                }
            }
        }));
        let lib_params = out.iter().find_map(|m| match m {
            Outgoing::Notification { method, params }
                if method == "textDocument/publishDiagnostics"
                    && params["uri"] == lib_uri =>
            {
                Some(params)
            }
            _ => None,
        });
        let params = lib_params.expect("lib diags");
        let diags = params["diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "sem-shadow"),
            "lib should report shadow: {diags:?}"
        );
        // No filename-substring leakage: main notification (if any) must not
        // claim lib's shadow under main's URI without matching path.
        for m in &out {
            if let Outgoing::Notification { method, params } = m {
                if method == "textDocument/publishDiagnostics" && params["uri"] == main_uri {
                    let diags = params["diagnostics"].as_array().unwrap();
                    for d in diags {
                        // main itself should not show sem-shadow from lib body
                        // unless analysis attributes it to main (it should not).
                        if d["code"] == "sem-shadow" {
                            panic!("shadow leaked onto main: {d}");
                        }
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn did_close_clears_diagnostics() {
        let (path, uri) = temp_file("cl.echo", "$ x = 1\n$ x = 2\n");
        let mut s = LspSession::new();
        let _ = s.handle(&json!({
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": 1,
                    "text": "$ x = 1\n$ x = 2\n"
                }
            }
        }));
        let out = s.handle(&json!({
            "method": "textDocument/didClose",
            "params": {
                "textDocument": { "uri": uri }
            }
        }));
        let params = first_diag_params(&out).expect("clear");
        assert_eq!(params["diagnostics"].as_array().unwrap().len(), 0);
        assert!(params.get("version").is_none());
        assert!(s.store().is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unknown_method_returns_error() {
        let mut s = LspSession::new();
        let out = s.handle(&json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "textDocument/codeAction",
            "params": {}
        }));
        match &out[0] {
            Outgoing::Error { code, .. } => assert_eq!(*code, -32601),
            other => panic!("expected error, got {other:?}"),
        }
    }
}
