//! `odl server`: a language server. Editors get odoo-lint's findings as
//! diagnostics while typing, its fixes as quick fixes, and a "fix all"
//! source action. Only files of Odoo addons are linted.

use crate::diagnostics::Violation;
use crate::fix::{Applicability, Edit, FixMode};
use crate::settings::{CliOverrides, Settings};
use crate::sources::Sources;
use crate::{fixer, linter};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{CodeActionRequest, GotoDefinition, Request as _};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand, CodeActionParams, CodeActionProviderCapability,
    CodeDescription, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, NumberOrString, Position,
    PositionEncodingKind, PublishDiagnosticsParams, Range, SaveOptions, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url, WorkspaceEdit,
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const FIX_ALL: &str = "source.fixAll.odoo-lint";

/// How positions count characters within a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

impl Encoding {
    /// The client's preferred encoding among the ones offered; UTF-16 is the
    /// protocol's default.
    fn negotiate(params: &InitializeParams) -> Self {
        let offered = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_ref());
        let offered = offered.map(Vec::as_slice).unwrap_or_default();
        if offered.contains(&PositionEncodingKind::UTF8) {
            Encoding::Utf8
        } else if offered.contains(&PositionEncodingKind::UTF32) {
            Encoding::Utf32
        } else {
            Encoding::Utf16
        }
    }

    fn kind(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
            Encoding::Utf32 => PositionEncodingKind::UTF32,
        }
    }

    fn width(self, text: &str) -> u32 {
        let width = match self {
            Encoding::Utf8 => text.len(),
            Encoding::Utf16 => text.encode_utf16().count(),
            Encoding::Utf32 => text.chars().count(),
        };
        width as u32
    }

    /// The byte offset of `position` in `text`.
    fn offset(self, text: &str, position: Position) -> usize {
        let line_start: usize = text
            .split_inclusive('\n')
            .take(position.line as usize)
            .map(str::len)
            .sum();
        let line = text[line_start.min(text.len())..]
            .split('\n')
            .next()
            .unwrap_or_default();
        let mut width = 0;
        for (i, c) in line.char_indices() {
            if width >= position.character {
                return line_start + i;
            }
            width += match self {
                Encoding::Utf8 => c.len_utf8() as u32,
                Encoding::Utf16 => c.len_utf16() as u32,
                Encoding::Utf32 => 1,
            };
        }
        line_start + line.len()
    }

    /// The position of byte `offset` in `text`.
    fn position(self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let before = &text[..offset];
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        Position::new(
            before.matches('\n').count() as u32,
            self.width(&text[line_start..offset]),
        )
    }
}

/// The range a violation is shown on: from its column to the end of its line.
fn violation_range(text: &str, violation: &Violation, encoding: Encoding) -> Range {
    let line_number = violation.line.saturating_sub(1);
    let line = text.split('\n').nth(line_number).unwrap_or_default();
    let line = line.strip_suffix('\r').unwrap_or(line);
    let column = line
        .char_indices()
        .nth(violation.column.saturating_sub(1))
        .map_or(line.len(), |(i, _)| i);
    let line_number = line_number as u32;
    let mut end = encoding.width(line);
    let start = encoding.width(&line[..column]);
    if end == start {
        // An empty range is easy to miss; cover the line break instead.
        end = start + 1;
    }
    Range::new(Position::new(line_number, start), Position::new(line_number, end))
}

fn diagnostic(text: &str, violation: &Violation, encoding: Encoding) -> Diagnostic {
    let severity = if violation.code.starts_with(['E', 'F']) {
        DiagnosticSeverity::ERROR
    } else {
        DiagnosticSeverity::WARNING
    };
    Diagnostic {
        range: violation_range(text, violation, encoding),
        severity: Some(severity),
        code: Some(NumberOrString::String(violation.code.clone())),
        code_description: Url::parse(&format!(
            "https://odoo-lint.readthedocs.io/en/latest/rules/{}.html",
            violation.code
        ))
        .ok()
        .map(|href| CodeDescription { href }),
        source: Some("odoo-lint".to_string()),
        message: format!("{} ({})", violation.message, violation.name),
        ..Diagnostic::default()
    }
}

fn is_po(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "po" || e == "pot")
}

struct Server {
    connection: Connection,
    encoding: Encoding,
    /// Open documents: their text and version.
    documents: HashMap<Url, (String, i32)>,
    /// The open documents' text, for the linter.
    sources: Sources,
}

