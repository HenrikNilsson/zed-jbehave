//! End-to-end test driving the real `tower-lsp-server` service: initialize
//! against `test-fixtures/sample-project`, open its `.story` fixture, and
//! assert go-to-definition resolves steps to the Java methods in
//! `AccountSteps.java` (including via aliases), returns nothing for undefined
//! steps, and that opening the story publishes "Undefined step" diagnostics.

use std::path::{Path, PathBuf};

use futures::stream::StreamExt;
use jbehave_language_server::lsp::Backend;
use serde_json::{Value, json};
use tower::util::ServiceExt;
use tower_lsp_server::jsonrpc::{Id, Request, Response};
use tower_lsp_server::ls_types::{GotoDefinitionResponse, Uri};
use tower_lsp_server::{ExitedError, LspService};

fn sample_project_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../test-fixtures/sample-project")
}

fn file_uri(relative: &str) -> Uri {
    Uri::from_file_path(sample_project_dir().join(relative)).expect("absolute path")
}

async fn send<S>(service: &mut S, req: Request) -> Option<Response>
where
    S: tower::Service<Request, Response = Option<Response>, Error = ExitedError>,
{
    service.ready().await.expect("service ready");
    service.call(req).await.expect("service call")
}

async fn request(
    service: &mut LspService<Backend>,
    method: &'static str,
    params: Value,
) -> Response {
    send(
        service,
        Request::build(method)
            .id(Id::Number(1))
            .params(params)
            .finish(),
    )
    .await
    .expect("a request gets a response")
}

async fn notify(service: &mut LspService<Backend>, method: &'static str, params: Value) {
    send(service, Request::build(method).params(params).finish()).await;
}

fn line_of(source: &str, needle: &str) -> u32 {
    source
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("line containing `{needle}` not found")) as u32
}

#[tokio::test]
async fn goto_definition_resolves_story_steps_to_java_methods() {
    let (mut service, mut socket) = LspService::new(Backend::new);
    let project_uri = Uri::from_file_path(sample_project_dir()).expect("absolute path");

    request(
        &mut service,
        "initialize",
        json!({
            "capabilities": {},
            "workspaceFolders": [{"uri": project_uri, "name": "sample-project"}],
        }),
    )
    .await;

    let story_uri = file_uri("stories/account.story");
    let story_text = std::fs::read_to_string(story_uri.to_file_path().unwrap()).unwrap();
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {"uri": story_uri, "languageId": "jbehave", "version": 1, "text": story_text},
        }),
    )
    .await;

    let steps_source = std::fs::read_to_string(
        file_uri("src/test/java/steps/AccountSteps.java")
            .to_file_path()
            .unwrap(),
    )
    .unwrap();

    // A plain `@Given` step resolves to the `hasBalance` method.
    let given_line = line_of(&story_text, "Given Bob has 100 in account");
    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": given_line, "character": 8},
        }),
    )
    .await;
    let location =
        match serde_json::from_value::<GotoDefinitionResponse>(def.result().unwrap().clone())
            .unwrap()
        {
            GotoDefinitionResponse::Scalar(loc) => loc,
            other => panic!("expected a single location, got {other:?}"),
        };
    assert_eq!(
        std::fs::canonicalize(location.uri.to_file_path().unwrap()).unwrap(),
        std::fs::canonicalize(
            file_uri("src/test/java/steps/AccountSteps.java")
                .to_file_path()
                .unwrap()
        )
        .unwrap(),
    );
    assert_eq!(
        location.range.start.line,
        line_of(
            &steps_source,
            "@Given(\"$customer has $amount in account\")"
        ),
    );

    // A step written via `@Alias` resolves to the same method.
    let alias_line = line_of(&story_text, "When Bob takes out 50");
    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": alias_line, "character": 8},
        }),
    )
    .await;
    let location =
        match serde_json::from_value::<GotoDefinitionResponse>(def.result().unwrap().clone())
            .unwrap()
        {
            GotoDefinitionResponse::Scalar(loc) => loc,
            other => panic!("expected a single location, got {other:?}"),
        };
    assert_eq!(
        location.range.start.line,
        line_of(&steps_source, "@When(\"$customer withdraws $amount\")"),
    );

    // An undefined step has no definition to jump to.
    let undefined_line = line_of(&story_text, "Given a totally unknown precondition");
    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": undefined_line, "character": 8},
        }),
    )
    .await;
    assert!(
        def.result().map(|r| r.is_null()).unwrap_or(true),
        "undefined step should resolve to nothing, got: {:?}",
        def.result(),
    );

    // Opening the story also published diagnostics: the two undefined steps
    // are Error-severity "Undefined step" diagnostics, and nothing is flagged
    // ambiguous (the fixture's alias patterns resolve uniquely).
    let publish = socket.next().await.expect("a publishDiagnostics message");
    assert_eq!(publish.method(), "textDocument/publishDiagnostics");
    let params = publish.params().expect("params");
    assert!(params["uri"].as_str().unwrap().ends_with("account.story"));
    let diagnostics = params["diagnostics"].as_array().unwrap();
    let undefined: Vec<&Value> = diagnostics
        .iter()
        .filter(|d| d["message"].as_str().unwrap().starts_with("Undefined step"))
        .collect();
    assert_eq!(
        undefined.len(),
        2,
        "two undefined steps, got: {undefined:?}"
    );
    let ambiguous: Vec<&Value> = diagnostics
        .iter()
        .filter(|d| d["message"].as_str().unwrap().starts_with("Ambiguous step"))
        .collect();
    assert!(
        ambiguous.is_empty(),
        "no ambiguous steps expected: {ambiguous:?}"
    );
}

