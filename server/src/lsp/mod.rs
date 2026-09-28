use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::index::{ResolvedMatch, StepDefinitionIndex};
use crate::matcher::DEFAULT_PLACEHOLDER_PREFIX;
use crate::story;

/// Per-workspace settings the server accepts from the client. The user sets
/// these under `"lsp": { "jbehave-language-server": { "settings": {...} } }`;
/// Zed passes them through unchanged as the `settings` of a
/// `workspace/didChangeConfiguration` notification.
#[derive(Debug, Default)]
pub struct ServerSettings {
    /// The JBehave parameter prefix used when compiling step patterns
    /// (default `$`), for projects that don't use the stock one.
    pub parameter_prefix: char,
}

impl ServerSettings {
    fn from_value(value: serde_json::Value) -> Self {
        let parameter_prefix = value
            .get("parameter_prefix")
            .and_then(|p| p.as_str())
            .and_then(|s| s.chars().next())
            .unwrap_or(DEFAULT_PLACEHOLDER_PREFIX);
        Self { parameter_prefix }
    }
}

pub struct Backend {
    client: Client,
    index: Arc<StepDefinitionIndex>,
    /// Text of every `.story` document the client has told us about, keyed
    /// by URI. `.java`/`.groovy` files don't need their text kept — their
    /// step definitions are extracted eagerly into the index — but go-to-
    /// definition has to parse the story buffer at request time.
    documents: DashMap<Uri, StoryDocument>,
}

struct StoryDocument {
    /// Last version seen from the client, reused when publishing diagnostics
    /// for saves (which carry no version of their own).
    version: i32,
    text: String,
}

fn has_extension(uri: &Uri, ext: &str) -> bool {
    Path::new(uri.path().as_str())
        .extension()
        .and_then(|e| e.to_str())
        == Some(ext)
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            index: Arc::new(StepDefinitionIndex::new()),
            documents: DashMap::new(),
        }
    }

    /// Handles a full-document update from the client: refreshes the index
    /// for `.java`/`.groovy`, and for `.story` stores the buffer (with its
    /// version, so published diagnostics stay correctly ordered) and
    /// re-publishes step-resolution diagnostics.
    async fn sync_document(&self, uri: Uri, version: i32, text: &str) {
        if has_extension(&uri, "java") {
            self.index.update_java_file(uri, text);
        } else if has_extension(&uri, "groovy") {
            self.index.update_groovy_file(uri, text);
        } else if has_extension(&uri, "story") {
            self.documents.insert(
                uri.clone(),
                StoryDocument {
                    version,
                    text: text.to_string(),
                },
            );
            self.publish_story_diagnostics(&uri, text, version).await;
        }
    }

    /// Snapshot of every open `.story` buffer for passes that must revisit
    /// them all (prefix change, step-definition file deletion).
    fn open_stories(&self) -> Vec<(Uri, String, i32)> {
        self.documents
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    entry.value().text.clone(),
                    entry.value().version,
                )
            })
            .collect()
    }

    /// Resolves every step in a `.story` buffer against the step-definition
    /// index and publishes the result: "Undefined step" (Error) when no
    /// definition matches, "Ambiguous step: matches N step definitions"
    /// (Warning) when more than one distinct method does.
    async fn publish_story_diagnostics(&self, uri: &Uri, source: &str, version: i32) {
        let parsed = story::parse(source);
        let mut diagnostics = Vec::new();
        for step in &parsed.steps {
            match self.index.resolve(step.step_type, &step.text) {
                ResolvedMatch::None => diagnostics.push(Diagnostic::new_simple(
                    step.range,
                    format!("Undefined step: {}", step.text),
                )),
                ResolvedMatch::Ambiguous { candidates, .. } => diagnostics.push(Diagnostic {
                    range: step.range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!(
                        "Ambiguous step: matches {} step definitions",
                        candidates.len()
                    ),
                    ..Diagnostic::default()
                }),
                ResolvedMatch::Unique(_) => {}
            }
        }
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, Some(version))
            .await;
    }
}