impl Server {
    fn path(uri: &Url) -> Option<PathBuf> {
        (uri.scheme() == "file").then(|| uri.to_file_path().ok()).flatten()
    }

    /// Settings for a file, from the configuration found above it.
    fn settings(path: &Path) -> Option<Settings> {
        Settings::load(path, None, CliOverrides::default())
            .ok()
            .map(|loaded| loaded.settings)
            .filter(|settings| !settings.is_excluded(path))
    }

    /// The violations in a file of an Odoo addon; `None` for other files.
    fn lint(&self, path: &Path) -> Option<(Settings, Vec<Violation>)> {
        let linted = path
            .extension()
            .is_some_and(|e| e == "py" || e == "po" || e == "pot" || e == "xml");
        if !linted {
            return None;
        }
        let file = path.to_path_buf();
        let units = linter::lint_units(std::slice::from_ref(&file), &self.sources);
        if units.get(&file).is_none_or(|unit| *unit == file) {
            return None;
        }
        let settings = Self::settings(path)?;
        let mut violations = linter::lint_files_with(std::slice::from_ref(&file), &settings, &self.sources);
        // Module checks also report on other files of the module.
        violations.retain(|v| Path::new(&v.file_path) == file);
        Some((settings, violations))
    }

    fn publish(&self, uri: &Url) -> Result<()> {
        let Some((text, version)) = self.documents.get(uri) else {
            return Ok(());
        };
        let diagnostics = Self::path(uri)
            .and_then(|path| self.lint(&path))
            .map(|(_, violations)| violations.iter().map(|v| diagnostic(text, v, self.encoding)).collect())
            .unwrap_or_default();
        self.notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version: Some(*version),
        })
    }

    fn notify<N: lsp_types::notification::Notification>(&self, params: N::Params) -> Result<()> {
        let notification = Notification::new(N::METHOD.to_string(), params);
        self.connection.sender.send(Message::Notification(notification))?;
        Ok(())
    }

    fn open(&mut self, uri: Url, text: String, version: i32) -> Result<()> {
        if let Some(path) = Self::path(&uri) {
            self.sources.set(path, text.clone());
        }
        self.documents.insert(uri.clone(), (text, version));
        self.publish(&uri)
    }

    /// The edits of a fix as a workspace edit; `None` when a target cannot
    /// be addressed (a `.po` file with `\r\n` line breaks, whose offsets are
    /// on normalised text).
    fn workspace_edit(&self, file: &Path, edits: &[Edit]) -> Option<WorkspaceEdit> {
        let mut by_target: BTreeMap<PathBuf, Vec<&Edit>> = BTreeMap::new();
        for edit in edits {
            let target = edit.path.clone().unwrap_or_else(|| file.to_path_buf());
            by_target.entry(target).or_default().push(edit);
        }
        let mut changes = HashMap::new();
        for (target, edits) in by_target {
            let text = self.sources.read_to_string(&target).ok()?;
            if is_po(&target) && text.contains('\r') {
                return None;
            }
            let text_edits = edits
                .iter()
                .map(|e| TextEdit {
                    range: Range::new(
                        self.encoding.position(&text, e.start),
                        self.encoding.position(&text, e.end),
                    ),
                    new_text: e.content.clone(),
                })
                .collect();
            changes.insert(Url::from_file_path(&target).ok()?, text_edits);
        }
        Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        })
    }

    fn code_actions(&self, params: &CodeActionParams) -> Vec<CodeActionOrCommand> {
        let uri = &params.text_document.uri;
        let (Some(path), Some((text, _))) = (Self::path(uri), self.documents.get(uri)) else {
            return Vec::new();
        };
        let Some((settings, violations)) = self.lint(&path) else {
            return Vec::new();
        };
        let wanted = |kind: &CodeActionKind| {
            params
                .context
                .only
                .as_ref()
                .is_none_or(|only| only.iter().any(|o| kind.as_str().starts_with(o.as_str())))
        };
        let mut actions = Vec::new();
        if wanted(&CodeActionKind::QUICKFIX) {
            let lines = params.range.start.line..=params.range.end.line;
            for violation in &violations {
                let Some(fix) = &violation.fix else { continue };
                let diagnostic = diagnostic(text, violation, self.encoding);
                if !lines.contains(&diagnostic.range.start.line) {
                    continue;
                }
                let Some(edit) = self.workspace_edit(&path, &fix.edits) else {
                    continue;
                };
                let safe = fix.applicability == Applicability::Safe;
                let title = if safe {
                    format!("{} ({})", fix.title, violation.code)
                } else {
                    format!("{} ({}, unsafe: review it)", fix.title, violation.code)
                };
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diagnostic]),
                    edit: Some(edit),
                    is_preferred: Some(safe),
                    ..CodeAction::default()
                }));
            }
        }
        let fix_all = CodeActionKind::from(FIX_ALL);
        let any_safe = violations
            .iter()
            .any(|v| v.fix.as_ref().is_some_and(|f| f.applicability == Applicability::Safe));
        if any_safe && wanted(&fix_all) {
            if let Some(edit) = self.fix_all(&path, &settings) {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Fix all safely fixable odoo-lint problems".to_string(),
                    kind: Some(fix_all),
                    edit: Some(edit),
                    ..CodeAction::default()
                }));
            }
        }
        actions
    }

    /// All safe fixes for a file, as whole-document replacements.
    fn fix_all(&self, path: &Path, settings: &Settings) -> Option<WorkspaceEdit> {
        let sources = self.sources.snapshot();
        let result = fixer::fix_paths_with(&[path.to_path_buf()], settings, FixMode::Safe, &sources);
        let mut changes = HashMap::new();
        for (target, old, new) in result.changed {
            // The fixer works on normalised `.po` text; the document must match.
            let current = self.sources.read_to_string(&target).ok()?;
            let old = if is_po(&target) { current.clone() } else { old };
            let range = Range::new(Position::new(0, 0), self.encoding.position(&old, old.len()));
            let new = if is_po(&target) && current.contains("\r\n") {
                new.replace('\n', "\r\n")
            } else {
                new
            };
            changes.insert(
                Url::from_file_path(&target).ok()?,
                vec![TextEdit { range, new_text: new }],
            );
        }
        (!changes.is_empty()).then(|| WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        })
    }

    fn handle_notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams = serde_json::from_value(notification.params)?;
                let doc = params.text_document;
                self.open(doc.uri, doc.text, doc.version)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams = serde_json::from_value(notification.params)?;
                // Full synchronisation: the last change is the whole text.
                if let Some(change) = params.content_changes.into_iter().last() {
                    let doc = params.text_document;
                    self.open(doc.uri, change.text, doc.version)?;
                }
            }
            DidSaveTextDocument::METHOD => {
                let _: DidSaveTextDocumentParams = serde_json::from_value(notification.params)?;
                // Other open files can depend on the saved one (a `.po` on its
                // `.pot`, a module's files on its manifest).
                let uris: Vec<Url> = self.documents.keys().cloned().collect();
                for uri in uris {
                    self.publish(&uri)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams = serde_json::from_value(notification.params)?;
                let uri = params.text_document.uri;
                self.documents.remove(&uri);
                if let Some(path) = Self::path(&uri) {
                    self.sources.remove(&path);
                }
                self.notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics: Vec::new(),
                    version: None,
                })?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Where the XML id, model or field at a position is defined.
    fn definition(&self, params: &lsp_types::TextDocumentPositionParams) -> Option<lsp_types::Location> {
        let uri = &params.text_document.uri;
        let path = Self::path(uri)?;
        let text = match self.documents.get(uri) {
            Some((text, _)) => text.clone(),
            None => std::fs::read_to_string(&path).ok()?,
        };
        let offset = self.encoding.offset(&text, params.position);
        let settings = Self::settings(&path)?;
        let module = linter::modules_of(std::slice::from_ref(&path), &self.sources)
            .into_iter()
            .next()?;
        let found = crate::definition::definition(&module, &settings.addons_path, &path, &text, offset)?;
        let line = found.line.saturating_sub(1) as u32;
        Some(lsp_types::Location {
            uri: Url::from_file_path(&found.path).ok()?,
            range: Range::new(Position::new(line, 0), Position::new(line, 0)),
        })
    }

    fn handle_request(&self, request: Request) -> Result<()> {
        let response = match request.method.as_str() {
            CodeActionRequest::METHOD => {
                let params: CodeActionParams = serde_json::from_value(request.params)?;
                Response::new_ok(request.id, self.code_actions(&params))
            }
            GotoDefinition::METHOD => {
                let params: lsp_types::GotoDefinitionParams = serde_json::from_value(request.params)?;
                Response::new_ok(request.id, self.definition(&params.text_document_position_params))
            }
            method => Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request: {method}"),
            ),
        };
        self.connection.sender.send(Message::Response(response))?;
        Ok(())
    }
}