/// The extension forwards the user's `lsp.jbehave-language-server.settings`
/// block, which Zed pushes to the server as a `workspace/didChangeConfiguration`
/// notification right after `initialize`. A workspace whose steps use a
/// non-`$` parameter prefix must become resolvable the moment that config
/// lands, and the pending story diagnostics must be re-evaluated.
#[tokio::test]
async fn changing_parameter_prefix_via_configuration_updates_resolution_and_diagnostics() {
    let (mut service, mut socket) = LspService::new(Backend::new);
    let project_uri = Uri::from_file_path(sample_project_dir()).expect("absolute path");

    request(
        &mut service,
        "initialize",
        json!({
            "capabilities": {},
            "workspaceFolders": [{"uri": project_uri, "name": "sample-project"}],
        }),
    )
    .await;

    let story_uri = file_uri("stories/percent.story");
    let story_text = std::fs::read_to_string(story_uri.to_file_path().unwrap()).unwrap();
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {"uri": story_uri, "languageId": "jbehave", "version": 1, "text": story_text},
        }),
    )
    .await;

    // The fixture's steps only compile under a `%` prefix, so before any
    // config the story reports an undefined step.
    let undefined_line = line_of(&story_text, "Given Alice holds 7 in wallet");
    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": undefined_line, "character": 8},
        }),
    )
    .await;
    assert!(
        def.result().map(|r| r.is_null()).unwrap_or(true),
        "percent-prefixed step must not resolve under the default `$` prefix"
    );

    // Opening the story published diagnostics flagging that same step as
    // undefined under the default `$` prefix.
    let initial_publish = socket.next().await.expect("a publishDiagnostics message");
    assert_eq!(initial_publish.method(), "textDocument/publishDiagnostics");
    let initial_params = initial_publish.params().expect("params");
    let initial_diagnostics = initial_params["diagnostics"].as_array().unwrap();
    let undefined: Vec<&Value> = initial_diagnostics
        .iter()
        .filter(|d| d["message"].as_str().unwrap().starts_with("Undefined step"))
        .collect();
    assert_eq!(
        undefined.len(),
        1,
        "one undefined step before any config, got: {undefined:?}"
    );

    // Zed sends the workspace settings after initialize; switching the prefix
    // must both make the step resolve and refresh the diagnostics.
    notify(
        &mut service,
        "workspace/didChangeConfiguration",
        json!({"settings": {"parameter_prefix": "%"}}),
    )
    .await;

    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": undefined_line, "character": 8},
        }),
    )
    .await;
    let location =
        match serde_json::from_value::<GotoDefinitionResponse>(def.result().unwrap().clone())
            .unwrap()
        {
            GotoDefinitionResponse::Scalar(loc) => loc,
            other => panic!("expected a single location, got {other:?}"),
        };
    assert!(
        location
            .uri
            .to_file_path()
            .unwrap()
            .ends_with("PercentSteps.java")
    );

    // The config change re-published the story's diagnostics; the undefined
    // step is gone.
    let republished = socket
        .next()
        .await
        .expect("a publishDiagnostics message after config change");
    assert_eq!(republished.method(), "textDocument/publishDiagnostics");
    let params = republished.params().expect("params");
    let diagnostics = params["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .all(|d| !d["message"].as_str().unwrap().starts_with("Undefined step")),
        "no undefined-step diagnostics expected after prefix change: {diagnostics:?}"
    );
}

