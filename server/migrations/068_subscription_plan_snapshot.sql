-- Snapshot do plano do catálogo na assinatura (Fase 2). Quando a loja
-- assina um plano do super admin, guardamos os termos aqui para o billing
-- não depender do catálogo (que pode mudar) nem do `plan_kind` fixo.
-- `plan_id = NULL` → assinatura legada (billing usa `plan_for(plan_kind)`).
ALTER TABLE subscriptions
    ADD COLUMN IF NOT EXISTS plan_id            UUID,
    ADD COLUMN IF NOT EXISTS plan_name          TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS plan_amount NUMERIC(14, 2) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS plan_period_months INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS trial_days         INTEGER NOT NULL DEFAULT 0;