fn capabilities(encoding: Encoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                include_text: Some(false),
            })),
            ..TextDocumentSyncOptions::default()
        })),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX, CodeActionKind::from(FIX_ALL)]),
            resolve_provider: Some(false),
            ..CodeActionOptions::default()
        })),
        ..ServerCapabilities::default()
    }
}

/// Serves one client over `connection` until it shuts down.
pub fn serve(connection: Connection) -> Result<()> {
    // Files change while the server runs; the index must follow.
    crate::index::revalidate_cache();
    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let encoding = Encoding::negotiate(&params);
    connection.initialize_finish(
        id,
        json!({
            "capabilities": capabilities(encoding),
            "serverInfo": {"name": "odoo-lint", "version": env!("CARGO_PKG_VERSION")},
        }),
    )?;
    let mut server = Server {
        connection,
        encoding,
        documents: HashMap::new(),
        sources: Sources::default(),
    };
    while let Ok(message) = server.connection.receiver.recv() {
        match message {
            Message::Request(request) => {
                if server.connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                server.handle_request(request)?;
            }
            Message::Notification(notification) => {
                if let Err(err) = server.handle_notification(notification) {
                    eprintln!("odoo-lint: {err}");
                }
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// `odl server`: serves on stdin/stdout.
pub fn run() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    serve(connection)?;
    io_threads.join()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_server::RequestId;
    use std::fs;

    const MODEL: &str = "from odoo import models\n\n\nclass Partner(models.Model):\n    _inherit = \"res.partner\"\n\n    def write(self, vals):\n        self._cr.execute(\"SELECT 1\")\n        return super().write(vals)\n";

    struct Client {
        connection: Connection,
        next_id: i32,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Client {
        fn start(encodings: &[&str]) -> Client {
            let (server, connection) = Connection::memory();
            let thread = std::thread::spawn(move || serve(server).unwrap());
            let mut client = Client {
                connection,
                next_id: 0,
                thread: Some(thread),
            };
            let result = client.request(
                "initialize",
                json!({"capabilities": {"general": {"positionEncodings": encodings}}}),
            );
            assert!(result["capabilities"]["codeActionProvider"].is_object());
            assert_eq!(result["capabilities"]["definitionProvider"], json!(true));
            client.notify("initialized", json!({}));
            client
        }

        fn notify(&self, method: &str, params: serde_json::Value) {
            let notification = Notification::new(method.to_string(), params);
            self.connection
                .sender
                .send(Message::Notification(notification))
                .unwrap();
        }

        fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
            self.next_id += 1;
            let id = RequestId::from(self.next_id);
            let request = Request::new(id.clone(), method.to_string(), params);
            self.connection.sender.send(Message::Request(request)).unwrap();
            loop {
                if let Message::Response(response) = self.connection.receiver.recv().unwrap() {
                    assert_eq!(response.id, id);
                    return response.result.unwrap_or_default();
                }
            }
        }

        fn diagnostics(&self) -> serde_json::Value {
            loop {
                if let Message::Notification(n) = self.connection.receiver.recv().unwrap() {
                    if n.method == PublishDiagnostics::METHOD {
                        return n.params;
                    }
                }
            }
        }

        fn stop(mut self) {
            self.request("shutdown", serde_json::Value::Null);
            self.notify("exit", serde_json::Value::Null);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    fn addon(dir: &Path) -> PathBuf {
        let module = dir.join("acme_lsp");
        fs::create_dir_all(module.join("models")).unwrap();
        fs::write(
            module.join("__manifest__.py"),
            "{\n    'name': 'LSP',\n    'license': 'AGPL-3',\n    'author': 'Odoo Community Association (OCA)',\n}\n",
        )
        .unwrap();
        fs::write(module.join("README.rst"), "LSP\n").unwrap();
        fs::write(dir.join("odoo-lint.toml"), "target-version = \"19.0\"\n").unwrap();
        module.join("models/partner.py")
    }

    #[test]
    fn go_to_definition() {
        let dir = tempfile::tempdir().unwrap();
        // `base` next to the module: the index finds it without addons-path.
        let base = dir.path().join("base");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("__manifest__.py"), "{'name': 'base'}\n").unwrap();
        fs::write(base.join("__init__.py"), "").unwrap();
        fs::write(
            base.join("models.py"),
            "from odoo import models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n",
        )
        .unwrap();
        let path = addon(dir.path());
        fs::write(path.parent().unwrap().parent().unwrap().join("__init__.py"), "").unwrap();
        fs::write(&path, MODEL).unwrap();
        let uri = Url::from_file_path(&path).unwrap();
        let mut client = Client::start(&["utf-16"]);
        // On `res.partner` in `_inherit = "res.partner"` (line 5, 0-based 4).
        let result = client.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": {"line": 4, "character": 20}}),
        );
        // The index works on canonical paths (`/private/var` on macOS, long
        // names on Windows): compare the files, not their spellings.
        let target = Url::parse(result["uri"].as_str().unwrap())
            .unwrap()
            .to_file_path()
            .unwrap();
        assert_eq!(
            target.canonicalize().unwrap(),
            base.join("models.py").canonicalize().unwrap()
        );
        assert_eq!(result["range"]["start"]["line"], json!(3));
        // Not on anything: no location.
        let nothing = client.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": {"line": 6, "character": 8}}),
        );
        assert!(nothing.is_null(), "{nothing}");
        client.stop();
    }

    #[test]
    fn diagnostics_and_fixes_follow_the_editor() {
        let dir = tempfile::tempdir().unwrap();
        let path = addon(dir.path());
        // On disk the file is clean; the editor's unsaved text is not.
        fs::write(&path, MODEL.replace("self._cr", "self.env.cr")).unwrap();
        let uri = Url::from_file_path(&path).unwrap();
        let mut client = Client::start(&["utf-16"]);
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "python", "version": 1, "text": MODEL}}),
        );
        let published = client.diagnostics();
        let diagnostics = published["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1, "{published}");
        assert_eq!(diagnostics[0]["code"], "W8165");
        assert_eq!(diagnostics[0]["range"]["start"], json!({"line": 7, "character": 8}));
        assert_eq!(diagnostics[0]["severity"], 2);

        let actions = client.request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 7, "character": 0}, "end": {"line": 7, "character": 0}},
                "context": {"diagnostics": []},
            }),
        );
        let actions = actions.as_array().unwrap();
        let quick_fix = &actions[0];
        assert_eq!(quick_fix["kind"], "quickfix");
        assert_eq!(quick_fix["isPreferred"], true);
        let edits = &quick_fix["edit"]["changes"][uri.as_str()];
        assert_eq!(
            edits,
            &json!([{"range": {"start": {"line": 7, "character": 13}, "end": {"line": 7, "character": 16}}, "newText": "env.cr"}])
        );
        let fix_all = &actions[1];
        assert_eq!(fix_all["kind"], FIX_ALL);
        let new_text = fix_all["edit"]["changes"][uri.as_str()][0]["newText"].as_str().unwrap();
        assert_eq!(new_text, MODEL.replace("self._cr", "self.env.cr"));

        // Fixed in the editor: the problem goes away without saving.
        client.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": new_text}]}),
        );
        assert_eq!(client.diagnostics()["diagnostics"], json!([]));
        client.stop();
    }

    #[test]
    fn files_outside_addons_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.py");
        fs::write(&path, MODEL).unwrap();
        let mut client = Client::start(&[]);
        let uri = Url::from_file_path(&path).unwrap();
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "python", "version": 1, "text": MODEL}}),
        );
        assert_eq!(client.diagnostics()["diagnostics"], json!([]));
        let actions = client.request(
            "textDocument/codeAction",
            json!({"textDocument": {"uri": uri}, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}, "context": {"diagnostics": []}}),
        );
        assert_eq!(actions, json!([]));
        client.stop();
    }

    #[test]
    fn positions_in_each_encoding() {
        let text = "a = 'é😀'\nb";
        let offset = text.find('b').unwrap();
        assert_eq!(Encoding::Utf8.position(text, offset), Position::new(1, 0));
        let end = text.find('\n').unwrap();
        assert_eq!(Encoding::Utf8.position(text, end), Position::new(0, 12));
        assert_eq!(Encoding::Utf16.position(text, end), Position::new(0, 9));
        assert_eq!(Encoding::Utf32.position(text, end), Position::new(0, 8));
    }
}
