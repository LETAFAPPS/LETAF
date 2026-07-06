//! Sessão efêmera do cadastro de cartão via página hosted (Efi.js).
//!
//! Regras aplicadas (AI_RULES.md §11):
//! - A tokenização do cartão é client-side (Efi.js) — o PAN nunca chega
//!   ao server. Esta sessão só liga o `company_id` (autenticado no app)
//!   à página pública de cadastro, por um token de uso curto.
//! - Vive em memória (não persiste): é descartável e expira em 20 min.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

const TTL: Duration = Duration::from_secs(20 * 60);

/// Estado da sessão, consultado pelo desktop via polling.
#[derive(Clone)]
pub enum CardSessionStatus {
    Pending,
    Completed,
    Failed(String),
}

struct Entry {
    company_id: Uuid,
    status: CardSessionStatus,
    created: Instant,
}

/// Store thread-safe de sessões de cadastro de cartão.
#[derive(Default)]
pub struct CardSessionStore {
    inner: Mutex<HashMap<String, Entry>>,
}

impl CardSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cria uma sessão para `company_id` e devolve o token opaco.
    pub fn create(&self, company_id: Uuid) -> String {
        let token = Uuid::new_v4().simple().to_string();
        let mut g = self.inner.lock().unwrap();
        g.retain(|_, e| e.created.elapsed() < TTL);
        g.insert(
            token.clone(),
            Entry {
                company_id,
                status: CardSessionStatus::Pending,
                created: Instant::now(),
            },
        );
        token
    }

    /// `company_id` da sessão se ela existir e não expirou.
    pub fn company_of(&self, token: &str) -> Option<Uuid> {
        let g = self.inner.lock().unwrap();
        g.get(token)
            .filter(|e| e.created.elapsed() < TTL)
            .map(|e| e.company_id)
    }

    pub fn set_status(&self, token: &str, status: CardSessionStatus) {
        let mut g = self.inner.lock().unwrap();
        if let Some(e) = g.get_mut(token) {
            e.status = status;
        }
    }

    /// Status da sessão, validando que pertence a `company_id` (o app
    /// só consulta as próprias sessões).
    pub fn status(&self, token: &str, company_id: Uuid) -> Option<CardSessionStatus> {
        let g = self.inner.lock().unwrap();
        g.get(token)
            .filter(|e| e.company_id == company_id)
            .map(|e| e.status.clone())
    }
}
