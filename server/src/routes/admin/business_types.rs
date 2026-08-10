//! Tipos de empresa (ramo do estabelecimento) — nível PLATAFORMA, global.
//!
//! Os 3 tipos são fixos (semeados na migração 102) e definem o tema do site e
//! os recursos de produto por tipo. Não há mais tela de CRUD: só resta a
//! LISTAGEM, usada pelo seletor "Tipo de empresa" no cadastro de empresas —
//! por isso o gate é `companies` (quem gerencia empresas precisa da lista).

use axum::extract::State;
use axum::Json;
use uuid::Uuid;

use letaf_core::business_type::model::BusinessType;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

/// Payload de tipo de empresa devolvido ao seletor do cadastro de empresas.
#[derive(serde::Serialize)]
pub(super) struct BusinessTypePayload {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub theme: String,
    pub active: bool,
    pub sort_order: i32,
}

fn payload(b: BusinessType) -> BusinessTypePayload {
    BusinessTypePayload {
        id: b.id,
        name: b.name,
        description: b.description,
        theme: b.theme,
        active: b.active,
        sort_order: b.sort_order,
    }
}

/// GET /admin/business-types — lista os tipos (alimenta o seletor de tipo no
/// cadastro de empresa). Gate `companies` (não há mais tela própria).
pub(super) async fn list_business_types(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<BusinessTypePayload>>, ServerError> {
    auth.require_screen("companies")?;
    let items = state.business_type_service.find_all().await?;
    Ok(Json(items.into_iter().map(payload).collect()))
}
