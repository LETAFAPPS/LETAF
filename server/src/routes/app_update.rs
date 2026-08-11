use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::context::AppState;

/// Rotas de atualização do desktop (públicas — é sobre o binário do app,
/// não dados de empresa, então NÃO usam TenantContext/§multi-tenant).
///
/// Regras (AI_RULES.md §11): o servidor é a autoridade sobre qual é a
/// última versão e se a atualização é obrigatória (`min_supported`). O
/// desktop apenas compara e renderiza o modal.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/app/version", get(version))
        .route("/app/download/{file}", get(download))
}

/// GET /app/version — devolve o `manifest.json` COMO ESTÁ (apenas valida
/// que é JSON), preservando campos arbitrários como `sha256` e futuros.
/// Sem manifesto (ou JSON inválido) = 204 No Content, que o desktop
/// interpreta como "nenhuma atualização".
///
/// Formato esperado (o desktop consome `latest`, `min_supported`,
/// `notes`, `files` e `sha256`):
/// ```json
/// { "latest": "0.2.0", "min_supported": "0.1.5", "notes": "…",
///   "files":  { "linux": "letaf-0.2.0.AppImage" },
///   "sha256": { "linux": "<hex>" } }
/// ```
async fn version(State(state): State<AppState>) -> Response {
    let path = std::path::Path::new(&state.config.app_updates_dir).join("manifest.json");
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NO_CONTENT.into_response();
    };
    // Valida que é JSON, mas devolve os bytes originais (não faz round-trip
    // por um struct — assim campos como `sha256` não se perdem).
    if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
        tracing::warn!("manifest.json inválido em {}", path.display());
        return StatusCode::NO_CONTENT.into_response();
    }
    ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
}

/// GET /app/download/{file} — serve um binário do diretório de updates.
///
/// Segurança: só aceita NOME de arquivo simples — rejeita separadores e
/// `..` para impedir path traversal (ler arquivos fora do diretório).
async fn download(State(state): State<AppState>, Path(file): Path<String>) -> Response {
    if file.is_empty() || file.contains('/') || file.contains('\\') || file.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let path = std::path::Path::new(&state.config.app_updates_dir).join(&file);
    // STREAM em vez de `tokio::fs::read` (que carregaria o binário INTEIRO em
    // RAM): a rota é pública e sem rate limit, então ler ~100 MB por conexão
    // permitia OOM com poucas conexões concorrentes (§11/§13). `ReaderStream`
    // envia em chunks, com memória O(chunk), não O(arquivo).
    let Ok(f) = tokio::fs::File::open(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Content-Length quando o tamanho é conhecido (melhora o progresso no
    // cliente); ausência apenas cai em chunked encoding.
    let len = f.metadata().await.ok().map(|m| m.len());
    let stream = tokio_util::io::ReaderStream::new(f);
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{file}\""),
        );
    if let Some(n) = len {
        builder = builder.header(header::CONTENT_LENGTH, n);
    }
    builder
        .body(axum::body::Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
