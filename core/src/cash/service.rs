use std::sync::Arc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use uuid::Uuid;

use super::model::{
    CashMovement, CashSession, MethodTotals, MovementKind, SessionStatus, SessionSummary,
};
use super::repository::{CashMovementRepository, CashSessionRepository};
use crate::error::CoreError;

/// Service da gestão de caixa.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §14):
/// - Orquestra abertura/fechamento + lançamento de movimentos.
/// - Valida tudo (sem confiar em dados do cliente).
/// - Calcula totais agregando o livro-razão; UI nunca soma.
pub struct CashService {
    sessions: Arc<dyn CashSessionRepository>,
    movements: Arc<dyn CashMovementRepository>,
}

impl CashService {
    pub fn new(
        sessions: Arc<dyn CashSessionRepository>,
        movements: Arc<dyn CashMovementRepository>,
    ) -> Self {
        Self { sessions, movements }
    }

    // ── Sessões ──────────────────────────────────────────────────

    pub async fn find_active(&self, company_id: Uuid) -> Result<Option<CashSession>, CoreError> {
        self.sessions.find_active(company_id).await
    }

    pub async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashSession>, CoreError> {
        self.sessions.find_by_id(company_id, id).await
    }

    pub async fn find_recent(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CashSession>, CoreError> {
        self.sessions.find_recent(company_id, limit.max(1)).await
    }

    /// Abre uma nova sessão. Rejeita se já existir sessão Open ou se
    /// `initial_change` for negativo.
    ///
    /// Cria automaticamente o movimento `Opening` com `amount =
    /// initial_change` — assim o `cash_expected` da sessão fica
    /// derivável só pelo livro-razão (uma fonte de verdade).
    pub async fn open_session(
        &self,
        company_id: Uuid,
        operator_id: Uuid,
        operator_name: String,
        initial_change: Decimal,
        notes: Option<String>,
    ) -> Result<CashSession, CoreError> {
        if initial_change < Decimal::ZERO {
            return Err(CoreError::Validation(
                "Troco inicial não pode ser negativo".into(),
            ));
        }
        if self.sessions.find_active(company_id).await?.is_some() {
            return Err(CoreError::Validation(
                "Já existe um caixa aberto. Feche o atual antes de abrir outro.".into(),
            ));
        }
        let session = CashSession::new(
            company_id,
            operator_id,
            operator_name,
            initial_change,
            notes,
        );
        self.sessions.create(&session).await?;

        // Lançamento Opening — saldo em dinheiro inicia com troco.
        if initial_change > Decimal::ZERO {
            let mv = CashMovement::new(
                company_id,
                session.base.id,
                MovementKind::Opening,
                initial_change,
                Some("cash".to_string()),
                "Abertura".into(),
                None,
                None,
            );
            self.movements.create(&mv).await?;
        }
        Ok(session)
    }

    /// Fecha a sessão informada. Persiste `counted_cash`, `close_notes`,
    /// `closed_at` e flag `Closed`. Não cria movimento — o `counted_cash`
    /// é o que o operador "viu na gaveta", não um movimento de fato.
    pub async fn close_session(
        &self,
        company_id: Uuid,
        session_id: Uuid,
        counted_cash: Decimal,
        notes: Option<String>,
    ) -> Result<CashSession, CoreError> {
        let mut session = self
            .sessions
            .find_by_id(company_id, session_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Sessão de caixa não encontrada".into()))?;
        if session.status == SessionStatus::Closed {
            return Err(CoreError::Validation("Sessão já foi fechada".into()));
        }
        if counted_cash < Decimal::ZERO {
            return Err(CoreError::Validation(
                "Valor contado não pode ser negativo".into(),
            ));
        }
        let now = chrono::Utc::now().naive_utc();
        session.closed_at = Some(now);
        session.counted_cash = Some(counted_cash);
        session.status = SessionStatus::Closed;
        session.close_notes = notes;
        session.base.updated_at = now;
        session.base.synced = false;
        self.sessions.update(&session).await?;
        Ok(session)
    }

    // ── Movimentos ───────────────────────────────────────────────

    /// Registra uma sangria (saída manual) na sessão aberta.
    pub async fn register_sangria(
        &self,
        company_id: Uuid,
        session_id: Uuid,
        amount: Decimal,
        reason: String,
        detail: Option<String>,
    ) -> Result<CashMovement, CoreError> {
        self.assert_session_open(company_id, session_id).await?;
        self.assert_positive_amount(amount)?;
        let trimmed_reason = reason.trim();
        if trimmed_reason.is_empty() {
            return Err(CoreError::Validation(
                "Motivo da sangria é obrigatório".into(),
            ));
        }
        let mv = CashMovement::new(
            company_id,
            session_id,
            MovementKind::Sangria,
            amount,
            Some("cash".to_string()),
            trimmed_reason.to_string(),
            detail,
            None,
        );
        self.movements.create(&mv).await?;
        Ok(mv)
    }

    /// Registra um suprimento (entrada manual) na sessão aberta.
    pub async fn register_suprimento(
        &self,
        company_id: Uuid,
        session_id: Uuid,
        amount: Decimal,
        origin: String,
        detail: Option<String>,
    ) -> Result<CashMovement, CoreError> {
        self.assert_session_open(company_id, session_id).await?;
        self.assert_positive_amount(amount)?;
        let trimmed = origin.trim();
        if trimmed.is_empty() {
            return Err(CoreError::Validation(
                "Origem do suprimento é obrigatória".into(),
            ));
        }
        let mv = CashMovement::new(
            company_id,
            session_id,
            MovementKind::Suprimento,
            amount,
            Some("cash".to_string()),
            trimmed.to_string(),
            detail,
            None,
        );
        self.movements.create(&mv).await?;
        Ok(mv)
    }

    /// Registra uma venda como movimento da sessão. Chamado pelo
    /// `OrderService::create_pdv` quando o pedido é criado dentro de
    /// uma sessão aberta. `method` é o pagamento principal — vendas
    /// parceladas (dinheiro + cartão) ficam como UM movimento agregado
    /// pra simplificar agregação por método; o detalhe do split fica
    /// em `Order.notes`.
    pub async fn register_sale_movement(
        &self,
        company_id: Uuid,
        session_id: Uuid,
        order_id: Uuid,
        amount: Decimal,
        method: String,
    ) -> Result<CashMovement, CoreError> {
        self.assert_session_open(company_id, session_id).await?;
        self.assert_positive_amount(amount)?;
        let mv = CashMovement::new(
            company_id,
            session_id,
            MovementKind::Sale,
            amount,
            Some(method),
            "Venda PDV".into(),
            None,
            Some(order_id),
        );
        self.movements.create(&mv).await?;
        Ok(mv)
    }

    // ── Agregação ────────────────────────────────────────────────

    /// Agrega o livro-razão da sessão em totais (por método, sangria,
    /// suprimento) + saldo em dinheiro esperado. UI usa direto.
    pub async fn session_summary(
        &self,
        company_id: Uuid,
        session_id: Uuid,
    ) -> Result<SessionSummary, CoreError> {
        let movements = self
            .movements
            .find_by_session(company_id, session_id)
            .await?;
        Ok(summarize(&movements))
    }

    pub async fn find_movements(
        &self,
        company_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<CashMovement>, CoreError> {
        self.movements.find_by_session(company_id, session_id).await
    }

    // ── Sync (delegação direta aos repos) ────────────────────────

    pub async fn find_unsynced_sessions(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<CashSession>, CoreError> {
        self.sessions.find_unsynced(company_id).await
    }
    pub async fn mark_session_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.sessions.mark_synced(company_id, id, updated_at).await
    }
    pub async fn find_sessions_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<CashSession>, CoreError> {
        self.sessions.find_updated_since(company_id, since).await
    }
    /// Página do pull de sessões por keyset `(updated_at, id)`.
    pub async fn find_sessions_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CashSession>, CoreError> {
        self.sessions.find_updated_since_paged(company_id, since, after_id, limit).await
    }
    pub async fn sync_upsert_session(
        &self,
        company_id: Uuid,
        mut session: CashSession,
    ) -> Result<(), CoreError> {
        if session.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        session.base.synced = true;
        self.sessions.sync_upsert(&session).await
    }

    pub async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<CashMovement>, CoreError> {
        self.movements.find_unsynced(company_id).await
    }
    pub async fn mark_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.movements.mark_synced(company_id, id, updated_at).await
    }
    pub async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<CashMovement>, CoreError> {
        self.movements.find_updated_since(company_id, since).await
    }
    /// Página do pull de movimentos por keyset `(updated_at, id)`.
    pub async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CashMovement>, CoreError> {
        self.movements.find_updated_since_paged(company_id, since, after_id, limit).await
    }
    pub async fn sync_upsert_movement(
        &self,
        company_id: Uuid,
        mut mv: CashMovement,
    ) -> Result<(), CoreError> {
        if mv.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        mv.base.synced = true;
        self.movements.sync_upsert(&mv).await
    }

    // ── Helpers ──────────────────────────────────────────────────

    async fn assert_session_open(
        &self,
        company_id: Uuid,
        session_id: Uuid,
    ) -> Result<(), CoreError> {
        let s = self
            .sessions
            .find_by_id(company_id, session_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Sessão de caixa não encontrada".into()))?;
        if s.status != SessionStatus::Open {
            return Err(CoreError::Validation(
                "Sessão de caixa não está aberta".into(),
            ));
        }
        Ok(())
    }

    fn assert_positive_amount(&self, amount: Decimal) -> Result<(), CoreError> {
        if amount <= Decimal::ZERO {
            return Err(CoreError::Validation("Valor deve ser positivo".into()));
        }
        Ok(())
    }
}

/// Agrega uma lista de movimentos. Pura — testável sem repo.
pub fn summarize(movements: &[CashMovement]) -> SessionSummary {
    let mut s = SessionSummary::default();
    let mut cash_balance = Decimal::ZERO;
    for mv in movements {
        match mv.kind {
            MovementKind::Opening => {
                cash_balance += mv.amount;
            }
            MovementKind::Sale => {
                s.sales_total += mv.amount;
                s.sales_count += 1;
                let key = mv.method.clone().unwrap_or_else(|| "cash".into());
                let entry = s.by_method.entry(key.clone()).or_insert(MethodTotals {
                    amount: Decimal::ZERO,
                    count: 0,
                });
                entry.amount += mv.amount;
                entry.count += 1;
                if key == "cash" {
                    cash_balance += mv.amount;
                }
            }
            MovementKind::Sangria => {
                s.sangria_total += mv.amount;
                s.sangria_count += 1;
                cash_balance -= mv.amount;
            }
            MovementKind::Suprimento => {
                s.suprimento_total += mv.amount;
                s.suprimento_count += 1;
                cash_balance += mv.amount;
            }
        }
    }
    // Garante chaves padrão para a UI conseguir renderizar sempre 4
    // linhas (cash/credit/debit/pix), mesmo sem vendas no método.
    for default_key in ["cash", "credit", "debit", "pix"] {
        s.by_method.entry(default_key.to_string()).or_default();
    }
    s.cash_expected = if cash_balance.abs() < dec!(0.005) {
        Decimal::ZERO
    } else {
        cash_balance
    };
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::BaseFields;
    use rust_decimal_macros::dec;

    fn mv(
        kind: MovementKind,
        amount: Decimal,
        method: Option<&str>,
    ) -> CashMovement {
        CashMovement {
            base: BaseFields::new(Uuid::new_v4()),
            session_id: Uuid::new_v4(),
            kind,
            amount,
            method: method.map(String::from),
            reason: String::new(),
            detail: None,
            order_id: None,
        }
    }

    #[test]
    fn summarize_empty() {
        let s = summarize(&[]);
        assert_eq!(s.sales_total, dec!(0));
        assert_eq!(s.cash_expected, dec!(0));
        // garante chaves padrão
        assert!(s.by_method.contains_key("cash"));
        assert!(s.by_method.contains_key("pix"));
    }

    #[test]
    fn summarize_basic_flow() {
        let movs = vec![
            mv(MovementKind::Opening, dec!(100), Some("cash")),
            mv(MovementKind::Sale, dec!(50), Some("cash")),
            mv(MovementKind::Sale, dec!(30), Some("pix")),
            mv(MovementKind::Suprimento, dec!(20), Some("cash")),
            mv(MovementKind::Sangria, dec!(40), Some("cash")),
        ];
        let s = summarize(&movs);
        assert_eq!(s.sales_total, dec!(80));
        assert_eq!(s.sales_count, 2);
        assert_eq!(s.sangria_total, dec!(40));
        assert_eq!(s.suprimento_total, dec!(20));
        // cash: 100 (opening) + 50 (sale cash) + 20 (suprimento) − 40 (sangria) = 130
        assert_eq!(s.cash_expected, dec!(130));
        assert_eq!(s.by_method["cash"].amount, dec!(50));
        assert_eq!(s.by_method["pix"].amount, dec!(30));
    }
}
