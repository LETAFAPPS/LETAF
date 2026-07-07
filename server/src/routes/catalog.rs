use axum::extract::{Path, State};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::Serialize;
use uuid::Uuid;

use crate::context::AppState;
use crate::error::ServerError;
use crate::media::{decode_image, IMMUTABLE_CACHE};
use crate::middleware::tenant::TenantContext;

/// Rotas públicas do catálogo (cardápio digital).
///
/// Regras aplicadas (AI_RULES.md §3, §5 Web):
/// - Empresa identificada automaticamente pelo subdomínio
/// - Sem autenticação necessária (catálogo público)
/// - Somente leitura
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/catalog/products", get(list_products))
        .route("/catalog/categories", get(list_categories))
        .route("/catalog/category-icons", get(list_category_icons))
        .route("/catalog/subcategories", get(list_subcategories))
        .route("/catalog/business-hours", get(list_business_hours))
        .route("/catalog/banners", get(list_banners))
        .route("/catalog/coupons/validate", get(validate_coupon))
        .route("/catalog/info", get(get_info))
        // Mídia pública servida como bytes (não base64 inline) — HTML enxuto,
        // cache longo, tenant pelo Host. Requer que o proxy roteie
        // `/catalog/*` para a API (mesmo bloco dos demais /catalog).
        .route("/catalog/media/product/{id}", get(media_product))
        .route("/catalog/media/banner/{id}", get(media_banner))
        .route("/catalog/media/logo", get(media_logo))
        .route("/catalog/media/cover", get(media_cover))
}

