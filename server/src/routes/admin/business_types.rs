//! Painel do super admin — catálogo de tipos de empresa (ramo do
//! estabelecimento). CRUD gerido apenas pelo super admin (§11 — a
//! autoridade é o backend). Nível PLATAFORMA (global, sem `company_id`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::business_type::model::BusinessType;
use letaf_core::business_type::service::BusinessTypeInput;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::audit;

/// Payload de tipo de empresa devolvido ao painel.
#[derive(Serialize)]
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

pub(super) async fn list_business_types(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<BusinessTypePayload>>, ServerError> {
    auth.require_screen("business_types")?;
    let items = state.business_type_service.find_all().await?;
    Ok(Json(items.into_iter().map(payload).collect()))
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    letaf_core::business_type::model::DEFAULT_THEME.to_string()
}

#[derive(Deserialize)]
pub(super) struct BusinessTypeBody {
    name: String,
    #[serde(default)]
    description: String,
    /// Tema visual do site (slug). Default/ inválido → resolvido no service.
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default = "default_true")]
    active: bool,
    #[serde(default)]
    sort_order: i32,
}

impl BusinessTypeBody {
    fn into_input(self) -> BusinessTypeInput {
        BusinessTypeInput {
            name: self.name,
            description: self.description,
            theme: self.theme,
            active: self.active,
            sort_order: self.sort_order,
        }
    }
}

pub(super) async fn create_business_type(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<BusinessTypeBody>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    auth.require_screen("business_types")?;
    let item = state.business_type_service.create(body.into_input()).await?;
    audit(&state, &auth, "business_type.create", "business_type", Some(item.id), String::new(), String::new()).await;
    Ok((StatusCode::CREATED, Json(json!({ "id": item.id }))))
}

pub(super) async fn update_business_type(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<BusinessTypeBody>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("business_types")?;
    state.business_type_service.update(id, body.into_input()).await?;
    audit(&state, &auth, "business_type.update", "business_type", Some(id), String::new(), String::new()).await;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_business_type(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("business_types")?;
    state.business_type_service.soft_delete(id).await?;
    audit(&state, &auth, "business_type.delete", "business_type", Some(id), String::new(), String::new()).await;
    Ok(Json(json!({ "ok": true })))
}
