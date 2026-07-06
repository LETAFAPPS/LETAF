-- Snapshot do plano do catálogo na assinatura (Fase 2) — espelha o
-- servidor. SQLite exige um ALTER por coluna.
ALTER TABLE subscriptions ADD COLUMN plan_id TEXT;
ALTER TABLE subscriptions ADD COLUMN plan_name TEXT NOT NULL DEFAULT '';
ALTER TABLE subscriptions ADD COLUMN plan_amount REAL NOT NULL DEFAULT 0;
ALTER TABLE subscriptions ADD COLUMN plan_period_months INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subscriptions ADD COLUMN trial_days INTEGER NOT NULL DEFAULT 0;
