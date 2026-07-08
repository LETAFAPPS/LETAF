//! Servidor SSR do cardápio (Leptos + axum). Camada de apresentação
//! separada da API REST (AI_RULES.md §1/§3): aqui só roteia/renderiza;
//! os dados vêm da API (letaf-server).
// axum + Leptos geram tipos de router profundos; a rota extra de proxy de
// mídia estoura o limite padrão (128) na resolução de traits.
#![recursion_limit = "256"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use letaf_web::app::*;

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    let app = Router::new()
        // Proxy de mídia do catálogo → API. Mantém a imagem na MESMA ORIGEM do
        // cardápio (sem CORS, sem base absoluta): o `<img src="/catalog/media/…">`
        // bate aqui e é encaminhado à API preservando o `Host` (tenant). Funciona
        // em dev e em prod sem depender de proxy reverso externo (AI_RULES §3).
        .route("/catalog/media/{*rest}", axum::routing::get(media_proxy))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    log!("cardápio SSR escutando em http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

/// Encaminha `GET /catalog/media/*` para a API (`LETAF_API_BASE`, padrão
/// `127.0.0.1:3001`) preservando o `Host` para a API resolver o tenant.
/// Repassa bytes + `Content-Type`/`Cache-Control` da resposta.
#[cfg(feature = "ssr")]
async fn media_proxy(
    axum::extract::Path(rest): axum::extract::Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::{header, HeaderValue, StatusCode};
    use axum::response::IntoResponse;

    // Só encaminha mídia do catálogo. Restringe `rest` aos prefixos esperados
    // e rejeita `..` — senão um `..%2f..%2f` faria traversal para outras rotas
    // internas da API (SSRF de leitura, §11).
    let prefix_ok = matches!(
        rest.split('/').next(),
        Some("product") | Some("banner") | Some("logo") | Some("cover")
    );
    if !prefix_ok || rest.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }

    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let tenant = host.split(':').next().unwrap_or(host);
    let base = std::env::var("LETAF_API_BASE").unwrap_or_else(|_| "http://127.0.0.1:3001".into());
    let qs = query.map(|q| format!("?{q}")).unwrap_or_default();
    let url = format!("{base}/catalog/media/{rest}{qs}");

    let resp = match letaf_web::http_client().get(&url).header(header::HOST, tenant).send().await {
        Ok(r) => r,
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let pick = |h: &reqwest::header::HeaderName, default: &str| {
        resp.headers().get(h).and_then(|v| v.to_str().ok()).unwrap_or(default).to_owned()
    };
    let ct = pick(&reqwest::header::CONTENT_TYPE, "application/octet-stream");
    let cc = pick(&reqwest::header::CACHE_CONTROL, "public, max-age=31536000, immutable");
    let body = resp.bytes().await.unwrap_or_default();

    let mut out = body.into_response();
    *out.status_mut() = status;
    if let Ok(v) = HeaderValue::from_str(&ct) {
        out.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&cc) {
        out.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    out
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // Sem main no cliente — a entrada de hidratação está em `lib.rs`.
}