/// Constrói a resposta de mídia: bytes decodificados + Content-Type +
/// cache imutável. Imagem ausente/base64 inválido → 404.
fn media_response(data: Option<String>) -> Response {
    match data.as_deref().and_then(decode_image) {
        Some((bytes, mime)) => {
            let mut resp = bytes.into_response();
            let headers = resp.headers_mut();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE_CACHE));
            resp
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// GET /catalog/media/product/{id} — imagem do produto (público, tenant por Host).
async fn media_product(
    State(state): State<AppState>,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Response {
    let data = state
        .product_service
        .find_by_id(tenant.company_id, id)
        .await
        .ok()
        .flatten()
        .and_then(|p| p.image_data);
    media_response(data)
}

/// GET /catalog/media/banner/{id} — imagem do banner.
async fn media_banner(
    State(state): State<AppState>,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Response {
    let data = state
        .banner_service
        .find_by_id(tenant.company_id, id)
        .await
        .ok()
        .flatten()
        .map(|b| b.image_data);
    media_response(data)
}

/// GET /catalog/media/logo — logotipo da empresa.
async fn media_logo(State(state): State<AppState>, tenant: TenantContext) -> Response {
    let data = state
        .company_service
        .find_by_id(tenant.company_id)
        .await
        .ok()
        .flatten()
        .and_then(|c| c.logo_data);
    media_response(data)
}

/// GET /catalog/media/cover — capa da empresa.
async fn media_cover(State(state): State<AppState>, tenant: TenantContext) -> Response {
    let data = state
        .company_service
        .find_by_id(tenant.company_id)
        .await
        .ok()
        .flatten()
        .and_then(|c| c.cover_data);
    media_response(data)
}

/// Monta a URL relativa de mídia com cache-busting por `updated_at` (epoch).
/// Relativa de propósito: o navegador resolve contra o subdomínio do tenant,
/// e a API resolve a empresa pelo Host — sem base absoluta nem company_id.
fn media_url(path: &str, updated_at: chrono::NaiveDateTime) -> String {
    format!("/catalog/media/{path}?v={}", updated_at.and_utc().timestamp())
}

/// Resposta de pré-validação de cupom (carrinho web). É só uma
/// PRÉVIA: a validação definitiva (limites de uso, primeira compra)
/// acontece na criação do pedido, que conhece o cliente (§11).
#[derive(Serialize)]
struct CouponValidateResponse {
    valid: bool,
    code: String,
    coupon_type: String,
    discount: f64,
    message: String,
}

#[derive(serde::Deserialize)]
struct CouponValidateQuery {
    code: String,
    #[serde(default)]
    subtotal: f64,
}

/// GET /catalog/coupons/validate?code=&subtotal= — pré-valida um
/// cupom para o carrinho (público, tenant por subdomínio).
async fn validate_coupon(
    State(state): State<AppState>,
    tenant: TenantContext,
    axum::extract::Query(q): axum::extract::Query<CouponValidateQuery>,
) -> Result<Json<CouponValidateResponse>, ServerError> {
    let now = chrono::Utc::now().naive_utc();
    // Prévia: sem identidade do cliente, usamos contagens neutras
    // (0). first_purchase/limites são reavaliados na criação.
    match state.coupon_service
        .evaluate(tenant.company_id, &q.code, Decimal::from_f64(q.subtotal).unwrap_or(Decimal::ZERO), now, 0, 0, 0)
        .await
    {
        Ok((coupon, discount)) => {
            let is_free_shipping = coupon.coupon_type == "free_shipping";
            Ok(Json(CouponValidateResponse {
                valid: true,
                code: coupon.code,
                coupon_type: coupon.coupon_type,
                discount: discount.to_f64().unwrap_or(0.0),
                message: if is_free_shipping {
                    "Cupom de frete grátis aplicado".to_string()
                } else {
                    format!("Cupom aplicado: -R$ {discount:.2}")
                },
            }))
        }
        Err(letaf_core::error::CoreError::Validation(msg)) => Ok(Json(CouponValidateResponse {
            valid: false,
            code: q.code.trim().to_uppercase(),
            coupon_type: String::new(),
            discount: 0.0,
            message: msg,
        })),
        Err(e) => Err(ServerError::Core(e)),
    }
}

#[derive(Serialize)]
struct CatalogInfo {
    name: String,
    /// URL relativa da mídia (não mais base64 inline) — `None` quando não há
    /// logo/capa cadastrada.
    logo_url: Option<String>,
    cover_url: Option<String>,
    address: Option<String>,
    phone: Option<String>,
}

#[derive(Serialize)]
struct CatalogProduct {
    id: Uuid,
    name: String,
    description: Option<String>,
    price: Option<f64>,
    unit: String,
    /// Estoque atual usado pelo carrinho web para impor limite de adição.
    /// Quando `unlimited_stock=true`, este valor é irrelevante (o cliente
    /// ignora o teto). Produtos zerados sem `unlimited_stock` não chegam
    /// aqui (já filtrados antes).
    stock_quantity: f64,
    /// Quando `true`, o produto não esgota: card mostra "Disponível" e
    /// o carrinho não impõe limite de quantidade.
    unlimited_stock: bool,
    category_id: Option<Uuid>,
    subcategory_id: Option<Uuid>,
    /// URL relativa da imagem (`/catalog/media/product/{id}?v=...`) ou `None`
    /// quando o produto não tem imagem. Substitui o base64 inline (SEO/LCP).
    image_url: Option<String>,
    /// Cor de fundo detectada nas bordas da imagem do produto (`#RRGGBB`).
    /// `None` quando a imagem é transparente ou indetectável — a UI cai no
    /// fundo padrão do tema (igual ao placeholder).
    cover_color: Option<String>,
    /// Janela de disponibilidade do produto (JSON com 7 dias). `None` =
    /// sempre disponível. Cliente parseia e decide se mostra "Adicionar"
    /// ou "Indisponível" comparando com a hora atual.
    availability_schedule: Option<String>,
    /// Tipo de desconto: "fixed", "percent", "bulk_fixed", "bulk_percent"
    /// ou ausente (sem desconto).
    discount_kind: Option<String>,
    discount_value: Option<f64>,
    discount_min_qty: Option<f64>,
    /// Faixas (tiers) para descontos `bulk_*`: JSON array
    /// `[{"min_qty", "value"}, ...]`. Quando preenchido, é a fonte única
    /// dos tiers (`discount_value`/`discount_min_qty` ficam None).
    discount_tiers: Option<String>,
    /// Grupos de adicionais (Fase 4) com seus itens já hidratados —
    /// cliente web monta o overlay de seleção sem queries extras.
    /// Vazio quando o produto não tem adicionais configurados.
    addon_groups: Vec<CatalogAddonGroup>,
    /// Variações por-produto (Fase 5). Parseadas server-side a partir
    /// do JSON em `products.variations` — frontend não precisa lidar
    /// com a estrutura crua. Vazio = produto sem variações.
    variations: Vec<CatalogVariation>,
}

#[derive(Serialize, Clone)]
struct CatalogVariation {
    title: String,
    /// `"single"` | `"multi"` | `"max_value"`.
    selection: String,
    required: bool,
    /// Mín./máx. de opções selecionáveis (Fase 5B) — só relevantes em
    /// multi/max_value. `0` = sem restrição (compat. dados antigos).
    min_select: i64,
    max_select: i64,
    options: Vec<CatalogVariationOption>,
}

#[derive(Serialize, Clone)]
struct CatalogVariationOption {
    name: String,
    price: f64,
}

#[derive(Serialize, Clone)]
struct CatalogAddonGroup {
    id: Uuid,
    name: String,
    /// `"single"` | `"multi"`.
    selection: String,
    min_select: i32,
    max_select: i32,
    addons: Vec<CatalogAddon>,
}

#[derive(Serialize, Clone)]
struct CatalogAddon {
    id: Uuid,
    name: String,
    price: f64,
}

#[derive(Serialize)]
struct CatalogCategory {
    id: Uuid,
    name: String,
    description: Option<String>,
    /// Slug do ícone (allowlist em `letaf_core::category::icons`).
    /// `None` = sem ícone — UI renderiza placeholder.
    icon_name: Option<String>,
}

/// Item da resposta de `/catalog/category-icons`: par slug + rótulo
/// em PT-BR. Usado pelos clientes para popular o picker do formulário
/// de categoria (cards de ícone clicáveis).
#[derive(Serialize)]
struct CatalogCategoryIcon {
    slug: &'static str,
    label: &'static str,
}

#[derive(Serialize)]
struct CatalogBusinessHoursEntry {
    day_of_week: i32,
    open_time: String,
    close_time: String,
    is_open: bool,
}

#[derive(Serialize)]
struct CatalogBusinessHoursResponse {
    store_override: String,
    hours: Vec<CatalogBusinessHoursEntry>,
}

#[derive(Serialize)]
struct CatalogSubcategory {
    id: Uuid,
    category_id: Uuid,
    name: String,
}

/// GET /catalog/info — retorna nome, logo e capa do estabelecimento (público).
async fn get_info(
    State(state): State<AppState>,
    tenant: TenantContext,
) -> Result<Json<CatalogInfo>, ServerError> {
    let company = state.company_service
        .find_by_id(tenant.company_id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Company not found".into())))?;
    let v = company.updated_at;
    Ok(Json(CatalogInfo {
        name: company.name,
        logo_url: company.logo_data.as_ref().map(|_| media_url("logo", v)),
        cover_url: company.cover_data.as_ref().map(|_| media_url("cover", v)),
        address: company.address,
        phone: company.phone,
    }))
}

/// GET /catalog/products — lista produtos do cardápio (público).
///
/// Regras aplicadas (AI_RULES.md §3, §8, §11):
/// - Somente produtos ativos são retornados (regra encapsulada no service).
/// - Produtos zerados continuam visíveis: o front os marca como
///   "Indisponível" (mesmo tratamento de produtos fora do horário).
///   Assim o operador não precisa esconder/reexibir manualmente o item.
/// - Clientes nunca veem produtos desativados pelo operador.
async fn list_products(
    State(state): State<AppState>,
    tenant: TenantContext,
) -> Result<Json<Vec<CatalogProduct>>, ServerError> {
    let products = state.product_service.find_active(tenant.company_id).await?;
    let all_groups = state.addon_group_service.find_all(tenant.company_id).await?;
    let all_addons = state.addon_service.find_all(tenant.company_id).await?;
    // Pré-processa em HashMaps (AI_RULES.md §13 — evita varredura O(n²)
    // quando o catálogo cresce). `groups_by_id` para resolução por id e
    // `addons_by_group` para lookup já agrupado (com filtro de ativos).
    let groups_by_id: std::collections::HashMap<uuid::Uuid, &letaf_core::addon_group::model::AddonGroup> =
        all_groups.iter().map(|g| (g.base.id, g)).collect();
    let mut addons_by_group: std::collections::HashMap<uuid::Uuid, Vec<CatalogAddon>> =
        std::collections::HashMap::new();
    for a in &all_addons {
        if !a.active { continue; }
        addons_by_group.entry(a.group_id).or_default().push(CatalogAddon {
            id: a.base.id, name: a.name.clone(), price: a.price.to_f64().unwrap_or(0.0),
        });
    }
    let mut catalog: Vec<CatalogProduct> = Vec::with_capacity(products.len());
    for p in products.into_iter() {
        let groups: Vec<CatalogAddonGroup> = p.addon_group_ids.iter()
            .filter_map(|gid| groups_by_id.get(gid).copied())
            .map(|g| CatalogAddonGroup {
                id: g.base.id,
                name: g.name.clone(),
                selection: g.selection.clone(),
                min_select: g.min_select,
                max_select: g.max_select,
                addons: addons_by_group.get(&g.base.id).cloned().unwrap_or_default(),
            })
            .collect();
        let image_url = p
            .image_data
            .as_ref()
            .map(|_| media_url(&format!("product/{}", p.base.id), p.base.updated_at));
        catalog.push(CatalogProduct {
            id: p.base.id,
            name: p.name,
            description: p.description,
            price: p.price.and_then(|d| d.to_f64()),
            unit: p.unit,
            stock_quantity: p.stock_quantity,
            unlimited_stock: p.unlimited_stock,
            category_id: p.category_id,
            subcategory_id: p.subcategory_id,
            image_url,
            cover_color: p.cover_color,
            availability_schedule: p.availability_schedule,
            discount_kind: p.discount_kind,
            discount_value: p.discount_value.and_then(|d| d.to_f64()),
            discount_min_qty: p.discount_min_qty,
            discount_tiers: p.discount_tiers,
            addon_groups: groups,
            variations: parse_variations(p.variations.as_deref()),
        });
    }
    Ok(Json(catalog))
}

/// Parseia o JSON `variations` persistido em `Product.variations`
/// para a estrutura estruturada que o cliente consome.
///
/// Regras aplicadas (AI_RULES.md §1, §11):
/// - JSON inválido ou ausente → vetor vazio (sem variações). Backend
///   já valida no save (`validate_variations`); aqui é apenas leitura
///   defensiva.
/// - Estrutura malformada por entrada manual no banco é tolerada —
///   melhor exibir o produto sem variações do que quebrar o catálogo.
fn parse_variations(raw: Option<&str>) -> Vec<CatalogVariation> {
    let Some(s) = raw else { return Vec::new(); };
    let trimmed = s.trim();
    if trimmed.is_empty() { return Vec::new(); }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else { return Vec::new(); };
    arr.iter().filter_map(|v| {
        let obj = v.as_object()?;
        let title = obj.get("title")?.as_str()?.to_string();
        let selection = obj.get("selection")?.as_str()?.to_string();
        let required = obj.get("required").and_then(|x| x.as_bool()).unwrap_or(false);
        let min_select = obj.get("min_select").and_then(|x| x.as_i64()).unwrap_or(0);
        let max_select = obj.get("max_select").and_then(|x| x.as_i64()).unwrap_or(0);
        let options_arr = obj.get("options")?.as_array()?;
        let options: Vec<CatalogVariationOption> = options_arr.iter().filter_map(|opt| {
            let o = opt.as_object()?;
            let name = o.get("name")?.as_str()?.to_string();
            // Tolerante a preço número (legado) ou string decimal (novo formato).
            let price = letaf_core::money::price_from_json(o.get("price")?)?.to_f64()?;
            Some(CatalogVariationOption { name, price })
        }).collect();
        if options.is_empty() { return None; }
        Some(CatalogVariation { title, selection, required, min_select, max_select, options })
    }).collect()
}

/// GET /catalog/subcategories — lista subcategorias do cardápio (público).
async fn list_subcategories(
    State(state): State<AppState>,
    tenant: TenantContext,
) -> Result<Json<Vec<CatalogSubcategory>>, ServerError> {
    let items = state.subcategory_service.find_all(tenant.company_id).await?;
    let catalog: Vec<CatalogSubcategory> = items
        .into_iter()
        .map(|s| CatalogSubcategory {
            id: s.base.id,
            category_id: s.category_id,
            name: s.name,
        })
        .collect();
    Ok(Json(catalog))
}

/// GET /catalog/business-hours — retorna override e horários de funcionamento (público).
async fn list_business_hours(
    State(state): State<AppState>,
    tenant: TenantContext,
) -> Result<Json<CatalogBusinessHoursResponse>, ServerError> {
    let company = state.company_service
        .find_by_id(tenant.company_id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Company not found".into())))?;
    let items = state.business_hours_service.find_all(tenant.company_id).await?;
    let hours: Vec<CatalogBusinessHoursEntry> = items
        .into_iter()
        .map(|bh| CatalogBusinessHoursEntry {
            day_of_week: bh.day_of_week,
            open_time: bh.open_time,
            close_time: bh.close_time,
            is_open: bh.is_open,
        })
        .collect();
    Ok(Json(CatalogBusinessHoursResponse {
        store_override: company.store_override,
        hours,
    }))
}

/// GET /catalog/categories — lista categorias do cardápio (público).
async fn list_categories(
    State(state): State<AppState>,
    tenant: TenantContext,
) -> Result<Json<Vec<CatalogCategory>>, ServerError> {
    let categories = state.category_service.find_all(tenant.company_id).await?;
    let catalog: Vec<CatalogCategory> = categories
        .into_iter()
        .map(|c| CatalogCategory {
            id: c.base.id,
            name: c.name,
            description: c.description,
            icon_name: c.icon_name,
        })
        .collect();
    Ok(Json(catalog))
}

/// GET /catalog/category-icons — lista de ícones disponíveis para
/// uso em `Category.icon_name`. Resposta estática (allowlist no core),
/// sem auth — alimenta o picker do formulário no desktop.
async fn list_category_icons() -> Json<Vec<CatalogCategoryIcon>> {
    let icons: Vec<CatalogCategoryIcon> = letaf_core::category::icons::ICONS
        .iter()
        .map(|(slug, label)| CatalogCategoryIcon { slug, label })
        .collect();
    Json(icons)
}

#[derive(Serialize)]
struct CatalogBanner {
    id: Uuid,
    title: String,
    /// URL relativa da imagem do banner (não base64 inline).
    image_url: String,
    item_type: String,
    item_id: Option<Uuid>,
    item_url: Option<String>,
    sort_order: i32,
}

/// GET /catalog/banners — lista banners ATIVOS do estabelecimento.
/// Público (sem auth) — alimenta o carousel do topo do cardápio web.
async fn list_banners(
    State(state): State<AppState>,
    tenant: TenantContext,
) -> Result<Json<Vec<CatalogBanner>>, ServerError> {
    let items = state.banner_service.find_active(tenant.company_id).await?;
    let catalog: Vec<CatalogBanner> = items
        .into_iter()
        .map(|b| CatalogBanner {
            image_url: media_url(&format!("banner/{}", b.base.id), b.base.updated_at),
            id: b.base.id,
            title: b.title,
            item_type: b.item_type,
            item_id: b.item_id,
            item_url: b.item_url,
            sort_order: b.sort_order,
        })
        .collect();
    Ok(Json(catalog))
}