/// The extension attaches the server to Java/Groovy as well as JBehave, so
/// Zed forwards their document events too. Editing a `.java` file must
/// refresh the index without a server restart: a changed `@Given` pattern
/// stops resolving, and restoring it starts resolving again.
#[tokio::test]
async fn editing_a_java_file_updates_step_resolution_without_reinitialize() {
    async fn defined(service: &mut LspService<Backend>, uri: &Uri, line: u32) -> bool {
        let def = request(
            service,
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": 8},
            }),
        )
        .await;
        !def.result().map(|r| r.is_null()).unwrap_or(true)
    }

    let (mut service, _socket) = LspService::new(Backend::new);
    let project_uri = Uri::from_file_path(sample_project_dir()).expect("absolute path");

    request(
        &mut service,
        "initialize",
        json!({
            "capabilities": {},
            "workspaceFolders": [{"uri": project_uri, "name": "sample-project"}],
        }),
    )
    .await;

    let story_uri = file_uri("stories/account.story");
    let story_text = std::fs::read_to_string(story_uri.to_file_path().unwrap()).unwrap();
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {"uri": story_uri, "languageId": "jbehave", "version": 1, "text": story_text},
        }),
    )
    .await;

    let java_uri = file_uri("src/test/java/steps/AccountSteps.java");
    let original = std::fs::read_to_string(java_uri.to_file_path().unwrap()).unwrap();
    let given_line = line_of(&story_text, "Given Bob has 100 in account");
    assert!(
        defined(&mut service, &story_uri, given_line).await,
        "step resolves before any edit"
    );

    // Rewriting the `@Given` pattern must invalidate the old match…
    let edited = original.replace(
        "@Given(\"$customer has $amount in account\")",
        "@Given(\"$customer has $amount coins\")",
    );
    assert_ne!(edited, original, "fixture should contain the pattern");
    notify(
        &mut service,
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": java_uri, "version": 2},
            "contentChanges": [{"text": edited}],
        }),
    )
    .await;
    assert!(
        !defined(&mut service, &story_uri, given_line).await,
        "edited-away pattern must stop resolving"
    );

    // …and restoring the source must bring it back.
    notify(
        &mut service,
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": java_uri, "version": 3},
            "contentChanges": [{"text": original}],
        }),
    )
    .await;
    assert!(
        defined(&mut service, &story_uri, given_line).await,
        "restored pattern resolves again"
    );
}

/// Deleting a step-definition file must drop its entries: without the
/// `workspace/didChangeWatchedFiles` handler the index kept pointing into
/// deleted files. The notification is URI-only — no fixture file on disk is
/// touched by this test.
#[tokio::test]
async fn deleting_a_java_file_drops_its_definitions_and_republishes() {
    fn undefined_count(publish: &tower_lsp_server::jsonrpc::Request) -> usize {
        publish.params().expect("params")["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["message"].as_str().unwrap().starts_with("Undefined step"))
            .count()
    }

    let (mut service, mut socket) = LspService::new(Backend::new);
    let project_uri = Uri::from_file_path(sample_project_dir()).expect("absolute path");

    request(
        &mut service,
        "initialize",
        json!({
            "capabilities": {},
            "workspaceFolders": [{"uri": project_uri, "name": "sample-project"}],
        }),
    )
    .await;

    let story_uri = file_uri("stories/account.story");
    let story_text = std::fs::read_to_string(story_uri.to_file_path().unwrap()).unwrap();
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {"uri": story_uri, "languageId": "jbehave", "version": 1, "text": story_text},
        }),
    )
    .await;

    // account.story has 8 steps, 2 of which are undefined up front.
    let initial = socket.next().await.expect("initial publishDiagnostics");
    assert_eq!(undefined_count(&initial), 2);

    let java_uri = file_uri("src/test/java/steps/AccountSteps.java");
    let given_line = line_of(&story_text, "Given Bob has 100 in account");
    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": given_line, "character": 8},
        }),
    )
    .await;
    assert!(
        def.result().map(|r| !r.is_null()).unwrap_or(false),
        "step resolves while its file exists"
    );

    notify(
        &mut service,
        "workspace/didChangeWatchedFiles",
        json!({"changes": [{"uri": java_uri, "type": 3}]}),
    )
    .await;

    // The delete re-publishes: all 8 steps are now undefined…
    let republished = socket
        .next()
        .await
        .expect("publishDiagnostics after delete");
    assert_eq!(republished.method(), "textDocument/publishDiagnostics");
    assert_eq!(undefined_count(&republished), 8);

    // …and go-to-definition no longer leads into the deleted file.
    let def = request(
        &mut service,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": story_uri},
            "position": {"line": given_line, "character": 8},
        }),
    )
    .await;
    assert!(
        def.result().map(|r| r.is_null()).unwrap_or(true),
        "deleted file's steps must stop resolving"
    );
}