/// Walks the workspace looking for `.java`/`.groovy` files and indexes every
/// step definition found. This scans the whole workspace tree (respecting
/// `.gitignore` via the `ignore` crate) rather than assuming Maven/Gradle's
/// conventional `src/main/java` layout, since JBehave projects don't
/// universally follow it.
///
/// Scoped to workspace source files per the project's documented v1
/// limitation: step definitions living in dependency `.jar`s aren't
/// resolved (see docs/PARITY.md).
///
/// Pure blocking filesystem work — invoked via `spawn_blocking` from
/// `initialize`, never on the async runtime.
fn index_workspace(index: &StepDefinitionIndex, root: &Path) {
    for entry in ignore::WalkBuilder::new(root).build().flatten() {
        let path = entry.path();
        let is_source = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("java") | Some("groovy")
        );
        if !is_source {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(uri) = Uri::from_file_path(path) else {
            continue;
        };
        if has_extension(&uri, "java") {
            index.update_java_file(uri, &source);
        } else {
            index.update_groovy_file(uri, &source);
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let roots: Vec<PathBuf> = params
            .workspace_folders
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| f.uri.to_file_path().map(|p| p.into_owned()))
            .collect();
        for root in roots {
            let index = Arc::clone(&self.index);
            let _ = tokio::task::spawn_blocking(move || index_workspace(&index, &root)).await;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "jbehave-language-server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Watch step-definition sources for deletion so the index never
        // points into deleted files (see `did_change_watched_files`).
        // Detached: a client that never answers `client/registerCapability`
        // must not wedge initialization.
        let client = self.client.clone();
        tokio::spawn(async move {
            let options = DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![FileSystemWatcher {
                    glob_pattern: GlobPattern::String("**/*.{java,groovy}".to_string()),
                    kind: Some(WatchKind::Delete),
                }],
            };
            let _ = client
                .register_capability(vec![Registration {
                    id: "jbehave-watched-deletes".to_string(),
                    method: "workspace/didChangeWatchedFiles".to_string(),
                    register_options: Some(
                        serde_json::to_value(options).unwrap_or(serde_json::Value::Null),
                    ),
                }])
                .await;
        });
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "jbehave-language-server: indexed {} step definition(s)",
                    self.index.len()
                ),
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.sync_document(doc.uri, doc.version, &doc.text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Server only advertises FULL sync, so there's exactly one change
        // event and it contains the whole document.
        if let Some(change) = params.content_changes.into_iter().next() {
            self.sync_document(
                params.text_document.uri,
                params.text_document.version,
                &change.text,
            )
            .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            let uri = params.text_document.uri;
            let version = self.documents.get(&uri).map(|doc| doc.version).unwrap_or(0);
            self.sync_document(uri, version, &text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if has_extension(&uri, "story") {
            self.documents.remove(&uri);
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let settings = ServerSettings::from_value(params.settings);
        self.index.set_placeholder_prefix(settings.parameter_prefix);

        // Changing the parameter prefix changes which patterns match every
        // open `.story` buffer, so re-run resolution on them all.
        for (uri, text, version) in self.open_stories() {
            self.publish_story_diagnostics(&uri, &text, version).await;
        }
    }

    /// Drops step definitions whose source file was deleted and re-resolves
    /// every open story: without this, go-to-definition keeps jumping into
    /// deleted files and diagnostics stay green for steps that no longer
    /// exist. Only deletions are watched — creations/changes already arrive
    /// as `did_open`/`did_change`/`did_save`.
    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let mut removed = false;
        for event in &params.changes {
            if event.typ == FileChangeType::DELETED
                && (has_extension(&event.uri, "java") || has_extension(&event.uri, "groovy"))
            {
                self.index.remove_file(&event.uri);
                removed = true;
            }
        }
        if removed {
            for (uri, text, version) in self.open_stories() {
                self.publish_story_diagnostics(&uri, &text, version).await;
            }
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let text_document_position = params.text_document_position_params;
        let uri = text_document_position.text_document.uri;
        let position = text_document_position.position;

        let Some(document) = self.documents.get(&uri) else {
            return Ok(None);
        };
        let parsed = story::parse(&document.text);
        let Some(step) = story::step_at(&parsed, position) else {
            return Ok(None);
        };

        Ok(match self.index.resolve(step.step_type, &step.text) {
            ResolvedMatch::None => None,
            ResolvedMatch::Unique(def) => Some(GotoDefinitionResponse::Scalar(Location {
                uri: def.file,
                range: def.method_range,
            })),
            ResolvedMatch::Ambiguous { candidates, .. } => Some(GotoDefinitionResponse::Array(
                candidates
                    .iter()
                    .map(|c| Location {
                        uri: c.file.clone(),
                        range: c.method_range,
                    })
                    .collect(),
            )),
        })
    }
}

pub async fn run() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
